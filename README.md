# CassetteDB

A single-file JSON document database inspired by SQLite, written in Rust.

## Design Goals

- **Single-file portability**: Every database is a self-contained `.cassette` file (plus a `.cassette.wal` during transactions).
- **ACID transactions**: Durability and atomicity via a Write-Ahead Log (WAL).
- **JSONPath-like queries**: Tiny DSL for filtering, comparisons, and logical combinators.
- **Full-text search**: Custom inverted index built over all string fields in documents.
- **Zero external server**: Embeddable library with a lightweight CLI.

## Architecture

```
┌─────────────────────────────────────┐
│           CassetteEngine            │
├─────────────┬──────────┬────────────┤
│   Storage   │   WAL    │   Index    │
│  (.cassette)│(.cassette│ (inverted) │
│             │  .wal)   │            │
└─────────────┴──────────┴────────────┘
```

- **Storage**: Page-based file format (4 KiB pages). Page 0 is the header; subsequent pages store document data.
- **WAL**: Append-only log with CRC32 checksums and commit flags. Uncommitted records are ignored on recovery.
- **Index**: In-memory inverted index (term → doc_ids) rebuilt from WAL on open, kept in sync with mutations.
- **Query**: Parses expressions like `age > 28`, `search("alice")`, or `age >= 30 and search("engineer")`.

## Building

```bash
cd cassettedb
cargo build --release
```

The `cassette` binary will be at `target/release/cassette`.

## CLI Usage

```bash
# Initialize a new database
cassette init music.cassette

# Insert a document
cassette insert music.cassette '{"artist":"Radiohead","album":"OK Computer","year":1997}'

# Query
cassette query music.cassette 'year > 1995'
cassette query music.cassette 'search("radiohead")'
cassette query music.cassette 'year >= 1990 and search("computer")'

# Get / delete by ID
cassette get music.cassette <uuid>
cassette delete music.cassette <uuid>

# Compact (rewrite main file, truncate WAL)
cassette compact music.cassette

# Dump everything
cassette dump music.cassette
```

## Query Language

| Expression | Meaning |
|------------|---------|
| `*` or `$` | All documents |
| `$.field == value` | Equality (string, number, bool) |
| `$.field > num` | Greater than |
| `$.field < num` | Less than |
| `$.field >= num` | Greater than or equal |
| `$.field <= num` | Less than or equal |
| `search("term")` | Full-text search |
| `expr1 and expr2` | Logical AND |
| `expr1 or expr2` | Logical OR |

Paths may omit the leading `$.` for brevity.

## Library Usage

```rust
use cassettedb::engine::CassetteEngine;
use cassettedb::document::Document;
use cassettedb::query::Query;
use serde_json::json;
use std::path::Path;

let mut db = CassetteEngine::open(Path::new("mydb.cassette"))?;
let id = db.insert(Document::new(json!({"name": "Ada"})))?;
let results = db.query(&Query::parse("name == \"Ada\"")?);
```

## Roadmap

- [x] Page-based storage with free-page list
- [x] WAL with commit flags and recovery
- [x] Inverted full-text index
- [x] JSONPath-like query DSL
- [x] CLI: init, insert, query, compact, dump, delete, get
- [ ] B-tree secondary indexes
- [ ] Range queries on indexed fields
- [ ] Multi-document transactions (BEGIN / COMMIT / ROLLBACK)
- [ ] Memory-mapped I/O (`memmap2`)
- [ ] Tantivy integration (optional feature)
- [ ] Replication / change-feed

## License

MIT
