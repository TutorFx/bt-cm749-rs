//! Automatic rollback (port of test_rollback_simulation.sh, plus a real signal test).

mod common;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::*;

#[test]
fn dkms_build_failure_rolls_back_with_its_exit_code() {
    let sb = Sandbox::new(OS_UBUNTU, "6.8.0-31-generic");
    sb.mock_defaults();
    sb.mock_script(
        "dkms",
        "case \"$1\" in build) echo 'ERROR: Kernel compilation failed on test mock' >&2; exit 2;; esac\nexit 0\n",
    );

    sb.command(&["install"]).assert().code(2).stdout(predicates::str::contains("ROLLBACK CONCLUÍDO"));
    sb.assert_called("dkms remove bt-cm749/0.3 --all --force");
    sb.assert_called("modprobe btusb");
    sb.assert_not_called("dkms install");
    assert!(!sb.module_dir("0.3").exists(), "module dir must be removed by the rollback");
}

#[test]
fn old_gcc_without_gcc12_fails_before_touching_usr_src() {
    let sb = Sandbox::new(OS_ARCH, "6.12.0-arch1");
    sb.mock_defaults();
    sb.mock("gcc", 0, "9.3.0");

    sb.command(&["install"]).assert().code(3).stderr(predicates::str::contains("version 12 is required"));
    sb.assert_not_called("dkms build");
    assert!(!sb.module_dir("0.3").exists());
}

#[test]
fn missing_debian_headers_exit_3() {
    let sb = Sandbox::new(OS_UBUNTU, "6.8.0-31-generic");
    sb.mock_defaults();
    sb.mock("dpkg-query", 1, "");
    sb.command(&["install"]).assert().code(3).stdout(predicates::str::contains("ROLLBACK CONCLUÍDO"));
    sb.assert_not_called("dkms build");
}

#[test]
fn package_manager_failure_propagates() {
    let sb = Sandbox::new(OS_FEDORA, "6.12.9-200.fc41.x86_64");
    sb.mock_defaults();
    sb.mock("dnf", 1, "");
    sb.command(&["install"]).assert().code(1).stdout(predicates::str::contains("ROLLBACK CONCLUÍDO"));
    assert!(!sb.module_dir("0.3").exists());
}

#[test]
fn sigint_during_build_rolls_back() {
    let sb = Sandbox::new(OS_ARCH, "7.1.2-arch3-1");
    sb.mock_defaults();
    // `exec` so the forwarded SIGTERM reaches the sleeping process itself.
    sb.mock_script("dkms", "case \"$1\" in build) exec /bin/sleep 30;; esac\nexit 0\n");

    let child = Command::new(assert_cmd::cargo::cargo_bin("bt-cm749"))
        .arg("install")
        .env_clear()
        .envs(sb.env())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !sb.calls().iter().any(|c| c.starts_with("dkms build")) {
        assert!(Instant::now() < deadline, "dkms build never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Signal only our process: the tool must forward it to the running child.
    let pid = rustix::process::Pid::from_raw(child.id() as i32).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();

    let started = Instant::now();
    let out = child.wait_with_output().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10), "child was not interrupted");
    assert_eq!(out.status.code(), Some(130));
    assert!(String::from_utf8_lossy(&out.stdout).contains("ROLLBACK CONCLUÍDO"));
    sb.assert_called("dkms remove bt-cm749/0.3 --all --force");
    sb.assert_not_called("dkms install");
    assert!(!sb.module_dir("0.3").exists());
}
