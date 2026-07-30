//! Kernel source retrieval: cache selection, download with mirror fallback, sha256
//! verification and extraction of a single subdirectory from the tarball.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use ureq::Agent;
use ureq::config::IpFamily;

use crate::error::{Error, IoContext, Result};

pub const SYSTEM_CACHE: &str = "/var/cache/dkms-kernel-src";
pub const TMP_CACHE: &str = "/tmp/dkms-kernel-src";
pub const MIRRORS: &[&str] =
    &["https://mirrors.edge.kernel.org/pub/linux/kernel", "https://cdn.kernel.org/pub/linux/kernel"];

/// `/var/cache/dkms-kernel-src` when writable, else `/tmp/dkms-kernel-src`, else `/tmp`.
pub fn cache_dir(explicit: Option<&Path>) -> PathBuf {
    if let Some(dir) = explicit {
        return dir.to_path_buf();
    }
    let system = Path::new(SYSTEM_CACHE);
    if system.is_dir() && rustix::fs::access(system, rustix::fs::Access::WRITE_OK).is_ok() {
        return system.to_path_buf();
    }
    if fs::create_dir_all(TMP_CACHE).is_ok() {
        return TMP_CACHE.into();
    }
    "/tmp".into()
}

/// Leftover partial downloads, removed by the rollback.
pub fn partial_downloads() -> Vec<PathBuf> {
    [SYSTEM_CACHE, TMP_CACHE]
        .iter()
        .filter_map(|d| fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "tmp"))
        .collect()
}

pub fn tarball_name(source_version: &str) -> String {
    format!("linux-{source_version}.tar.xz")
}

/// Returns the cached tarball, downloading it first when missing.
pub fn ensure_tarball(cache: &Path, major: u32, source_version: &str, installed: &str) -> Result<PathBuf> {
    let name = tarball_name(source_version);
    let target = cache.join(&name);
    if target.is_file() {
        return Ok(target);
    }
    println!("Downloading source {source_version} for installed kernel {installed}...");
    let agent: Agent = Agent::config_builder()
        .ip_family(IpFamily::Ipv4Only)
        .timeout_connect(Some(Duration::from_secs(30)))
        .build()
        .into();
    let partial = cache.join(format!("{name}.tmp"));
    for (i, mirror) in MIRRORS.iter().enumerate() {
        if i > 0 {
            println!("Primary mirror failed, trying CDN fallback...");
        }
        let base = format!("{mirror}/v{major}.x");
        match download(&agent, &format!("{base}/{name}"), &partial)
            .and_then(|()| verify(&agent, &base, &name, &partial))
        {
            Ok(()) => {
                fs::rename(&partial, &target).ctx(|| format!("moving {} into place", partial.display()))?;
                return Ok(target);
            }
            Err(e) => eprintln!("{base}/{name}: {e}"),
        }
    }
    Err(Error::Download(name))
}

/// Downloads into `partial`, resuming an earlier attempt when the server allows it.
fn download(agent: &Agent, url: &str, partial: &Path) -> Result<(), String> {
    let have = fs::metadata(partial).map(|m| m.len()).unwrap_or(0);
    let mut req = agent.get(url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let mut resp = match req.call() {
        // Range past the end: the partial file is already complete.
        Err(ureq::Error::StatusCode(416)) => return Ok(()),
        r => r.map_err(|e| e.to_string())?,
    };
    let resumed = resp.status() == 206;
    let total = resp.body().content_length().map(|l| if resumed { l + have } else { l });
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(partial)
        .map_err(|e| format!("{}: {e}", partial.display()))?;

    let bar = if io::stderr().is_terminal() {
        let bar = total.map_or_else(ProgressBar::no_length, ProgressBar::new);
        bar.set_style(
            ProgressStyle::with_template("{bytes}/{total_bytes} [{wide_bar}] {bytes_per_sec} eta {eta}")
                .expect("valid template"),
        );
        bar.set_position(if resumed { have } else { 0 });
        bar
    } else {
        ProgressBar::hidden()
    };
    let mut reader = resp.body_mut().as_reader();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        bar.inc(n as u64);
        if let Some(sig) = crate::signals::pending() {
            return Err(format!("interrupted by signal {sig}"));
        }
    }
    bar.finish_and_clear();
    file.sync_all().map_err(|e| e.to_string())
}

/// Checks `file` against `sha256sums.asc` from the same mirror. A missing checksum
/// list only warns (mirrors lag behind); a mismatch deletes the file and fails.
fn verify(agent: &Agent, base: &str, name: &str, file: &Path) -> Result<(), String> {
    let sums = match agent.get(format!("{base}/sha256sums.asc")).call().and_then(|mut r| r.body_mut().read_to_string())
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Warning: could not fetch sha256sums.asc ({e}); skipping checksum verification.");
            return Ok(());
        }
    };
    let Some(expected) = expected_sha256(&sums, name) else {
        eprintln!("Warning: {name} not listed in sha256sums.asc; skipping checksum verification.");
        return Ok(());
    };
    let actual = sha256_file(file).map_err(|e| e.to_string())?;
    if actual != expected {
        let _ = fs::remove_file(file);
        return Err(format!("checksum mismatch (expected {expected}, got {actual})"));
    }
    println!("Checksum OK ({name}).");
    Ok(())
}

pub fn expected_sha256(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let (hash, file) = (parts.next()?, parts.next()?);
        (file == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Extracts `<top>/<subdir>/**` from the tarball into `dest` with the prefix removed,
/// like `tar --xform=s,linux-X/subdir,.,`. Stops reading once past the subdirectory.
pub fn extract_subdir(tarball: &Path, top: &str, subdir: &str, dest: &Path) -> Result<usize> {
    let file = File::open(tarball).ctx(|| format!("opening {}", tarball.display()))?;
    let decoder = liblzma::read::XzDecoder::new_multi_decoder(BufReader::with_capacity(1 << 20, file));
    let mut archive = tar::Archive::new(decoder);
    let prefix = Path::new(top).join(subdir.trim_matches('/'));
    let mut extracted = 0;
    let read_err = |e: io::Error| Error::Io { context: format!("reading {}", tarball.display()), source: e };
    for entry in archive.entries().map_err(read_err)? {
        let mut entry = entry.map_err(read_err)?;
        let path = entry.path().map_err(read_err)?.into_owned();
        let Ok(rel) = path.strip_prefix(&prefix) else {
            if extracted > 0 {
                break; // kernel tarballs are sorted by path
            }
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(Error::Io {
                context: format!("refusing unsafe path {}", path.display()),
                source: io::ErrorKind::InvalidData.into(),
            });
        }
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).ctx(|| format!("creating {}", parent.display()))?;
        }
        entry.unpack(&out).ctx(|| format!("extracting {}", out.display()))?;
        extracted += 1;
    }
    if extracted == 0 {
        return Err(Error::Io {
            context: format!("{} not found in {}", prefix.display(), tarball.display()),
            source: io::ErrorKind::NotFound.into(),
        });
    }
    Ok(extracted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tarball(dir: &Path, entries: &[(&str, &str)]) -> PathBuf {
        let path = dir.join("linux-9.9.tar.xz");
        let enc = liblzma::write::XzEncoder::new(File::create(&path).unwrap(), 1);
        let mut builder = tar::Builder::new(enc);
        for (name, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, body.as_bytes()).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
        path
    }

    #[test]
    fn extracts_only_the_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball = make_tarball(
            tmp.path(),
            &[
                ("linux-9.9/Makefile", "top"),
                ("linux-9.9/drivers/bluetooth/btusb.c", "btusb"),
                ("linux-9.9/drivers/bluetooth/sub/x.h", "x"),
                ("linux-9.9/drivers/bluetoothx/other.c", "no"),
                ("linux-9.9/drivers/net/eth.c", "no"),
            ],
        );
        let dest = tmp.path().join("out");
        fs::create_dir(&dest).unwrap();
        let n = extract_subdir(&tarball, "linux-9.9", "drivers/bluetooth", &dest).unwrap();
        assert_eq!(n, 2);
        assert_eq!(fs::read_to_string(dest.join("btusb.c")).unwrap(), "btusb");
        assert_eq!(fs::read_to_string(dest.join("sub/x.h")).unwrap(), "x");
        assert!(!dest.join("other.c").exists() && !dest.join("eth.c").exists());
    }

    #[test]
    fn missing_subdirectory_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball = make_tarball(tmp.path(), &[("linux-9.9/Makefile", "top")]);
        assert!(extract_subdir(&tarball, "linux-9.9", "drivers/bluetooth", tmp.path()).is_err());
    }

    #[test]
    fn finds_checksum_line() {
        let sums = "-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\n\
            aaaa  linux-6.6.tar.xz\n\
            0123456789abcdef0123456789abcdef0123456789abcdef0123456789ABCDEF  linux-6.6.70.tar.xz\n";
        assert_eq!(
            expected_sha256(sums, "linux-6.6.70.tar.xz").unwrap(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        assert_eq!(expected_sha256(sums, "linux-6.6.tar.xz"), None);
        assert_eq!(expected_sha256(sums, "linux-6.7.tar.xz"), None);
    }

    #[test]
    fn hashes_files() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), "abc").unwrap();
        assert_eq!(
            sha256_file(tmp.path()).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
