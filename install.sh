#!/usr/bin/env sh
# kb installer
# Installs kb into ~/.local/bin and configures shell PATH.
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/cortiz/kb/main/install.sh | sh
#   or
#   ./install.sh [--bin-dir <dir>] [--version <ver>] [--no-modify-path] [-y]
set -eu

KB_REPO="${KB_REPO:-cortiz/kb}"
KB_VERSION="${KB_VERSION:-latest}"
KB_INSTALL_DIR="${KB_INSTALL_DIR:-$HOME/.local/bin}"
KB_RAW_URL="${KB_RAW_URL:-https://raw.githubusercontent.com/${KB_REPO}/main}"
MODIFY_PATH=1
NON_INTERACTIVE=0

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
kb installer - installs kb into ~/.local/bin

Usage:
  install.sh [options]

Options:
  -b, --bin-dir <dir>    Install directory (default: ~/.local/bin)
  -v, --version <ver>    Version to install (default: latest)
  --repo <owner/repo>    GitHub repository (default: cortiz/kb)
  --no-modify-path       Do not prompt or modify shell configuration files
  -y, --yes              Non-interactive mode (accept all defaults)
  -h, --help             Show this help message
USAGE
  exit 0
}

# Parse arguments
while [ $# -gt 0 ]; do
  case "$1" in
    -b|--bin-dir)
      KB_INSTALL_DIR="$2"
      shift 2
      ;;
    -v|--version)
      KB_VERSION="$2"
      shift 2
      ;;
    --repo)
      KB_REPO="$2"
      shift 2
      ;;
    --no-modify-path)
      MODIFY_PATH=0
      shift
      ;;
    -y|--yes)
      NON_INTERACTIVE=1
      shift
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

# Check for download tool
if command -v curl >/dev/null 2>&1; then
  HTTP_GET="curl -fsSL"
elif command -v wget >/dev/null 2>&1; then
  HTTP_GET="wget -qO-"
else
  log_error "curl or wget is required to install kb."
  exit 1
fi

# Detect OS
OS="$(uname -s)"
case "$OS" in
  Linux*)  OS_TYPE="linux" ;;
  Darwin*) OS_TYPE="darwin" ;;
  *)
    log_error "Unsupported operating system: $OS. kb supports Linux and macOS."
    exit 1
    ;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH_TYPE="x86_64" ;;
  aarch64|arm64) ARCH_TYPE="arm64" ;;
  *)
    log_error "Unsupported architecture: $ARCH. kb supports x86_64 and arm64."
    exit 1
    ;;
esac

# Map to release platform string
if [ "$OS_TYPE" = "linux" ] && [ "$ARCH_TYPE" = "x86_64" ]; then
  PLATFORM="linux-x86_64"
elif [ "$OS_TYPE" = "linux" ] && [ "$ARCH_TYPE" = "arm64" ]; then
  PLATFORM="linux-arm64"
elif [ "$OS_TYPE" = "darwin" ] && [ "$ARCH_TYPE" = "arm64" ]; then
  PLATFORM="macos-apple-silicon"
elif [ "$OS_TYPE" = "darwin" ] && [ "$ARCH_TYPE" = "x86_64" ]; then
  PLATFORM="macos-intel"
else
  log_error "Unsupported platform combination: $OS_TYPE on $ARCH_TYPE"
  exit 1
fi

log_info "Detected platform: ${BOLD}$PLATFORM${RESET}"

# Resolve version tag
TAG="$KB_VERSION"
if [ "$TAG" = "latest" ]; then
  log_info "Fetching latest release version from ${BOLD}github.com/${KB_REPO}${RESET}..."
  LATEST_RELEASE_URL="https://api.github.com/repos/${KB_REPO}/releases/latest"
  TAG_JSON=$($HTTP_GET "$LATEST_RELEASE_URL" 2>/dev/null || true)
  if [ -n "$TAG_JSON" ]; then
    TAG=$(printf '%s\n' "$TAG_JSON" | grep '"tag_name":' | head -n1 | sed -E 's/.*"tag_name": "([^"]+)".*/\1/')
  fi

  # Fallback if GitHub API was empty or rate-limited: check release redirect
  if [ -z "$TAG" ] || [ "$TAG" = "latest" ]; then
    REDIRECT_URL=$(curl -sIL -o /dev/null -w '%{url_effective}' "https://github.com/${KB_REPO}/releases/latest" 2>/dev/null || true)
    case "$REDIRECT_URL" in
      */releases/tag/*)
        TAG="${REDIRECT_URL##*/}"
        ;;
    esac
  fi

  if [ -z "$TAG" ] || [ "$TAG" = "latest" ]; then
    log_error "Could not resolve latest release for ${KB_REPO}."
    log_error "You can specify a version explicitly: KB_VERSION=v0.2.0 ./install.sh"
    exit 1
  fi
fi

# Clean version for archive name (e.g. v0.2.0 -> 0.2.0)
CLEAN_VERSION="${TAG#v}"
log_info "Installing kb ${BOLD}${TAG}${RESET} ($PLATFORM) into ${BOLD}${KB_INSTALL_DIR}${RESET}..."

# Setup temporary directory
TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t 'kb-install')
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

ARCHIVE_NAME="kb-${CLEAN_VERSION}-${PLATFORM}.tar.gz"
DOWNLOAD_URL="${KB_DOWNLOAD_URL:-https://github.com/${KB_REPO}/releases/download/${TAG}/${ARCHIVE_NAME}}"
CHECKSUMS_URL="${KB_CHECKSUMS_URL:-https://github.com/${KB_REPO}/releases/download/${TAG}/SHA256SUMS}}"

log_info "Downloading ${DOWNLOAD_URL}..."
if ! $HTTP_GET "$DOWNLOAD_URL" > "$TMP_DIR/$ARCHIVE_NAME"; then
  log_error "Failed to download $DOWNLOAD_URL."
  log_error "Please check that release $TAG exists at https://github.com/${KB_REPO}/releases."
  exit 1
fi

# Optional checksum verification
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
          log_error "Checksum verification failed!"
          log_error "Expected: $EXPECTED_HASH"
          log_error "Actual:   $ACTUAL_HASH"
          exit 1
        fi
        log_success "Checksum verified: ${ACTUAL_HASH}"
      fi
    fi
  else
    log_warn "No SHA256SUMS file found for release; skipping checksum verification."
  fi
fi

# Extract
log_info "Extracting archive..."
tar -xzf "$TMP_DIR/$ARCHIVE_NAME" -C "$TMP_DIR"

# Locate binary
KB_BIN=""
if [ -f "$TMP_DIR/kb-${CLEAN_VERSION}-${PLATFORM}/kb" ]; then
  KB_BIN="$TMP_DIR/kb-${CLEAN_VERSION}-${PLATFORM}/kb"
elif [ -f "$TMP_DIR/kb" ]; then
  KB_BIN="$TMP_DIR/kb"
else
  KB_BIN=$(find "$TMP_DIR" -name kb -type f | head -n1 || true)
fi

if [ -z "$KB_BIN" ] || [ ! -f "$KB_BIN" ]; then
  log_error "Binary 'kb' not found in downloaded archive."
  exit 1
fi

# Install binary
mkdir -p "$KB_INSTALL_DIR"
cp "$KB_BIN" "$KB_INSTALL_DIR/kb"
chmod 755 "$KB_INSTALL_DIR/kb"

if [ "$OS_TYPE" = "darwin" ]; then
  xattr -d com.apple.quarantine "$KB_INSTALL_DIR/kb" 2>/dev/null || true
fi

log_success "Installed kb binary to ${BOLD}${KB_INSTALL_DIR}/kb${RESET}"

# Install update helper script
UPDATE_SCRIPT_DEST="$KB_INSTALL_DIR/kb-update"
SCRIPT_SOURCE="$(dirname "$0")/scripts/update.sh"

KB_UPDATE_BIN=""
if [ -f "$TMP_DIR/kb-${CLEAN_VERSION}-${PLATFORM}/kb-update" ]; then
  KB_UPDATE_BIN="$TMP_DIR/kb-${CLEAN_VERSION}-${PLATFORM}/kb-update"
elif [ -f "$TMP_DIR/kb-update" ]; then
  KB_UPDATE_BIN="$TMP_DIR/kb-update"
fi

if [ -n "$KB_UPDATE_BIN" ] && [ -f "$KB_UPDATE_BIN" ]; then
  cp "$KB_UPDATE_BIN" "$UPDATE_SCRIPT_DEST"
  chmod 755 "$UPDATE_SCRIPT_DEST"
  log_success "Installed update helper to ${BOLD}${UPDATE_SCRIPT_DEST}${RESET}"
elif [ -f "$SCRIPT_SOURCE" ]; then
  cp "$SCRIPT_SOURCE" "$UPDATE_SCRIPT_DEST"
  chmod 755 "$UPDATE_SCRIPT_DEST"
  log_success "Installed update helper to ${BOLD}${UPDATE_SCRIPT_DEST}${RESET}"
else
  # Fetch update script from repo
  if $HTTP_GET "${KB_RAW_URL}/scripts/update.sh" > "$UPDATE_SCRIPT_DEST" 2>/dev/null; then
    chmod 755 "$UPDATE_SCRIPT_DEST"
    log_success "Installed update helper to ${BOLD}${UPDATE_SCRIPT_DEST}${RESET}"
  fi
fi

# PATH check & shell configuration
check_and_configure_path() {
  # Check if KB_INSTALL_DIR is already in PATH
  case ":${PATH}:" in
    *:"$KB_INSTALL_DIR":*)
      log_success "${BOLD}${KB_INSTALL_DIR}${RESET} is already in your PATH."
      return 0
      ;;
  esac

  log_warn "${BOLD}${KB_INSTALL_DIR}${RESET} is not in your PATH."

  if [ "$MODIFY_PATH" -eq 0 ]; then
    log_info "To add kb to your PATH, add this line to your shell configuration:"
    printf "  export PATH=\"%s:\$PATH\"\n\n" "$KB_INSTALL_DIR"
    return 0
  fi

  # Detect shell configuration file
  SHELL_NAME=$(basename "${SHELL:-bash}")
  RC_FILE=""

  case "$SHELL_NAME" in
    zsh)
      if [ -f "$HOME/.zshrc" ]; then
        RC_FILE="$HOME/.zshrc"
      else
        RC_FILE="$HOME/.zprofile"
      fi
      ;;
    bash)
      if [ "$OS_TYPE" = "darwin" ]; then
        if [ -f "$HOME/.bash_profile" ]; then
          RC_FILE="$HOME/.bash_profile"
        elif [ -f "$HOME/.zshrc" ]; then
          # Default on macOS is zsh
          RC_FILE="$HOME/.zshrc"
        else
          RC_FILE="$HOME/.bashrc"
        fi
      else
        if [ -f "$HOME/.bashrc" ]; then
          RC_FILE="$HOME/.bashrc"
        else
          RC_FILE="$HOME/.profile"
        fi
      fi
      ;;
    fish)
      RC_FILE="$HOME/.config/fish/config.fish"
      ;;
    *)
      RC_FILE="$HOME/.profile"
      ;;
  esac

  EXPORT_LINE="export PATH=\"$KB_INSTALL_DIR:\$PATH\""
  if [ "$SHELL_NAME" = "fish" ]; then
    EXPORT_LINE="fish_add_path \"$KB_INSTALL_DIR\""
  fi

  DO_ADD="y"
  if [ "$NON_INTERACTIVE" -eq 0 ]; then
    # Try reading from terminal even if script is piped through curl
    if [ -t 0 ]; then
      printf "Would you like to add %s to PATH in %s? [Y/n] " "$KB_INSTALL_DIR" "$RC_FILE"
      read -r RESPONSE || RESPONSE="y"
    elif [ -r /dev/tty ]; then
      printf "Would you like to add %s to PATH in %s? [Y/n] " "$KB_INSTALL_DIR" "$RC_FILE"
      read -r RESPONSE < /dev/tty || RESPONSE="y"
    else
      RESPONSE="y"
    fi

    case "$RESPONSE" in
      [nN]*) DO_ADD="n" ;;
      *)     DO_ADD="y" ;;
    esac
  fi

  if [ "$DO_ADD" = "y" ]; then
    mkdir -p "$(dirname "$RC_FILE")"
    touch "$RC_FILE"

    # Avoid duplicate entry
    if grep -F "$KB_INSTALL_DIR" "$RC_FILE" >/dev/null 2>&1; then
      log_info "$RC_FILE already references $KB_INSTALL_DIR."
    else
      printf '\n# Added by kb installer\n%s\n' "$EXPORT_LINE" >> "$RC_FILE"
      log_success "Added PATH configuration to ${BOLD}${RC_FILE}${RESET}"
    fi

    log_info "To use kb immediately in this terminal, run:"
    if [ "$SHELL_NAME" = "fish" ]; then
      printf "  source %s\n\n" "$RC_FILE"
    else
      printf "  source %s\n\n" "$RC_FILE"
    fi
  else
    log_info "Skipped modifying shell configuration."
    log_info "Add the following line to your shell profile manually:"
    printf "  %s\n\n" "$EXPORT_LINE"
  fi
}

check_and_configure_path

# Quick sanity check
if [ -x "$KB_INSTALL_DIR/kb" ]; then
  INSTALLED_VER=$("$KB_INSTALL_DIR/kb" --version 2>/dev/null || echo "kb ${TAG}")
  printf "\n${GREEN}${BOLD}✓ %s installed successfully!${RESET}\n\n" "$INSTALLED_VER"
  printf "Get started with:\n"
  printf "  ${BOLD}kb init${RESET}        Initialize your knowledge base in ~/kb\n"
  printf "  ${BOLD}kb --help${RESET}      Explore commands\n"
  printf "  ${BOLD}kb-update${RESET}      Check for and install future updates\n\n"
fi
