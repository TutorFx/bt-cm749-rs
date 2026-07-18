//! Kernel version detection, replacing `kernel-version_get.sh`.

use std::cmp::Ordering;
use std::fs;
use std::path::Path;

use crate::error::{Error, Result};
use crate::exec::{Cmd, Runner};
use crate::os_release::Distro;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelVersion {
    /// Full release string as used by DKMS and `/lib/modules` (e.g. `6.8.0-31-generic`).
    pub release: String,
    pub major: u32,
    pub minor: u32,
    pub sub: Option<u32>,
}

impl KernelVersion {
    /// Parses the leading `major.minor[.sub]` of a release string. Unlike the shell
    /// parameter expansion it replaced, this does not take `x86_64` from
    /// `6.12.9-200.fc41.x86_64` as the sublevel.
    pub fn parse(release: &str) -> Option<KernelVersion> {
        let mut nums = [None; 3];
        let mut rest = release;
        for (i, slot) in nums.iter_mut().enumerate() {
            if i > 0 {
                match rest.strip_prefix('.') {
                    Some(r) => rest = r,
                    None => break,
                }
            }
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                break;
            }
            *slot = rest[..digits].parse().ok();
            rest = &rest[digits..];
        }
        Some(KernelVersion { release: release.to_string(), major: nums[0]?, minor: nums[1]?, sub: nums[2] })
    }

    /// Upstream tarball version: `6.12.10`, or `6.8` for a `.0` sublevel.
    pub fn source_version(&self) -> String {
        match self.sub {
            Some(sub) if sub != 0 => format!("{}.{}.{}", self.major, self.minor, sub),
            _ => format!("{}.{}", self.major, self.minor),
        }
    }

    /// Kernels from 6.18 on are the ones the fix is advertised for.
    pub fn is_recent(&self) -> bool {
        (self.major, self.minor) >= (6, 18)
    }

    pub fn describe(&self) -> String {
        let sub = self.sub.map(|s| s.to_string()).unwrap_or_default();
        format!(
            "Detected kernel version {}.\nCorresponding kernel source version is {}.{}.{}.",
            self.release, self.major, self.minor, sub
        )
    }
}

/// Resolves the target kernel: explicit value, else the newest installed
/// `linux-image-*` on Debian, else the running kernel.
pub fn detect(
    explicit: Option<&str>,
    distro: Distro,
    headers_root: &Path,
    runner: &dyn Runner,
) -> Result<KernelVersion> {
    let release = match explicit {
        Some(v) => v.to_string(),
        None => {
            let from_dpkg = if distro == Distro::Debian { newest_debian_image(runner) } else { None };
            from_dpkg.unwrap_or_else(|| rustix::system::uname().release().to_string_lossy().into_owned())
        }
    };
    let mut kv = KernelVersion::parse(&release).ok_or(Error::KernelVersion)?;
    if kv.sub.is_none() && distro == Distro::Debian {
        kv.sub = debian_headers_sublevel(&headers_root.join(format!("linux-headers-{release}")));
    }
    Ok(kv)
}

/// Newest installed (`ii`) `linux-image-<digit>...` package, compared as versions
/// rather than with the lexicographic `sort -r` of the original script.
fn newest_debian_image(runner: &dyn Runner) -> Option<String> {
    let out = runner.output(&Cmd::new("dpkg").arg("-l")).ok()?;
    out.stdout
        .lines()
        .filter(|l| l.starts_with("ii"))
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter_map(|pkg| pkg.strip_prefix("linux-image-"))
        .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
        .max_by(|a, b| version_cmp(a, b))
        .map(str::to_string)
}

/// `SUBLEVEL` from a Debian headers Makefile, following a single-line `include`.
fn debian_headers_sublevel(headers_dir: &Path) -> Option<u32> {
    let mut makefile = headers_dir.join("Makefile");
    let mut content = fs::read_to_string(&makefile).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    if let [only] = lines.as_slice() {
        if let Some(target) = only.strip_prefix("include ") {
            makefile = target.trim().into();
            content = fs::read_to_string(&makefile).ok()?;
        }
    }
    content.lines().find_map(|l| {
        let (key, value) = l.split_once('=')?;
        (key.trim() == "SUBLEVEL").then(|| value.trim().parse().ok())?
    })
}

/// Natural ordering: digit runs compare numerically, everything else bytewise.
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    fn chunks(s: &str) -> Vec<(bool, &str)> {
        let mut out = Vec::new();
        let mut start = 0;
        let bytes = s.as_bytes();
        for i in 1..=bytes.len() {
            if i == bytes.len() || bytes[i].is_ascii_digit() != bytes[start].is_ascii_digit() {
                out.push((bytes[start].is_ascii_digit(), &s[start..i]));
                start = i;
            }
        }
        out
    }
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(&cb) {
        let ord = match (x, y) {
            ((true, x), (true, y)) => {
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                x.len().cmp(&y.len()).then(x.cmp(y))
            }
            ((_, x), (_, y)) => x.cmp(y),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    ca.len().cmp(&cb.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::testing::RecordingRunner;

    #[test]
    fn parses_distro_release_strings() {
        let cases = [
            ("7.1.2-arch3-1", 7, 1, Some(2), "7.1.2"),
            ("6.8.0-31-generic", 6, 8, Some(0), "6.8"),
            ("6.12.9-200.fc41.x86_64", 6, 12, Some(9), "6.12.9"),
            ("6.12.5-200.fc41.x86_64", 6, 12, Some(5), "6.12.5"),
            ("6.6.70", 6, 6, Some(70), "6.6.70"),
            ("6.8", 6, 8, None, "6.8"),
            ("6.8-rc1", 6, 8, None, "6.8"),
        ];
        for (rel, major, minor, sub, src) in cases {
            let kv = KernelVersion::parse(rel).unwrap();
            assert_eq!((kv.major, kv.minor, kv.sub), (major, minor, sub), "{rel}");
            assert_eq!(kv.source_version(), src, "{rel}");
        }
        assert!(KernelVersion::parse("garbage").is_none());
        assert!(KernelVersion::parse("6").is_none());
    }

    #[test]
    fn recent_threshold() {
        assert!(!KernelVersion::parse("6.17.9").unwrap().is_recent());
        assert!(KernelVersion::parse("6.18.0").unwrap().is_recent());
        assert!(KernelVersion::parse("7.0.1").unwrap().is_recent());
    }

    #[test]
    fn version_ordering_is_numeric() {
        assert_eq!(version_cmp("6.8.0-31-generic", "6.8.0-9-generic"), Ordering::Greater);
        assert_eq!(version_cmp("6.10.1", "6.9.12"), Ordering::Greater);
        assert_eq!(version_cmp("6.8.0", "6.8.0"), Ordering::Equal);
        assert_eq!(version_cmp("6.8", "6.8.1"), Ordering::Less);
    }

    #[test]
    fn debian_picks_newest_installed_image() {
        let dpkg = "\
ii  linux-image-6.8.0-9-generic   6.8.0-9.9    amd64  Signed kernel image
ii  linux-image-6.8.0-31-generic  6.8.0-31.31  amd64  Signed kernel image
rc  linux-image-6.9.0-1-generic   6.9.0-1.1    amd64  Removed, config left
ii  linux-image-generic           6.8.0.31.31  amd64  Generic Linux kernel image
";
        let runner = RecordingRunner::default().respond("dpkg -l", 0, dpkg);
        let kv = detect(None, Distro::Debian, Path::new("/nonexistent"), &runner).unwrap();
        assert_eq!(kv.release, "6.8.0-31-generic");
    }

    #[test]
    fn explicit_version_skips_detection() {
        let runner = RecordingRunner::default();
        let kv = detect(Some("6.12.9-200.fc41.x86_64"), Distro::Fedora, Path::new("/"), &runner).unwrap();
        assert_eq!(kv.source_version(), "6.12.9");
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn debian_sublevel_from_headers_makefile() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("linux-headers-6.8-common");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("Makefile"), "VERSION = 6\nPATCHLEVEL = 8\nSUBLEVEL = 12\n").unwrap();
        let hdr = root.path().join("linux-headers-6.8-amd64");
        fs::create_dir_all(&hdr).unwrap();
        fs::write(hdr.join("Makefile"), format!("include {}\n", real.join("Makefile").display())).unwrap();

        let kv = detect(Some("6.8-amd64"), Distro::Debian, root.path(), &RecordingRunner::default()).unwrap();
        assert_eq!(kv.sub, Some(12));
        assert_eq!(kv.source_version(), "6.8.12");
    }
}
