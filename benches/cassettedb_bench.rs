use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, SamplingMode, Throughput};
use tempfile::TempDir;
use rand::seq::SliceRandom;

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

            let mut rng = rand::rng();
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

#[cfg(feature = "replication")]
fn bench_replication_lag(c: &mut Criterion) {
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;
    use tokio::sync::broadcast;

    let mut group = c.benchmark_group("replication_lag");
    group.sampling_mode(SamplingMode::Flat);
    group.sample_size(50);
    group.measurement_time(std::time::Duration::from_secs(10));

    group.bench_function("command_roundtrip", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("follower.cassette");

        // Start leader
        let (tx, _rx) = broadcast::channel::<cassettedb::replication::ReplicateCmd>(1024);
        let tx_arc = Arc::new(tx);
        let tx_clone = Arc::clone(&tx_arc);

        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn leader acceptor
        let leader_handle = rt.spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let mut rx = tx_clone.subscribe();
                tokio::spawn(async move {
                    let (_, mut writer) = socket.into_split();
                    loop {
                        match rx.recv().await {
                            Ok(cmd) => {
                                let line = serde_json::to_string(&cmd).unwrap() + "\n";
                                if writer.write_all(line.as_bytes()).await.is_err() {
                                    break;
                                }
                                if writer.flush().await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
        });

        // Connect follower
        let follower_path = path.clone();
        let follower_handle = rt.spawn(async move {
            let stream = TcpStream::connect(addr).await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            // Handshake
            let handshake = serde_json::json!({ "role": "follower" });
            writer.write_all((serde_json::to_string(&handshake).unwrap() + "\n").as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            let mut cassette = cassettedb::db::Cassette::open(&follower_path).unwrap();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Ok(cmd) = serde_json::from_str::<cassettedb::replication::ReplicateCmd>(&line) {
                            match cmd {
                                cassettedb::replication::ReplicateCmd::Insert { collection, doc } => {
                                    let _ = cassette.insert(&collection, doc);
                                }
                                cassettedb::replication::ReplicateCmd::Update { collection, id, doc } => {
                                    let _ = cassette.update(&collection, &id, doc);
                                }
                                cassettedb::replication::ReplicateCmd::Delete { collection, id } => {
                                    let _ = cassette.delete(&collection, &id);
                                }
                                cassettedb::replication::ReplicateCmd::Heartbeat => {}
                            }
                            let _ = cassette.save(&follower_path);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Give time for connection to establish
        std::thread::sleep(std::time::Duration::from_millis(100));

        let cmd = cassettedb::replication::ReplicateCmd::Insert {
            collection: "users".to_string(),
            doc: make_doc(0),
        };

        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let _ = tx_arc.send(cmd.clone());
                // Small delay to allow processing; we're measuring broadcast + network + apply
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
            start.elapsed()
        });

        drop(leader_handle);
        drop(follower_handle);
    });

    group.finish();
}

#[cfg(not(feature = "replication"))]
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
