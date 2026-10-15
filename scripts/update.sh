#!/usr/bin/env sh
# kb-update: checks for and installs updates for kb.
# Usage:
#   kb-update [--check] [--force] [--version <ver>]
set -eu

KB_REPO="${KB_REPO:-cortiz/kb}"
KB_VERSION="${KB_VERSION:-latest}"
KB_INSTALL_DIR="${KB_INSTALL_DIR:-$HOME/.local/bin}"
CHECK_ONLY=0
FORCE=0

# Colors (if terminal supports it)
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  BOLD="\033[1m"
  GREEN="\033[32m"
  BLUE="\033[34m"
  YELLOW="\033[33m"
  RED="\033[31m"
  RESET="\033[0m"
else
  BOLD=""
  GREEN=""
  BLUE=""
  YELLOW=""
  RED=""
  RESET=""
fi

log_info() {
  printf "${BLUE}info:${RESET} %s\n" "$1"
}

log_success() {
  printf "${GREEN}${BOLD}success:${RESET} %s\n" "$1"
}

log_warn() {
  printf "${YELLOW}warning:${RESET} %s\n" "$1"
}

log_error() {
  printf "${RED}${BOLD}error:${RESET} %s\n" "$1" >&2
}

usage() {
  cat << USAGE
kb-update - checks for and installs updates for kb

Usage:
  kb-update [options]

Options:
  -c, --check            Check if an update is available without installing
  -f, --force            Reinstall even if kb is already at the latest version
  -v, --version <ver>    Update/downgrade to a specific version (default: latest)
  -b, --bin-dir <dir>    Custom installation directory (default: ~/.local/bin)
  --repo <owner/repo>    GitHub repository (default: cortiz/kb)
  -h, --help             Show this help message
USAGE
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    -c|--check)
      CHECK_ONLY=1
      shift
      ;;
    -f|--force)
      FORCE=1
      shift
      ;;
    -v|--version)
      KB_VERSION="$2"
      shift 2
      ;;
    -b|--bin-dir)
      KB_INSTALL_DIR="$2"
      shift 2
      ;;
    --repo)
      KB_REPO="$2"
      shift 2
      ;;
    -h|--help)
      usage
      ;;
    *)
      log_error "Unknown option: $1"
      usage
      ;;
  esac
done

# Locate existing kb binary
KB_BIN=""
if [ -x "$KB_INSTALL_DIR/kb" ]; then
  KB_BIN="$KB_INSTALL_DIR/kb"
elif command -v kb >/dev/null 2>&1; then
  KB_BIN="$(command -v kb)"
  KB_INSTALL_DIR="$(dirname "$KB_BIN")"
fi

CURRENT_VER=""
if [ -n "$KB_BIN" ] && [ -x "$KB_BIN" ]; then
  VER_OUTPUT=$("$KB_BIN" --version 2>/dev/null || true)
  # Extract version number (e.g. "kb 0.2.0" -> "0.2.0")
  CURRENT_VER=$(printf '%s\n' "$VER_OUTPUT" | sed -E 's/.*kb ([0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?).*/\1/')
  if [ -z "$CURRENT_VER" ]; then
    CURRENT_VER=$(printf '%s\n' "$VER_OUTPUT" | awk '{print $NF}')
  fi
fi

if [ -n "$CURRENT_VER" ]; then
  log_info "Current installed version: ${BOLD}${CURRENT_VER}${RESET} (${KB_BIN})"
else
  log_warn "No existing kb installation detected in PATH or ${KB_INSTALL_DIR}."
fi

# Check for download tool
if command -v curl >/dev/null 2>&1; then
  HTTP_GET="curl -fsSL"
elif command -v wget >/dev/null 2>&1; then
  HTTP_GET="wget -qO-"
else
  log_error "curl or wget is required to check for updates."
  exit 1
fi

# Detect platform
OS="$(uname -s)"
case "$OS" in
  Linux*)  OS_TYPE="linux" ;;
  Darwin*) OS_TYPE="darwin" ;;
  *)
    log_error "Unsupported operating system: $OS"
    exit 1
    ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH_TYPE="x86_64" ;;
  aarch64|arm64) ARCH_TYPE="arm64" ;;
  *)
    log_error "Unsupported architecture: $ARCH"
    exit 1
    ;;
esac

if [ "$OS_TYPE" = "linux" ] && [ "$ARCH_TYPE" = "x86_64" ]; then
  PLATFORM="linux-x86_64"
elif [ "$OS_TYPE" = "linux" ] && [ "$ARCH_TYPE" = "arm64" ]; then
  PLATFORM="linux-arm64"
elif [ "$OS_TYPE" = "darwin" ] && [ "$ARCH_TYPE" = "arm64" ]; then
  PLATFORM="macos-apple-silicon"
elif [ "$OS_TYPE" = "darwin" ] && [ "$ARCH_TYPE" = "x86_64" ]; then
  PLATFORM="macos-intel"
else
  log_error "Unsupported platform: $OS_TYPE on $ARCH_TYPE"
  exit 1
fi

# Fetch target release tag
TAG="$KB_VERSION"
if [ "$TAG" = "latest" ]; then
  log_info "Checking latest release from github.com/${KB_REPO}..."
  LATEST_JSON=$($HTTP_GET "https://api.github.com/repos/${KB_REPO}/releases/latest" 2>/dev/null || true)
  if [ -n "$LATEST_JSON" ]; then
    TAG=$(printf '%s\n' "$LATEST_JSON" | grep '"tag_name":' | head -n1 | sed -E 's/.*"tag_name": "([^"]+)".*/\1/')
  fi

  if [ -z "$TAG" ] || [ "$TAG" = "latest" ]; then
    REDIRECT_URL=$(curl -sIL -o /dev/null -w '%{url_effective}' "https://github.com/${KB_REPO}/releases/latest" 2>/dev/null || true)
    case "$REDIRECT_URL" in
      */releases/tag/*)
        TAG="${REDIRECT_URL##*/}"
        ;;
    esac
  fi

  if [ -z "$TAG" ] || [ "$TAG" = "latest" ]; then
    log_error "Could not fetch latest release information for ${KB_REPO}."
    exit 1
  fi
fi

CLEAN_TARGET="${TAG#v}"
CLEAN_CURRENT="${CURRENT_VER#v}"

log_info "Latest available release: ${BOLD}${TAG}${RESET}"

# Compare versions
if [ "$CLEAN_CURRENT" = "$CLEAN_TARGET" ] && [ "$FORCE" -eq 0 ]; then
  log_success "kb is already at the latest version (${BOLD}${CURRENT_VER}${RESET})."
  exit 0
fi

if [ "$CHECK_ONLY" -eq 1 ]; then
  if [ "$CLEAN_CURRENT" != "$CLEAN_TARGET" ]; then
    printf "\n${YELLOW}${BOLD}Update available:${RESET} %s -> ${GREEN}${BOLD}%s${RESET}\n" "${CURRENT_VER:-none}" "$TAG"
    printf "Run %skb-update%s to install.\n\n" "$BOLD" "$RESET"
  fi
  exit 0
fi

# Perform update
log_info "Updating kb to ${BOLD}${TAG}${RESET} ($PLATFORM)..."

TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t 'kb-update')
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

ARCHIVE_NAME="kb-${CLEAN_TARGET}-${PLATFORM}.tar.gz"
DOWNLOAD_URL="${KB_DOWNLOAD_URL:-https://github.com/${KB_REPO}/releases/download/${TAG}/${ARCHIVE_NAME}}"
CHECKSUMS_URL="${KB_CHECKSUMS_URL:-https://github.com/${KB_REPO}/releases/download/${TAG}/SHA256SUMS}"

log_info "Downloading ${DOWNLOAD_URL}..."
if ! $HTTP_GET "$DOWNLOAD_URL" > "$TMP_DIR/$ARCHIVE_NAME"; then
  log_error "Failed to download $DOWNLOAD_URL"
  exit 1
fi

# Checksum verification
if [ -z "${KB_SKIP_CHECKSUM:-}" ]; then
  log_info "Verifying SHA-256 checksum..."
  if $HTTP_GET "$CHECKSUMS_URL" > "$TMP_DIR/SHA256SUMS" 2>/dev/null; then
    EXPECTED_HASH=$(awk -v f="$ARCHIVE_NAME" '$2 == f { print $1 }' "$TMP_DIR/SHA256SUMS")
    if [ -n "$EXPECTED_HASH" ]; then
      if command -v sha256sum >/dev/null 2>&1; then
        ACTUAL_HASH=$(sha256sum "$TMP_DIR/$ARCHIVE_NAME" | cut -d' ' -f1)
      elif command -v shasum >/dev/null 2>&1; then
        ACTUAL_HASH=$(shasum -a 256 "$TMP_DIR/$ARCHIVE_NAME" | cut -d' ' -f1)
      else
        ACTUAL_HASH=""
      fi

      if [ -n "$ACTUAL_HASH" ]; then
        if [ "$EXPECTED_HASH" != "$ACTUAL_HASH" ]; then
          log_error "Checksum mismatch!"
          log_error "Expected: $EXPECTED_HASH"
          log_error "Actual:   $ACTUAL_HASH"
          exit 1
        fi
        log_success "Checksum verified: ${ACTUAL_HASH}"
      fi
    fi
  fi
fi

# Extract
tar -xzf "$TMP_DIR/$ARCHIVE_NAME" -C "$TMP_DIR"

NEW_BIN=""
if [ -f "$TMP_DIR/kb-${CLEAN_TARGET}-${PLATFORM}/kb" ]; then
  NEW_BIN="$TMP_DIR/kb-${CLEAN_TARGET}-${PLATFORM}/kb"
elif [ -f "$TMP_DIR/kb" ]; then
  NEW_BIN="$TMP_DIR/kb"
else
  NEW_BIN=$(find "$TMP_DIR" -name kb -type f | head -n1 || true)
fi

if [ -z "$NEW_BIN" ] || [ ! -f "$NEW_BIN" ]; then
  log_error "Binary 'kb' not found in downloaded archive."
  exit 1
fi

# Atomic replacement
mkdir -p "$KB_INSTALL_DIR"
TARGET_PATH="$KB_INSTALL_DIR/kb"
cp "$NEW_BIN" "${TARGET_PATH}.new"
chmod 755 "${TARGET_PATH}.new"
mv -f "${TARGET_PATH}.new" "$TARGET_PATH"

if [ "$OS_TYPE" = "darwin" ]; then
  xattr -d com.apple.quarantine "$TARGET_PATH" 2>/dev/null || true
fi

# Self-update the update script from the archive if present, else fallback to raw
UPDATER_TARGET="$KB_INSTALL_DIR/kb-update"
NEW_UPDATER=""
if [ -f "$TMP_DIR/kb-${CLEAN_TARGET}-${PLATFORM}/kb-update" ]; then
  NEW_UPDATER="$TMP_DIR/kb-${CLEAN_TARGET}-${PLATFORM}/kb-update"
elif [ -f "$TMP_DIR/kb-update" ]; then
  NEW_UPDATER="$TMP_DIR/kb-update"
fi

if [ -n "$NEW_UPDATER" ] && [ -f "$NEW_UPDATER" ]; then
  cp "$NEW_UPDATER" "${UPDATER_TARGET}.new"
  chmod 755 "${UPDATER_TARGET}.new"
  mv -f "${UPDATER_TARGET}.new" "$UPDATER_TARGET"
else
  RAW_UPDATER="https://raw.githubusercontent.com/${KB_REPO}/${TAG}/scripts/update.sh"
  if $HTTP_GET "$RAW_UPDATER" > "${UPDATER_TARGET}.new" 2>/dev/null; then
    chmod 755 "${UPDATER_TARGET}.new"
    mv -f "${UPDATER_TARGET}.new" "$UPDATER_TARGET"
  fi
fi

NEW_VER=$("$TARGET_PATH" --version 2>/dev/null || echo "kb ${TAG}")
printf "\n${GREEN}${BOLD}✓ Successfully updated to %s!${RESET}\n\n" "$NEW_VER"
