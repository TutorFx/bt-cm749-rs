use std::env;
use std::path::PathBuf;

pub const MODULE_NAME: &str = "bt-cm749";
pub const MODULE_VERSION: &str = "0.3";
/// Versions registered in DKMS by the former shell implementation.
pub const LEGACY_MODULE_VERSIONS: &[&str] = &["0.2"];
/// Name of the binary copied into the DKMS source tree and invoked as PRE_BUILD.
pub const PREBUILD_BIN: &str = "bt-cm749";
pub const PATCH_FILE: &str = "bt-cm749.patch";
pub const PATCH: &str = include_str!("../assets/bt-cm749.patch");

/// Runtime configuration. Defaults point at the real system; the environment
/// variables honoured by the original scripts (OS_RELEASE_FILE, CUSTOM_USR_SRC,
/// KERNEL_VERSION, SKIP_ROOT_CHECK) override them so tests can run in a sandbox,
/// as do BT_CM749_CACHE_DIR, BT_CM749_MODULES_ROOT and BT_CM749_DKMS_ROOT.
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
}
