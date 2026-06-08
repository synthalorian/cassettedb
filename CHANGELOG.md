# Changelog

All notable changes to CassetteDB are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.9.0] - 2025-06-08

### Release Candidate

This is the v0.9.0 release candidate for CassetteDB. The public API is now
frozen for the upcoming v1.0.0 stable release.

### Added

- **Comprehensive README** with usage examples for Rust, C, Python, Node.js,
  and Go.
- **Python bindings** (`bindings/python/`) using `ctypes` to wrap the
  CassetteDB C FFI. Includes a `pyproject.toml`, an idiomatic `CassetteDB`
  class with context-manager support, and automatic library discovery.
- **Node.js bindings** (`bindings/node/`) using `napi-rs`. Exposes a native
  `CassetteDB` class with `insert`, `get`, `update`, `delete`, `query`,
  `dump`, and `compact` methods.
- **Go bindings** (`bindings/go/`) using `cgo` against `libcassettedb`.
  Provides an idiomatic `cassettedb.Open()` API with typed errors and
  optional JSON unmarshalling helpers (`GetJSON`, `QueryJSON`).
- This `CHANGELOG.md` file documenting the release.

### Changed

- Workspace `Cargo.toml` updated to include the new Node.js binding crate
  under `bindings/node`.
- README roadmap updated to mark v0.9.0 language bindings as completed.

### Fixed

- No bug fixes in this release candidate. All known blockers are resolved.

### Deprecated

- Nothing deprecated in this release.

### Removed

- Nothing removed in this release.

### Security

- No security-related changes in this release.

## [0.7.0] - 2024

### Added

- C FFI bindings (`libcassettedb`) and generated `cassette.h` header.
- Cross-platform build scripts (`scripts/build.sh`, `scripts/build.ps1`,
  `scripts/build.py`).
- Backup / snapshot commands.
- Replication change-feed infrastructure.
- TCP server, HTTP REST server, connection pooling, and authentication.
- Raft consensus, cluster management, sharding, and distributed transactions.
- Configuration migration system and crash reporter utilities.

## [0.1.0] - 2024

### Added

- Initial CassetteDB release.
- Page-based storage engine with a free-page list.
- Write-ahead logging (WAL) with CRC32 checksums and commit flags.
- In-memory inverted full-text index.
- JSONPath-like query DSL.
- CLI commands: `init`, `insert`, `query`, `compact`, `dump`, `delete`,
  `get`.

[0.9.0]: https://github.com/synthalorian/cassettedb/releases/tag/v0.9.0
[0.7.0]: https://github.com/synthalorian/cassettedb/releases/tag/v0.7.0
[0.1.0]: https://github.com/synthalorian/cassettedb/releases/tag/v0.1.0
