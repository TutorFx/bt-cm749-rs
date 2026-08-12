//! Installer for the DKMS-patched `btusb` module supporting Barrot BR8554 based
//! Bluetooth adapters (UGREEN CM748/CM749, USB 33fa:0010 and 33fa:0012).

pub mod context;
pub mod distro;
pub mod dkms;
pub mod error;
pub mod exec;
pub mod install;
pub mod kernel;
pub mod os_release;
pub mod patch;
pub mod prebuild;
pub mod signals;
pub mod source;
