use cassettedb::document::Document;
use cassettedb::engine::CassetteEngine;
use cassettedb::query::Query;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde_json::json;
use std::path::PathBuf;
use tempfile::TempDir;

fn create_test_db() -> (TempDir, PathBuf, CassetteEngine) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bench.cassette");
    let engine = CassetteEngine::open(&path).unwrap();
    (dir, path, engine)
}

fn bench_insert(c: &mut Criterion) {
    c.bench_function("insert_single", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        b.iter(|| {
            let doc = Document::new(json!({"name": "test", "value": 42}));
            black_box(engine.insert(doc).unwrap());
        });
    });

    c.bench_function("insert_batch_100", |b| {
        b.iter(|| {
            let (_dir, _path, mut engine) = create_test_db();
            for i in 0..100 {
                let doc = Document::new(json!({"id": i, "name": "batch_test"}));
                black_box(engine.insert(doc).unwrap());
            }
        });
    });
}

fn bench_query(c: &mut Criterion) {
    c.bench_function("query_eq", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..1000 {
            engine
                .insert(Document::new(json!({"age": i % 100, "name": "user"})))
                .unwrap();
        }
        let q = Query::parse("age == 50").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });

    c.bench_function("query_range", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..1000 {
            engine
                .insert(Document::new(json!({"age": i, "name": "user"})))
                .unwrap();
        }
        let q = Query::parse("age > 500").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });

    c.bench_function("query_and", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..1000 {
            engine
                .insert(Document::new(json!({"age": i % 100, "active": i % 2 == 0})))
                .unwrap();
        }
        let q = Query::parse("age > 50 and active == true").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });
}

fn bench_search(c: &mut Criterion) {
    c.bench_function("search_fulltext", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..1000 {
            engine
                .insert(Document::new(json!({
                    "title": format!("Document number {}", i),
                    "body": "Lorem ipsum dolor sit amet"
                })))
                .unwrap();
        }
        let q = Query::parse("search(\"lorem\")").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });
}

fn bench_compact(c: &mut Criterion) {
    c.bench_function("compact_1000_docs", |b| {
        b.iter_with_setup(
            || {
                let (_dir, _path, mut engine) = create_test_db();
                for i in 0..1000 {
                    engine
                        .insert(Document::new(json!({"id": i, "data": "test"})))
                        .unwrap();
                }
                engine
            },
            |mut engine| {
                black_box(engine.compact().unwrap());
            },
        );
    });
}

criterion_group!(
    benches,
    bench_insert,
    bench_query,
    bench_search,
    bench_compact
);
criterion_main!(benches);
