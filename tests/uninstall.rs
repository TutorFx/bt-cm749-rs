//! Uninstall and revert (port of test_uninstall_simulation.sh).

mod common;

use common::*;

fn install_then_uninstall(os: &str, kernel: &str) -> Sandbox {
    let sb = Sandbox::new(os, kernel);
    sb.mock_defaults();
    sb.command(&["install"]).assert().success();
    assert_file(&sb.module_dir("0.3").join("dkms.conf"));
    sb.command(&["uninstall"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Desinstalação e reversão concluídas"));
    assert!(!sb.module_dir("0.3").exists(), "module sources must be removed");
    sb.assert_called("modprobe -r btusb");
    sb.assert_called("dkms remove bt-cm749/0.3 --all --force");
    sb.assert_called("dkms remove bt-cm749/0.2 --all --force");
    sb.assert_called("depmod -a");
    sb.assert_called("modprobe btusb");
    sb
}

#[test]
fn debian_ubuntu() {
    let sb = install_then_uninstall(OS_UBUNTU, "6.8.0-31-generic");
    sb.assert_called("update-initramfs -u -k 6.8.0-31-generic");
}

#[test]
fn arch_linux() {
    let sb = install_then_uninstall(OS_ARCH, "7.1.2-arch3-1");
    sb.assert_called("mkinitcpio -P");
}

#[test]
fn fedora() {
    let sb = install_then_uninstall(OS_FEDORA, "6.12.5-200.fc41.x86_64");
    sb.assert_called("dracut --regenerate-all --force --parallel");
}

#[test]
fn removes_legacy_sources() {
    let sb = Sandbox::new(OS_ARCH, "7.1.2-arch3-1");
    sb.mock_defaults();
    std::fs::create_dir_all(sb.module_dir("0.2")).unwrap();
    sb.command(&["uninstall"]).assert().success();
    assert!(!sb.module_dir("0.2").exists());
}

#[test]
fn tolerates_missing_tools() {
    let sb = Sandbox::new(OS_ARCH, "7.1.2-arch3-1");
    // No mocks at all: nothing on PATH.
    sb.command(&["uninstall"]).assert().success();
}
