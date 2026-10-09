//! Distro-specific steps: build prerequisites, Debian kernel headers, initramfs.

use crate::error::{Error, Result};
use crate::exec::{Cmd, Runner};
use crate::os_release::Distro;
use crate::say;

/// Installs the build prerequisites (non-interactively, unlike the original scripts).
pub fn install_prerequisites(distro: Distro, runner: &dyn Runner) -> Result<()> {
    let cmd = match distro {
        Distro::Debian => Cmd::new("apt").args(["install", "-y", "build-essential", "dkms", "dwarves"]),
        Distro::Arch => {
            Cmd::new("pacman").args(["-S", "--needed", "--noconfirm", "pahole", "dkms", "base-devel", "linux-headers"])
        }
        Distro::Fedora => Cmd::new("dnf").args(["install", "-y", "dwarves", "dkms", "kernel-devel", "kernel-headers"]),
        Distro::Unknown => {
            say!(
                "Preparation steps not (yet) supported for your Linux distro. \
                 You might want to modify the distro-specific commands."
            );
            return Ok(());
        }
    };
    runner.run(&cmd)
}

/// On Debian/Ubuntu, makes sure `linux-headers-<kver>` is installed (exit 3 otherwise).
pub fn ensure_headers(distro: Distro, kernel_release: &str, runner: &dyn Runner) -> Result<()> {
    if distro != Distro::Debian {
        return Ok(());
    }
    let package = format!("linux-headers-{kernel_release}");
    if package_installed(&package, runner) {
        return Ok(());
    }
    say!(
        "Please consider installing the package linux-headers-generic to auto-install kernel headers with every new kernel."
    );
    say!("Installing {package} now.");
    runner.run(&Cmd::new("apt").args(["update", "-y"]))?;
    runner.run(&Cmd::new("apt").args(["install", "-y", &package]))?;
    if package_installed(&package, runner) { Ok(()) } else { Err(Error::MissingHeaders(package)) }
}

/// Exact package lookup through dpkg-query (the scripts grepped `dpkg -l`, which
/// also matched other packages containing the name).
fn package_installed(package: &str, runner: &dyn Runner) -> bool {
    runner
        .output(&Cmd::new("dpkg-query").args(["-W", "-f=${Status}", package]))
        .is_ok_and(|o| o.code == 0 && o.stdout.contains("install ok installed"))
}

/// Regenerates the initramfs with the distro's tool, if that tool is present.
pub fn update_initramfs(distro: Distro, kernel_release: &str, runner: &dyn Runner) -> Result<()> {
    let cmd = match distro {
        Distro::Debian => Cmd::new("update-initramfs").args(["-u", "-k", kernel_release]),
        Distro::Arch => Cmd::new("mkinitcpio").arg("-P"),
        Distro::Fedora => Cmd::new("dracut").args(["--regenerate-all", "--force", "--parallel"]),
        Distro::Unknown => return Ok(()),
    };
    if runner.exists(&cmd.program) { runner.run(&cmd) } else { Ok(()) }
}

/// `gcc -dumpversion` major below 12 requires `gcc-12`; returns the `CC=` override.
pub fn compiler_override(runner: &dyn Runner) -> Result<Option<String>> {
    let Ok(out) = runner.output(&Cmd::new("gcc").arg("-dumpversion")) else {
        return Ok(None);
    };
    let Some(major) = out.stdout.trim().split('.').next().and_then(|m| m.parse::<u32>().ok()) else {
        return Ok(None);
    };
    if major >= 12 {
        return Ok(None);
    }
    match runner.which("gcc-12") {
        Some(path) => Ok(Some(path.display().to_string())),
        None => Err(Error::GccTooOld(major)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::testing::RecordingRunner;

    #[test]
    fn prerequisite_commands_per_distro() {
        let cases = [
            (Distro::Debian, "apt install -y build-essential dkms dwarves"),
            (Distro::Arch, "pacman -S --needed --noconfirm pahole dkms base-devel linux-headers"),
            (Distro::Fedora, "dnf install -y dwarves dkms kernel-devel kernel-headers"),
        ];
        for (distro, expected) in cases {
            let r = RecordingRunner::default();
            install_prerequisites(distro, &r).unwrap();
            assert_eq!(r.calls(), [expected]);
        }
        let r = RecordingRunner::default();
        install_prerequisites(Distro::Unknown, &r).unwrap();
        assert!(r.calls().is_empty());
    }

    #[test]
    fn prerequisite_failure_propagates_exit_code() {
        let r = RecordingRunner::default().respond("pacman", 7, "");
        assert_eq!(install_prerequisites(Distro::Arch, &r).unwrap_err().exit_code(), 7);
    }

    #[test]
    fn debian_headers_present() {
        let r = RecordingRunner::default().respond("dpkg-query", 0, "install ok installed");
        ensure_headers(Distro::Debian, "6.8.0-31-generic", &r).unwrap();
        assert_eq!(r.calls(), ["dpkg-query -W -f=${Status} linux-headers-6.8.0-31-generic"]);
    }

    #[test]
    fn debian_headers_missing_and_uninstallable() {
        let r = RecordingRunner::default().respond("dpkg-query", 1, "");
        let err = ensure_headers(Distro::Debian, "6.8.0-31-generic", &r).unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert!(r.calls().contains(&"apt install -y linux-headers-6.8.0-31-generic".to_string()));
    }

    #[test]
    fn headers_only_checked_on_debian() {
        let r = RecordingRunner::default();
        ensure_headers(Distro::Arch, "7.1.2-arch3-1", &r).unwrap();
        assert!(r.calls().is_empty());
    }

    #[test]
    fn initramfs_commands_and_missing_tool() {
        let cases = [
            (Distro::Debian, "update-initramfs -u -k 6.8.0-31-generic"),
            (Distro::Arch, "mkinitcpio -P"),
            (Distro::Fedora, "dracut --regenerate-all --force --parallel"),
        ];
        for (distro, expected) in cases {
            let r = RecordingRunner::default();
            update_initramfs(distro, "6.8.0-31-generic", &r).unwrap();
            assert_eq!(r.calls(), [expected]);
        }
        let r = RecordingRunner::default().missing("mkinitcpio");
        update_initramfs(Distro::Arch, "x", &r).unwrap();
        assert!(r.calls().is_empty());
    }

    #[test]
    fn modern_gcc_needs_no_override() {
        let r = RecordingRunner::default().respond("gcc -dumpversion", 0, "14.2.1\n");
        assert_eq!(compiler_override(&r).unwrap(), None);
    }

    #[test]
    fn old_gcc_without_gcc12_fails_with_3() {
        let r = RecordingRunner::default().respond("gcc -dumpversion", 0, "9.3.0\n").missing("gcc-12");
        assert_eq!(compiler_override(&r).unwrap_err().exit_code(), 3);
    }

    #[test]
    fn old_gcc_with_gcc12_overrides_cc() {
        let r = RecordingRunner::default().respond("gcc -dumpversion", 0, "11\n");
        assert_eq!(compiler_override(&r).unwrap().as_deref(), Some("/usr/bin/gcc-12"));
    }
}
