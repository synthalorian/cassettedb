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
