//! `install` and `uninstall` flows (`setup_bt-cm749.sh` / `uninstall_bt-cm749.sh`).

use std::fs;
use std::path::Path;

use crate::context::{Context, LEGACY_MODULE_VERSIONS, MODULE_NAME, MODULE_VERSION};
use crate::error::{Error, IoContext, Result};
use crate::exec::{Cmd, Runner};
use crate::os_release::Distro;
use crate::stock::{self, StockDriver};
use crate::{distro, dkms, kernel, signals, source};

const RULE: &str = "============================================================";

pub fn check_root(ctx: &Context, action: &'static str) -> Result<()> {
    if ctx.skip_root_check || rustix::process::geteuid().is_root() { Ok(()) } else { Err(Error::NotRoot(action)) }
}

/// Installs the module; any failure or SIGINT/SIGTERM rolls the system back and the
/// original error (and exit code) is returned. Unless `force` is set, nothing is
/// installed when the kernel's stock btusb already supports the adapters.
pub fn install(ctx: &Context, prebuild_bin: &Path, force: bool, runner: &dyn Runner) -> Result<()> {
    check_root(ctx, "setup")?;
    signals::install().ctx(|| "installing signal handlers".into())?;
    let result = install_steps(ctx, prebuild_bin, force, runner);
    if let Err(err) = &result {
        rollback(ctx, err, runner);
    }
    result
}

fn install_steps(ctx: &Context, prebuild_bin: &Path, force: bool, runner: &dyn Runner) -> Result<()> {
    let distro = Distro::detect(&ctx.os_release_file);
    let kv = kernel::detect(ctx.kernel_version.as_deref(), distro, &ctx.headers_root, runner)?;
    println!("{}", kv.describe());
    match stock::inspect(&ctx.modules_root, &ctx.dkms_root, &kv.release) {
        StockDriver::Supported(path) if !force => {
            println!(
                "The stock btusb driver of kernel {} ({}) already supports 33fa:0010 / 33fa:0012.\n\
                 Nothing to install. Use --force to build the DKMS module anyway.",
                kv.release,
                path.display()
            );
            if ctx.module_dir().exists() || LEGACY_MODULE_VERSIONS.iter().any(|v| ctx.module_dir_for(v).exists()) {
                println!(
                    "An existing {MODULE_NAME} DKMS installation is no longer needed: remove it with 'sudo bt-cm749 uninstall'."
                );
            }
            return Ok(());
        }
        StockDriver::Supported(_) => println!("Stock btusb already has the fix; installing anyway (--force)."),
        StockDriver::Missing(path) => println!("Stock btusb ({}) lacks the Barrot fix.", path.display()),
        StockDriver::Unknown(reason) => println!("Could not inspect the stock btusb driver ({reason}); proceeding."),
    }
    println!("Setting up {MODULE_NAME} (v{MODULE_VERSION}) for kernel {}...", kv.release);
    if kv.is_recent() {
        println!(
            "Note: Kernel {} detected. Applying DKMS fix to ensure device 33fa:0010 / 33fa:0012 support.",
            kv.release
        );
    }

    distro::install_prerequisites(distro, runner)?;
    distro::ensure_headers(distro, &kv.release, runner)?;
    let cc = distro::compiler_override(runner)?;
    dkms::remove_legacy(ctx, runner);
    dkms::create_source_tree(ctx, cc.as_deref(), prebuild_bin, runner)?;
    check_interrupted()?;

    println!("Building for kernel version {}", kv.release);
    dkms::build_and_install(&kv.release, runner)?;
    println!("Updating initramfs if applicable...");
    distro::update_initramfs(distro, &kv.release, runner)?;

    println!("\n=== DKMS Module Status ===");
    dkms::print_status(runner);
    println!(
        "\nInstallation complete! You can reload the module now with \
         'sudo modprobe -r btusb && sudo modprobe btusb' or reboot your system.\n    \
         Check status anytime with 'sudo dkms status'."
    );
    check_interrupted()
}

fn check_interrupted() -> Result<()> {
    signals::pending().map_or(Ok(()), |sig| Err(Error::Interrupted(sig)))
}

fn rollback(ctx: &Context, err: &Error, runner: &dyn Runner) {
    signals::suppress();
    println!("\n{RULE}");
    println!(" [ERRO] Falha detectada durante a instalação: {err}");
    println!(" [ROLLBACK] Iniciando reversão automática para manter o sistema estável...");
    println!("{RULE}");

    if runner.exists("dkms") {
        println!(" -> Removendo módulo DKMS {MODULE_NAME}/{MODULE_VERSION}...");
        dkms::remove(MODULE_VERSION, true, runner);
    }
    let dir = ctx.module_dir();
    if dir.is_dir() {
        println!(" -> Removendo diretório de fontes {}...", dir.display());
        let _ = fs::remove_dir_all(&dir);
    }
    println!(" -> Limpando artefatos temporários residuais...");
    for partial in source::partial_downloads() {
        let _ = fs::remove_file(partial);
    }
    if runner.exists("modprobe") {
        runner.run_quiet(&Cmd::new("modprobe").arg("btusb"));
    }

    println!("{RULE}");
    println!(" [ROLLBACK CONCLUÍDO] O sistema foi revertido com segurança.");
    println!(" Código de erro original: {}", err.exit_code());
    println!("{RULE}");
}

/// Removes the module (current and legacy versions) and restores the stock driver.
/// Every step is best-effort, as in the original script.
pub fn uninstall(ctx: &Context, runner: &dyn Runner) -> Result<()> {
    check_root(ctx, "uninstallation")?;
    println!("{RULE}");
    println!(" Desinstalando {MODULE_NAME} (v{MODULE_VERSION})...");
    println!("{RULE}");

    if runner.exists("modprobe") {
        println!(" -> Descarregando módulo btusb...");
        runner.run_quiet(&Cmd::new("modprobe").args(["-r", "btusb"]));
    }
    let versions: Vec<&str> = std::iter::once(MODULE_VERSION).chain(LEGACY_MODULE_VERSIONS.iter().copied()).collect();
    if runner.exists("dkms") {
        for version in &versions {
            println!(" -> Removendo módulo {MODULE_NAME}/{version} do DKMS...");
            dkms::remove(version, true, runner);
        }
    }
    for version in &versions {
        let dir = ctx.module_dir_for(version);
        if dir.is_dir() {
            println!(" -> Removendo diretório de fontes {}...", dir.display());
            fs::remove_dir_all(&dir).ctx(|| format!("removing {}", dir.display()))?;
        }
    }
    if runner.exists("depmod") {
        println!(" -> Executando depmod -a...");
        runner.run_quiet(&Cmd::new("depmod").arg("-a"));
    }

    let distro = Distro::detect(&ctx.os_release_file);
    let release = kernel::detect(ctx.kernel_version.as_deref(), distro, &ctx.headers_root, runner)
        .map(|kv| kv.release)
        .unwrap_or_else(|_| rustix::system::uname().release().to_string_lossy().into_owned());
    println!(" -> Atualizando initramfs...");
    if let Err(e) = distro::update_initramfs(distro, &release, runner) {
        eprintln!("Warning: {e}");
    }

    if runner.exists("modprobe") {
        println!(" -> Recarregando driver btusb nativo...");
        runner.run_quiet(&Cmd::new("modprobe").arg("btusb"));
    }

    println!("\n=== DKMS Status ===");
    if runner.exists("dkms") {
        let status = Cmd::new("dkms").args(["status", "-m", MODULE_NAME, "-v", MODULE_VERSION]);
        if !matches!(runner.status(&status), Ok(0)) {
            println!("Módulo {MODULE_NAME} não está mais registrado no DKMS.");
        }
    }
    println!("\nDesinstalação e reversão concluídas com sucesso!");
    Ok(())
}
