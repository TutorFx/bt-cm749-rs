//! Inspects the distribution's own btusb module. Most current kernels (upstream
//! since 6.18/7.x, and distro backports such as Ubuntu 6.8.0-146) already support the
//! Barrot adapters, but none keeps them off the LE Coded PHY yet, so the DKMS module
//! is only unnecessary when the stock driver has both fixes.
//!
//! The second fix is recognised by the message of our own patch, which no stock driver
//! logs: until a kernel carries an equivalent fix, [`StockDriver::Supported`] is not
//! reported for real systems. When upstream fixes the hang, its own marker (or the
//! kernel version it landed in) has to be added here, or the module keeps being
//! installed where it is no longer needed.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::context::MODULE_NAME;

/// `struct usb_device_id` entries as compiled into the quirks table:
/// match_flags = USB_DEVICE_ID_MATCH_DEVICE (0x0003), idVendor, idProduct (little endian).
const DEVICE_IDS: [&[u8]; 2] = [&[0x03, 0x00, 0xfa, 0x33, 0x10, 0x00], &[0x03, 0x00, 0xfa, 0x33, 0x12, 0x00]];
/// Warning string added by the upstream patch's event-continuation handling.
const CONTINUATION_FIX: &[u8] = b"Unexpected continuation";
/// Message logged by the LE Coded PHY workaround (`assets/btusb-barrot-le-coded.patch`).
/// It only identifies that patch: an upstream fix will not log it (see the module docs).
const LE_CODED_FIX: &[u8] = b"Barrot: LE Coded PHY disabled";

#[derive(Debug, PartialEq, Eq)]
pub enum StockDriver {
    /// The stock module has every fix: nothing to install.
    Supported(PathBuf),
    /// The stock module supports the adapters (device IDs and event-continuation fix)
    /// but lacks the workaround for the firmware hang during LE scans.
    Partial(PathBuf),
    /// The stock module does not support the adapters.
    Missing(PathBuf),
    /// No loadable stock module found (headers-only system, built-in driver) or unreadable.
    Unknown(String),
}

const NAMES: [&str; 4] = ["btusb.ko", "btusb.ko.zst", "btusb.ko.xz", "btusb.ko.gz"];

/// Looks for `<modules_root>/<release>/kernel/drivers/bluetooth/btusb.ko[.zst|.xz|.gz]`,
/// or for the copy DKMS keeps in `<dkms_root>/<module>/original_module/<release>/<arch>/`
/// when an installed DKMS module displaced it. Our own module (under `updates/` or
/// `extra/`) is never inspected.
pub fn inspect(modules_root: &Path, dkms_root: &Path, release: &str) -> StockDriver {
    let in_tree = modules_root.join(release).join("kernel/drivers/bluetooth");
    let backups = std::fs::read_dir(dkms_root.join(MODULE_NAME).join("original_module").join(release))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path());
    let dirs: Vec<PathBuf> = std::iter::once(in_tree.clone()).chain(backups).collect();
    let Some(path) = dirs.iter().flat_map(|d| NAMES.iter().map(move |n| d.join(n))).find(|p| p.is_file()) else {
        return StockDriver::Unknown(format!("no stock btusb module in {}", in_tree.display()));
    };
    match read_module(&path) {
        Ok(bytes) if !supports_adapters(&bytes) => StockDriver::Missing(path),
        Ok(bytes) if contains(&bytes, LE_CODED_FIX) => StockDriver::Supported(path),
        Ok(_) => StockDriver::Partial(path),
        Err(e) => StockDriver::Unknown(format!("could not read {}: {e}", path.display())),
    }
}

/// Device IDs in the quirks table and the event-continuation fix.
pub fn supports_adapters(module: &[u8]) -> bool {
    DEVICE_IDS.iter().all(|id| contains(module, id)) && contains(module, CONTINUATION_FIX)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn read_module(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut reader: Box<dyn Read> = match path.extension().and_then(|e| e.to_str()) {
        Some("zst") => Box::new(ruzstd::decoding::StreamingDecoder::new(file).map_err(io::Error::other)?),
        Some("xz") => Box::new(liblzma::read::XzDecoder::new(file)),
        Some("gz") => Box::new(flate2::read::GzDecoder::new(file)),
        _ => Box::new(file),
    };
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
pub mod tests {
    use std::fs;
    use std::io::Write;

    use super::*;

    /// Fake module content: `adapters` adds the upstream fix markers, `le_coded` the
    /// LE Coded PHY workaround's message.
    pub fn module_bytes(adapters: bool, le_coded: bool) -> Vec<u8> {
        let mut m = b"\x7fELF...btusb...".to_vec();
        if adapters {
            for id in DEVICE_IDS {
                m.extend_from_slice(&[0, 0]);
                m.extend_from_slice(id);
            }
            m.extend_from_slice(b"\0Unexpected continuation: %d bytes\0");
        }
        if le_coded {
            m.extend_from_slice(b"\x016Barrot: LE Coded PHY disabled to avoid a firmware hang\n\0");
        }
        m
    }

    fn write_module(root: &Path, release: &str, name: &str, content: &[u8]) {
        let dir = root.join(release).join("kernel/drivers/bluetooth");
        fs::create_dir_all(&dir).unwrap();
        let data = match name.rsplit('.').next() {
            Some("zst") => ruzstd::encoding::compress_to_vec(content, ruzstd::encoding::CompressionLevel::Fastest),
            Some("xz") => {
                let mut enc = liblzma::write::XzEncoder::new(Vec::new(), 1);
                enc.write_all(content).unwrap();
                enc.finish().unwrap()
            }
            Some("gz") => {
                let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
                enc.write_all(content).unwrap();
                enc.finish().unwrap()
            }
            _ => content.to_vec(),
        };
        fs::write(dir.join(name), data).unwrap();
    }

    #[test]
    fn detects_fix_in_every_compression() {
        for name in ["btusb.ko", "btusb.ko.zst", "btusb.ko.xz", "btusb.ko.gz"] {
            let root = tempfile::tempdir().unwrap();
            write_module(root.path(), "7.0.0-38-generic", name, &module_bytes(true, true));
            assert!(
                matches!(
                    inspect(root.path(), Path::new("/nonexistent"), "7.0.0-38-generic"),
                    StockDriver::Supported(_)
                ),
                "{name}"
            );
        }
    }

    #[test]
    fn missing_fix() {
        let root = tempfile::tempdir().unwrap();
        write_module(root.path(), "6.14.0-37-generic", "btusb.ko.zst", &module_bytes(false, false));
        assert!(matches!(
            inspect(root.path(), Path::new("/nonexistent"), "6.14.0-37-generic"),
            StockDriver::Missing(_)
        ));
    }

    /// Every current kernel: upstream supports the adapters, but not the LE Coded PHY fix.
    #[test]
    fn adapter_support_without_le_coded_fix_is_partial() {
        let root = tempfile::tempdir().unwrap();
        write_module(root.path(), "7.2.11-arch1-1", "btusb.ko.zst", &module_bytes(true, false));
        assert!(matches!(inspect(root.path(), Path::new("/nonexistent"), "7.2.11-arch1-1"), StockDriver::Partial(_)));
    }

    /// The LE Coded PHY message alone means nothing without adapter support.
    #[test]
    fn le_coded_fix_without_adapter_support_is_missing() {
        let root = tempfile::tempdir().unwrap();
        write_module(root.path(), "6.12.10-arch1-1", "btusb.ko", &module_bytes(false, true));
        assert!(matches!(inspect(root.path(), Path::new("/nonexistent"), "6.12.10-arch1-1"), StockDriver::Missing(_)));
    }

    #[test]
    fn partial_adapter_support_is_not_enough() {
        let mut m = module_bytes(false, false);
        m.extend_from_slice(DEVICE_IDS[0]);
        m.extend_from_slice(CONTINUATION_FIX);
        assert!(!supports_adapters(&m));
    }

    #[test]
    fn falls_back_to_dkms_original_module_backup() {
        let root = tempfile::tempdir().unwrap();
        let backup = root.path().join("dkms/bt-cm749/original_module/7.1.2-arch3-1/x86_64");
        fs::create_dir_all(&backup).unwrap();
        fs::write(
            backup.join("btusb.ko.zst"),
            ruzstd::encoding::compress_to_vec(
                &module_bytes(true, false)[..],
                ruzstd::encoding::CompressionLevel::Fastest,
            ),
        )
        .unwrap();
        let found = inspect(&root.path().join("modules"), &root.path().join("dkms"), "7.1.2-arch3-1");
        assert_eq!(found, StockDriver::Partial(backup.join("btusb.ko.zst")));
    }

    #[test]
    fn no_module_is_unknown() {
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(inspect(root.path(), Path::new("/nonexistent"), "7.1.2-arch3-1"), StockDriver::Unknown(_)));
    }

    #[test]
    fn corrupt_module_is_unknown() {
        let root = tempfile::tempdir().unwrap();
        write_module(root.path(), "k", "btusb.ko.xz", b"x");
        let dir = root.path().join("k/kernel/drivers/bluetooth");
        fs::write(dir.join("btusb.ko.xz"), b"not xz").unwrap();
        assert!(matches!(inspect(root.path(), Path::new("/nonexistent"), "k"), StockDriver::Unknown(_)));
    }

    #[test]
    fn real_modules_on_this_machine_do_not_crash() {
        let release = rustix::system::uname().release().to_string_lossy().into_owned();
        let _ = inspect(Path::new("/usr/lib/modules"), Path::new("/var/lib/dkms"), &release);
    }
}
