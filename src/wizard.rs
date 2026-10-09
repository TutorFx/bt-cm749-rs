//! Interactive wizard, shown when `bt-cm749` runs without a subcommand in a terminal.
//!
//! It shows what was detected, offers only the actions that make sense, and runs them
//! behind one spinner per step. Command output goes to a log file, which is shown
//! only when something fails.

use std::cell::{Cell, RefCell};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use cliclack::{ProgressBar, intro, log, note, outro, outro_cancel, select, spinner};
use console::style;

use crate::context::{Context, LEGACY_MODULE_VERSIONS};
use crate::error::{Error, IoContext, Result};
use crate::exec::{Cmd, LoggingRunner, Runner, SystemRunner};
use crate::os_release::{self, Distro};
use crate::stock::{self, StockDriver};
use crate::{install, kernel, ui};

const ISSUES: &str = "https://github.com/TutorFx/bt-cm749-rs/issues";
const LOG_FILE: &str = "/var/log/bt-cm749.log";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Install,
    Reinstall,
    Uninstall,
    Exit,
}

/// What the menu is based on.
#[derive(Debug, Clone, Copy)]
pub struct State {
    /// Whether the kernel's own btusb has the fix (`None`: could not tell).
    pub stock_fixed: Option<bool>,
    /// Whether a bt-cm749 DKMS installation (current or legacy) exists.
    pub installed: bool,
}

/// Menu entries (action, label, hint) and the preselected action.
pub fn choices(state: State) -> (Vec<(Action, &'static str, &'static str)>, Action) {
    match (state.installed, state.stock_fixed) {
        (true, Some(true)) => (
            vec![
                (Action::Uninstall, "Remove the fix", "recommended: your kernel no longer needs it"),
                (Action::Reinstall, "Reinstall the fix", ""),
                (Action::Exit, "Exit", "keep everything as it is"),
            ],
            Action::Uninstall,
        ),
        (true, _) => (
            vec![
                (Action::Exit, "Exit", "the fix is installed; keep it"),
                (Action::Reinstall, "Reinstall the fix", "rebuild it for the current kernel"),
                (Action::Uninstall, "Remove the fix", "go back to your distribution's driver"),
            ],
            Action::Exit,
        ),
        (false, Some(true)) => (
            vec![
                (Action::Exit, "Exit", "nothing to do: your kernel already supports the adapter"),
                (Action::Install, "Install anyway", "not needed"),
            ],
            Action::Exit,
        ),
        (false, _) => {
            (vec![(Action::Install, "Install the fix", "recommended"), (Action::Exit, "Exit", "")], Action::Install)
        }
    }
}

/// `vendor:product` of a plugged-in Barrot adapter, read from sysfs.
pub fn find_adapter(sysfs: &Path) -> Option<String> {
    fs::read_dir(sysfs.join("bus/usb/devices")).ok()?.flatten().find_map(|dev| {
        let read = |name: &str| fs::read_to_string(dev.path().join(name)).ok().map(|s| s.trim().to_string());
        let (vendor, product) = (read("idVendor")?, read("idProduct")?);
        (vendor == "33fa" && matches!(product.as_str(), "0010" | "0012")).then(|| format!("{vendor}:{product}"))
    })
}

/// Friendly description of what a command is doing, `None` for quick lookups.
pub fn describe(cmd: &Cmd) -> Option<String> {
    let first = cmd.args.first().map(String::as_str).unwrap_or_default();
    let text = match (cmd.program.as_str(), first) {
        ("apt", "update") => "Updating the package lists".into(),
        ("apt" | "pacman" | "dnf", _) => format!("Installing build tools ({})", cmd.program),
        ("dpkg-query", _) => "Checking the kernel headers".into(),
        ("gcc", _) => "Checking the compiler".into(),
        ("dkms", "remove") => "Removing the previous installation".into(),
        ("dkms", "build") => "Downloading the kernel source and building the driver (a few minutes)".into(),
        ("dkms", "install") => "Installing the driver".into(),
        ("update-initramfs" | "mkinitcpio" | "dracut", _) => "Updating the boot image".into(),
        ("depmod", _) => "Updating the module index".into(),
        ("modprobe", "-r") => "Unloading the Bluetooth driver".into(),
        ("modprobe", _) => "Loading the Bluetooth driver".into(),
        _ => return None,
    };
    Some(text)
}

/// One spinner per step; finished steps stay on screen with a check mark.
#[derive(Default)]
struct Steps {
    current: RefCell<Option<(ProgressBar, String)>>,
}

impl Steps {
    fn step(&self, message: &str) {
        let mut current = self.current.borrow_mut();
        if current.as_ref().is_some_and(|(_, m)| m == message) {
            return;
        }
        if let Some((bar, done)) = current.take() {
            bar.stop(done);
        }
        let bar = spinner();
        bar.start(message);
        *current = Some((bar, message.to_string()));
    }

    fn finish(&self) {
        if let Some((bar, done)) = self.current.borrow_mut().take() {
            bar.stop(done);
        }
    }

    fn fail(&self) {
        if let Some((bar, failed)) = self.current.borrow_mut().take() {
            bar.error(failed);
        }
    }
}

/// During an install, `dkms remove ... --force` only comes from the rollback (the
/// pre-install cleanup omits `--force`).
fn is_rollback(cmd: &Cmd) -> bool {
    cmd.program == "dkms" && cmd.args.first().is_some_and(|a| a == "remove") && cmd.args.iter().any(|a| a == "--force")
}

fn ui_err(e: io::Error) -> Error {
    Error::Io { context: "terminal".into(), source: e }
}

pub fn run(ctx: &Context) -> Result<()> {
    intro(style(" bt-cm749 ").on_cyan().black().to_string() + " Bluetooth fix for UGREEN CM748 / CM749")
        .map_err(ui_err)?;

    let distro = Distro::detect(&ctx.os_release_file);
    let pretty = fs::read_to_string(&ctx.os_release_file)
        .ok()
        .and_then(|s| os_release::parse(&s).remove("PRETTY_NAME"))
        .unwrap_or_else(|| format!("{distro:?}"));
    let kv = match kernel::detect(ctx.kernel_version.as_deref(), distro, &ctx.headers_root, &SystemRunner) {
        Ok(kv) => kv,
        Err(e) => {
            outro_cancel(format!("Could not determine the kernel version: {e}")).map_err(ui_err)?;
            return Err(Error::Shown(e.exit_code()));
        }
    };
    let stock = stock::inspect(&ctx.modules_root, &ctx.dkms_root, &kv.release);
    let adapter = find_adapter(&ctx.sysfs_root);
    let installed = ctx.module_dir().exists() || LEGACY_MODULE_VERSIONS.iter().any(|v| ctx.module_dir_for(v).exists());

    let stock_line = match &stock {
        StockDriver::Supported(_) => "already supports the adapter".to_string(),
        StockDriver::Missing(_) => "lacks the fix".to_string(),
        StockDriver::Unknown(_) => "could not be checked".to_string(),
    };
    note(
        "Your system",
        format!(
            "Distribution:   {pretty}\nKernel:         {}\nAdapter:        {}\nKernel driver:  {stock_line}\nbt-cm749 fix:   {}",
            kv.release,
            adapter.as_deref().map_or("not plugged in".to_string(), |id| format!("{id} plugged in")),
            if installed { "installed" } else { "not installed" },
        ),
    )
    .map_err(ui_err)?;
    if adapter.is_none() {
        log::warning("No UGREEN CM748/CM749 (33fa:0010 / 33fa:0012) is plugged in. You can still install the fix; it is used as soon as you plug the adapter in.")
            .map_err(ui_err)?;
    }

    let state = State {
        stock_fixed: match stock {
            StockDriver::Supported(_) => Some(true),
            StockDriver::Missing(_) => Some(false),
            StockDriver::Unknown(_) => None,
        },
        installed,
    };
    let (items, default) = choices(state);
    let action = match select("What do you want to do?").items(&items).initial_value(default).interact() {
        Ok(action) => action,
        Err(e) if e.kind() == io::ErrorKind::Interrupted => {
            outro_cancel("Cancelled. Nothing was changed.").map_err(ui_err)?;
            return Ok(());
        }
        Err(e) => return Err(ui_err(e)),
    };
    if action == Action::Exit {
        outro("Nothing was changed.").map_err(ui_err)?;
        return Ok(());
    }
    if install::check_root(ctx, "setup").is_err() {
        outro_cancel("Administrator rights are needed. Run it again with: sudo bt-cm749").map_err(ui_err)?;
        return Err(Error::Shown(1));
    }

    let (log_path, log) = open_log()?;
    let steps = Steps::default();
    let rolling_back = Cell::new(false);
    let installing = action != Action::Uninstall;
    let runner = LoggingRunner::new(log.try_clone().ctx(|| "opening the log".into())?, |cmd: &Cmd| {
        if rolling_back.get() {
            return;
        }
        if installing && is_rollback(cmd) {
            rolling_back.set(true);
            steps.fail();
            steps.step("Undoing the changes");
        } else if let Some(text) = describe(cmd) {
            steps.step(&text);
        }
    });
    ui::redirect(Some(log.try_clone().ctx(|| "opening the log".into())?));
    steps.step("Preparing");
    let result = match action {
        Action::Install | Action::Reinstall => env::current_exe()
            .ctx(|| "locating own executable".into())
            .and_then(|exe| install::install(ctx, &exe, true, &runner)),
        Action::Uninstall => install::uninstall(ctx, &runner),
        Action::Exit => unreachable!(),
    };
    let reload = match (&result, action) {
        (Ok(()), Action::Install | Action::Reinstall) => Some(reload_driver(&runner, &log_path)),
        _ => None,
    };
    ui::redirect(None);

    if let Err(err) = result {
        if rolling_back.get() {
            steps.finish()
        } else {
            steps.fail()
        }
        let tail = fs::read_to_string(&log_path).unwrap_or_default();
        let tail: Vec<&str> = tail.lines().rev().take(15).collect();
        let tail: Vec<&str> = tail.into_iter().rev().skip_while(|l| l.trim().is_empty()).collect();
        note("Last lines of the log", tail.join("\n")).map_err(ui_err)?;
        let what = if action == Action::Uninstall { "Removing the fix" } else { "The installation" };
        let undo = if action == Action::Uninstall { "" } else { " and was undone, so your system is unchanged" };
        outro_cancel(format!(
            "{what} failed{undo}: {err}\nFull log: {}\nPlease report it at {ISSUES}",
            log_path.display()
        ))
        .map_err(ui_err)?;
        return Err(Error::Shown(err.exit_code()));
    }
    steps.finish();

    match reload {
        None => outro("Done. Your distribution's original Bluetooth driver is active again."),
        Some(Reload::Loaded) => outro("Done! Unplug the adapter, plug it back in and turn Bluetooth on in your settings."),
        Some(Reload::NeedsRestart) => outro("Done! Restart your computer to start using the fixed driver."),
        Some(Reload::SecureBoot) => {
            note(
                "Secure Boot",
                "Secure Boot is blocking the new driver. To allow it, run:\n\n    sudo mokutil --import /var/lib/dkms/mok.pub\n\n\
                 Choose a one-time password, restart, pick \"Enroll MOK\" on the blue screen\n\
                 that appears during startup and type the same password.",
            )
            .map_err(ui_err)?;
            outro("Almost done: enroll the key as shown above.")
        }
    }
    .map_err(ui_err)
}

enum Reload {
    Loaded,
    NeedsRestart,
    SecureBoot,
}

fn reload_driver(runner: &LoggingRunner<'_>, log_path: &Path) -> Reload {
    if !runner.exists("modprobe") {
        return Reload::NeedsRestart;
    }
    let before = fs::metadata(log_path).map(|m| m.len()).unwrap_or(0);
    let unloaded = matches!(runner.status(&Cmd::new("modprobe").args(["-r", "btusb"])), Ok(0));
    let loaded = matches!(runner.status(&Cmd::new("modprobe").arg("btusb")), Ok(0));
    if unloaded && loaded {
        return Reload::Loaded;
    }
    let output = fs::read(log_path).unwrap_or_default();
    let new_output = String::from_utf8_lossy(output.get(before as usize..).unwrap_or_default());
    if new_output.contains("Key was rejected") { Reload::SecureBoot } else { Reload::NeedsRestart }
}

/// `/var/log/bt-cm749.log`, or a file in the temp dir when that is not writable.
fn open_log() -> Result<(PathBuf, File)> {
    let open = |p: &Path| OpenOptions::new().create(true).append(true).open(p);
    if let Ok(file) = open(Path::new(LOG_FILE)) {
        return Ok((LOG_FILE.into(), file));
    }
    let path = env::temp_dir().join("bt-cm749.log");
    let file = open(&path).ctx(|| format!("opening {}", path.display()))?;
    Ok((path, file))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(stock_fixed: Option<bool>, installed: bool) -> State {
        State { stock_fixed, installed }
    }

    #[test]
    fn menu_follows_the_situation() {
        let cases = [
            (state(Some(false), false), Action::Install, vec![Action::Install, Action::Exit]),
            (state(None, false), Action::Install, vec![Action::Install, Action::Exit]),
            (state(Some(true), false), Action::Exit, vec![Action::Exit, Action::Install]),
            (state(Some(true), true), Action::Uninstall, vec![Action::Uninstall, Action::Reinstall, Action::Exit]),
            (state(Some(false), true), Action::Exit, vec![Action::Exit, Action::Reinstall, Action::Uninstall]),
        ];
        for (st, default, actions) in cases {
            let (items, preselected) = choices(st);
            assert_eq!(preselected, default, "{st:?}");
            assert_eq!(items.iter().map(|i| i.0).collect::<Vec<_>>(), actions, "{st:?}");
        }
    }

    #[test]
    fn finds_a_plugged_adapter_in_sysfs() {
        let sys = tempfile::tempdir().unwrap();
        let add = |name: &str, vendor: &str, product: &str| {
            let dev = sys.path().join("bus/usb/devices").join(name);
            fs::create_dir_all(&dev).unwrap();
            fs::write(dev.join("idVendor"), format!("{vendor}\n")).unwrap();
            fs::write(dev.join("idProduct"), format!("{product}\n")).unwrap();
        };
        add("1-1", "1d6b", "0002");
        assert_eq!(find_adapter(sys.path()), None);
        add("1-2", "33fa", "0012");
        assert_eq!(find_adapter(sys.path()).as_deref(), Some("33fa:0012"));
        assert_eq!(find_adapter(Path::new("/nonexistent")), None);
    }

    #[test]
    fn describes_long_running_commands() {
        let d = |program: &str, args: &[&str]| describe(&Cmd::new(program).args(args.iter().copied()));
        assert_eq!(d("pacman", &["-S", "dkms"]).unwrap(), "Installing build tools (pacman)");
        assert_eq!(d("apt", &["update", "-y"]).unwrap(), "Updating the package lists");
        assert!(d("dkms", &["build", "-k", "x"]).unwrap().starts_with("Downloading the kernel source"));
        assert_eq!(d("modprobe", &["-r", "btusb"]).unwrap(), "Unloading the Bluetooth driver");
        assert_eq!(d("dkms", &["status"]), None);
        assert_eq!(d("uname", &[]), None);
    }

    #[test]
    fn recognises_the_rollback() {
        assert!(is_rollback(&Cmd::new("dkms").args(["remove", "bt-cm749/0.3", "--all", "--force"])));
        assert!(!is_rollback(&Cmd::new("dkms").args(["remove", "bt-cm749/0.3", "--all"])));
        assert!(!is_rollback(&Cmd::new("modprobe").args(["-r", "btusb"])));
    }
}
