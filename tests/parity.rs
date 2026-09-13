//! Behavioural parity with the original shell scripts: both implementations run in
//! identical sandboxes and the mocked command logs are compared after normalising
//! the intentional differences. Ignored by default; needs the shell repository:
//! `BT_CM749_SHELL_DIR=../bt-cm749-fix cargo test --test parity -- --ignored`.

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::*;

fn shell_dir() -> PathBuf {
    let dir = std::env::var("BT_CM749_SHELL_DIR").map(PathBuf::from).unwrap_or_else(|_| "../bt-cm749-fix".into());
    assert!(dir.join("setup_bt-cm749.sh").is_file(), "shell implementation not found at {}", dir.display());
    dir.canonicalize().unwrap()
}

/// Intentional differences (see CHANGELOG): non-interactive package installs, DKMS
/// version 0.3, exact dpkg-query instead of `dpkg -l | grep`, legacy 0.2 cleanup and
/// the gcc check moved before the first DKMS change (checked separately).
fn normalise(calls: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = calls
        .into_iter()
        .filter(|c| !c.starts_with("dpkg-query ") && !c.starts_with("dpkg -l") && c != "gcc -dumpversion")
        .map(|c| {
            c.replace("install -y ", "install ").replace("bt-cm749/0.3", "bt-cm749/0.2").replace("-v 0.3", "-v 0.2")
        })
        .collect();
    out.dedup();
    out
}

fn mock_all(sb: &Sandbox) {
    sb.mock_defaults();
    sb.mock("dpkg", 0, "ii  linux-headers-6.8.0-31-generic  6.8.0-31.31  amd64  headers");
}

fn run_shell(sb: &Sandbox, script: &str) -> i32 {
    let mut env = sb.env();
    for (k, v) in env.iter_mut() {
        if k == "PATH" {
            *v = format!("{v}:/usr/bin:/bin");
        }
    }
    Command::new(shell_dir().join(script)).env_clear().envs(env).output().unwrap().status.code().unwrap_or(-1)
}

fn compare(os: &str, kernel: &str, rust_args: &[&[&str]], scripts: &[&str], setup: impl Fn(&Sandbox)) {
    let (rust, shell) = (Sandbox::new(os, kernel), Sandbox::new(os, kernel));
    for sb in [&rust, &shell] {
        mock_all(sb);
        setup(sb);
    }
    let rust_codes: Vec<i32> =
        rust_args.iter().map(|a| rust.command(a).output().unwrap().status.code().unwrap_or(-1)).collect();
    let shell_codes: Vec<i32> = scripts.iter().map(|s| run_shell(&shell, s)).collect();
    assert_eq!(rust_codes, shell_codes, "exit codes differ");
    assert_eq!(normalise(rust.calls()), normalise(shell.calls()), "command sequences differ");
    let gcc_checked = |sb: &Sandbox| sb.calls().iter().any(|c| c == "gcc -dumpversion");
    assert_eq!(gcc_checked(&rust), gcc_checked(&shell), "gcc check differs");
    assert_eq!(rust.module_dir("0.3").exists(), shell.module_dir("0.2").exists(), "module dir state differs");
}

#[test]
#[ignore = "needs the shell implementation"]
fn install_and_uninstall_match_shell() {
    let flows: [&[&str]; 2] = [&["install"], &["uninstall"]];
    let scripts = ["setup_bt-cm749.sh", "uninstall_bt-cm749.sh"];
    compare(OS_ARCH, "7.1.2-arch3-1", &flows, &scripts, |_| {});
    compare(OS_UBUNTU, "6.8.0-31-generic", &flows, &scripts, |_| {});
    compare(OS_FEDORA, "6.12.9-200.fc41.x86_64", &flows, &scripts, |_| {});
}

#[test]
#[ignore = "needs the shell implementation"]
fn failed_build_rollback_matches_shell() {
    compare(OS_UBUNTU, "6.8.0-31-generic", &[&["install"]], &["setup_bt-cm749.sh"], |sb| {
        sb.mock_script("dkms", "case \"$1\" in build) exit 2;; esac\nexit 0\n");
    });
}
