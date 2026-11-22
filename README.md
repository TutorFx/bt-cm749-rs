# bt-cm749

[![CI](https://github.com/TutorFx/bt-cm749-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/TutorFx/bt-cm749-rs/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/TutorFx/bt-cm749-rs)](https://github.com/TutorFx/bt-cm749-rs/releases/latest)

Fixes Bluetooth on Linux for UGREEN Bluetooth 5.4 USB adapters and other adapters with a Barrot Technology chip:
- UGREEN CM748
- UGREEN CM749
- USB devices with the IDs `33fa:0010` or `33fa:0012` (check with `lsusb`)
- Other devices based on the Barrot BR8554 chip

Works on Arch Linux, Manjaro, Fedora, Debian, Ubuntu, Linux Mint and other derivatives, with Linux kernels 6.x and 7.x.

It does two things:
- **Adds support for the adapters** to kernels that lack it. Linux 6.18 and newer, and Ubuntu 24.04 with its current updates, already have it.
- **Stops the adapters from hanging** a few seconds after they start looking for Bluetooth devices (see [Bluetooth works for a few seconds, then stops](#bluetooth-works-for-a-few-seconds-then-stops)). No kernel includes this fix yet.

The installer checks your kernel first and only installs what it is missing. On Ubuntu and its derivatives the second fix cannot be built, so there the installer changes nothing when the kernel already supports the adapter.

## Install

1. Plug in the adapter.
2. Open a terminal, paste this command and press <kbd>Enter</kbd>. It asks for your password; nothing is shown while you type it.

   ```bash
   curl -fsSL https://github.com/TutorFx/bt-cm749-rs/releases/latest/download/install.sh | sudo sh
   ```

3. A short wizard opens. It shows what it found and suggests what to do. Use the arrow keys and <kbd>Enter</kbd> to confirm:

   ```
   ┌   bt-cm749  Bluetooth fix for UGREEN CM748 / CM749
   │
   ◇  Your system ──────────────────────────╮
   │  Distribution:   Arch Linux            │
   │  Kernel:         7.1.2-arch3-1         │
   │  Adapter:        33fa:0012 plugged in  │
   │  Kernel driver:  lacks the fix         │
   │  bt-cm749 fix:   not installed         │
   ├────────────────────────────────────────╯
   │
   ◆  What do you want to do?
   │  ● Install the fix (recommended)
   │  ○ Exit
   ```

4. Follow the last message. It is one of these:
   - **"Nothing was changed"**, after the wizard said your kernel already has the fixes, or that the rest cannot be built for it: there was nothing the installer could add. If Bluetooth still does not work, unplug the adapter, plug it back in and restart. If it works for a few seconds and then stops, see [Bluetooth works for a few seconds, then stops](#bluetooth-works-for-a-few-seconds-then-stops).
   - **"Done!"**: unplug the adapter, plug it back in, and turn Bluetooth on in your settings, or restart if asked to.
   - **"Secure Boot is blocking the new driver"**: see [Secure Boot](#secure-boot) below.

The fix is rebuilt automatically whenever your system installs a new kernel, so you only need to do this once.

If `curl` is missing, use `wget -qO- <same address> | sudo sh` instead, or install `curl` from your software center.

### What the installer does

It downloads the latest [release](https://github.com/TutorFx/bt-cm749-rs/releases/latest) built by this repository's [GitHub Actions pipeline](https://github.com/TutorFx/bt-cm749-rs/actions/workflows/release.yml), picks the file for your processor (x86_64 or ARM64), checks its sha256 checksum, copies it to `/usr/local/bin/bt-cm749` and starts the wizard. You can [read the script](install.sh) before running it. The same wizard opens whenever you run `sudo bt-cm749` later, for example to remove the fix.

If anything goes wrong, the installation is undone automatically, so the system is left as it was. The wizard then shows the end of the log; the full log is in `/var/log/bt-cm749.log`.

Without a terminal, for example in scripts, the installer runs the same steps without asking. The same happens with `--force`, which installs even when the kernel already has the fix. This non-interactive mode will be retired once the wizard covers every case.

### Manual install

Download `bt-cm749-x86_64-linux-musl` (most PCs) or `bt-cm749-aarch64-linux-musl` (ARM, for example a Raspberry Pi) from the [latest release](https://github.com/TutorFx/bt-cm749-rs/releases/latest), then:

```bash
chmod +x bt-cm749-x86_64-linux-musl
sudo ./bt-cm749-x86_64-linux-musl install
sudo modprobe -r btusb && sudo modprobe btusb   # or restart the computer
```

### Uninstall

```bash
curl -fsSL https://github.com/TutorFx/bt-cm749-rs/releases/latest/download/install.sh | sudo sh -s -- --uninstall
```

This goes back to your distribution's own Bluetooth driver. If you installed manually, run `sudo bt-cm749 uninstall` (or the downloaded file with `uninstall`). It also removes installations made with the old shell scripts (`bt-cm749/0.2`).

## Troubleshooting

### Secure Boot

With Secure Boot enabled, the new driver must be signed with a key your computer trusts. The installer tells you when this happens; loading the driver then fails with:
```
modprobe: ERROR: could not insert 'btusb': Key was rejected by service
```
To trust the key once:
```bash
sudo mokutil --import /var/lib/dkms/mok.pub
```
Choose a one-time password, restart, select **Enroll MOK** on the blue screen that appears during startup, and type the same password.

### Checking what was detected

```bash
bt-cm749 detect
```
This shows your distribution, the kernel version and which fixes your kernel's own driver already has. It changes nothing and does not need `sudo`. To see whether the fix is installed, run `sudo dkms status` and look for a line starting with `bt-cm749/0.3`.

### Bluetooth works for a few seconds, then stops

The firmware of these adapters can hang while they look for Bluetooth LE devices. Bluetooth turns on, then the adapter stops responding a few seconds later. It has been seen when the Bluetooth page of the system settings was opened, and after the computer woke up from suspend. After that, Bluetooth cannot be turned on again, and `journalctl -k | grep hci0` shows:

```
Bluetooth: hci0: command 0x2042 tx timeout
Bluetooth: hci0: Unable to disable scanning: -110
Bluetooth: hci0: Opcode 0x0401 failed: -12
Bluetooth: hci0: Controller not accepting commands anymore: ncmd = 0
Bluetooth: hci0: Opcode 0x0c03 failed: -12
```

**To get it working again**, unplug the adapter and plug it back in. Restarting the computer is not enough: on most computers the USB ports stay powered during a restart, so the adapter keeps its broken state.

**To stop it from happening**, [install](#install) bt-cm749. It keeps the adapter off the LE Coded PHY, the long-range Bluetooth LE mode, which almost no device uses. Every other Bluetooth and Bluetooth LE feature keeps working. Without that mode the hang did not happen again in testing. To check that the fix is active, run `journalctl -k | grep 'LE Coded'` after plugging the adapter in:

```
btusb 1-6:1.0: Barrot: LE Coded PHY disabled to avoid a firmware hang
```

**If the fix cannot be installed** (Ubuntu and its derivatives) **or the hang still happens**, turn off Bluetooth LE on the adapter. This works if all your devices use classic Bluetooth, as headphones, speakers, soundbars, game controllers and phones usually do. Devices that only support Bluetooth LE, such as some recent mice and keyboards, smartwatches and fitness bands, then cannot connect through this adapter.

1. Open `/etc/bluetooth/main.conf` as root. In the `[General]` section, replace the line `#ControllerMode = dual` with:
   ```ini
   ControllerMode = bredr
   ```
   Or run `sudo sed -i 's/^#ControllerMode = dual/ControllerMode = bredr/' /etc/bluetooth/main.conf`.
2. Run `sudo systemctl restart bluetooth`, then unplug the adapter and plug it back in.

To undo this, put the `#` back (`#ControllerMode = dual`) and repeat step 2.

<details>
<summary>Technical details</summary>

Seen on a UGREEN CM749 (`33fa:0010`, bcdDevice 88.91, HCI revision 0x0202) with kernel 7.2.11, whose stock `btusb` already supports the adapter. Traced with `btmon`:

- When the kernel disables an active LE scan (*LE Set Extended Scan Enable*, opcode 0x2042, about 5 s into the LE phase of a normal discovery that scans on the LE 1M and LE Coded PHYs), the firmware sometimes never answers. The next command, *Inquiry* (0x0401), gets *Memory Capacity Exceeded* (0x07) with `ncmd 0`. Every later `HCI_Reset` (0x0c03) gets the same status, until the adapter loses power.
- The same command sequence, with the same parameters, sometimes completes normally and sometimes hangs. This points to a race in the firmware rather than to a particular command. With the stock driver, three discoveries started with `bluetoothctl` completed normally: LE only, dual mode, and dual mode while discoverable. Opening the Bluetooth page of GNOME Settings, which makes the computer discoverable and starts a discovery, hung the adapter on the first scan cycle, about 8 s later. A resume from suspend also triggered the hang.
- **The fix:** [`btusb-barrot-le-coded.patch`](assets/btusb-barrot-le-coded.patch) sets the kernel's `HCI_QUIRK_BROKEN_LE_CODED` (kernel 6.6 and newer) for the Barrot IDs, so the kernel scans on LE 1M only. With the GNOME Settings Bluetooth page open, 42 scan cycles completed without an error: two DualSense controllers and a soundbar were connected and in use, and the computer was suspended and resumed. Why the Coded PHY scan triggers the race is not known.
- Forcing the legacy LE scan commands with `BTUSB_BROKEN_EXT_SCAN` (available since kernel 7.2) does not help. The firmware rejects *LE Set Scan Parameters* (0x200b) with *Command Disallowed* (`-16`), so LE scanning stops working altogether.
- With `ControllerMode = bredr`, the kernel only runs classic inquiries. With the GNOME Settings Bluetooth page open for three minutes, discovery stayed active the whole time with no kernel errors, and a connected soundbar stayed connected.

</details>

### Still not working

Open an [issue](https://github.com/TutorFx/bt-cm749-rs/issues) with:
- the full output of the installer;
- the output of `bt-cm749 detect`;
- the output of `lsusb | grep -i 33fa`;
- the output of `journalctl -k -b | grep -iE 'bluetooth|btusb'`.

## How it works

`bt-cm749 install` first checks the stock `btusb.ko` of the target kernel. It looks in `/usr/lib/modules/<kver>/kernel/...`, or in DKMS's backup of it if a DKMS module displaced it. If the stock module already contains the Barrot device IDs, the event-continuation fix and the LE Coded PHY fix, it stops there. On Ubuntu and its derivatives it also stops when only the LE Coded PHY fix is missing, because the module cannot be built there. `--force` overrides this check.

Otherwise it installs the build prerequisites with your package manager and registers a DKMS module that replaces the stock `btusb` driver. When DKMS builds the module for a kernel, including every future kernel you install (`AUTOINSTALL`), the following happens:

1. DKMS runs the binary's `prebuild` step, which downloads the matching upstream source from kernel.org.
2. The download is verified against kernel.org's `sha256sums.asc` and cached in `/var/cache/dkms-kernel-src`.
3. Only `drivers/bluetooth/` is extracted from the tarball.
4. Two patches are applied, in this order:
   - the [upstream Barrot patch](assets/bt-cm749.patch), with the device IDs and the event-continuation fix. If the kernel already contains it, the source is left untouched;
   - the [LE Coded PHY patch](assets/btusb-barrot-le-coded.patch), which sets `HCI_QUIRK_BROKEN_LE_CODED` for the Barrot IDs so the firmware does not hang during LE scans. It compiles to nothing on kernels older than 6.6, which lack the quirk.
5. Only `btusb.ko` is built, and it is installed to `updates/dkms` (`extra/` on Fedora).

If any step fails, or the installation is interrupted (`Ctrl+C`, `SIGTERM`), `install` rolls the system back. It removes the DKMS registration, the source tree in `/usr/src` and any partial downloads, then reloads the stock driver. The process exits with the original error code (130/143 for signals).

### Command-line reference

```bash
sudo bt-cm749                  # interactive wizard (needs a terminal)
bt-cm749 detect                # what was detected (no root needed)
sudo bt-cm749 install          # install (skipped if the stock driver has the fix)
sudo bt-cm749 install --force  # install anyway
sudo bt-cm749 uninstall        # remove and go back to the stock driver
```

| Flag | Environment variable | Default |
|---|---|---|
| `--kernel <release>` | `KERNEL_VERSION` | newest installed `linux-image-*` on Debian, else the running kernel |
| `install --force` | | install even if the stock driver already has the fix |
| `--modules-root <dir>` | `BT_CM749_MODULES_ROOT` | `/usr/lib/modules` |
| `--os-release <path>` | `OS_RELEASE_FILE` | `/etc/os-release` |
| `--usr-src <dir>` | `CUSTOM_USR_SRC` | `/usr/src` |
| `--cache-dir <dir>` | `BT_CM749_CACHE_DIR` | `/var/cache/dkms-kernel-src`, else `/tmp/dkms-kernel-src` |
| `--skip-root-check` | `SKIP_ROOT_CHECK` | off |

| Exit code | Meaning |
|---|---|
| 1 | Not root, patch failure, or I/O error |
| 2 | Kernel source unavailable |
| 3 | Missing headers or gcc < 12 without `gcc-12` |
| 4 | Kernel version could not be determined |
| other | Exit code of the failing `dkms`/package-manager command |
| 130 / 143 | Interrupted by `SIGINT` / `SIGTERM` |

## Known limitations

- **Ubuntu stable kernels.** Ubuntu's kernels backport Bluetooth core changes from newer kernels, so the vanilla kernel.org `btusb.c` no longer compiles against their headers. Every supported Ubuntu 24.04 kernel (6.8.0-146, HWE 6.17 and 7.0) already supports the adapters, so the installer skips them. Only the end-of-life HWE kernels 6.11 and 6.14 lack that support; upgrade the kernel instead. The LE Coded PHY fix is not available on Ubuntu either. If the adapter hangs there, use the `ControllerMode = bredr` workaround from [Bluetooth works for a few seconds, then stops](#bluetooth-works-for-a-few-seconds-then-stops).
- **The LE Coded PHY fix needs kernel 6.6 or newer**, the first version with `HCI_QUIRK_BROKEN_LE_CODED`. On older kernels the patch compiles to nothing.
- **A future kernel fix for the LE scan hang will not be recognised.** The installer recognises that fix by the message its own patch logs. If a kernel fixes the hang in another way, the installer still treats it as missing and installs the module, until bt-cm749 is updated to recognise it.
- Fedora's DKMS installs modules to `extra/` instead of `updates/dkms`. `depmod` still prefers them over the built-in driver.

## Development

Requires Rust 1.87 or newer.

```bash
cargo build --release        # target/release/bt-cm749
make test        # unit + sandboxed integration tests (no root, no network)
make lint        # rustfmt + clippy
make test-all    # also downloads kernel sources, compiles btusb.ko against the
                 # running kernel's headers and checks parity with the shell scripts
make static      # static musl binary in dist/ (built in an Alpine container)
make e2e         # real DKMS install/uninstall in Arch and Fedora containers,
                 # and the stock-driver skip on Ubuntu 24.04
```

Integration tests run the binary with a `PATH` that contains only logging mocks, so no real `dkms`, `modprobe` or package manager is ever invoked. The patch engine is checked against GNU patch output on real `btusb.c` sources from 6.6, 6.12, 7.1 and 7.2 (`tests/fixtures/`).

### CI and releases

[`ci.yml`](.github/workflows/ci.yml) runs on every push and pull request:
- formatting, clippy and the test suite, plus a run on the minimum Rust version (1.87);
- shellcheck on the installer and the CI scripts;
- the patch against real kernel sources;
- the static x86_64 and aarch64 binaries ([`build.yml`](.github/workflows/build.yml));
- a real DKMS install/uninstall in Arch, Fedora and Ubuntu containers ([`ci/dkms-e2e.sh`](ci/dkms-e2e.sh));
- a test of the one-command installer in its non-interactive mode ([`ci/installer-test.sh`](ci/installer-test.sh)).

The wizard's logic (menu choices, adapter detection, step descriptions, command logging) is unit-tested. Its screens were checked by driving the binary in a pseudo-terminal.

To publish a release, push a version tag:
```bash
git tag v0.3.0 && git push origin v0.3.0
```
[`release.yml`](.github/workflows/release.yml) then builds the binaries and their `.sha256` files, and attaches them to a GitHub release together with `install.sh` and install instructions. The install command in this README always fetches the latest release.

## Fixes over the shell version

This is a Rust rewrite of [mxpph/bt-cm749-fix](https://github.com/mxpph/bt-cm749-fix), a set of shell scripts that are themselves a fork of [xoocoon/hp-15-ew0xxx-snd-fix](https://github.com/xoocoon/hp-15-ew0xxx-snd-fix). The rewrite ships as a single static binary. It keeps the scripts' behaviour, including the automatic rollback, uninstall and multi-distro test suite added on top of the fork, and fixes the problems listed below.

Bugs in the original scripts:

- **Wrong kernel source on DKMS rebuilds.** dkms does not export `kernelver` to `PRE_BUILD` scripts, so the scripts fell back to `uname -r`. When DKMS rebuilt the module for a newly installed kernel, it patched the source of the *running* kernel. `dkms.conf` now passes `--kernel ${kernelver}`, which dkms expands itself.
- **Fedora could not download the source.** `6.12.9-200.fc41.x86_64` was parsed as source version `6.12.x86_64`, which returned a 404.
- **Silent corruption on kernels that already have the fix (7.2+).** GNU patch rejected hunk #1, re-applied the others with fuzz and duplicated the device IDs, while `grep BTUSB_BARROT` still reported success. The new patch engine detects already-merged hunks and leaves the source untouched.
- **Installs hung without a TTY.** `apt install` and `dnf install` ran without `-y`.
- **Wrong kernel on Debian.** The newest `linux-image-*` was chosen with a lexicographic `sort -r`, so `6.8.0-9` ranked above `6.8.0-31`, and removed (`rc`) packages counted too.
- **Wrong headers check on Debian.** It grepped `dpkg -l`, which also matched other packages whose name contained the headers package's name.
- **Derivatives not recognised.** The `os-release` regex ignored quoting and `ID_LIKE` lists, so Linux Mint, Manjaro, EndeavourOS and Nobara were not detected.
- **Leftover directory on gcc failure.** The gcc ≥ 12 check ran after `/usr/src` had already been written, so the rollback had to clean up.
- **`SIGTERM` did not trigger the rollback.** A `SIGTERM` sent only to the installer never reached the running `dkms` or package-manager command.

Additions:

- **Fix for the firmware hang during LE scans.** A second patch keeps the adapters off the LE Coded PHY. Their firmware can hang when an LE scan on that PHY is disabled, and then stays broken until the adapter is unplugged. See [Bluetooth works for a few seconds, then stops](#bluetooth-works-for-a-few-seconds-then-stops).
- **Skips installs that are not needed.** `install` first inspects the kernel's own `btusb` module, and does nothing when it already has every fix. On Ubuntu kernels it also does nothing when the driver already supports the adapters, because the module cannot be built there; this covers Ubuntu 24.04's 6.8.0-146 and its HWE kernels. `--force` overrides this.
- **Verified downloads.** Kernel tarballs are checked against kernel.org's `sha256sums.asc`, and concurrent downloads of the same tarball are serialised with a lock.
- **Only `btusb.ko` is built.** The scripts built the whole `drivers/bluetooth` directory; unrelated drivers there break the build against some distro headers.
- **Static musl binary.** The installer copies itself into the DKMS tree, so it keeps working across libc upgrades.
- **Broader test suite.** Golden tests against GNU patch on real `btusb.c` sources, a real `SIGINT` rollback test, a parity test against the shell scripts, and real DKMS install/uninstall cycles in Arch, Fedora and Ubuntu containers.

See [CHANGELOG.md](CHANGELOG.md) for the details.

## Credit

- Fork of [mxpph/bt-cm749-fix](https://github.com/mxpph/bt-cm749-fix) by mxpph, which adapted the scripts to the UGREEN CM748/CM749 adapters.
- Based on [xoocoon/hp-15-ew0xxx-snd-fix](https://github.com/xoocoon/hp-15-ew0xxx-snd-fix) by xoocoon and contributors: the DKMS + kernel-source patching approach.
- The Barrot patch is the upstream kernel commit [7722d6fb54e4](https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/patch/drivers/bluetooth?id=7722d6fb54e428a8f657fccf422095a8d7e2d72c).
- The LE Coded PHY patch relies on `HCI_QUIRK_BROKEN_LE_CODED`, added to the kernel in commit [253f3399f4c0](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/commit/?id=253f3399f4c09ce6f4e67350f839be0361b4d5ff) for Intel controllers that misbehave on that PHY.
