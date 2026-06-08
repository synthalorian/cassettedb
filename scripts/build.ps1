# CassetteDB Cross-Platform Build Script (PowerShell)
# Supports: Windows, Linux (via WSL), macOS

[CmdletBinding()]
param(
    [string]$Target = "",
    [switch]$Release,
    [string]$Features = "",
    [int]$Jobs = 0,
    [switch]$Verbose,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Resolve-Path (Join-Path $ScriptDir "..")

function Write-Info { param([string]$Message) Write-Host "[INFO] $Message" -ForegroundColor Cyan }
function Write-Success { param([string]$Message) Write-Host "[OK] $Message" -ForegroundColor Green }
function Write-Warn { param([string]$Message) Write-Host "[WARN] $Message" -ForegroundColor Yellow }
function Write-Error { param([string]$Message) Write-Host "[ERROR] $Message" -ForegroundColor Red; exit 1 }

function Show-Help {
    @"
CassetteDB Build Script (PowerShell)

Usage: .\build.ps1 [OPTIONS]

Options:
    -Target <TARGET>       Build target (default: host)
    -Release               Build in release mode
    -Features <FEATURES>   Comma-separated feature list
    -Jobs <N>              Number of parallel jobs
    -Verbose               Verbose output
    -Help                  Show this help message

Supported targets:
    x86_64-pc-windows-msvc
    x86_64-pc-windows-gnu
    aarch64-pc-windows-msvc

Examples:
    .\build.ps1 -Release
    .\build.ps1 -Features tantivy-search -Release
"@
}

function Detect-Host {
    $arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { "i686" }
    return "$arch-pc-windows-msvc"
}

function Check-Dependencies {
    Write-Info "Checking dependencies..."
    
    try {
        $cargoVersion = cargo --version 2>$null
        if (-not $cargoVersion) {
            Write-Error "Rust/Cargo not found. Please install Rust: https://rustup.rs"
        }
    } catch {
        Write-Error "Rust/Cargo not found. Please install Rust: https://rustup.rs"
    }
    
    if ($Target -and $Target -ne (Detect-Host)) {
        $installed = rustup target list --installed 2>$null
        if ($installed -notcontains $Target) {
            Write-Warn "Target $Target not installed. Installing..."
            rustup target add $Target
        }
    }
    
    Write-Success "Dependencies OK"
}

function Build-Project {
    $target = if ($Target) { $Target } else { Detect-Host }
    $mode = if ($Release) { "--release" } else { "" }
    $featuresArg = if ($Features) { "--features $Features" } else { "" }
    $jobsArg = if ($Jobs -gt 0) { "--jobs $Jobs" } else { "" }
    
    Write-Info "Building CassetteDB..."
    Write-Info "  Target: $target"
    Write-Info "  Mode: $(if ($Release) { 'release' } else { 'debug' })"
    if ($Features) { Write-Info "  Features: $Features" }
    
    $targetArg = if ($target -ne (Detect-Host)) { "--target $target" } else { "" }
    
    $cmd = "cargo build $mode $targetArg $featuresArg $jobsArg"
    Write-Info "  Command: $cmd"
    
    Invoke-Expression $cmd
    
    Write-Success "Build complete"
}

function Test-Project {
    Write-Info "Running tests..."
    
    $targetArg = if ($Target) { "--target $Target" } else { "" }
    $featuresArg = if ($Features) { "--features $Features" } else { "" }
    
    Invoke-Expression "cargo test $targetArg $featuresArg"
    Write-Success "Tests passed"
}

function Package-Project {
    $target = if ($Target) { $Target } else { Detect-Host }
    $version = (Get-Content (Join-Path $ProjectRoot "Cargo.toml") | Select-String '^version').Line.Split('"')[1]
    
    Write-Info "Packaging CassetteDB v$version for $target..."
    
    $pkgDir = Join-Path $ProjectRoot "dist" "cassettedb-$version-$target"
    New-Item -ItemType Directory -Force -Path $pkgDir | Out-Null
    
    $suffix = ".exe"
    
    $binDir = Join-Path $ProjectRoot "target"
    if ($target -ne (Detect-Host)) {
        $binDir = Join-Path $binDir $target
    }
    $binDir = Join-Path $binDir "release"
    
    # Copy binaries
    Copy-Item (Join-Path $binDir "cassette$suffix") $pkgDir -Force
    
    # Copy headers if FFI was built
    $header = Join-Path $ProjectRoot "cassette.h"
    if (Test-Path $header) {
        Copy-Item $header $pkgDir -Force
    }
    
    # Copy README
    Copy-Item (Join-Path $ProjectRoot "README.md") $pkgDir -Force
    
    # Create archive
    $distDir = Join-Path $ProjectRoot "dist"
    $zipPath = Join-Path $distDir "cassettedb-$version-$target.zip"
    Compress-Archive -Path $pkgDir -DestinationPath $zipPath -Force
    
    Write-Success "Package created: dist\cassettedb-$version-$target.zip"
}

# Main
if ($Help) {
    Show-Help
    exit 0
}

Write-Info "CassetteDB Build Script (PowerShell)"
Write-Info "Project root: $ProjectRoot"

Check-Dependencies
Build-Project
Test-Project

if ($Release) {
    Package-Project
}

Write-Success "All done!"
