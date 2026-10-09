use std::path::PathBuf;

use std::io::IsTerminal;

use clap::{CommandFactory, Parser, Subcommand};

use crate::context::Context;
use crate::error::{IoContext, Result};
use crate::exec::SystemRunner;
use crate::os_release::Distro;
use crate::stock::{self, StockDriver};
use crate::{install, kernel, prebuild, wizard};

/// Installs a DKMS-patched btusb driver for Barrot BR8554 based Bluetooth adapters
/// (UGREEN CM748/CM749, USB 33fa:0010 / 33fa:0012).
#[derive(Debug, Parser)]
#[command(
    version,
    about,
    after_help = "Run `sudo bt-cm749` without a command in a terminal for an interactive wizard."
)]
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
    /// Kernel modules root used to inspect the stock driver [env: BT_CM749_MODULES_ROOT; default: /usr/lib/modules]
    #[arg(long, global = true, value_name = "DIR")]
    pub modules_root: Option<PathBuf>,
    /// Do not require root [env: SKIP_ROOT_CHECK]
    #[arg(long, global = true)]
    pub skip_root_check: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build and install the patched btusb module through DKMS
    Install {
        /// Target kernel release [env: KERNEL_VERSION; default: newest installed]
        #[arg(long, short)]
        kernel: Option<String>,
        /// Install even if the kernel's stock btusb already supports the adapters
        #[arg(long)]
        force: bool,
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
        /// Only build these modules (repeatable); default: the whole directory
        #[arg(long, value_name = "MODULE")]
        only: Vec<String>,
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
        if let Some(p) = &self.modules_root {
            ctx.modules_root = p.clone();
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
    let Some(command) = &cli.command else {
        if interactive() {
            return wizard::run(&cli.context(None));
        }
        // Scripts and pipes get the usage instead of prompts they cannot answer.
        let _ = Cli::command().print_help();
        std::process::exit(2);
    };
    match command {
        Command::Install { kernel, force } => {
            let exe = std::env::current_exe().ctx(|| "locating own executable".into())?;
            install::install(&cli.context(kernel.as_ref()), &exe, *force, &runner)
        }
        Command::Uninstall { kernel } => install::uninstall(&cli.context(kernel.as_ref()), &runner),
        Command::Detect { kernel } => {
            let ctx = cli.context(kernel.as_ref());
            let distro = Distro::detect(&ctx.os_release_file);
            let kv = kernel::detect(ctx.kernel_version.as_deref(), distro, &ctx.headers_root, &runner)?;
            println!("Distro family: {distro:?}");
            println!("{}", kv.describe());
            println!("Upstream tarball: linux-{}.tar.xz", kv.source_version());
            match stock::inspect(&ctx.modules_root, &ctx.dkms_root, &kv.release) {
                StockDriver::Supported(p) => {
                    println!("Stock btusb: already supports 33fa:0010/0012 ({}); install not needed", p.display())
                }
                StockDriver::Missing(p) => {
                    println!("Stock btusb: lacks the Barrot fix ({}); install needed", p.display())
                }
                StockDriver::Unknown(reason) => println!("Stock btusb: unknown ({reason})"),
            }
            Ok(())
        }
        Command::Prebuild { kernel, only, subdir } => {
            let ctx = cli.context(kernel.as_ref());
            let cwd = std::env::current_dir().ctx(|| "reading current directory".into())?;
            prebuild::prebuild(&ctx, only, subdir, &cwd, &runner)
        }
    }
}

/// Prompts need a terminal to draw on (stderr) and to read keys from (/dev/tty).
fn interactive() -> bool {
    std::io::stderr().is_terminal() && std::fs::File::open("/dev/tty").is_ok()
}
