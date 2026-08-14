use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::context::Context;
use crate::error::{IoContext, Result};
use crate::exec::SystemRunner;
use crate::os_release::Distro;
use crate::{install, kernel, prebuild};

/// Installs a DKMS-patched btusb driver for Barrot BR8554 based Bluetooth adapters
/// (UGREEN CM748/CM749, USB 33fa:0010 / 33fa:0012).
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// os-release file used for distro detection [env: OS_RELEASE_FILE]
    #[arg(long, global = true, value_name = "PATH")]
    pub os_release: Option<PathBuf>,
    /// Base directory for DKMS module sources [env: CUSTOM_USR_SRC; default: /usr/src]
    #[arg(long, global = true, value_name = "DIR")]
    pub usr_src: Option<PathBuf>,
    /// Kernel tarball cache [env: BT_CM749_CACHE_DIR; default: /var/cache/dkms-kernel-src]
    #[arg(long, global = true, value_name = "DIR")]
    pub cache_dir: Option<PathBuf>,
    /// Do not require root [env: SKIP_ROOT_CHECK]
    #[arg(long, global = true)]
    pub skip_root_check: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build and install the patched btusb module through DKMS
    Install {
        /// Target kernel release [env: KERNEL_VERSION; default: newest installed]
        #[arg(long, short)]
        kernel: Option<String>,
    },
    /// Remove the DKMS module and restore the distribution's btusb driver
    Uninstall {
        #[arg(long, short)]
        kernel: Option<String>,
    },
    /// Show the detected distro and kernel versions
    Detect {
        #[arg(long, short)]
        kernel: Option<String>,
    },
    /// DKMS PRE_BUILD hook: fetch, extract and patch the driver sources
    #[command(hide = true)]
    Prebuild {
        /// Kernel being built [env: kernelver]
        #[arg(long, short, env = "kernelver")]
        kernel: Option<String>,
        /// Kernel source subdirectory to extract
        subdir: String,
    },
}

impl Cli {
    fn context(&self, kernel: Option<&String>) -> Context {
        let mut ctx = Context::from_env();
        if let Some(p) = &self.os_release {
            ctx.os_release_file = p.clone();
        }
        if let Some(p) = &self.usr_src {
            ctx.usr_src = p.clone();
        }
        if let Some(p) = &self.cache_dir {
            ctx.cache_dir = Some(p.clone());
        }
        if let Some(k) = kernel.filter(|k| !k.is_empty()) {
            ctx.kernel_version = Some(k.clone());
        }
        ctx.skip_root_check |= self.skip_root_check;
        ctx
    }
}

pub fn run(cli: Cli) -> Result<()> {
    let runner = SystemRunner;
    match &cli.command {
        Command::Install { kernel } => {
            let exe = std::env::current_exe().ctx(|| "locating own executable".into())?;
            install::install(&cli.context(kernel.as_ref()), &exe, &runner)
        }
        Command::Uninstall { kernel } => install::uninstall(&cli.context(kernel.as_ref()), &runner),
        Command::Detect { kernel } => {
            let ctx = cli.context(kernel.as_ref());
            let distro = Distro::detect(&ctx.os_release_file);
            let kv = kernel::detect(ctx.kernel_version.as_deref(), distro, &ctx.headers_root, &runner)?;
            println!("Distro family: {distro:?}");
            println!("{}", kv.describe());
            println!("Upstream tarball: linux-{}.tar.xz", kv.source_version());
            Ok(())
        }
        Command::Prebuild { kernel, subdir } => {
            let ctx = cli.context(kernel.as_ref());
            let cwd = std::env::current_dir().ctx(|| "reading current directory".into())?;
            prebuild::prebuild(&ctx, subdir, &cwd, &runner)
        }
    }
}
