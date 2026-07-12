use std::env;
use std::path::PathBuf;

pub const MODULE_NAME: &str = "bt-cm749";
pub const MODULE_VERSION: &str = "0.3";

/// Runtime configuration. Defaults point at the real system; the environment
/// variables honoured by the original scripts (OS_RELEASE_FILE, CUSTOM_USR_SRC,
/// KERNEL_VERSION, SKIP_ROOT_CHECK) override them so tests can run in a sandbox.
#[derive(Debug, Clone)]
pub struct Context {
    pub os_release_file: PathBuf,
    pub usr_src: PathBuf,
    /// Where distro kernel headers live (`linux-headers-<kver>/` on Debian).
    pub headers_root: PathBuf,
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
