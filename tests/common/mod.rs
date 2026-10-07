//! Sandbox for end-to-end runs of the binary (port of tests/framework/test_helpers.sh).
//!
//! PATH contains *only* the mock directory, so no real dkms, modprobe or package
//! manager can ever be reached; every mock appends its command line to a log.

#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

pub const OS_ARCH: &str = "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\nBUILD_ID=rolling\n";
pub const OS_UBUNTU: &str =
    "NAME=\"Ubuntu\"\nVERSION=\"24.04 LTS (Noble Numbat)\"\nID=ubuntu\nID_LIKE=debian\nVERSION_ID=\"24.04\"\n";
pub const OS_FEDORA: &str = "NAME=\"Fedora Linux\"\nVERSION=\"41 (Workstation Edition)\"\nID=fedora\nVERSION_ID=41\n";

pub struct Sandbox {
    pub dir: TempDir,
    kernel: Option<String>,
}

impl Sandbox {
    pub fn new(os_release: &str, kernel: &str) -> Self {
        let sb = Sandbox {
            dir: tempfile::Builder::new().prefix("bt_test_sandbox_").tempdir().unwrap(),
            kernel: Some(kernel.into()),
        };
        for d in ["mock_bin", "etc", "usr/src", "cache", "lib/modules"] {
            fs::create_dir_all(sb.path(d)).unwrap();
        }
        fs::write(sb.path("etc/os-release"), os_release).unwrap();
        fs::write(sb.log(), "").unwrap();
        sb
    }

    pub fn without_kernel_override(mut self) -> Self {
        self.kernel = None;
        self
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    pub fn log(&self) -> PathBuf {
        self.path("mock_calls.log")
    }

    pub fn module_dir(&self, version: &str) -> PathBuf {
        self.path(&format!("usr/src/bt-cm749-{version}"))
    }

    /// Mock that logs its arguments, prints `stdout` and exits with `code`.
    pub fn mock(&self, name: &str, code: i32, stdout: &str) -> &Self {
        let print = if stdout.is_empty() { String::new() } else { format!("printf '%s\\n' '{stdout}'\n") };
        self.mock_script(name, &format!("{print}exit {code}\n"))
    }

    /// Mock with a custom shell body (runs after the logging line).
    pub fn mock_script(&self, name: &str, body: &str) -> &Self {
        let path = self.path("mock_bin").join(name);
        fs::write(&path, format!("#!/bin/sh\necho \"{name} $*\" >> \"$MOCK_LOG\"\n{body}")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        self
    }

    /// The tools every flow touches, all succeeding.
    pub fn mock_defaults(&self) -> &Self {
        for tool in ["dkms", "modprobe", "depmod", "apt", "pacman", "dnf", "update-initramfs", "mkinitcpio", "dracut"] {
            self.mock(tool, 0, "");
        }
        self.mock("gcc", 0, "14.2.1");
        self.mock("dpkg-query", 0, "install ok installed");
        self
    }

    pub fn command(&self, args: &[&str]) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("bt-cm749").unwrap();
        cmd.args(args).env_clear().envs(self.env());
        cmd
    }

    pub fn env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            ("PATH".into(), self.path("mock_bin").display().to_string()),
            ("MOCK_LOG".into(), self.log().display().to_string()),
            ("OS_RELEASE_FILE".into(), self.path("etc/os-release").display().to_string()),
            ("CUSTOM_USR_SRC".into(), self.path("usr/src").display().to_string()),
            ("BT_CM749_CACHE_DIR".into(), self.path("cache").display().to_string()),
            ("BT_CM749_MODULES_ROOT".into(), self.path("lib/modules").display().to_string()),
            ("SKIP_ROOT_CHECK".into(), "1".into()),
        ];
        if let Some(k) = &self.kernel {
            env.push(("KERNEL_VERSION".into(), k.clone()));
        }
        env
    }

    /// Places an uncompressed stock `btusb.ko` for `release`, with or without the
    /// Barrot fix markers (quirks-table entries and the continuation warning).
    pub fn stock_btusb(&self, release: &str, fixed: bool) -> &Self {
        let dir = self.path(&format!("lib/modules/{release}/kernel/drivers/bluetooth"));
        fs::create_dir_all(&dir).unwrap();
        let mut ko = b"\x7fELF stock btusb".to_vec();
        if fixed {
            ko.extend_from_slice(&[0x03, 0x00, 0xfa, 0x33, 0x10, 0x00, 0x03, 0x00, 0xfa, 0x33, 0x12, 0x00]);
            ko.extend_from_slice(b"Unexpected continuation: %d bytes");
        }
        fs::write(dir.join("btusb.ko"), ko).unwrap();
        self
    }

    pub fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.log()).unwrap().lines().map(str::to_string).collect()
    }

    #[track_caller]
    pub fn assert_called(&self, line: &str) {
        let calls = self.calls();
        assert!(calls.iter().any(|c| c == line), "`{line}` not invoked; calls: {calls:#?}");
    }

    #[track_caller]
    pub fn assert_not_called(&self, prefix: &str) {
        let calls = self.calls();
        assert!(!calls.iter().any(|c| c.starts_with(prefix)), "`{prefix}` unexpectedly invoked; calls: {calls:#?}");
    }
}

pub fn assert_file(path: &Path) {
    assert!(path.is_file(), "{} does not exist", path.display());
}
