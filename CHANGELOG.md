# Changelog

## 0.3.0 — Rust rewrite

The shell scripts (`setup_bt-cm749.sh`, `uninstall_bt-cm749.sh`, `dkms-module_*.sh`, `kernel-*.sh`) are replaced by a single static binary, `bt-cm749`, with the subcommands `install`, `uninstall`, `detect` and the internal `prebuild`. The DKMS module is now registered as `bt-cm749/0.3`. `install` and `uninstall` remove the `bt-cm749/0.2` registrations left by the shell version.

### Fixed
- **Wrong kernel source in DKMS builds.** dkms does not export `kernelver` to `PRE_BUILD` scripts, so the old script fell back to `uname -r`. That meant building for a newly installed kernel used the running kernel's source. `dkms.conf` now passes `--kernel ${kernelver}`, which dkms expands.
- **Fedora.** The source version of `6.12.9-200.fc41.x86_64` was parsed as `6.12.x86_64`, so the download returned 404.
- **Kernels that already contain the fix with edited context, such as 7.2.** GNU patch rejected hunk #1 and re-applied #2 and #3 with fuzz, which duplicated the device IDs and the continuation handler. The `grep BTUSB_BARROT` fallback then reported success. The new engine detects such hunks as already applied and leaves the source untouched.
- `apt install` / `dnf install` now run non-interactively (`-y`), so they no longer hang without a TTY.
- Debian: the newest `linux-image-*` is chosen by version, not lexicographically (`6.8.0-31` > `6.8.0-9`). Only installed (`ii`) packages are considered.
- Debian: the headers check uses an exact `dpkg-query` lookup instead of `dpkg -l | grep`, which matched substrings.
- `os-release` parsing handles quoting and `ID_LIKE` lists, so Linux Mint, Manjaro, EndeavourOS and Nobara are detected.
- The gcc ≥ 12 check runs before anything is written to `/usr/src` or DKMS.
- Concurrent builds no longer race on the same tarball download (file lock).
- The declared minimum Rust version is now 1.87, the oldest that builds the dependencies (`ruzstd` needs `is_multiple_of`), and CI checks it.
- `SIGTERM` sent only to the installer now reaches the running child and triggers the rollback. The rollback is also tested with a real signal.

### Added
- Interactive wizard, built with `cliclack` and shown when `bt-cm749` runs without a command in a terminal. It shows the distribution, the kernel, whether the adapter is plugged in and whether the kernel's own driver already has the fix, then offers only the actions that make sense. Each step runs behind a spinner, command output goes to `/var/log/bt-cm749.log`, and the end of the log is shown if something fails. After installing it reloads the driver and explains Secure Boot key enrollment when needed. Without a terminal, `bt-cm749` prints its usage and exits with code 2.
- One-command installer (`install.sh`, published with every release) for non-technical users. It picks the binary for the processor, verifies its sha256 checksum, installs it to `/usr/local/bin` and opens the wizard. Without a terminal, or with `--force` / `--uninstall`, it runs the steps itself (a mode to be retired later).
- `install` does nothing when the target kernel's stock `btusb` already supports the adapters. It detects the quirks-table entries and the continuation fix in the module (`.ko`, `.zst`, `.xz`, `.gz`), including DKMS's backup of a displaced stock module. This covers upstream kernels with the merged fix and distro backports such as Ubuntu 24.04's 6.8.0-146 and its HWE 6.17/7.0 kernels. `--force` overrides the check, and `detect` reports the result.
- sha256 verification of kernel tarballs against kernel.org's `sha256sums.asc`. A partial download resumes, and the primary-to-CDN mirror fallback is kept.
- Native unified-diff engine (offset + fuzz, idempotent), validated byte-for-byte against GNU patch. GNU patch is still used as a fallback, with a dry run first.
- Only `btusb.ko` is built (generated `Kbuild`), instead of every driver in `drivers/bluetooth`.
- `bt-cm749 detect` for diagnostics, CLI flags mirroring the environment variables, and static musl release binaries for x86_64 and aarch64.
- Test suite:
  - unit tests;
  - sandboxed end-to-end tests whose `PATH` contains only mocks;
  - golden tests on real `btusb.c` sources;
  - a parity test against the shell scripts;
  - real DKMS install/uninstall in Arch and Fedora containers.

### Known limitations
- Ubuntu stable kernels backport Bluetooth core changes, so vanilla `btusb.c` does not compile against their headers. This was already the case with the shell version. Supported Ubuntu kernels already ship the fix, so the installer skips them. See the README.
