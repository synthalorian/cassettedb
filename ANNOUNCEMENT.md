# CassetteDB v1.0.0 — Announcement

We are thrilled to announce the stable release of **CassetteDB v1.0.0**, a single-file JSON document database inspired by SQLite, written in Rust.

## What is CassetteDB?

CassetteDB is an embeddable, zero-config JSON document store that gives you:

- **Single-file portability** — Every database is a self-contained `.cassette` file (plus a `.cassette.wal` during active transactions).
- **ACID transactions** — Durability and atomicity via a Write-Ahead Log (WAL) with CRC32 checksums and commit flags.
- **JSONPath-like queries** — A tiny DSL for filtering, comparisons, and logical combinators.
- **Full-text search** — A custom inverted index built over all string fields in your documents.
- **Zero external server** — Use it as an embeddable library or via the lightweight CLI.
- **Multi-language bindings** — First-class support for Rust, C, Python, Node.js, and Go.

## What's New in v1.0.0

This stable release marks the completion of our public API freeze and includes everything developed across the v0.x series:

- **B-tree secondary indexes** and range queries on indexed fields
- **Multi-document transactions** ( /  / )
- **Memory-mapped I/O** via 
- **Optional Tantivy integration** for advanced full-text search
- **Replication / change-feed** infrastructure
- **Backup / snapshot** commands
- **TCP server** and **HTTP REST API** with connection pooling and token-based authentication
- **Raft consensus**, automatic failover, sharding, and distributed transactions
- **Comprehensive test suite**, fuzz testing, crash recovery stress tests, and a memory safety audit
- **Cross-platform builds** for Linux (glibc and musl), macOS (x86_64 and Apple Silicon), and Windows
- **C FFI bindings** with generated headers, plus idiomatic bindings for Python, Node.js, and Go

## Performance

CassetteDB is designed for workloads where SQLite's rigid schema is overkill but a full NoSQL server is too heavy. In our benchmark suite, CassetteDB shows competitive insert and query throughput for JSON document workloads while maintaining a tiny footprint. See [](docs/SQLITE_BENCHMARK.md) for a detailed comparison with SQLite.

## Quick Start



## Language Bindings

### Rust


### C


### Python


### Node.js


### Go
{name:Ada,age:36}

## Community

- **Repository**: https://github.com/synthalorian/cassettedb
- **Issues and Discussions**: https://github.com/synthalorian/cassettedb/issues
- **crates.io**: https://crates.io/crates/cassettedb

We'd love your feedback, bug reports, and contributions. Thank you to everyone who tested the release candidates and helped shape CassetteDB into what it is today.

Happy querying!

— The CassetteDB Team
