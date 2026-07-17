//! Installer for the DKMS-patched `btusb` module supporting Barrot BR8554 based
//! Bluetooth adapters (UGREEN CM748/CM749, USB 33fa:0010 and 33fa:0012).

pub mod context;
pub mod error;
pub mod exec;
pub mod os_release;
pub mod signals;
