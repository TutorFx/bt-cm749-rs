mod common;

use common::*;

#[test]
fn requires_root() {
    if rustix::process::geteuid().is_root() {
        return;
    }
    let sb = Sandbox::new(OS_ARCH, "7.1.2-arch3-1");
    sb.mock_defaults();
    for (cmd, word) in [("install", "setup"), ("uninstall", "uninstallation")] {
        sb.command(&[cmd])
            .env_remove("SKIP_ROOT_CHECK")
            .assert()
            .code(1)
            .stderr(predicates::str::contains(format!("Only root can perform this {word}")));
    }
    assert!(sb.calls().is_empty());
}

#[test]
fn detect_reports_versions() {
    let sb = Sandbox::new(OS_UBUNTU, "6.8.0-31-generic");
    sb.command(&["detect"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Distro family: Debian"))
        .stdout(predicates::str::contains("Corresponding kernel source version is 6.8.0."))
        .stdout(predicates::str::contains("linux-6.8.tar.xz"));
}

#[test]
fn unparseable_kernel_exits_4() {
    let sb = Sandbox::new(OS_ARCH, "not-a-version");
    sb.command(&["detect"]).assert().code(4);
}

#[test]
fn prebuild_without_sublevel_exits_4() {
    let sb = Sandbox::new(OS_ARCH, "x");
    sb.command(&["prebuild", "--kernel", "6.8-rc1", "drivers/bluetooth"]).assert().code(4);
}

#[test]
fn without_a_command_and_terminal_prints_usage() {
    // Under the test harness stderr is a pipe, so the wizard must not start.
    let sb = Sandbox::new(OS_ARCH, "7.1.2-arch3-1");
    sb.mock_defaults();
    sb.command(&[])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("Usage: bt-cm749 [OPTIONS] [COMMAND]"))
        .stdout(predicates::str::contains("interactive wizard"));
    assert!(sb.calls().is_empty());
}
