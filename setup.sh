#!/usr/bin/env bash
# ==============================================================================
# Vary AI (project tx_switch) - One-Click Environment Setup Script
# Supported Platforms: macOS, Linux (Ubuntu/Debian, Fedora, Arch)
# ==============================================================================

set -euo pipefail

# Text colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

info() {
    echo -e "${CYAN}[INFO]${NC} $*"
}

success() {
    echo -e "${GREEN}[SUCCESS]${NC} $*"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}

banner() {
    echo -e "${BLUE}"
    echo "============================================================"
    echo "   Vary AI development environment setup"
    echo "============================================================"
    echo -e "${NC}"
}

# 1. OS Detection
detect_os() {
    OS="$(uname -s)"
    case "$OS" in
        Darwin)
            OS_TYPE="macos"
            ;;
        Linux)
            OS_TYPE="linux"
            if [ -f /etc/os-release ]; then
                # shellcheck source=/dev/null
                . /etc/os-release
                DISTRO="${ID:-linux}"
            else
                DISTRO="unknown"
            fi
            ;;
        *)
            error "Unsupported operating system: $OS. See the Windows build guide for platform setup."
            exit 1
            ;;
    esac
    info "Detected operating system: $OS_TYPE ${DISTRO:+($DISTRO)}"
}

# 2. Check System Prerequisites
check_system_deps() {
    info "Checking system build prerequisites..."

    if [ "$OS_TYPE" = "macos" ]; then
        if ! xcode-select -p &>/dev/null; then
            warn "Xcode Command Line Tools not found; requesting installation..."
            xcode-select --install || true
            warn "Complete the Xcode CLT installer before running this script again."
            exit 1
        else
            success "Xcode Command Line Tools are available"
        fi
    elif [ "$OS_TYPE" = "linux" ]; then
        info "Checking Linux Tauri 2 system libraries..."
        case "$DISTRO" in
            ubuntu|debian|pop)
                if ! dpkg -s libwebkit2gtk-4.1-dev build-essential libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev &>/dev/null; then
                    warn "Some Tauri 2 build dependencies are missing; installing with sudo apt-get..."
                    sudo apt-get update && sudo apt-get install -y \
                        libwebkit2gtk-4.1-dev \
                        build-essential \
                        curl \
                        wget \
                        file \
                        libssl-dev \
                        libgtk-3-dev \
                        libayatana-appindicator3-dev \
                        librsvg2-dev
                fi
                ;;
            fedora)
                if ! rpm -q webkit2gtk4.1-devel gtk3-devel openssl-devel libayatana-appindicator-devel &>/dev/null; then
                    warn "Some Tauri 2 build dependencies are missing; installing with sudo dnf..."
                    sudo dnf install -y \
                        webkit2gtk4.1-devel \
                        openssl-devel \
                        curl \
                        wget \
                        file \
                        gtk3-devel \
                        libayatana-appindicator-devel \
                        librsvg2-devel
                fi
                ;;
            arch|manjaro)
                if ! pacman -Qi webkit2gtk-4.1 gtk3 openssl &>/dev/null; then
                    warn "Some Tauri 2 build dependencies are missing; installing with sudo pacman..."
                    sudo pacman -Syu --needed --noconfirm \
                        webkit2gtk-4.1 \
                        base-devel \
                        curl \
                        wget \
                        file \
                        openssl \
                        gtk3 \
                        libappindicator-gtk3 \
                        librsvg
                fi
                ;;
            *)
                warn "Unknown Linux distribution ($DISTRO). Install the required WebKitGTK, GTK, appindicator and SSL development libraries manually."
                ;;
        esac
        success "Linux prerequisite checks completed"
    fi
}

# 3. Check & Install Node.js and pnpm
check_node_pnpm() {
    info "Checking Node.js..."
    if ! command -v node &>/dev/null; then
        error "Node.js was not found. Install the development version specified in .node-version."
        if [ "$OS_TYPE" = "macos" ] && command -v brew &>/dev/null; then
            info "Homebrew can install Node.js: brew install node"
        else
            info "Use your preferred Node version manager, such as nvm or fnm."
        fi
        exit 1
    fi

    NODE_VERSION="$(node -v | sed 's/^v//')"
    NODE_MAJOR="$(echo "$NODE_VERSION" | cut -d. -f1)"
    if [ "$NODE_MAJOR" -lt 18 ]; then
        error "Node v$NODE_VERSION is below this bootstrap script minimum of 18. Use the version in .node-version for development."
        exit 1
    fi
    success "Node.js version: v$NODE_VERSION"

    info "Checking pnpm..."
    if ! command -v pnpm &>/dev/null; then
        warn "pnpm not found; trying Corepack or a global npm installation..."
        if command -v corepack &>/dev/null; then
            corepack enable || true
            corepack prepare pnpm@10.12.3 --activate || true
        fi
        if ! command -v pnpm &>/dev/null; then
            npm install -g pnpm@10.12.3 || true
        fi
        if ! command -v pnpm &>/dev/null; then
            error "pnpm installation failed; install the packageManager version from package.json manually."
            exit 1
        fi
    fi
    success "pnpm version: $(pnpm -v)"
}

# 4. Check & Install Rust Toolchain
check_rust() {
    info "Checking the Rust toolchain..."
    if ! command -v rustup &>/dev/null || ! command -v cargo &>/dev/null; then
        warn "Rust toolchain not found; starting the official rustup installer..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
        # shellcheck source=/dev/null
        source "$HOME/.cargo/env"
    fi

    # Ensure cargo is in PATH for current shell
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck source=/dev/null
        source "$HOME/.cargo/env"
    fi

    # Read channel from rust-toolchain.toml if present
    TOOLCHAIN_CHANNEL="stable"
    if [ -f "rust-toolchain.toml" ]; then
        DETECTED_CHANNEL=$(grep -E '^channel\s*=' rust-toolchain.toml | cut -d'"' -f2 || echo "stable")
        if [ -n "$DETECTED_CHANNEL" ]; then
            TOOLCHAIN_CHANNEL="$DETECTED_CHANNEL"
        fi
    fi

    info "Ensuring Rust $TOOLCHAIN_CHANNEL and the rustfmt/clippy components are installed..."
    rustup toolchain install "$TOOLCHAIN_CHANNEL" --profile minimal || true
    rustup default "$TOOLCHAIN_CHANNEL" || true
    rustup component add rustfmt clippy --toolchain "$TOOLCHAIN_CHANNEL" || true

    success "Rust toolchain: $(cargo --version)"
}

# 5. Install Project Dependencies
install_project_deps() {
    info "Installing frontend and project dependencies (pnpm install)..."
    pnpm install
    success "Frontend dependencies installed"
}

# 6. Verify Compilation
verify_build() {
    info "Verifying the frontend build..."
    pnpm build:renderer
    success "Frontend build passed"

    info "Checking Rust dependencies and compilation (cargo check)..."
    cargo check --manifest-path src-tauri/Cargo.toml
    success "Rust check passed"
}

# 7. Print Completion Summary
print_summary() {
    echo ""
    echo -e "${GREEN}============================================================${NC}"
    echo -e "${GREEN}Vary AI development environment setup completed.${NC}"
    echo -e "${GREEN}============================================================${NC}"
    echo ""
    echo -e "Development commands:"
    echo -e "  ${CYAN}pnpm dev${NC}             - Start the desktop application with frontend hot reload"
    echo -e "  ${CYAN}pnpm dev:renderer${NC}    - Start only the Vite server at http://localhost:3000"
    echo -e "  ${CYAN}pnpm test:unit${NC}       - Run frontend Vitest tests"
    echo -e "  ${CYAN}cargo test --lib --manifest-path src-tauri/Cargo.toml${NC} - Run Rust library tests"
    echo -e "  ${CYAN}pnpm build${NC}           - Build desktop artifacts for the configured host/target"
    echo ""
    echo -e "${YELLOW}Note:${NC}"
    echo -e "If the window does not appear, check for an existing Vary AI instance and quit it normally before restarting."
    echo ""
}

main() {
    banner
    detect_os
    check_system_deps
    check_node_pnpm
    check_rust
    install_project_deps
    verify_build
    print_summary
}

main "$@"
