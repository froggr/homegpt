#!/bin/bash
# Build HomeGPT and install script dependencies
#
# Usage:
#   ./scripts/build.sh           # Build release binary
#   ./scripts/build.sh install   # Build + copy to ~/.cargo/bin

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

log() { echo -e "${GREEN}[build]${NC} $1"; }
err() { echo -e "${RED}[build]${NC} $1"; }

cd "$PROJECT_DIR"

# Check Rust toolchain
if ! command -v cargo > /dev/null 2>&1; then
    err "Rust not installed. Install with: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# Build HomeGPT binary (headless — no GUI deps)
log "Building HomeGPT (release, headless)..."
cargo build --no-default-features --release

log "Binary: $PROJECT_DIR/target/release/homegpt"

# Install Node.js dependencies for sidecar scripts
if command -v npm > /dev/null 2>&1; then
    log "Installing script dependencies..."
    cd "$SCRIPT_DIR"
    npm install --silent
    cd "$PROJECT_DIR"
else
    log "npm not found — skipping script dependency install"
fi

# Optional: install binary to PATH
if [ "${1:-}" = "install" ]; then
    log "Installing to ~/.cargo/bin/homegpt..."
    cp "$PROJECT_DIR/target/release/homegpt" "$HOME/.cargo/bin/homegpt"
    log "Installed. Run 'homegpt --help' to verify."
fi

log "Done."
