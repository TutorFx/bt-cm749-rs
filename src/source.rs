//! Kernel source tarballs: extraction of a single subdirectory, the counterpart of
//! `tar -xf ... --xform` in kernel-module_patch.sh.

use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Component, Path};

use crate::error::{Error, IoContext, Result};

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
    use std::path::PathBuf;

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
}
