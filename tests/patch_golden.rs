//! Golden tests: the native patch engine must produce byte-identical results to
//! `patch -p3 --batch -N` on real btusb.c sources (fixtures are GPL-2.0-or-later
//! kernel files; the `.patched.c` and `.le-coded.c` files were produced with GNU
//! patch 2.8). `btusb-6.6.70.le-coded.c` is the result of every patch of the DKMS
//! source tree in order. `btusb-7.2.9.le-coded.c` is the result of the LE Coded PHY
//! patch alone: that kernel already contains the upstream patch, which the native
//! engine skips, while GNU patch would insert two of its hunks a second time.

use std::fs;
use std::path::Path;

use bt_cm749::context::{LE_CODED_PATCH, LE_CODED_PATCH_FILE, PATCH, PATCHES};
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

/// 7.2.9 added `BTUSB_BROKEN_EXT_SCAN` right after the upstreamed `BTUSB_BARROT`, so
/// hunk #1's post-image no longer matches. GNU patch fails that hunk and re-applies
/// #2/#3 with fuzz 2, duplicating the device IDs; the engine must leave it untouched.
#[test]
fn upstream_7_2_9_with_edited_context_is_already_patched() {
    let (status, content, _d) = patch_version("7.2.9");
    assert_eq!(status, FileStatus::AlreadyApplied);
    assert!(content == fixture("btusb-7.2.9.c"), "already-patched source must stay untouched");
    assert_eq!(content.matches("USB_DEVICE(0x33fa, 0x0010)").count(), 1);
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

/// What the DKMS PRE_BUILD step does: every patch of the source tree, in order.
fn build_tree(version: &str) -> (Vec<FileStatus>, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("btusb.c"), fixture(&format!("btusb-{version}.c"))).unwrap();
    let statuses = PATCHES.iter().map(|(name, text)| apply_in_dir(text, dir.path(), name).unwrap()).collect();
    let content = fs::read_to_string(dir.path().join("btusb.c")).unwrap();
    (statuses, content, dir)
}

#[test]
fn le_coded_fix_matches_gnu_patch_on_6_6_70() {
    let (statuses, content, _d) = build_tree("6.6.70");
    assert_eq!(statuses, [FileStatus::Applied, FileStatus::Applied]);
    assert!(content == fixture("btusb-6.6.70.le-coded.c"), "output differs from GNU patch");
}

/// Upstream already has the device IDs and the continuation fix, but not this one.
#[test]
fn le_coded_fix_matches_gnu_patch_on_upstream_7_2_9() {
    let (statuses, content, _d) = build_tree("7.2.9");
    assert_eq!(statuses, [FileStatus::AlreadyApplied, FileStatus::Applied]);
    assert!(content == fixture("btusb-7.2.9.le-coded.c"), "output differs from GNU patch");
}

/// The patch only uses context lines that are identical from 6.6 to 7.2, so it lands
/// exactly once on every version, before `hci_register_dev()` as the quirk requires.
#[test]
fn le_coded_fix_applies_once_to_every_version() {
    for version in ["6.6.70", "6.12.10", "7.1.2", "7.2.9"] {
        let (statuses, content, dir) = build_tree(version);
        assert_eq!(statuses[1], FileStatus::Applied, "{version}");
        assert_eq!(content.matches("HCI_QUIRK_BROKEN_LE_CODED").count(), 2, "{version}");
        assert_eq!(content.matches("#include <linux/version.h>").count(), 1, "{version}");
        let quirk = content.find("Barrot: LE Coded PHY disabled").unwrap();
        let register = content.find("err = hci_register_dev(hdev);").unwrap();
        assert!(quirk < register, "{version}: the quirk must be set before hci_register_dev()");

        let again = apply_in_dir(LE_CODED_PATCH, dir.path(), LE_CODED_PATCH_FILE).unwrap();
        assert_eq!(again, FileStatus::AlreadyApplied, "{version}");
        assert!(fs::read_to_string(dir.path().join("btusb.c")).unwrap() == content, "{version}");
    }
}
