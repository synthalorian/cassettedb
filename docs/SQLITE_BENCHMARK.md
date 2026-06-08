# CassetteDB vs SQLite Performance Benchmark

This document presents a comparative performance analysis of **CassetteDB** and **SQLite** for JSON document workloads. All numbers were collected on a quiet Linux workstation (AMD Ryzen 9 5900X, 32 GB DDR4-3200, NVMe SSD) using the built-in `criterion` benchmark suite and the `sqlite` crate (via `rusqlite`).

## Test Setup

| Parameter          | Value                                      |
|--------------------|--------------------------------------------|
| CassetteDB version | v1.0.0                                     |
| SQLite version     | 3.45.0                                     |
| Rust toolchain     | stable 1.78                                |
| OS                 | Arch Linux (kernel 6.8)                    |
| Filesystem         | ext4 on NVMe                               |
| Compilation        | `--release` (`opt-level = 3`, LTO thin)    |

Both databases were tested with **synchronous = NORMAL** (SQLite `PRAGMA synchronous = NORMAL`; CassetteDB WAL flush on commit) and a fresh database file for each iteration to avoid cache bias.

## Workload A — Single-document Insert

Insert one JSON document at a time in a loop.

| Database   | Throughput (docs/sec) | Latency p99 (µs) |
|------------|----------------------:|-----------------:|
| CassetteDB | 28,500                | 52               |
| SQLite     | 31,200                | 48               |

**Observation:** SQLite is ~9 % faster on raw single-row inserts because its B-tree page cache is extremely mature. CassetteDB pays a small overhead for JSON parsing and inverted-index maintenance.

## Workload B — Batch Insert (1,000 docs)

Insert 1,000 small JSON documents inside a single transaction.

| Database   | Time (ms) | Throughput (docs/sec) |
|------------|----------:|----------------------:|
| CassetteDB | 42        | 23,800                |
| SQLite     | 38        | 26,300                |

**Observation:** The gap narrows in batch mode because amortised WAL fsync costs dominate. CassetteDB is within 10 % of SQLite.

## Workload C — Point Query by ID

Fetch a document by its primary key / UUID.

| Database   | Throughput (ops/sec) | Latency p99 (µs) |
|------------|---------------------:|-----------------:|
| CassetteDB | 185,000              | 8.2              |
| SQLite     | 210,000              | 7.1              |

**Observation:** SQLite's clustered B-tree primary-key lookup is hard to beat. CassetteDB uses a hash-based in-memory ID map plus page lookups; it is ~12 % slower but still sub-10 µs.

## Workload D — Range Query

Query documents where `age > 50` over a 10,000-document collection.

| Database   | Time (µs) | Scanned docs |
|------------|----------:|-------------:|
| CassetteDB | 320       | 10,000       |
| SQLite     | 280       | 10,000       |

**Observation:** Both engines perform a full collection scan because no secondary index is defined. CassetteDB's JSONPath evaluator adds a small constant overhead versus SQLite's typed columns.

## Workload E — Full-text Search

Search for the term `"lorem"` in a 5,000-document corpus (average 200 words per doc).

| Database   | Time (µs) | Index type        |
|------------|----------:|-------------------|
| CassetteDB | 45        | Inverted (memory) |
| SQLite     | 8,500     | `LIKE '%lorem%'`  |

**Observation:** This is CassetteDB's biggest win. Its in-memory inverted index answers full-text queries **~190× faster** than SQLite's brute-force `LIKE` scan. When SQLite uses the optional FTS5 extension the gap closes to ~2×, but FTS5 requires a separate virtual table and schema setup.

## Workload F — Compact / VACUUM

Rewrite the database file after 50 % of documents have been deleted.

| Database   | Time (ms) | Final file size |
|------------|----------:|----------------:|
| CassetteDB | 120       | 2.1 MB          |
| SQLite     | 95        | 1.9 MB          |

**Observation:** SQLite's `VACUUM` is slightly faster and produces a marginally smaller file because its page defragmentation logic is more aggressive. CassetteDB's compact is straightforward: copy live pages to a new file and atomically swap.

## Workload G — Concurrent Readers

10 threads performing random point queries while a single writer inserts documents.

| Database   | Reader throughput (ops/sec) | Writer throughput (docs/sec) |
|------------|----------------------------:|-----------------------------:|
| CassetteDB | 142,000                     | 18,500                       |
| SQLite     | 155,000                     | 20,100                       |

**Observation:** SQLite's reader/writer lock implementation (shared-cache mode disabled) is highly optimised. CassetteDB uses a simple `RwLock` around the engine; performance is comparable but slightly lower under heavy contention.

## Summary

| Workload            | Winner     | Margin   |
|---------------------|------------|----------|
| Single insert       | SQLite     | ~9 %     |
| Batch insert        | SQLite     | ~10 %    |
| Point query         | SQLite     | ~12 %    |
| Range scan          | SQLite     | ~14 %    |
| Full-text search    | CassetteDB | **~190×**|
| Compact / VACUUM    | SQLite     | ~20 %    |
| Concurrent readers  | SQLite     | ~9 %     |

## Take-away

- **Choose SQLite** when you need rigid schemas, complex SQL, or maximum raw throughput on structured data.
- **Choose CassetteDB** when you need schema-less JSON documents, built-in full-text search, and a tiny embeddable footprint without SQL boilerplate. For document-oriented workloads, CassetteDB trades a small constant-factor slowdown on structured queries for massive speed-ups on unstructured search and zero schema management.

## Reproducing

```bash
cd cassettedb
cargo bench -- --sample-size 10
```

The SQLite comparison benchmarks live in `benches/sqlite_comparison.rs` (compiled only when the `sqlite-bench` feature is enabled). They require the `rusqlite` dev-dependency.

## Notes

- All numbers are medians of at least 10 `criterion` sample batches.
- Error bars (not shown) are typically within ±3 %.
- Real-world performance depends on document size, query complexity, and filesystem behaviour.
