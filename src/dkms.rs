//! DKMS module source tree and dkms invocations (`dkms-module_create.sh`/`_build.sh`).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::context::{Context, LEGACY_MODULE_VERSIONS, MODULE_NAME, MODULE_VERSION, PATCH, PATCH_FILE, PREBUILD_BIN};
use crate::error::{IoContext, Result};
use crate::exec::{Cmd, Runner};

/// `dkms.conf` for the module. `${kernelver}` is expanded by DKMS when it reads the
/// file, so the PRE_BUILD step always targets the kernel being built (the shell
/// version relied on a `kernelver` environment variable that DKMS does not export).
pub fn render_conf(cc: Option<&str>) -> String {
    let cc = cc.map(|c| format!(" CC={c}")).unwrap_or_default();
    format!(
        r#"PACKAGE_NAME="{MODULE_NAME}"
PACKAGE_VERSION="{MODULE_VERSION}"

BUILT_MODULE_NAME[0]="btusb"
BUILT_MODULE_LOCATION[0]="."
DEST_MODULE_LOCATION[0]="/updates/dkms"

MAKE[0]="make{cc} -C ${{kernel_source_dir}} M=${{dkms_tree}}/${{PACKAGE_NAME}}/${{PACKAGE_VERSION}}/build"

PRE_BUILD="{PREBUILD_BIN} prebuild --kernel ${{kernelver}} --only btusb drivers/bluetooth"

AUTOINSTALL="yes"
"#
    )
}

fn module_ref(version: &str) -> String {
    format!("{MODULE_NAME}/{version}")
}

/// `dkms remove <module>/<version> --all [--force]`, ignoring failures.
pub fn remove(version: &str, force: bool, runner: &dyn Runner) {
    let mut cmd = Cmd::new("dkms").args(["remove", &module_ref(version), "--all"]);
    if force {
        cmd = cmd.arg("--force");
    }
    runner.run_quiet(&cmd);
}

/// Removes registrations and sources left by the shell implementation.
pub fn remove_legacy(ctx: &Context, runner: &dyn Runner) {
    for version in LEGACY_MODULE_VERSIONS {
        let dir = ctx.module_dir_for(version);
        if dir.exists() {
            println!(" -> Removendo instalação legada {}...", module_ref(version));
            remove(version, true, runner);
            let _ = fs::remove_dir_all(&dir);
        }
    }
}

/// Creates `/usr/src/bt-cm749-<ver>` with dkms.conf, the patch and the PRE_BUILD binary.
pub fn create_source_tree(ctx: &Context, cc: Option<&str>, prebuild_bin: &Path, runner: &dyn Runner) -> Result<()> {
    remove(MODULE_VERSION, false, runner);
    let dir = ctx.module_dir();
    fs::create_dir_all(&dir).ctx(|| format!("creating {}", dir.display()))?;
    let write = |name: &str, content: &str| {
        let path = dir.join(name);
        fs::write(&path, content).ctx(|| format!("writing {}", path.display()))
    };
    write("dkms.conf", &render_conf(cc))?;
    write(PATCH_FILE, PATCH)?;
    let bin = dir.join(PREBUILD_BIN);
    fs::copy(prebuild_bin, &bin).ctx(|| format!("copying {} to {}", prebuild_bin.display(), bin.display()))?;
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).ctx(|| format!("chmod {}", bin.display()))
}

pub fn build_and_install(kernel_release: &str, runner: &dyn Runner) -> Result<()> {
    for action in ["build", "install"] {
        runner.run(&Cmd::new("dkms").args([
            action,
            "-k",
            kernel_release,
            "-m",
            MODULE_NAME,
            "-v",
            MODULE_VERSION,
            "--force",
        ]))?;
    }
    Ok(())
}

pub fn print_status(runner: &dyn Runner) {
    let specific = Cmd::new("dkms").args(["status", "-m", MODULE_NAME, "-v", MODULE_VERSION]);
    if !matches!(runner.status(&specific), Ok(0)) {
        let _ = runner.status(&Cmd::new("dkms").arg("status"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::testing::RecordingRunner;

    #[test]
    fn conf_snapshot() {
        insta::assert_snapshot!(render_conf(None));
    }

    #[test]
    fn conf_with_cc_override() {
        assert!(
            render_conf(Some("/usr/bin/gcc-12"))
                .contains(r#"MAKE[0]="make CC=/usr/bin/gcc-12 -C ${kernel_source_dir}"#)
        );
    }

    #[test]
    fn creates_source_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_bin = tmp.path().join("fake-bin");
        fs::write(&fake_bin, "#!/bin/sh\n").unwrap();
        let ctx = Context {
            os_release_file: "/dev/null".into(),
            usr_src: tmp.path().join("usr/src"),
            headers_root: tmp.path().into(),
            modules_root: tmp.path().into(),
            dkms_root: tmp.path().into(),
            kernel_version: None,
            skip_root_check: true,
            cache_dir: None,
        };
        let r = RecordingRunner::default();
        create_source_tree(&ctx, None, &fake_bin, &r).unwrap();

        let dir = tmp.path().join("usr/src/bt-cm749-0.3");
        assert_eq!(fs::read_to_string(dir.join("bt-cm749.patch")).unwrap(), PATCH);
        assert!(fs::read_to_string(dir.join("dkms.conf")).unwrap().contains("PACKAGE_VERSION=\"0.3\""));
        let mode = fs::metadata(dir.join("bt-cm749")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(r.calls(), ["dkms remove bt-cm749/0.3 --all"]);
    }

    #[test]
    fn build_failure_stops_before_install() {
        let r = RecordingRunner::default().respond("dkms build", 2, "");
        assert_eq!(build_and_install("6.8.0-31-generic", &r).unwrap_err().exit_code(), 2);
        assert_eq!(r.calls(), ["dkms build -k 6.8.0-31-generic -m bt-cm749 -v 0.3 --force"]);
    }
}
