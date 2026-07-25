//! Golden tests: the native patch engine must produce byte-identical results to
//! `patch -p3 --batch -N` on real btusb.c sources (fixtures are GPL-2.0-or-later
//! kernel files; the `.patched.c` files were produced with GNU patch 2.8).

use std::fs;
use std::path::Path;

use bt_cm749::context::PATCH;
use bt_cm749::patch::{FileStatus, apply_in_dir};

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/btusb").join(name)).unwrap()
}

fn patch_version(version: &str) -> (FileStatus, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("btusb.c"), fixture(&format!("btusb-{version}.c"))).unwrap();
    let status = apply_in_dir(PATCH, dir.path(), "bt-cm749.patch").unwrap();
    let content = fs::read_to_string(dir.path().join("btusb.c")).unwrap();
    (status, content, dir)
}

#[test]
fn matches_gnu_patch_on_6_6_70() {
    let (status, content, _d) = patch_version("6.6.70");
    assert_eq!(status, FileStatus::Applied);
    assert!(content == fixture("btusb-6.6.70.patched.c"), "output differs from GNU patch");
}

#[test]
fn matches_gnu_patch_on_6_12_10() {
    let (status, content, _d) = patch_version("6.12.10");
    assert_eq!(status, FileStatus::Applied);
    assert!(content == fixture("btusb-6.12.10.patched.c"), "output differs from GNU patch");
}

#[test]
fn upstream_7_1_2_is_already_patched() {
    let (status, content, _d) = patch_version("7.1.2");
    assert_eq!(status, FileStatus::AlreadyApplied);
    assert!(content == fixture("btusb-7.1.2.c"), "already-patched source must stay untouched");
}

#[test]
fn reapplying_is_idempotent() {
    for version in ["6.6.70", "6.12.10"] {
        let (_, first, dir) = patch_version(version);
        let status = apply_in_dir(PATCH, dir.path(), "bt-cm749.patch").unwrap();
        assert_eq!(status, FileStatus::AlreadyApplied, "{version}");
        assert!(fs::read_to_string(dir.path().join("btusb.c")).unwrap() == first, "{version}");
    }
}
