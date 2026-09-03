//! DKMS PRE_BUILD step (`kernel-module_patch.sh`): fetch the upstream source of the
//! kernel being built, extract the driver directory into the build dir and patch it.

use std::fs;
use std::path::Path;

use crate::context::Context;
use crate::error::{Error, IoContext, Result};
use crate::exec::{Cmd, Runner};
use crate::os_release::Distro;
use crate::patch::{self, FileStatus};
use crate::{kernel, source};

/// `only` restricts the build to the listed modules through a generated `Kbuild`
/// (which takes precedence over the extracted Makefile), so unrelated drivers in the
/// same directory can neither slow down nor break the DKMS build.
pub fn prebuild(ctx: &Context, only: &[String], subdir: &str, build_dir: &Path, runner: &dyn Runner) -> Result<()> {
    let distro = Distro::detect(&ctx.os_release_file);
    let kv = kernel::detect(ctx.kernel_version.as_deref(), distro, &ctx.headers_root, runner)?;
    println!("{}", kv.describe());
    if kv.sub.is_none() {
        return Err(Error::KernelSublevel);
    }
    println!("Building for kernel version {}", kv.release);

    let version = kv.source_version();
    let cache = source::cache_dir(ctx.cache_dir.as_deref());
    let tarball = source::ensure_tarball(&cache, kv.major, &version, &kv.release)?;

    println!("Extracting original source of the kernel module...");
    source::extract_subdir(&tarball, &format!("linux-{version}"), subdir, build_dir)?;
    if !only.is_empty() {
        let objs: Vec<String> = only.iter().map(|m| format!("{m}.o")).collect();
        let kbuild = build_dir.join("Kbuild");
        fs::write(&kbuild, format!("obj-m := {}\n", objs.join(" "))).ctx(|| format!("writing {}", kbuild.display()))?;
    }

    for path in patch_files(build_dir)? {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        println!("Applying patch: {name}");
        let text = fs::read_to_string(&path).ctx(|| format!("reading {}", path.display()))?;
        match patch::apply_in_dir(&text, build_dir, &name) {
            Ok(FileStatus::Applied) => println!("Patch {name} applied successfully."),
            Ok(FileStatus::AlreadyApplied) => println!("Patch {name} already present in source."),
            Err(err) if gnu_patch(&path, build_dir, runner) => {
                println!("{err}; GNU patch fallback applied {name} successfully.");
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// `*.patch` files of the build dir in name order.
fn patch_files(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .ctx(|| format!("listing {}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "patch"))
        .collect();
    files.sort();
    Ok(files)
}

/// Last resort when the native engine cannot place a hunk and GNU patch exists.
fn gnu_patch(patch_file: &Path, dir: &Path, runner: &dyn Runner) -> bool {
    if !runner.exists("patch") {
        return false;
    }
    let file = patch_file.display().to_string();
    let dir = dir.display().to_string();
    // Dry run first so a failing strip level never leaves a half-patched file.
    ["-p3", "-p1", "-p0"].iter().any(|strip| {
        let cmd = Cmd::new("patch").args(["-d", &dir, strip, "--batch", "-N", "-r", "-", "-i", &file]);
        matches!(runner.status(&cmd.clone().arg("--dry-run")), Ok(0)) && matches!(runner.status(&cmd), Ok(0))
    })
}
