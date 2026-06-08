#!/usr/bin/env python3
"""
CassetteDB Cross-Platform Build Script
Supports: Linux, macOS, Windows

This script provides a unified build interface across all platforms.
"""

import argparse
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent.resolve()
PROJECT_ROOT = SCRIPT_DIR.parent


def info(msg: str) -> None:
    print(f"\033[36m[INFO]\033[0m {msg}")


def success(msg: str) -> None:
    print(f"\033[32m[OK]\033[0m {msg}")


def warn(msg: str) -> None:
    print(f"\033[33m[WARN]\033[0m {msg}")


def error(msg: str) -> None:
    print(f"\033[31m[ERROR]\033[0m {msg}", file=sys.stderr)
    sys.exit(1)


def detect_host() -> str:
    """Detect the default Rust target for this machine."""
    arch = platform.machine().lower()
    system = platform.system().lower()
    
    # Normalize architecture names
    arch_map = {
        "amd64": "x86_64",
        "x86_64": "x86_64",
        "aarch64": "aarch64",
        "arm64": "aarch64",
        "i386": "i686",
        "i686": "i686",
    }
    arch = arch_map.get(arch, arch)
    
    if system == "linux":
        return f"{arch}-unknown-linux-gnu"
    elif system == "darwin":
        return f"{arch}-apple-darwin"
    elif system == "windows":
        return f"{arch}-pc-windows-msvc"
    else:
        error(f"Unsupported host OS: {system}")
        return ""  # unreachable


def check_dependencies(target: str) -> None:
    """Ensure required tools are available."""
    info("Checking dependencies...")
    
    if not shutil.which("cargo"):
        error("Rust/Cargo not found. Please install Rust: https://rustup.rs")
    
    if target != detect_host():
        result = subprocess.run(
            ["rustup", "target", "list", "--installed"],
            capture_output=True, text=True
        )
        if target not in result.stdout:
            warn(f"Target {target} not installed. Installing...")
            subprocess.run(["rustup", "target", "add", target], check=True)
    
    success("Dependencies OK")


def build(args: argparse.Namespace) -> None:
    """Build the project."""
    target = args.target or detect_host()
    mode = "--release" if args.release else ""
    features_arg = f"--features {args.features}" if args.features else ""
    jobs_arg = f"--jobs {args.jobs}" if args.jobs else ""
    target_arg = "--target {target}" if target != detect_host() else ""
    
    info("Building CassetteDB...")
    info(f"  Target: {target}")
    info(f"  Mode: {'release' if args.release else 'debug'}")
    if args.features:
        info(f"  Features: {args.features}")
    
    cmd = f"cargo build {mode} {target_arg} {features_arg} {jobs_arg}"
    info(f"  Command: {cmd}")
    
    subprocess.run(cmd, shell=True, check=True, cwd=PROJECT_ROOT)
    success("Build complete")


def test_build(args: argparse.Namespace) -> None:
    """Run the test suite."""
    info("Running tests...")
    
    target_arg = f"--target {args.target}" if args.target else ""
    features_arg = f"--features {args.features}" if args.features else ""
    
    cmd = f"cargo test {target_arg} {features_arg}"
    subprocess.run(cmd, shell=True, check=True, cwd=PROJECT_ROOT)
    success("Tests passed")


def package(args: argparse.Namespace) -> None:
    """Create distribution packages."""
    target = args.target or detect_host()
    
    # Read version from Cargo.toml
    cargo_toml = PROJECT_ROOT / "Cargo.toml"
    version = "0.0.0"
    with open(cargo_toml) as f:
        for line in f:
            if line.startswith("version"):
                version = line.split('"')[1]
                break
    
    info(f"Packaging CassetteDB v{version} for {target}...")
    
    pkg_dir = PROJECT_ROOT / "dist" / f"cassettedb-{version}-{target}"
    pkg_dir.mkdir(parents=True, exist_ok=True)
    
    suffix = ".exe" if "windows" in target else ""
    
    bin_dir = PROJECT_ROOT / "target"
    if target != detect_host():
        bin_dir = bin_dir / target
    bin_dir = bin_dir / "release"
    
    # Copy binaries
    src_bin = bin_dir / f"cassette{suffix}"
    if src_bin.exists():
        shutil.copy2(src_bin, pkg_dir)
    
    # Copy headers if FFI was built
    header = PROJECT_ROOT / "cassette.h"
    if header.exists():
        shutil.copy2(header, pkg_dir)
    
    # Copy documentation
    readme = PROJECT_ROOT / "README.md"
    if readme.exists():
        shutil.copy2(readme, pkg_dir)
    
    # Create archive
    dist_dir = PROJECT_ROOT / "dist"
    if "windows" in target:
        import zipfile
        zip_path = dist_dir / f"cassettedb-{version}-{target}.zip"
        with zipfile.ZipFile(zip_path, 'w', zipfile.ZIP_DEFLATED) as zf:
            for file in pkg_dir.iterdir():
                zf.write(file, file.name)
    else:
        tar_path = dist_dir / f"cassettedb-{version}-{target}.tar.gz"
        subprocess.run(
            ["tar", "czf", str(tar_path), str(pkg_dir.name)],
            cwd=dist_dir,
            check=True
        )
    
    success(f"Package created: dist/cassettedb-{version}-{target}.*")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="CassetteDB Cross-Platform Build Script",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Supported targets:
  x86_64-unknown-linux-gnu
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-gnu
  x86_64-apple-darwin
  aarch64-apple-darwin
  x86_64-pc-windows-msvc
  x86_64-pc-windows-gnu

Examples:
  %(prog)s --release
  %(prog)s --target x86_64-unknown-linux-musl --release
  %(prog)s --features tantivy-search --release
        """
    )
    
    parser.add_argument("-t", "--target", help="Build target (default: host)")
    parser.add_argument("-r", "--release", action="store_true", help="Build in release mode")
    parser.add_argument("-f", "--features", help="Comma-separated feature list")
    parser.add_argument("-j", "--jobs", type=int, help="Number of parallel jobs")
    parser.add_argument("-v", "--verbose", action="store_true", help="Verbose output")
    parser.add_argument("--no-test", action="store_true", help="Skip running tests")
    parser.add_argument("--package", action="store_true", help="Create distribution package")
    
    args = parser.parse_args()
    
    info("CassetteDB Build Script")
    info(f"Project root: {PROJECT_ROOT}")
    
    target = args.target or detect_host()
    check_dependencies(target)
    build(args)
    
    if not args.no_test:
        test_build(args)
    
    if args.release and args.package:
        package(args)
    
    success("All done!")


if __name__ == "__main__":
    main()
