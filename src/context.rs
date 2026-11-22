use std::env;
use std::path::PathBuf;

pub const MODULE_NAME: &str = "bt-cm749";
pub const MODULE_VERSION: &str = "0.3";
/// Versions registered in DKMS by the former shell implementation.
pub const LEGACY_MODULE_VERSIONS: &[&str] = &["0.2"];
/// Name of the binary copied into the DKMS source tree and invoked as PRE_BUILD.
pub const PREBUILD_BIN: &str = "bt-cm749";
pub const PATCH_FILE: &str = "bt-cm749.patch";
/// Upstream Barrot commit: device IDs and event-continuation handling.
pub const PATCH: &str = include_str!("../assets/bt-cm749.patch");
pub const LE_CODED_PATCH_FILE: &str = "btusb-barrot-le-coded.patch";
/// Keeps Barrot adapters off the LE Coded PHY, whose scans hang their firmware.
pub const LE_CODED_PATCH: &str = include_str!("../assets/btusb-barrot-le-coded.patch");
/// Patches written to the DKMS source tree. PRE_BUILD applies every `*.patch` there
/// in name order, which is also the order of this list.
pub const PATCHES: [(&str, &str); 2] = [(PATCH_FILE, PATCH), (LE_CODED_PATCH_FILE, LE_CODED_PATCH)];

/// Runtime configuration. Defaults point at the real system; the environment
/// variables honoured by the original scripts (OS_RELEASE_FILE, CUSTOM_USR_SRC,
/// KERNEL_VERSION, SKIP_ROOT_CHECK) override them so tests can run in a sandbox,
/// as do the BT_CM749_{CACHE_DIR,MODULES_ROOT,DKMS_ROOT,SYSFS_ROOT} variables.
#[derive(Debug, Clone)]
pub struct Context {
    pub os_release_file: PathBuf,
    pub usr_src: PathBuf,
    /// Where distro kernel headers live (`linux-headers-<kver>/` on Debian).
    pub headers_root: PathBuf,
    /// `/lib/modules` equivalent, used to inspect the distro's stock btusb module.
    pub modules_root: PathBuf,
    /// DKMS state tree, where displaced stock modules are kept.
    pub dkms_root: PathBuf,
    /// sysfs mount, used by the wizard to look for a plugged-in adapter.
    pub sysfs_root: PathBuf,
    pub kernel_version: Option<String>,
    pub skip_root_check: bool,
    /// Explicit kernel tarball cache, otherwise chosen at runtime.
    pub cache_dir: Option<PathBuf>,
}

impl Context {
    pub fn from_env() -> Self {
        let var = |name: &str| env::var(name).ok().filter(|v| !v.is_empty());
        Context {
            os_release_file: var("OS_RELEASE_FILE").map_or_else(|| "/etc/os-release".into(), PathBuf::from),
            usr_src: var("CUSTOM_USR_SRC").map_or_else(|| "/usr/src".into(), PathBuf::from),
            headers_root: "/usr/src".into(),
            modules_root: var("BT_CM749_MODULES_ROOT").map_or_else(|| "/usr/lib/modules".into(), PathBuf::from),
            dkms_root: var("BT_CM749_DKMS_ROOT").map_or_else(|| "/var/lib/dkms".into(), PathBuf::from),
            sysfs_root: var("BT_CM749_SYSFS_ROOT").map_or_else(|| "/sys".into(), PathBuf::from),
            kernel_version: var("KERNEL_VERSION"),
            skip_root_check: var("SKIP_ROOT_CHECK").is_some(),
            cache_dir: var("BT_CM749_CACHE_DIR").map(PathBuf::from),
        }
    }

    pub fn module_dir(&self) -> PathBuf {
        self.module_dir_for(MODULE_VERSION)
    }

    pub fn module_dir_for(&self, version: &str) -> PathBuf {
        self.usr_src.join(format!("{MODULE_NAME}-{version}"))
    }

    /// Whether a bt-cm749 DKMS source tree exists, current or from the shell version.
    pub fn has_installation(&self) -> bool {
        self.module_dir().exists() || LEGACY_MODULE_VERSIONS.iter().any(|v| self.module_dir_for(v).exists())
    }
}
