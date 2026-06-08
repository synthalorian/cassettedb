#!/usr/bin/env bash
# CassetteDB Cross-Platform Build Script
# Supports: Linux, macOS, Windows (via cross-compilation)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

info() { echo -e "${BLUE}[INFO]${NC} $*"; }
success() { echo -e "${GREEN}[OK]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# Default values
TARGET=""
RELEASE=false
FEATURES=""
JOBS=""
VERBOSE=false

usage() {
    cat <<EOF
CassetteDB Build Script

Usage: $(basename "$0") [OPTIONS]

Options:
    -t, --target <TARGET>     Build target (default: host)
    -r, --release             Build in release mode
    -f, --features <FEATURES> Comma-separated feature list
    -j, --jobs <N>            Number of parallel jobs
    -v, --verbose             Verbose output
    -h, --help                Show this help message

Supported targets:
    x86_64-unknown-linux-gnu
    x86_64-unknown-linux-musl
    aarch64-unknown-linux-gnu
    x86_64-apple-darwin
    aarch64-apple-darwin
    x86_64-pc-windows-gnu
    x86_64-pc-windows-msvc

Examples:
    $(basename "$0") --release
    $(basename "$0") --target x86_64-unknown-linux-musl --release
    $(basename "$0") --features tantivy-search --release
EOF
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -t|--target)
                TARGET="$2"
                shift 2
                ;;
            -r|--release)
                RELEASE=true
                shift
                ;;
            -f|--features)
                FEATURES="$2"
                shift 2
                ;;
            -j|--jobs)
                JOBS="$2"
                shift 2
                ;;
            -v|--verbose)
                VERBOSE=true
                shift
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                error "Unknown option: $1"
                ;;
        esac
    done
}

detect_host() {
    local arch
    arch="$(uname -m)"
    local os
    os="$(uname -s | tr '[:upper:]' '[:lower:]')"
    
    case "$os" in
        linux)
            echo "${arch}-unknown-linux-gnu"
            ;;
        darwin)
            echo "${arch}-apple-darwin"
            ;;
        mingw*|msys*|cygwin*)
            echo "${arch}-pc-windows-gnu"
            ;;
        *)
            error "Unsupported host OS: $os"
            ;;
    esac
}

check_dependencies() {
    info "Checking dependencies..."
    
    if ! command -v cargo &>/dev/null; then
        error "Rust/Cargo not found. Please install Rust: https://rustup.rs"
    fi
    
    if [[ -n "$TARGET" && "$TARGET" != "$(detect_host)" ]]; then
        if ! rustup target list --installed | grep -q "$TARGET"; then
            warn "Target $TARGET not installed. Installing..."
            rustup target add "$TARGET"
        fi
        
        # Check if we need cross-compilation tools
        if [[ "$TARGET" == *-musl ]]; then
            if ! command -v musl-gcc &>/dev/null && ! command -v musl-clang &>/dev/null; then
                warn "musl toolchain not found. You may need to install musl-tools."
            fi
        fi
    fi
    
    success "Dependencies OK"
}

build() {
    local target="${TARGET:-$(detect_host)}"
    local mode=""
    local features_arg=""
    local jobs_arg=""
    
    if $RELEASE; then
        mode="--release"
    fi
    
    if [[ -n "$FEATURES" ]]; then
        features_arg="--features $FEATURES"
    fi
    
    if [[ -n "$JOBS" ]]; then
        jobs_arg="--jobs $JOBS"
    fi
    
    info "Building CassetteDB..."
    info "  Target: $target"
    info "  Mode: $([ -n "$mode" ] && echo "release" || echo "debug")"
    [[ -n "$FEATURES" ]] && info "  Features: $FEATURES"
    
    cd "$PROJECT_ROOT"
    
    local target_arg=""
    if [[ "$target" != "$(detect_host)" ]]; then
        target_arg="--target $target"
    fi
    
    local cmd="cargo build $mode $target_arg $features_arg $jobs_arg"
    info "  Command: $cmd"
    
    if $VERBOSE; then
        eval "$cmd"
    else
        eval "$cmd" 2>&1 | while read -r line; do
            echo "    $line"
        done
    fi
    
    success "Build complete"
}

test_build() {
    info "Running tests..."
    cd "$PROJECT_ROOT"
    
    local target_arg=""
    if [[ -n "$TARGET" ]]; then
        target_arg="--target $TARGET"
    fi
    
    local features_arg=""
    if [[ -n "$FEATURES" ]]; then
        features_arg="--features $FEATURES"
    fi
    
    cargo test $target_arg $features_arg
    success "Tests passed"
}

package() {
    local target="${TARGET:-$(detect_host)}"
    local version
    version="$(grep '^version' "$PROJECT_ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')"
    
    info "Packaging CassetteDB v$version for $target..."
    
    local pkg_dir="$PROJECT_ROOT/dist/cassettedb-$version-$target"
    mkdir -p "$pkg_dir"
    
    local suffix=""
    if [[ "$target" == *windows* ]]; then
        suffix=".exe"
    fi
    
    local bin_dir="$PROJECT_ROOT/target"
    if [[ "$target" != "$(detect_host)" ]]; then
        bin_dir="$bin_dir/$target"
    fi
    bin_dir="$bin_dir/release"
    
    # Copy binaries
    cp "$bin_dir/cassette$suffix" "$pkg_dir/"
    
    # Copy headers if FFI was built
    if [[ -f "$PROJECT_ROOT/cassette.h" ]]; then
        cp "$PROJECT_ROOT/cassette.h" "$pkg_dir/"
    fi
    
    # Copy README and LICENSE
    cp "$PROJECT_ROOT/README.md" "$pkg_dir/"
    if [[ -f "$PROJECT_ROOT/LICENSE" ]]; then
        cp "$PROJECT_ROOT/LICENSE" "$pkg_dir/"
    fi
    
    # Create archive
    cd "$PROJECT_ROOT/dist"
    if [[ "$target" == *windows* ]]; then
        zip -r "cassettedb-$version-$target.zip" "cassettedb-$version-$target"
    else
        tar czf "cassettedb-$version-$target.tar.gz" "cassettedb-$version-$target"
    fi
    
    success "Package created: dist/cassettedb-$version-$target.*"
}

main() {
    parse_args "$@"
    
    info "CassetteDB Build Script"
    info "Project root: $PROJECT_ROOT"
    
    check_dependencies
    build
    test_build
    
    if $RELEASE; then
        package
    fi
    
    success "All done!"
}

main "$@"
