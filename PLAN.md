# CassetteDB Roadmap

## v0.1.0 ✅ — Initial scaffold
- Page-based storage with free-page list
- WAL with commit flags and recovery
- Inverted full-text index
- JSONPath-like query DSL
- CLI: init, insert, query, compact, dump, delete, get

## v0.2.0 — Indexes & transactions
- [ ] B-tree secondary indexes
- [ ] Range queries on indexed fields
- [ ] Multi-document transactions (BEGIN / COMMIT / ROLLBACK)
- [ ] Memory-mapped I/O (memmap2)

## v0.3.0 — Advanced queries
- [ ] Aggregation queries (count, sum, avg, min, max)
- [ ] Sort / order by
- [ ] Limit / offset pagination
- [ ] Query planner with index selection

## v0.4.0 — Phase 4: Performance & features
- [ ] Tantivy integration for advanced full-text search (optional feature)
- [ ] Replication / change-feed (append-only log for followers)
- [ ] Backup / snapshot command
- [ ] Benchmark suite
- [ ] Improve query parser error messages

## v0.5.0 — Server mode
- [ ] TCP server (custom protocol)
- [ ] HTTP REST API
- [ ] Connection pooling
- [ ] Authentication (token-based)
- [ ] Multi-database support

## v0.6.0 — Distributed
- [ ] Raft consensus for leader election
- [ ] Automatic failover
- [ ] Sharding support
- [ ] Distributed transactions
- [ ] Cluster management CLI

## v0.7.0 — Pre-release polish
- [ ] Comprehensive test suite
- [ ] CI/CD with GitHub Actions
- [ ] Cross-platform builds
- [ ] C FFI bindings
- [ ] Performance benchmark suite

## v0.8.0 — Stability
- [ ] Fuzz testing for storage layer
- [ ] Crash recovery stress tests
- [ ] Memory safety audit
- [ ] Config migration system
- [ ] Beta testing feedback integration

## v0.9.0 — Release candidate
- [ ] Final API freeze
- [ ] Documentation complete
- [ ] Language bindings (Python, Node.js, Go)
- [ ] Release notes draft

## v1.0.0 — Ship it
- [ ] Tag v1.0.0
- [ ] Publish to crates.io
- [ ] Announcement post
- [ ] Benchmark comparison with SQLite
- [ ] Community feedback channel
