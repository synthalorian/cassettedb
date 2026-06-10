use anyhow::Result;
use prometheus::{Counter, Encoder, Histogram, HistogramOpts, Registry, TextEncoder};
use std::net::{TcpListener, TcpStream};
use std::io::Write;
use std::sync::OnceLock;
use std::thread;

/// Global metrics registry and counters.
pub struct Metrics {
    pub registry: Registry,
    pub inserts: Counter,
    pub updates: Counter,
    pub deletes: Counter,
    pub queries: Counter,
    pub searches: Counter,
    pub insert_latency: Histogram,
    pub query_latency: Histogram,
    pub replication_lag: Histogram,
}

impl Metrics {
    fn new() -> Result<Self> {
        let registry = Registry::new();

        let inserts = Counter::new("cassettedb_inserts_total", "Total number of document inserts")?;
        let updates = Counter::new("cassettedb_updates_total", "Total number of document updates")?;
        let deletes = Counter::new("cassettedb_deletes_total", "Total number of document deletes")?;
        let queries = Counter::new("cassettedb_queries_total", "Total number of queries")?;
        let searches = Counter::new("cassettedb_searches_total", "Total number of full-text searches")?;

        let insert_latency = Histogram::with_opts(
            HistogramOpts::new("cassettedb_insert_latency_seconds", "Insert operation latency")
                .buckets(vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
        )?;
        let query_latency = Histogram::with_opts(
            HistogramOpts::new("cassettedb_query_latency_seconds", "Query operation latency")
                .buckets(vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
        )?;
        let replication_lag = Histogram::with_opts(
            HistogramOpts::new("cassettedb_replication_lag_seconds", "Replication lag")
                .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
        )?;

        registry.register(Box::new(inserts.clone()))?;
        registry.register(Box::new(updates.clone()))?;
        registry.register(Box::new(deletes.clone()))?;
        registry.register(Box::new(queries.clone()))?;
        registry.register(Box::new(searches.clone()))?;
        registry.register(Box::new(insert_latency.clone()))?;
        registry.register(Box::new(query_latency.clone()))?;
        registry.register(Box::new(replication_lag.clone()))?;

        Ok(Self {
            registry,
            inserts,
            updates,
            deletes,
            queries,
            searches,
            insert_latency,
            query_latency,
            replication_lag,
        })
    }
}

static GLOBAL_METRICS: OnceLock<Metrics> = OnceLock::new();

/// Initialize the global metrics registry.
pub fn init() -> Result<&'static Metrics> {
    GLOBAL_METRICS.set(Metrics::new()?)
        .map_err(|_| anyhow::anyhow!("metrics already initialized"))?;
    Ok(GLOBAL_METRICS.get().unwrap())
}

/// Get the global metrics instance.
pub fn get() -> Option<&'static Metrics> {
    GLOBAL_METRICS.get()
}

/// Convenience wrappers for recording operations.
pub fn record_insert(duration_secs: f64) {
    if let Some(m) = get() {
        m.inserts.inc();
        m.insert_latency.observe(duration_secs);
    }
}

pub fn record_update() {
    if let Some(m) = get() {
        m.updates.inc();
    }
}

pub fn record_delete() {
    if let Some(m) = get() {
        m.deletes.inc();
    }
}

pub fn record_query(duration_secs: f64) {
    if let Some(m) = get() {
        m.queries.inc();
        m.query_latency.observe(duration_secs);
    }
}

pub fn record_search() {
    if let Some(m) = get() {
        m.searches.inc();
    }
}

pub fn record_replication_lag(duration_secs: f64) {
    if let Some(m) = get() {
        m.replication_lag.observe(duration_secs);
    }
}

/// Start a simple blocking HTTP server on the given address that serves /metrics.
/// Spawns a background thread.
pub fn start_server(addr: &str) -> Result<()> {
    let listener = TcpListener::bind(addr)?;
    println!("Metrics server listening on http://{}/metrics", addr);

    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let _ = handle_request(&mut stream);
            }
        }
    });

    Ok(())
}

fn handle_request(stream: &mut TcpStream) -> Result<()> {
    let mut buf = [0u8; 1024];
    let n = std::io::Read::read(&mut &*stream, &mut buf)?;
    let request = String::from_utf8_lossy(&buf[..n]);

    if request.starts_with("GET /metrics") || request.starts_with("GET /metrics ") {
        let metrics = match get() {
            Some(m) => m,
            None => {
                let response = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 20\r\n\r\nMetrics not initialized";
                stream.write_all(response.as_bytes())?;
                return Ok(());
            }
        };

        let encoder = TextEncoder::new();
        let metric_families = metrics.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        let body = String::from_utf8(buffer)?;

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes())?;
    } else {
        let body = "CassetteDB Metrics Server\n\nEndpoints:\n  GET /metrics  Prometheus metrics\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes())?;
    }

    Ok(())
}
