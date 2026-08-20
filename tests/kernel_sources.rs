//! Tests against real kernel sources. Ignored by default: they download upstream
//! tarballs (~140 MB each, cached) and compile against installed headers.
//! Run with `cargo test -- --ignored`.

use std::fs;
use std::path::Path;
use std::process::Command;

use bt_cm749::context::PATCH;

fn prebuild(dir: &Path, kernel: &str) -> std::process::Output {
    fs::write(dir.join("bt-cm749.patch"), PATCH).unwrap();
    Command::new(assert_cmd::cargo::cargo_bin("bt-cm749"))
        .args(["prebuild", "--kernel", kernel, "drivers/bluetooth"])
        .current_dir(dir)
        .output()
        .unwrap()
}

/// Port of test_multikernel_patch.sh.
#[test]
#[ignore = "downloads kernel tarballs"]
fn patches_multiple_kernels() {
    for kver in ["7.1.2", "6.12.10", "6.6.70"] {
        let dir = tempfile::Builder::new().prefix(&format!("bt_kernel_test_{kver}_")).tempdir().unwrap();
        let out = prebuild(dir.path(), kver);
        assert!(
            out.status.success(),
            "{kver}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let src = fs::read_to_string(dir.path().join("btusb.c")).unwrap();
        for needle in ["BTUSB_BARROT", "0x33fa, 0x0010", "0x33fa, 0x0012", "Unexpected continuation"] {
            assert!(src.contains(needle), "{kver}: {needle} missing");
        }
        assert_eq!(src.matches("define BTUSB_BARROT").count(), 1, "{kver}: patched twice");

        let again = prebuild(dir.path(), kver);
        assert!(again.status.success(), "{kver}: re-run must be idempotent");
        assert_eq!(fs::read_to_string(dir.path().join("btusb.c")).unwrap().matches("define BTUSB_BARROT").count(), 1);
    }
}

/// Port of test_real_compilation.sh.
#[test]
#[ignore = "needs headers for the running kernel and network"]
fn compiles_btusb_against_running_kernel() {
    let release = rustix::system::uname().release().to_string_lossy().into_owned();
    let headers = Path::new("/usr/lib/modules").join(&release).join("build");
    assert!(headers.is_dir(), "kernel headers not found in {}", headers.display());

    let dir = tempfile::Builder::new().prefix("bt_real_build_").tempdir().unwrap();
    let out = prebuild(dir.path(), &release);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));

    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get()).to_string();
    let make = Command::new("make")
        .args(["-s", "-j", &jobs, "-C"])
        .arg(&headers)
        .arg(format!("M={}", dir.path().display()))
        .output()
        .unwrap();
    assert!(make.status.success(), "{}", String::from_utf8_lossy(&make.stderr));

    let ko = dir.path().join("btusb.ko");
    let bytes = fs::read(&ko).expect("btusb.ko was not built");
    assert!(bytes.windows(23).any(|w| w == b"Unexpected continuation"), "Barrot handler missing from btusb.ko");
    if let Ok(info) = Command::new("modinfo").arg(&ko).output() {
        let info = String::from_utf8_lossy(&info.stdout);
        assert!(info.contains("description:") && info.contains(&format!("vermagic:       {release} ")), "{info}");
    }
}
