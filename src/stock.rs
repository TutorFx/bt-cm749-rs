//! Inspects the distribution's own btusb module: most current kernels (upstream
//! since 6.18/7.x, and distro backports such as Ubuntu 6.8.0-146) already carry the
//! Barrot fix, in which case installing the DKMS module is unnecessary.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// `struct usb_device_id` entries as compiled into the quirks table:
/// match_flags = USB_DEVICE_ID_MATCH_DEVICE (0x0003), idVendor, idProduct (little endian).
const DEVICE_IDS: [&[u8]; 2] = [&[0x03, 0x00, 0xfa, 0x33, 0x10, 0x00], &[0x03, 0x00, 0xfa, 0x33, 0x12, 0x00]];
/// Warning string added by the patch's event-continuation handling.
const CONTINUATION_FIX: &[u8] = b"Unexpected continuation";

#[derive(Debug, PartialEq, Eq)]
pub enum StockDriver {
    /// The stock module already supports the Barrot adapters.
    Supported(PathBuf),
    /// The stock module lacks the fix.
    Missing(PathBuf),
    /// No loadable stock module found (headers-only system, built-in driver) or unreadable.
    Unknown(String),
}

/// Looks for `<modules_root>/<release>/kernel/drivers/bluetooth/btusb.ko[.zst|.xz|.gz]`.
/// Our own DKMS module lives under `updates/` or `extra/` and is never inspected.
pub fn inspect(modules_root: &Path, release: &str) -> StockDriver {
    let dir = modules_root.join(release).join("kernel/drivers/bluetooth");
    let Some(path) = ["btusb.ko", "btusb.ko.zst", "btusb.ko.xz", "btusb.ko.gz"]
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
    else {
        return StockDriver::Unknown(format!("no stock btusb module in {}", dir.display()));
    };
    match read_module(&path) {
        Ok(bytes) if has_fix(&bytes) => StockDriver::Supported(path),
        Ok(_) => StockDriver::Missing(path),
        Err(e) => StockDriver::Unknown(format!("could not read {}: {e}", path.display())),
    }
}

pub fn has_fix(module: &[u8]) -> bool {
    let contains = |needle: &[u8]| module.windows(needle.len()).any(|w| w == needle);
    DEVICE_IDS.iter().all(|id| contains(id)) && contains(CONTINUATION_FIX)
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

    /// Fake module content with or without the fix markers.
    pub fn module_bytes(fixed: bool) -> Vec<u8> {
        let mut m = b"\x7fELF...btusb...".to_vec();
        if fixed {
            for id in DEVICE_IDS {
                m.extend_from_slice(&[0, 0]);
                m.extend_from_slice(id);
            }
            m.extend_from_slice(b"\0Unexpected continuation: %d bytes\0");
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
            write_module(root.path(), "7.0.0-38-generic", name, &module_bytes(true));
            assert!(matches!(inspect(root.path(), "7.0.0-38-generic"), StockDriver::Supported(_)), "{name}");
        }
    }

    #[test]
    fn missing_fix() {
        let root = tempfile::tempdir().unwrap();
        write_module(root.path(), "6.14.0-37-generic", "btusb.ko.zst", &module_bytes(false));
        assert!(matches!(inspect(root.path(), "6.14.0-37-generic"), StockDriver::Missing(_)));
    }

    #[test]
    fn partial_fix_is_not_enough() {
        let mut m = module_bytes(false);
        m.extend_from_slice(DEVICE_IDS[0]);
        m.extend_from_slice(CONTINUATION_FIX);
        assert!(!has_fix(&m));
    }

    #[test]
    fn no_module_is_unknown() {
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(inspect(root.path(), "7.1.2-arch3-1"), StockDriver::Unknown(_)));
    }

    #[test]
    fn corrupt_module_is_unknown() {
        let root = tempfile::tempdir().unwrap();
        write_module(root.path(), "k", "btusb.ko.xz", b"x");
        let dir = root.path().join("k/kernel/drivers/bluetooth");
        fs::write(dir.join("btusb.ko.xz"), b"not xz").unwrap();
        assert!(matches!(inspect(root.path(), "k"), StockDriver::Unknown(_)));
    }

    #[test]
    fn real_modules_on_this_machine_do_not_crash() {
        let release = rustix::system::uname().release().to_string_lossy().into_owned();
        let _ = inspect(Path::new("/usr/lib/modules"), &release);
    }
}
