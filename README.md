# bt-cm749

DKMS installer that fixes Bluetooth on Linux for UGREEN Bluetooth 5.4 USB adapters and other Barrot Technology chipsets:
- UGREEN CM748
- UGREEN CM749
- USB devices matching IDs `33fa:0010` and `33fa:0012`
- Other devices based on the Barrot BR8554 chip

This is the Rust rewrite of the `bt-cm749-fix` shell scripts. It ships as a single static binary.

Supported distros: Arch Linux (and derivatives), Fedora, and Debian/Ubuntu. Supported kernels: 6.x and 7.x.

> **You may not need this at all.** The fix was merged upstream, so recent kernels already support these adapters out of the box. Distros have also backported it, for example Ubuntu 24.04's `6.8.0-146` and its 6.17/7.0 HWE kernels. Before installing anything, `bt-cm749 install` inspects your kernel's own `btusb` module. If the fix is already there, it says so and changes nothing. Run `bt-cm749 detect` to check without root.

## How it works

`bt-cm749 install` first checks the stock `btusb.ko` of the target kernel. It looks in `/usr/lib/modules/<kver>/kernel/...`, or in DKMS's backup of it if a DKMS module displaced it. If the stock module already contains the Barrot device IDs and the event-continuation fix, it stops there; `--force` overrides this check. Otherwise it registers a DKMS module that replaces the stock `btusb` driver. When DKMS builds the module for a kernel, including every future kernel you install (`AUTOINSTALL`), the following happens:

1. DKMS runs the binary's `prebuild` step, which downloads the matching upstream source from kernel.org.
2. The download is verified against kernel.org's `sha256sums.asc` and cached in `/var/cache/dkms-kernel-src`.
3. Only `drivers/bluetooth/` is extracted from the tarball.
4. The [upstream Barrot patch](assets/bt-cm749.patch) is applied. If the kernel already contains the fix, the source is left untouched.
5. Only `btusb.ko` is built, and it is installed to `updates/dkms` (`extra/` on Fedora).

## Usage

Download the static binary for your architecture from the releases page, or build it yourself (see [Development](#development)). Then run:

```bash
sudo ./bt-cm749-x86_64-linux-musl install
```

After a successful build, reload the driver or reboot:
```bash
sudo modprobe -r btusb && sudo modprobe btusb
```

Check that the DKMS module is active:
```bash
sudo dkms status
```

To inspect what the tool detects without changing anything:
```bash
bt-cm749 detect
```

### Secure Boot

With Secure Boot enabled, loading the module may fail with:
```
$ modprobe btusb
modprobe: ERROR: could not insert 'btusb': Key was rejected by service
```
To fix it, enroll the DKMS MOK key:
```bash
sudo mokutil --import /var/lib/dkms/mok.pub
```
Then reboot and enroll the key when prompted.

### Automatic rollback on failure

If any step fails, or the installation is interrupted (`Ctrl+C`, `SIGTERM`), `install` rolls the system back. It removes the DKMS registration, the source tree in `/usr/src`, and any partial downloads, then reloads the stock driver. The process exits with the original error code (130/143 for signals).

### Uninstallation

To remove the DKMS module and its sources, and go back to your distribution's stock `btusb` driver:
```bash
sudo bt-cm749 uninstall
```
This also removes leftovers of the former shell implementation (`bt-cm749/0.2`). Installing over a shell-based installation replaces it automatically.

### Options

| Flag | Environment variable | Default |
|---|---|---|
| `--kernel <release>` | `KERNEL_VERSION` | newest installed `linux-image-*` on Debian, else the running kernel |
| `install --force` | | install even if the stock driver already has the fix |
| `--modules-root <dir>` | `BT_CM749_MODULES_ROOT` | `/usr/lib/modules` |
| `--os-release <path>` | `OS_RELEASE_FILE` | `/etc/os-release` |
| `--usr-src <dir>` | `CUSTOM_USR_SRC` | `/usr/src` |
| `--cache-dir <dir>` | `BT_CM749_CACHE_DIR` | `/var/cache/dkms-kernel-src`, else `/tmp/dkms-kernel-src` |
| `--skip-root-check` | `SKIP_ROOT_CHECK` | off |

Exit codes:

| Code | Meaning |
|---|---|
| 1 | Not root, patch failure, or I/O error |
| 2 | Kernel source unavailable |
| 3 | Missing headers or gcc < 12 without `gcc-12` |
| 4 | Kernel version could not be determined |
| other | Exit code of the failing `dkms`/package-manager command |
| 130 / 143 | Interrupted by `SIGINT` / `SIGTERM` |

## Known limitations

- **Ubuntu stable kernels.** Ubuntu's kernels backport Bluetooth core changes from newer kernels, so the vanilla kernel.org `btusb.c` no longer compiles against their headers. In practice this doesn't matter: every supported Ubuntu 24.04 kernel (6.8.0-146, HWE 6.17 and 7.0) already ships the fix, so the installer skips. Only the end-of-life HWE kernels 6.11 and 6.14 lack it; upgrade the kernel instead.
- Fedora's DKMS installs modules to `extra/` instead of `updates/dkms`. `depmod` still prefers them over the built-in driver.

## Development

```bash
make test        # unit + sandboxed integration tests (no root, no network)
make lint        # rustfmt + clippy
make test-all    # also downloads kernel sources, compiles btusb.ko against the
                 # running kernel's headers and checks parity with the shell scripts
make static      # static musl binary in dist/ (built in an Alpine container)
make e2e         # real DKMS install/uninstall in Arch and Fedora containers,
                 # and the stock-driver skip on Ubuntu 24.04
```

Integration tests run the binary with a `PATH` that contains only logging mocks, so no real `dkms`, `modprobe` or package manager is ever invoked. The patch engine is checked against GNU patch output on real `btusb.c` sources from 6.6, 6.12, 7.1 and 7.2 (`tests/fixtures/`).

## Credit

This repo is based on https://github.com/xoocoon/hp-15-ew0xxx-snd-fix/

The patch is taken from here: https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/patch/drivers/bluetooth?id=7722d6fb54e428a8f657fccf422095a8d7e2d72c
