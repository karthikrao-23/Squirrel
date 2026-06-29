#!/usr/bin/env bash
# Squirrel — check and (optionally) install the services the app needs.
#
# Installs only what's missing:
#   Rust (rustup) · Node.js 20+ · Docker · openssl · curl
#
# Usage:
#   ./setup.sh           check, then ask before installing each missing tool
#   ./setup.sh --yes     install everything missing without prompting
#   ./setup.sh --check   only report what's present/missing (install nothing)
#   ./setup.sh --help    show this help
#
# Notes:
#   • Idempotent — anything already present is left untouched.
#   • Rust installs into your user account via rustup (no sudo).
#   • Node/openssl install via your OS package manager (Homebrew / apt / dnf /
#     pacman); some need sudo.
#   • Docker can't be fully auto-installed everywhere: on macOS it's a GUI app
#     (installed via Homebrew cask, but you launch it yourself); on Linux it
#     needs sudo and a re-login for group membership. The script does what it
#     safely can and tells you any remaining manual step.
#
# After this, run ./run.sh to start the app.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

# ---- logging ----------------------------------------------------------------
if [[ -t 1 ]]; then
  bold=$'\033[1m'; blue=$'\033[1;34m'; green=$'\033[1;32m'; yellow=$'\033[1;33m'; red=$'\033[1;31m'; dim=$'\033[2m'; rst=$'\033[0m'
else
  bold=''; blue=''; green=''; yellow=''; red=''; dim=''; rst=''
fi
log()  { printf '%s==>%s %s\n' "$blue" "$rst" "$*"; }
ok()   { printf '%s ✓ %s%s\n' "$green" "$*" "$rst"; }
warn() { printf '%s ! %s%s\n' "$yellow" "$*" "$rst"; }
die()  { printf '%s error:%s %s\n' "$red" "$rst" "$*" >&2; exit 1; }

# ---- args -------------------------------------------------------------------
ASSUME_YES=0; CHECK_ONLY=0
case "${1:-}" in
  -h|--help)  awk 'NR==1{next} /^#/{sub(/^# ?/,"");print;next} {exit}' "$0"; exit 0 ;;
  --yes|-y)   ASSUME_YES=1 ;;
  --check)    CHECK_ONLY=1 ;;
  "")         ;;
  *)          die "unknown option '$1' (try --help)" ;;
esac

# ---- platform detection -----------------------------------------------------
# PLATFORM is normalized (macOS|Linux|Windows|unknown); PM is the package manager.
PLATFORM="unknown"
PM=""              # brew | apt | dnf | pacman | zypper | winget
PM_INSTALL=""      # install command prefix (the simple Linux managers only)
case "$(uname -s)" in
  Darwin) PLATFORM="macOS"; PM="brew" ;;
  Linux)
    PLATFORM="Linux"
    if   command -v apt-get >/dev/null 2>&1; then PM="apt";    PM_INSTALL="sudo apt-get install -y"
    elif command -v dnf     >/dev/null 2>&1; then PM="dnf";    PM_INSTALL="sudo dnf install -y"
    elif command -v pacman  >/dev/null 2>&1; then PM="pacman"; PM_INSTALL="sudo pacman -S --noconfirm"
    elif command -v zypper  >/dev/null 2>&1; then PM="zypper"; PM_INSTALL="sudo zypper install -y"
    fi ;;
  MINGW*|MSYS*|CYGWIN*|Windows_NT)
    PLATFORM="Windows"
    command -v winget >/dev/null 2>&1 && PM="winget" ;;
  *) warn "unrecognized OS '$(uname -s)' — I can check, but you'll install manually." ;;
esac

# On Windows, the most reliable path is WSL2 (then everything follows the Linux
# branch). We still run best-effort under Git Bash / MSYS via winget.
if [[ "$PLATFORM" == "Windows" ]]; then
  warn "Windows detected. Recommended: WSL2 — run 'wsl --install' in an admin"
  warn "PowerShell, then run these scripts inside the Ubuntu shell (Linux path)."
  warn "Continuing best-effort in this shell${PM:+ via $PM}."
  [[ -z "$PM" ]] && warn "winget not found — install the prerequisites manually (links below)."
fi

# ---- consent ----------------------------------------------------------------
# Ask before changing the system. --yes assumes yes; --check always declines.
confirm() {  # confirm "<prompt>"
  (( CHECK_ONLY )) && return 1
  (( ASSUME_YES )) && return 0
  local reply
  printf '%s ?%s %s [y/N] ' "$yellow" "$rst" "$1"
  read -r reply </dev/tty || reply=""
  [[ "$reply" =~ ^[Yy]$ ]]
}

brew_install() {  # brew_install <pkg> [--cask]
  command -v brew >/dev/null 2>&1 || die "Homebrew not found — install it from https://brew.sh, then re-run."
  if [[ "${2:-}" == "--cask" ]]; then brew install --cask "$1"; else brew install "$1"; fi
}

# Generic "install via this OS's package manager".
winget_install() {  # winget_install <winget-id>
  winget install --silent --accept-package-agreements --accept-source-agreements --id "$1"
}
pm_install() {  # pm_install <brew-name> <linux-name> <winget-id>
  case "$PM" in
    brew) brew_install "$1" ;;
    apt|dnf|pacman|zypper) eval "$PM_INSTALL $2" ;;
    winget) winget_install "$3" ;;
    *) return 1 ;;
  esac
}

# Fallback Node install: the official prebuilt binary from nodejs.org, into
# /usr/local. Used when the package manager can't provide Node (e.g. Homebrew has
# no bottle on a pre-release macOS, or an unusual Linux distro). Works on any
# macOS/Linux version for x64/arm64.
install_node_binary() {
  local os arch
  case "$PLATFORM" in macOS) os=darwin ;; Linux) os=linux ;; *) return 1 ;; esac
  case "$(uname -m)" in
    arm64|aarch64) arch=arm64 ;;
    x86_64|amd64)  arch=x64 ;;
    *) warn "no Node binary for arch $(uname -m)"; return 1 ;;
  esac
  # Latest v22 LTS from the dist index, with a pinned fallback if that lookup fails.
  local ver
  ver="$(curl -fsSL https://nodejs.org/dist/index.json 2>/dev/null \
        | grep -o '"version":"v22[0-9.]*"' | head -1 | cut -d'"' -f4)"
  ver="${ver:-v22.12.0}"
  local pkg="node-${ver}-${os}-${arch}" tmp
  tmp="$(mktemp -d)"
  log "Downloading Node ${ver} (${os}-${arch}) from nodejs.org"
  if ! curl -fsSL "https://nodejs.org/dist/${ver}/${pkg}.tar.gz" -o "$tmp/node.tgz"; then
    rm -rf "$tmp"; return 1
  fi
  tar -xzf "$tmp/node.tgz" -C "$tmp" || { rm -rf "$tmp"; return 1; }
  # Install into /usr/local, per-subdir. bin + lib are all that's needed to run
  # node/npm; include + share (native-addon headers, man pages) are best-effort.
  # sudo is used only for a target that isn't already user-writable.
  local rc=0
  _cp_sub() {  # _cp_sub <subdir> <required?>
    local sub="$1" required="$2" s=""
    [[ -d "$tmp/$pkg/$sub" ]] || return 0
    if [[ -w "/usr/local/$sub" ]] || { [[ ! -e "/usr/local/$sub" ]] && [[ -w /usr/local ]]; }; then s=""; else s="sudo"; fi
    if $s mkdir -p "/usr/local/$sub" 2>/dev/null && $s cp -R "$tmp/$pkg/$sub/." "/usr/local/$sub/" 2>/dev/null; then
      return 0
    fi
    [[ "$required" == "required" ]] && rc=1
    return 0
  }
  _cp_sub bin required
  _cp_sub lib required
  _cp_sub include optional
  _cp_sub share optional
  rm -rf "$tmp"
  hash -r 2>/dev/null || true
  return $rc
}

# ---- per-tool handlers ------------------------------------------------------
# Each returns 0 if usable (already present or installed), 1 if still missing.

ensure_rust() {
  if command -v cargo >/dev/null 2>&1; then ok "Rust (cargo $(cargo --version | awk '{print $2}'))"; return 0; fi
  warn "Rust (cargo) is missing"
  if [[ "$PLATFORM" == "Windows" ]]; then
    if [[ "$PM" == "winget" ]] && confirm "Install Rust via winget (Rustlang.Rustup)?"; then
      winget_install Rustlang.Rustup
      warn "Rust installed — open a new terminal so cargo is on PATH"; return 0
    fi
    return 1
  fi
  if confirm "Install Rust via rustup (https://rustup.rs)?"; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # Make cargo usable for the rest of this script + tell the user about new shells.
    [[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
    if command -v cargo >/dev/null 2>&1; then
      ok "Rust installed"; warn "open a new terminal (or 'source ~/.cargo/env') so cargo is on PATH"; return 0
    fi
  fi
  return 1
}

ensure_node() {
  if command -v node >/dev/null 2>&1; then
    local major; major="$(node -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0)"
    if (( major >= 20 )); then ok "Node.js ($(node --version))"; return 0; fi
    warn "Node.js $(node --version) found, but 20+ is required"
  else
    warn "Node.js is missing"
  fi
  if confirm "Install Node.js (LTS) via $PM?"; then
    case "$PM" in
      brew)   brew_install node ;;
      apt)    curl -fsSL https://deb.nodesource.com/setup_lts.x | sudo -E bash - && sudo apt-get install -y nodejs ;;
      dnf)    sudo dnf install -y nodejs ;;
      pacman) sudo pacman -S --noconfirm nodejs npm ;;
      zypper) sudo zypper install -y nodejs ;;
      winget) winget_install OpenJS.NodeJS.LTS
              warn "Node.js installed — open a new terminal so node/npm are on PATH"; return 0 ;;
      *) ;;  # no package manager — fall through to the binary install below
    esac
    command -v node >/dev/null 2>&1 && { ok "Node.js installed ($(node --version))"; return 0; }
    # Package manager couldn't provide Node (missing bottle, odd distro, no PM):
    # fall back to the official prebuilt binary.
    if [[ "$PLATFORM" == "macOS" || "$PLATFORM" == "Linux" ]]; then
      warn "package manager didn't provide Node — falling back to the nodejs.org binary"
      install_node_binary && command -v node >/dev/null 2>&1 \
        && { ok "Node.js installed ($(node --version))"; return 0; }
    fi
  fi
  return 1
}

ensure_simple() {  # ensure_simple <command> <brew-name> <linux-name> <winget-id> <label>
  if command -v "$1" >/dev/null 2>&1; then ok "$5"; return 0; fi
  warn "$5 is missing"
  if confirm "Install $5 via $PM?"; then
    if pm_install "$2" "$3" "$4"; then
      # winget installs may not be on PATH until a new shell; accept optimistically.
      if command -v "$1" >/dev/null 2>&1 || [[ "$PM" == "winget" ]]; then ok "$5 installed"; return 0; fi
    fi
  fi
  return 1
}

ensure_docker() {
  if command -v docker >/dev/null 2>&1; then
    if docker info >/dev/null 2>&1; then ok "Docker (running)"; else
      ok "Docker installed"; warn "the Docker daemon isn't running — start Docker Desktop / 'sudo systemctl start docker'"
    fi
    return 0
  fi
  warn "Docker is missing"
  case "$PLATFORM" in
    macOS)
      if confirm "Install Docker Desktop via Homebrew cask?"; then
        brew_install docker --cask
        warn "Docker Desktop installed — LAUNCH it once (Applications → Docker) to start the daemon, then re-run ./run.sh"
        return 0
      fi ;;
    Linux)
      warn "Docker on Linux needs sudo and a re-login for group membership."
      if confirm "Install Docker Engine via the official convenience script (https://get.docker.com)?"; then
        curl -fsSL https://get.docker.com | sudo sh
        sudo usermod -aG docker "$USER" || true
        warn "added you to the 'docker' group — LOG OUT and back in (or 'newgrp docker') so it takes effect"
        return 0
      fi ;;
    Windows)
      if [[ "$PM" == "winget" ]] && confirm "Install Docker Desktop via winget?"; then
        winget_install Docker.DockerDesktop
        warn "Docker Desktop installed — enable the WSL2 backend, LAUNCH it once, then re-open this shell"
        return 0
      fi
      warn "install Docker Desktop from https://www.docker.com/products/docker-desktop (enable the WSL2 backend)" ;;
  esac
  return 1
}

# ---- run --------------------------------------------------------------------
log "Detected: $PLATFORM${PM:+ · package manager: $PM}"
(( CHECK_ONLY )) && log "Check-only mode — reporting status, installing nothing."

still_missing=()
ensure_rust                                                   || still_missing+=("Rust (https://rustup.rs)")
ensure_node                                                   || still_missing+=("Node.js 20+ (https://nodejs.org)")
ensure_simple openssl openssl openssl ShiningLight.OpenSSL.Light openssl || still_missing+=("openssl")
ensure_simple curl    curl    curl    cURL.cURL               curl    || still_missing+=("curl")
ensure_docker                                                 || still_missing+=("Docker (https://www.docker.com/products/docker-desktop)")

echo
if (( ${#still_missing[@]} == 0 )); then
  ok "All set."
  (( CHECK_ONLY )) || { log "Next: ./run.sh"; }
else
  warn "Still missing / needs a manual step:"
  for m in "${still_missing[@]}"; do printf '   - %s\n' "$m"; done
  (( CHECK_ONLY )) && exit 0
  die "install the items above, then re-run ./setup.sh (or ./run.sh)"
fi
