use cassettedb::document::Document;
use cassettedb::engine::CassetteEngine;
use cassettedb::query::Query;
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
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
    let mut group = c.benchmark_group("insert");
    
    group.bench_function("insert_single", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        b.iter(|| {
            let doc = Document::new(json!({"name": "test", "value": 42}));
            black_box(engine.insert(doc).unwrap());
        });
    });

    group.bench_function("insert_batch_100", |b| {
        b.iter(|| {
            let (_dir, _path, mut engine) = create_test_db();
            for i in 0..100 {
                let doc = Document::new(json!({"id": i, "name": "batch_test"}));
                black_box(engine.insert(doc).unwrap());
            }
        });
    });
    
    group.bench_function("insert_batch_1000", |b| {
        b.iter(|| {
            let (_dir, _path, mut engine) = create_test_db();
            for i in 0..1000 {
                let doc = Document::new(json!({"id": i, "name": "batch_test"}));
                black_box(engine.insert(doc).unwrap());
            }
        });
    });
    
    group.finish();
}

fn bench_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("query");
    
    group.bench_function("query_eq_small", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..100 {
            engine.insert(Document::new(json!({"age": i % 10, "name": "user"}))).unwrap();
        }
        let q = Query::parse("age == 5").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });

    group.bench_function("query_eq_large", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..10000 {
            engine.insert(Document::new(json!({"age": i % 100, "name": "user"}))).unwrap();
        }
        let q = Query::parse("age == 50").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });

    group.bench_function("query_range", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..1000 {
            engine.insert(Document::new(json!({"age": i, "name": "user"}))).unwrap();
        }
        let q = Query::parse("age > 500").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });

    group.bench_function("query_and", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..1000 {
            engine.insert(Document::new(json!({"age": i % 100, "active": i % 2 == 0}))).unwrap();
        }
        let q = Query::parse("age > 50 and active == true").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });
    
    group.bench_function("query_or", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..1000 {
            engine.insert(Document::new(json!({"age": i % 100, "role": if i % 2 == 0 { "admin" } else { "user" }}))).unwrap();
        }
        let q = Query::parse("age > 80 or role == \"admin\"").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });
    
    group.finish();
}

fn bench_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("search");
    
    group.bench_function("search_fulltext_small", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..100 {
            engine.insert(Document::new(json!({
                "title": format!("Document number {}", i),
                "body": "Lorem ipsum dolor sit amet"
            }))).unwrap();
        }
        let q = Query::parse("search(\"lorem\")").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });
    
    group.bench_function("search_fulltext_large", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        for i in 0..5000 {
            engine.insert(Document::new(json!({
                "title": format!("Document number {}", i),
                "body": "Lorem ipsum dolor sit amet consectetur adipiscing elit"
            }))).unwrap();
        }
        let q = Query::parse("search(\"lorem\")").unwrap();
        b.iter(|| {
            black_box(engine.query(&q));
        });
    });
    
    group.finish();
}

fn bench_update_delete(c: &mut Criterion) {
    let mut group = c.benchmark_group("update_delete");
    
    group.bench_function("update_single", |b| {
        let (_dir, _path, mut engine) = create_test_db();
        let mut ids = Vec::new();
        for i in 0..100 {
            ids.push(engine.insert(Document::new(json!({"value": i}))).unwrap());
        }
        let mut i = 0;
        b.iter(|| {
            let id = &ids[i % ids.len()];
            black_box(engine.update(id, json!({"value": i})).unwrap());
            i += 1;
        });
    });
    
    group.bench_function("delete_single", |b| {
        b.iter_with_setup(
            || {
                let (_dir, _path, mut engine) = create_test_db();
                let mut ids = Vec::new();
                for i in 0..100 {
                    ids.push(engine.insert(Document::new(json!({"value": i}))).unwrap());
                }
                (ids, engine)
            },
            |(ids, mut engine)| {
                for id in ids {
                    black_box(engine.delete(&id).unwrap());
                }
            },
        );
    });
    
    group.finish();
}

fn bench_compact(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact");
    
    for size in [100, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::new("compact_docs", size), size, |b, &size| {
            b.iter_with_setup(
                || {
                    let (_dir, _path, mut engine) = create_test_db();
                    for i in 0..size {
                        engine.insert(Document::new(json!({"id": i, "data": "test"}))).unwrap();
                    }
                    engine
                },
                |mut engine| {
                    black_box(engine.compact().unwrap());
                },
            );
        });
    }
    
    group.finish();
}

fn bench_raft(c: &mut Criterion) {
    use cassettedb::raft::{create_raft_node, ClusterCommand, RequestVoteResponse};
    
    let mut group = c.benchmark_group("raft");
    
    group.bench_function("raft_election_3_nodes", |b| {
        b.iter(|| {
            let node = create_raft_node("a".to_string(), vec!["b".to_string(), "c".to_string()]);
            let req = node.start_election();
            black_box(node.record_vote("b".to_string(), RequestVoteResponse {
                term: req.term,
                vote_granted: true,
            }));
        });
    });
    
    group.bench_function("raft_propose_command", |b| {
        let node = create_raft_node("a".to_string(), vec!["b".to_string(), "c".to_string()]);
        node.start_election();
        node.record_vote("b".to_string(), RequestVoteResponse {
            term: 1,
            vote_granted: true,
        });
        assert!(node.is_leader());
        
        b.iter(|| {
            black_box(node.propose(ClusterCommand::NoOp).unwrap());
        });
    });
    
    group.bench_function("raft_heartbeat_5_nodes", |b| {
        let node = create_raft_node("a".to_string(), vec![
            "b".to_string(), "c".to_string(), "d".to_string(), "e".to_string()
        ]);
        node.start_election();
        node.record_vote("b".to_string(), RequestVoteResponse {
            term: 1,
            vote_granted: true,
        });
        assert!(node.is_leader());
        
        b.iter(|| {
            for peer in &["b", "c", "d", "e"] {
                black_box(node.heartbeat_for(&peer.to_string()));
            }
        });
    });
    
    group.finish();
}

fn bench_shard(c: &mut Criterion) {
    use cassettedb::shard::ShardRouter;
    use serde_json::json;
    
    let mut group = c.benchmark_group("shard");
    
    group.bench_function("shard_insert_2_shards", |b| {
        let dir = TempDir::new().unwrap();
        let mut router = ShardRouter::with_shards(
            vec!["s0".to_string(), "s1".to_string()],
            dir.path(),
        ).unwrap();
        let mut i = 0;
        b.iter(|| {
            let doc = Document::new(json!({"id": i, "data": "test"}));
            black_box(router.insert(doc).unwrap());
            i += 1;
        });
    });
    
    group.bench_function("shard_query_4_shards", |b| {
        let dir = TempDir::new().unwrap();
        let mut router = ShardRouter::with_shards(
            vec!["s0".to_string(), "s1".to_string(), "s2".to_string(), "s3".to_string()],
            dir.path(),
        ).unwrap();
        for i in 0..1000 {
            let doc = Document::new(json!({"age": i % 100, "name": "user"}));
            router.insert(doc).unwrap();
        }
        let q = Query::parse("age > 50").unwrap();
        b.iter(|| {
            black_box(router.query_all(&q));
        });
    });
    
    group.bench_function("shard_hash_distribution", |b| {
        b.iter(|| {
            for i in 0..100 {
                black_box(ShardRouter::hash_key(&format!("doc-{}", i)));
            }
        });
    });
    
    group.finish();
}

fn bench_dist_tx(c: &mut Criterion) {
    use cassettedb::dist_tx::{TwoPhaseCoordinator, ParticipantVote, PrepareResponse};
    
    let mut group = c.benchmark_group("dist_tx");
    
    group.bench_function("dist_tx_begin", |b| {
        let coord = TwoPhaseCoordinator::new("coord".to_string());
        let mut i = 0;
        b.iter(|| {
            black_box(coord.begin(format!("tx-{}", i), vec!["p1".to_string(), "p2".to_string()]));
            i += 1;
        });
    });
    
    group.bench_function("dist_tx_full_2pc", |b| {
        let coord = TwoPhaseCoordinator::new("coord".to_string());
        let mut i = 0;
        b.iter(|| {
            let tx_id = format!("tx-{}", i);
            coord.begin(tx_id.clone(), vec!["p1".to_string(), "p2".to_string()]);
            let responses = vec![
                PrepareResponse {
                    tx_id: tx_id.clone(),
                    node_id: "p1".to_string(),
                    vote: ParticipantVote::Yes,
                },
                PrepareResponse {
                    tx_id: tx_id.clone(),
                    node_id: "p2".to_string(),
                    vote: ParticipantVote::Yes,
                },
            ];
            black_box(coord.prepare(&tx_id, responses).unwrap());
            black_box(coord.commit(&tx_id).unwrap());
            i += 1;
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_insert,
    bench_query,
    bench_search,
    bench_update_delete,
    bench_compact,
    bench_raft,
    bench_shard,
    bench_dist_tx
);
criterion_main!(benches);
