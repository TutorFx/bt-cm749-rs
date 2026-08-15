//! Install flow per distro (port of test_{arch,debian,fedora}_simulation.sh).

mod common;

use common::*;

fn assert_module_tree(sb: &Sandbox) {
    let dir = sb.module_dir("0.3");
    for f in ["dkms.conf", "bt-cm749.patch", "bt-cm749"] {
        assert_file(&dir.join(f));
    }
    let patch = std::fs::read_to_string(dir.join("bt-cm749.patch")).unwrap();
    for needle in ["BTUSB_BARROT", "0x33fa, 0x0010", "0x33fa, 0x0012"] {
        assert!(patch.contains(needle), "patch lacks {needle}");
    }
    let conf = std::fs::read_to_string(dir.join("dkms.conf")).unwrap();
    assert!(conf.contains(r#"PRE_BUILD="bt-cm749 prebuild --kernel ${kernelver} drivers/bluetooth""#));
}

#[test]
fn arch_linux() {
    let sb = Sandbox::new(OS_ARCH, "7.1.2-arch3-1");
    sb.mock_defaults();
    sb.command(&["install"]).assert().success().stdout(predicates::str::contains("Kernel 7.1.2-arch3-1 detected"));

    sb.assert_called("pacman -S --needed --noconfirm pahole dkms base-devel linux-headers");
    assert_module_tree(&sb);
    sb.assert_called("dkms build -k 7.1.2-arch3-1 -m bt-cm749 -v 0.3 --force");
    sb.assert_called("dkms install -k 7.1.2-arch3-1 -m bt-cm749 -v 0.3 --force");
    sb.assert_called("mkinitcpio -P");
    sb.assert_not_called("apt");
}

#[test]
fn debian_ubuntu() {
    let sb = Sandbox::new(OS_UBUNTU, "6.8.0-31-generic");
    sb.mock_defaults();
    sb.command(&["install"]).assert().success();

    sb.assert_called("apt install -y build-essential dkms dwarves");
    sb.assert_called("dpkg-query -W -f=${Status} linux-headers-6.8.0-31-generic");
    sb.assert_not_called("apt update");
    assert_module_tree(&sb);
    sb.assert_called("dkms build -k 6.8.0-31-generic -m bt-cm749 -v 0.3 --force");
    sb.assert_called("dkms install -k 6.8.0-31-generic -m bt-cm749 -v 0.3 --force");
    sb.assert_called("update-initramfs -u -k 6.8.0-31-generic");
}

#[test]
fn fedora() {
    let sb = Sandbox::new(OS_FEDORA, "6.12.9-200.fc41.x86_64");
    sb.mock_defaults();
    sb.command(&["install"]).assert().success();

    sb.assert_called("dnf install -y dwarves dkms kernel-devel kernel-headers");
    assert_module_tree(&sb);
    sb.assert_called("dkms build -k 6.12.9-200.fc41.x86_64 -m bt-cm749 -v 0.3 --force");
    sb.assert_called("dkms install -k 6.12.9-200.fc41.x86_64 -m bt-cm749 -v 0.3 --force");
    sb.assert_called("dracut --regenerate-all --force --parallel");
}

#[test]
fn fedora_source_version_ignores_arch_suffix() {
    // Regression: the shell parsed `x86_64` as the sublevel and asked for linux-6.12.x86_64.tar.xz.
    let sb = Sandbox::new(OS_FEDORA, "6.12.9-200.fc41.x86_64");
    sb.command(&["detect"]).assert().success().stdout(predicates::str::contains("linux-6.12.9.tar.xz"));
}

#[test]
fn linux_mint_is_handled_as_debian() {
    let sb = Sandbox::new("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n", "6.8.0-31-generic");
    sb.mock_defaults();
    sb.command(&["install"]).assert().success();
    sb.assert_called("apt install -y build-essential dkms dwarves");
    sb.assert_called("update-initramfs -u -k 6.8.0-31-generic");
}

#[test]
fn debian_installs_missing_headers() {
    let sb = Sandbox::new(OS_UBUNTU, "6.8.0-31-generic");
    sb.mock_defaults();
    // Not installed on the first query, installed after apt ran.
    sb.mock_script(
        "dpkg-query",
        "while IFS= read -r l; do case \"$l\" in 'apt install -y linux-headers'*) echo 'install ok installed'; exit 0;; esac; done < \"$MOCK_LOG\"\nexit 1\n",
    );
    sb.command(&["install"]).assert().success();
    sb.assert_called("apt update -y");
    sb.assert_called("apt install -y linux-headers-6.8.0-31-generic");
}

#[test]
fn unknown_distro_skips_distro_steps() {
    let sb = Sandbox::new("ID=opensuse-tumbleweed\n", "6.12.0-1-default");
    sb.mock_defaults();
    sb.command(&["install"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Preparation steps not (yet) supported"));
    sb.assert_called("dkms install -k 6.12.0-1-default -m bt-cm749 -v 0.3 --force");
    for tool in ["apt", "pacman", "dnf", "mkinitcpio", "dracut", "update-initramfs"] {
        sb.assert_not_called(tool);
    }
}

#[test]
fn old_gcc_uses_gcc12() {
    let sb = Sandbox::new(OS_UBUNTU, "6.8.0-31-generic");
    sb.mock_defaults();
    sb.mock("gcc", 0, "11");
    sb.mock("gcc-12", 0, "");
    sb.command(&["install"]).assert().success();
    let conf = std::fs::read_to_string(sb.module_dir("0.3").join("dkms.conf")).unwrap();
    let gcc12 = sb.path("mock_bin/gcc-12");
    assert!(conf.contains(&format!("make CC={} -C", gcc12.display())), "{conf}");
}

#[test]
fn replaces_legacy_shell_installation() {
    let sb = Sandbox::new(OS_ARCH, "7.1.2-arch3-1");
    sb.mock_defaults();
    std::fs::create_dir_all(sb.module_dir("0.2")).unwrap();
    std::fs::write(sb.module_dir("0.2").join("kernel-module_patch.sh"), "").unwrap();

    sb.command(&["install"]).assert().success();
    sb.assert_called("dkms remove bt-cm749/0.2 --all --force");
    assert!(!sb.module_dir("0.2").exists());
    assert_file(&sb.module_dir("0.3").join("dkms.conf"));
}

#[test]
fn debian_detects_newest_installed_kernel() {
    let sb = Sandbox::new(OS_UBUNTU, "").without_kernel_override();
    sb.mock_defaults();
    sb.mock_script(
        "dpkg",
        "printf '%s\\n' 'ii  linux-image-6.8.0-9-generic   6.8.0-9.9    amd64  k' 'ii  linux-image-6.8.0-31-generic  6.8.0-31.31  amd64  k'\n",
    );
    sb.command(&["install"]).assert().success();
    sb.assert_called("dkms build -k 6.8.0-31-generic -m bt-cm749 -v 0.3 --force");
}
