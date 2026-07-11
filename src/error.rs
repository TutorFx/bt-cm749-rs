use std::io;

/// Every failure the tool can report. Exit codes mirror the original shell scripts:
/// 1 = not root / patch failure, 2 = kernel source unavailable, 3 = missing build
/// prerequisites, 4 = kernel version undetectable; failed child commands propagate
/// their own exit code and signals map to 128 + signo.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Only root can perform this {0}. Aborting.")]
    NotRoot(&'static str),

    #[error(
        "Determining the kernel version not (yet) supported for your Linux distro. \
         You might want to modify the distro-specific commands. Aborting."
    )]
    KernelVersion,

    #[error(
        "Determining the kernel subversion not (yet) supported for your Linux distro. \
         You might want to modify the distro-specific commands. Aborting."
    )]
    KernelSublevel,

    #[error("Could not download or find kernel source {0}")]
    Download(String),

    #[error("Failed to apply patch {0}")]
    Patch(String),

    #[error("Could not install {0}. Try installing it manually.")]
    MissingHeaders(String),

    #[error(
        "Your system uses version {0} of gcc by default, but version 12 is required as a minimum.\n    \
         You might want to install it with the following command:\n    sudo apt install gcc-12."
    )]
    GccTooOld(u32),

    #[error("command `{cmd}` failed with exit code {code}")]
    CommandFailed { cmd: String, code: i32 },

    #[error("could not run `{cmd}`: {source}")]
    Spawn { cmd: String, source: io::Error },

    #[error("interrupted by signal {0}")]
    Interrupted(i32),

    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
}

impl Error {
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::NotRoot(_) | Error::Patch(_) | Error::Io { .. } => 1,
            Error::Download(_) => 2,
            Error::MissingHeaders(_) | Error::GccTooOld(_) => 3,
            Error::KernelVersion | Error::KernelSublevel => 4,
            Error::CommandFailed { code, .. } => *code,
            Error::Spawn { .. } => 127,
            Error::Interrupted(sig) => 128 + sig,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Attaches a human-readable context to I/O errors.
pub trait IoContext<T> {
    fn ctx(self, context: impl FnOnce() -> String) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn ctx(self, context: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|source| Error::Io { context: context(), source })
    }
}
