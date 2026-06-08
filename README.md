# CassetteDB

A single-file JSON document database inspired by SQLite, written in Rust.

## Design Goals

- **Single-file portability**: Every database is a self-contained `.cassette` file (plus a `.cassette.wal` during transactions).
- **ACID transactions**: Durability and atomicity via a Write-Ahead Log (WAL).
- **JSONPath-like queries**: Tiny DSL for filtering, comparisons, and logical combinators.
- **Full-text search**: Custom inverted index built over all string fields in documents.
- **Zero external server**: Embeddable library with a lightweight CLI.
- **Multi-language bindings**: Use CassetteDB from Rust, C, Python, Node.js, or Go.

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

To build the C FFI library:

```bash
cargo build -p libcassettedb --release
```

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

## Language Bindings

### Rust

```rust
use cassettedb::engine::CassetteEngine;
use cassettedb::document::Document;
use cassettedb::query::Query;
use serde_json::json;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut db = CassetteEngine::open(Path::new("mydb.cassette"))?;

    // Insert
    let id = db.insert(Document::new(json!({"name": "Ada", "age": 36})))?;
    println!("Inserted: {}", id);

    // Get
    if let Some(doc) = db.get(&id) {
        println!("Got: {:?}", doc.data);
    }

    // Update
    db.update(&id, json!({"name": "Ada", "age": 37}))?;

    // Query
    let results = db.query(&Query::parse("age > 30")?);
    println!("Matched {} documents", results.count);

    // Full-text search
    let results = db.query(&Query::parse(r#"search("Ada")"#)?);

    // Compact
    db.compact()?;

    // Delete
    db.delete(&id)?;

    Ok(())
}
```

### C

```c
#include "cassette.h"
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    CassetteDB *db = cassette_db_open("mydb.cassette");
    if (!db) {
        fprintf(stderr, "Failed to open database\n");
        return 1;
    }

    char *id = cassette_insert(db, "{\"name\":\"Ada\",\"age\":36}");
    printf("Inserted: %s\n", id);

    char *doc = cassette_get(db, id);
    printf("Got: %s\n", doc);
    cassette_free_string(doc);

    int rc = cassette_update(db, id, "{\"name\":\"Ada\",\"age\":37}");
    if (rc != 0) {
        char *err = cassette_last_error();
        fprintf(stderr, "Update failed: %s\n", err);
        cassette_free_string(err);
    }

    char *results = cassette_query(db, "age > 30");
    printf("Query results: %s\n", results);
    cassette_free_string(results);

    rc = cassette_compact(db);
    rc = cassette_delete(db, id);

    cassette_free_string(id);
    cassette_db_close(db);
    return 0;
}
```

Compile and link against `libcassettedb`:

```bash
gcc -o myapp myapp.c -L/target/release -lcassettedb -lpthread -ldl -lm
```

### Python

```python
from cassettedb import CassetteDB

db = CassetteDB("mydb.cassette")

doc_id = db.insert({"name": "Ada", "age": 36})
print(f"Inserted: {doc_id}")

doc = db.get(doc_id)
print(f"Got: {doc}")

db.update(doc_id, {"name": "Ada", "age": 37})

results = db.query("age > 30")
print(f"Matched: {results}")

results = db.query('search("Ada")')

db.compact()
db.delete(doc_id)
db.close()
```

Install the Python binding:

```bash
cd bindings/python
pip install -e .
```

### Node.js

```javascript
const { CassetteDB } = require('cassettedb');

const db = new CassetteDB('mydb.cassette');

const id = db.insert({ name: 'Ada', age: 36 });
console.log('Inserted:', id);

const doc = db.get(id);
console.log('Got:', doc);

db.update(id, { name: 'Ada', age: 37 });

const results = db.query('age > 30');
console.log('Matched:', results);

const searchResults = db.query('search("Ada")');

db.compact();
db.delete(id);
db.close();
```

Install the Node.js binding:

```bash
cd bindings/node
npm install
npm run build
```

### Go

```go
package main

import (
    "fmt"
    "log"

    "github.com/synthalorian/cassettedb/bindings/go"
)

func main() {
    db, err := cassettedb.Open("mydb.cassette")
    if err != nil {
        log.Fatal(err)
    }
    defer db.Close()

    id, err := db.Insert(`{"name":"Ada","age":36}`)
    if err != nil {
        log.Fatal(err)
    }
    fmt.Println("Inserted:", id)

    doc, err := db.Get(id)
    if err != nil {
        log.Fatal(err)
    }
    fmt.Println("Got:", doc)

    if err := db.Update(id, `{"name":"Ada","age":37}`); err != nil {
        log.Fatal(err)
    }

    results, err := db.Query("age > 30")
    if err != nil {
        log.Fatal(err)
    }
    fmt.Println("Matched:", results)

    if err := db.Compact(); err != nil {
        log.Fatal(err)
    }

    if err := db.Delete(id); err != nil {
        log.Fatal(err)
    }
}
```

Install the Go binding:

```bash
go get github.com/synthalorian/cassettedb/bindings/go
```

## Roadmap

- [x] Page-based storage with free-page list
- [x] WAL with commit flags and recovery
- [x] Inverted full-text index
- [x] JSONPath-like query DSL
- [x] CLI: init, insert, query, compact, dump, delete, get
- [x] C FFI bindings
- [x] Python, Node.js, and Go bindings (v0.9.0)
- [ ] B-tree secondary indexes
- [ ] Range queries on indexed fields
- [ ] Multi-document transactions (BEGIN / COMMIT / ROLLBACK)
- [ ] Memory-mapped I/O (`memmap2`)
- [ ] Tantivy integration (optional feature)
- [ ] Replication / change-feed

## Contributing

Contributions are welcome! Please open an issue or pull request on GitHub.

## License

MIT
