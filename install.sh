#!/bin/sh
# One-command installer for bt-cm749, the Bluetooth fix for UGREEN CM748/CM749
# and other Barrot BR8554 based USB adapters (IDs 33fa:0010 / 33fa:0012).
#
# It downloads the prebuilt program from the project's GitHub Releases, checks
# its sha256 checksum, installs it to /usr/local/bin and starts its interactive
# wizard:
#
#   curl -fsSL https://github.com/TutorFx/bt-cm749-rs/releases/latest/download/install.sh | sudo sh
#
# Remove everything again with:
#
#   curl -fsSL https://github.com/TutorFx/bt-cm749-rs/releases/latest/download/install.sh | sudo sh -s -- --uninstall
#
# Without a terminal (scripts, CI), or with --force/--uninstall, it runs the steps
# itself without prompts. That non-interactive mode will be retired once the
# wizard covers every case.
#
# Environment overrides (mostly for testing): BT_CM749_REPO, BT_CM749_VERSION (a
# release tag, default latest), BT_CM749_BASE_URL (a directory with the release
# files), BT_CM749_BIN_DIR (default /usr/local/bin), BT_CM749_NONINTERACTIVE=1.
set -eu

REPO="${BT_CM749_REPO:-TutorFx/bt-cm749-rs}"
VERSION="${BT_CM749_VERSION:-latest}"
BASE_URL="${BT_CM749_BASE_URL:-}"
BIN_DIR="${BT_CM749_BIN_DIR:-/usr/local/bin}"
BIN="${BIN_DIR}/bt-cm749"
ISSUES="https://github.com/${REPO}/issues"

if [ -t 1 ]; then
  BOLD=$(printf '\033[1m'); GREEN=$(printf '\033[32m'); YELLOW=$(printf '\033[33m'); RED=$(printf '\033[31m'); RESET=$(printf '\033[0m')
else
  BOLD=''; GREEN=''; YELLOW=''; RED=''; RESET=''
fi
step() { printf '%s==> %s%s\n' "$BOLD" "$*" "$RESET"; }
ok() { printf '%s%s%s\n' "$GREEN" "$*" "$RESET"; }
warn() { printf '%s%s%s\n' "$YELLOW" "$*" "$RESET"; }
fail() {
  printf '%sError: %s%s\n' "$RED" "$*" "$RESET" >&2
  exit 1
}

usage() {
  cat <<EOF
Usage: install.sh [--uninstall] [--force]

  (no option)   install the Bluetooth fix (skipped if your kernel already has it)
  --uninstall   remove the fix and go back to your distribution's driver
  --force       install even if your kernel's own driver already has the fix
EOF
}

ACTION=install
FORCE=''
for arg in "$@"; do
  case "$arg" in
    --uninstall) ACTION=uninstall ;;
    --force) FORCE=--force ;;
    -h | --help) usage; exit 0 ;;
    *) usage >&2; fail "unknown option: $arg" ;;
  esac
done

[ "$(uname -s)" = Linux ] || fail "this fix is for Linux only."
if [ "$(id -u)" -ne 0 ]; then
  # Started from a downloaded file: ask for the password once and start over.
  if [ -f "$0" ] && command -v sudo >/dev/null 2>&1; then
    exec sudo sh "$0" "$@"
  fi
  fail "administrator rights are needed. Run the command again with 'sudo' before 'sh', as shown in the instructions."
fi

case "$(uname -m)" in
  x86_64 | amd64) ARCH=x86_64 ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) fail "unsupported processor type '$(uname -m)'. Only 64-bit Intel/AMD (x86_64) and ARM (aarch64) are supported." ;;
esac
ASSET="bt-cm749-${ARCH}-linux-musl"

if [ -n "$BASE_URL" ]; then
  URL="$BASE_URL"
elif [ "$VERSION" = latest ]; then
  URL="https://github.com/${REPO}/releases/latest/download"
else
  URL="https://github.com/${REPO}/releases/download/${VERSION}"
fi

download() { # url dest
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$2" "$1"
  else
    fail "neither curl nor wget is installed. Install one of them with your package manager and try again."
  fi
}

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
trap 'exit 130' INT TERM

fetch_program() {
  step "Downloading bt-cm749 (${ARCH}) from ${URL}"
  download "${URL}/${ASSET}" "${TMP}/${ASSET}" || fail "download failed. Check your internet connection and try again."
  download "${URL}/${ASSET}.sha256" "${TMP}/${ASSET}.sha256" || fail "could not download the checksum file."
  step "Checking the download"
  (cd "$TMP" && sha256sum -c --status "${ASSET}.sha256") || fail "the downloaded file is damaged (checksum mismatch). Nothing was changed; please try again."
  mkdir -p "$BIN_DIR"
  install -m 0755 "${TMP}/${ASSET}" "$BIN"
}

if [ "$ACTION" = uninstall ]; then
  [ -x "$BIN" ] || fetch_program
  step "Removing the Bluetooth fix"
  "$BIN" uninstall || fail "uninstalling failed. Please report it at ${ISSUES} with the messages above."
  rm -f "$BIN"
  echo
  ok "Done. Your distribution's original Bluetooth driver is active again."
  exit 0
fi

fetch_program

# In a terminal, hand over to the wizard: it shows what it found, asks before
# changing anything and explains every step. Keys come from /dev/tty because stdin
# is this script when it is piped from curl.
if [ -z "$FORCE" ] && [ -z "${BT_CM749_NONINTERACTIVE:-}" ] && [ -t 2 ] && { : </dev/tty; } 2>/dev/null; then
  status=0
  "$BIN" </dev/tty || status=$?
  exit "$status"
fi

step "Installing the Bluetooth fix (this can take a few minutes)"
# Show the output live while keeping a copy, and keep the exit code (no pipefail in POSIX sh).
# shellcheck disable=SC2086 # FORCE is either empty or a single flag
{ "$BIN" install $FORCE 2>&1; echo $? >"${TMP}/rc"; } | tee "${TMP}/install.log"
if [ "$(cat "${TMP}/rc")" -ne 0 ]; then
  echo
  fail "the installation failed and was undone, so your system is unchanged. Please report it at ${ISSUES} and include the messages above."
fi

# Nothing was built when the kernel's own driver already supports the adapter.
if grep -q "Nothing to install" "${TMP}/install.log"; then
  echo
  ok "All set: your kernel already supports this adapter, so nothing had to be installed."
  echo "If Bluetooth still does not work, unplug the adapter, plug it back in and restart your computer."
  exit 0
fi

step "Reloading the Bluetooth driver"
if modprobe -r btusb 2>/dev/null && modprobe btusb 2>"${TMP}/modprobe.err"; then
  echo
  ok "Done! Unplug the adapter and plug it back in, then turn Bluetooth on in your settings."
elif grep -q "Key was rejected" "${TMP}/modprobe.err" 2>/dev/null; then
  echo
  warn "Almost done: Secure Boot is blocking the new driver. To allow it, run:"
  echo
  echo "    sudo mokutil --import /var/lib/dkms/mok.pub"
  echo
  warn "Choose a one-time password, restart your computer, pick 'Enroll MOK' on the blue screen"
  warn "that appears during startup and type the same password."
else
  echo
  ok "Done! Restart your computer to start using the fixed driver."
fi
