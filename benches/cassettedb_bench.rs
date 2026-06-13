use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, SamplingMode, Throughput};
use tempfile::TempDir;
use rand::seq::SliceRandom;
use rand::thread_rng;

fn make_doc(i: usize) -> serde_json::Value {
    serde_json::json!({
        "id": i,
        "name": format!("user_{}", i),
        "email": format!("user{}@example.com", i),
        "age": (i % 80) + 18,
        "bio": format!("This is a biography for user number {}. They love coding and music.", i),
        "profile": {
            "age": (i % 80) + 18,
            "city": format!("city_{}", i % 10),
        },
    })
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert");
    for size in [100, 1000, 5000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("cassettedb", size), size, |b, &size| {
            b.iter(|| {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("test.cassette");
                let mut db = cassettedb::db::Cassette::new();
                for i in 0..size {
                    let doc = serde_json::json!({
                        "id": i,
                        "name": format!("user_{}", i),
                        "email": format!("user{}@example.com", i),
                        "age": (i % 80) + 18,
                    });
                    db.insert("users", doc).unwrap();
                }
                db.save(&path).unwrap();
            });
        });
    }
    group.finish();
}

fn bench_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("query");
    for size in [100, 1000, 5000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("cassettedb", size), size, |b, &size| {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("test.cassette");
            let mut db = cassettedb::db::Cassette::new();
            for i in 0..size {
                db.insert("users", make_doc(i)).unwrap();
            }
            db.save(&path).unwrap();

            b.iter(|| {
                let db = cassettedb::db::Cassette::open(&path).unwrap();
                let _results = db.query("users", "name=user_50").unwrap();
            });
        });
    }
    group.finish();
}

fn bench_query_latency_percentiles(c: &mut Criterion) {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.cassette");
    let mut db = cassettedb::db::Cassette::new();
    for i in 0..10000 {
        db.insert("users", make_doc(i)).unwrap();
    }
    db.save(&path).unwrap();

    let mut group = c.benchmark_group("query_latency");
    group.sampling_mode(SamplingMode::Flat);
    group.sample_size(500);
    group.measurement_time(std::time::Duration::from_secs(15));

    group.bench_function("exact_match_10k", |b| {
        b.iter(|| {
            let db = cassettedb::db::Cassette::open(&path).unwrap();
            let _results = db.query("users", "name=user_5000").unwrap();
        });
    });

    group.bench_function("search_10k", |b| {
        b.iter(|| {
            let db = cassettedb::db::Cassette::open(&path).unwrap();
            let _results = db.search("users", "coding music").unwrap();
        });
    });

    group.bench_function("jsonpath_10k", |b| {
        b.iter(|| {
            let db = cassettedb::db::Cassette::open(&path).unwrap();
            let _results = db.query_jsonpath("users", "$.profile.city").unwrap();
        });
    });

    group.finish();
}

fn bench_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("search");
    for size in [100, 1000, 5000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("cassettedb", size), size, |b, &size| {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("test.cassette");
            let mut db = cassettedb::db::Cassette::new();
            for i in 0..size {
                db.insert("users", make_doc(i)).unwrap();
            }
            db.save(&path).unwrap();

            b.iter(|| {
                let db = cassettedb::db::Cassette::open(&path).unwrap();
                let _results = db.search("users", "coding music").unwrap();
            });
        });
    }
    group.finish();
}

fn bench_jsonpath(c: &mut Criterion) {
    let mut group = c.benchmark_group("jsonpath");
    for size in [100, 1000, 5000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("cassettedb", size), size, |b, &size| {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("test.cassette");
            let mut db = cassettedb::db::Cassette::new();
            for i in 0..size {
                db.insert("users", make_doc(i)).unwrap();
            }
            db.save(&path).unwrap();

            b.iter(|| {
                let db = cassettedb::db::Cassette::open(&path).unwrap();
                let _results = db.query_jsonpath("users", "$.profile.city").unwrap();
            });
        });
    }
    group.finish();
}

fn bench_random_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_reads");
    for size in [100, 1000, 5000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("cassettedb", size), size, |b, &size| {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("test.cassette");
            let mut db = cassettedb::db::Cassette::new();
            let mut ids = Vec::with_capacity(size);
            for i in 0..size {
                let id = db.insert("users", make_doc(i)).unwrap();
                ids.push(id);
            }
            db.save(&path).unwrap();

            let mut rng = thread_rng();
            ids.shuffle(&mut rng);

            b.iter(|| {
                let db = cassettedb::db::Cassette::open(&path).unwrap();
                for id in &ids {
                    let _ = db.get("users", id);
                }
            });
        });
    }
    group.finish();
}

fn bench_sequential_scans(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_scans");
    for size in [100, 1000, 5000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("cassettedb", size), size, |b, &size| {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("test.cassette");
            let mut db = cassettedb::db::Cassette::new();
            for i in 0..size {
                db.insert("users", make_doc(i)).unwrap();
            }
            db.save(&path).unwrap();

            b.iter(|| {
                let db = cassettedb::db::Cassette::open(&path).unwrap();
                let _results = db.scan("users").unwrap();
            });
        });
    }
    group.finish();
}

fn bench_write_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_throughput");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("insert_1k_docs", |b| {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.cassette");
        let mut db = cassettedb::db::Cassette::new();
        let mut counter = 0usize;

        b.iter(|| {
            for _ in 0..1000 {
                let doc = make_doc(counter);
                db.insert("users", doc).unwrap();
                counter += 1;
            }
            db.save(&path).unwrap();
        });
    });

    group.finish();
}

fn bench_replication_lag(_c: &mut Criterion) {
    // Replication lag benchmark requires the `replication` feature
}

criterion_group!(
    benches,
    bench_insert,
    bench_query,
    bench_search,
    bench_jsonpath,
    bench_query_latency_percentiles,
    bench_random_reads,
    bench_sequential_scans,
    bench_write_throughput,
    bench_replication_lag
);
criterion_main!(benches);
