//! Skipping the installation when the kernel's own btusb already has the fix.

mod common;

use common::*;
use predicates::str::contains;

#[test]
fn skips_when_stock_driver_is_fixed() {
    let sb = Sandbox::new(OS_UBUNTU, "6.8.0-146-generic");
    sb.mock_defaults();
    sb.stock_btusb("6.8.0-146-generic", true);
    sb.command(&["install"]).assert().success().stdout(contains("Nothing to install"));
    assert!(sb.calls().is_empty(), "nothing may be touched: {:?}", sb.calls());
    assert!(!sb.module_dir("0.3").exists());
}

#[test]
fn force_installs_anyway() {
    let sb = Sandbox::new(OS_ARCH, "7.2.9-arch1-1");
    sb.mock_defaults();
    sb.stock_btusb("7.2.9-arch1-1", true);
    sb.command(&["install", "--force"]).assert().success().stdout(contains("installing anyway"));
    sb.assert_called("dkms install -k 7.2.9-arch1-1 -m bt-cm749 -v 0.3 --force");
}

#[test]
fn installs_when_stock_driver_lacks_fix() {
    let sb = Sandbox::new(OS_UBUNTU, "6.14.0-37-generic");
    sb.mock_defaults();
    sb.stock_btusb("6.14.0-37-generic", false);
    sb.command(&["install"]).assert().success().stdout(contains("lacks the Barrot fix"));
    sb.assert_called("dkms install -k 6.14.0-37-generic -m bt-cm749 -v 0.3 --force");
}

#[test]
fn checks_the_target_kernel_not_another_one() {
    let sb = Sandbox::new(OS_ARCH, "6.12.10-arch1-1");
    sb.mock_defaults();
    sb.stock_btusb("7.2.9-arch1-1", true);
    sb.command(&["install"]).assert().success();
    sb.assert_called("dkms install -k 6.12.10-arch1-1 -m bt-cm749 -v 0.3 --force");
}

#[test]
fn detect_reports_stock_status() {
    let sb = Sandbox::new(OS_UBUNTU, "7.0.0-38-generic");
    sb.stock_btusb("7.0.0-38-generic", true);
    sb.command(&["detect"]).assert().success().stdout(contains("install not needed"));
}
