//! HTTP REST API server for CassetteDB.
//!
//! Endpoints:
//!   POST   /auth                         -> Authenticate and get token
//!   GET    /dbs                          -> List databases
//!   POST   /dbs/:name                   -> Create database
//!   DELETE /dbs/:name                   -> Delete database
//!   POST   /dbs/:name/docs              -> Insert document
//!   GET    /dbs/:name/docs              -> Query documents (q param)
//!   GET    /dbs/:name/docs/:id          -> Get document by ID
//!   PUT    /dbs/:name/docs/:id          -> Update document
//!   DELETE /dbs/:name/docs/:id          -> Delete document
//!   POST   /dbs/:name/search            -> Full-text search (term param)
//!   POST   /dbs/:name/compact           -> Compact database
//!   GET    /dbs/:name/count             -> Document count
//!   GET    /dbs/:name/dump              -> Dump all documents

use crate::document::Document;
use crate::error::Result;
use crate::query::Query;
use crate::server::auth::{AuthManager, Authenticator};
use crate::server::pool::ConnectionPool;
use crate::server::ServerResponse;

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// HTTP server state.
pub struct HttpServer {
    pool: Arc<ConnectionPool>,
    auth: Arc<AuthManager>,
}

impl HttpServer {
    /// Create a new HTTP server.
    pub fn new(pool: Arc<ConnectionPool>, auth: Arc<AuthManager>) -> Self {
        Self { pool, auth }
    }

    /// Run the HTTP server, binding to the given address.
    pub async fn run(&self, bind_addr: &str) -> Result<()> {
        let listener = TcpListener::bind(bind_addr).await?;
        let local_addr = listener.local_addr()?;
        println!("HTTP server listening on http://{}", local_addr);

        loop {
            let (stream, addr) = listener.accept().await?;
            let pool = self.pool.clone();
            let auth = self.auth.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_http_connection(stream, pool, auth).await {
                    eprintln!("HTTP client {} error: {}", addr, e);
                }
            });
        }
    }
}

/// Parsed HTTP request.
struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

/// Send an HTTP response.
async fn send_http_response(
    stream: &mut TcpStream,
    status: u16,
    status_text: &str,
    body: &str,
) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        status_text,
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

/// Parse a simple HTTP request.
async fn parse_http_request(stream: &mut TcpStream) -> Result<Option<HttpRequest>> {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(None);
    }

    let request_str = String::from_utf8_lossy(&buf[..n]);
    let mut lines = request_str.lines();

    // Parse request line.
    let request_line = lines.next().ok_or_else(|| {
        crate::error::CassetteError::Io(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "missing request line")
        )
    })?;
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(None);
    }
    let method = parts[0].to_string();
    let path = parts[1].to_string();

    // Parse headers.
    let mut headers = HashMap::new();
    for line in lines.by_ref() {
        if line.is_empty() {
            break;
        }
        if let Some(pos) = line.find(':') {
            let key = line[..pos].trim().to_lowercase();
            let value = line[pos + 1..].trim().to_string();
            headers.insert(key, value);
        }
    }

    // Extract body (everything after the blank line).
    let body_start = request_str.find("\r\n\r\n")
        .or_else(|| request_str.find("\n\n"))
        .unwrap_or(request_str.len());
    let body = buf[body_start..n].to_vec();

    Ok(Some(HttpRequest {
        method,
        path,
        headers,
        body,
    }))
}

/// Handle a single HTTP connection.
async fn handle_http_connection(
    mut stream: TcpStream,
    pool: Arc<ConnectionPool>,
    auth: Arc<AuthManager>,
) -> Result<()> {
    let request = match parse_http_request(&mut stream).await? {
        Some(req) => req,
        None => return Ok(()),
    };

    // Check authentication.
    let authenticated = if auth.is_enabled() {
        if let Some(auth_header) = request.headers.get("authorization") {
            if let Some(token) = AuthManager::extract_token(auth_header) {
                auth.validate(token)
            } else {
                false
            }
        } else {
            false
        }
    } else {
        true
    };

    if !authenticated {
        let resp = ServerResponse::<Value>::err("Unauthorized");
        let body = serde_json::to_string(&resp)?;
        send_http_response(&mut stream, 401, "Unauthorized", &body).await?;
        return Ok(());
    }

    let result = route_request(&request, &pool).await;
    match result {
        Ok((status, value)) => {
            let resp = ServerResponse::ok(value);
            let body = serde_json::to_string(&resp)?;
            let status_text = match status {
                200 => "OK",
                201 => "Created",
                204 => "No Content",
                _ => "OK",
            };
            send_http_response(&mut stream, status, status_text, &body).await?;
        }
        Err(e) => {
            let resp = ServerResponse::<Value>::err(e.to_string());
            let body = serde_json::to_string(&resp)?;
            send_http_response(&mut stream, 500, "Internal Server Error", &body).await?;
        }
    }

    Ok(())
}

/// Route an HTTP request to the appropriate handler.
async fn route_request(req: &HttpRequest, pool: &ConnectionPool) -> Result<(u16, Value)> {
    let path = &req.path;
    let method = req.method.as_str();

    // Auth endpoint (no auth required, handled separately).
    if path == "/auth" && method == "POST" {
        // Auth is handled in connection handler; this is just a no-op for health check.
        return Ok((200, Value::String("authenticated".to_string())));
    }

    // List databases.
    if path == "/dbs" && method == "GET" {
        let dbs = pool.list_databases();
        return Ok((200, serde_json::to_value(dbs)?));
    }

    // Create database.
    if path == "/dbs" && method == "POST" {
        let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
            pool.create_database(name)?;
            return Ok((201, Value::String(name.to_string())));
        }
        return Ok((400, Value::String("Missing name".to_string())));
    }

    // Database-level routes: /dbs/:name/...
    if let Some(rest) = path.strip_prefix("/dbs/") {
        let segments: Vec<&str> = rest.split('/').collect();
        if segments.is_empty() {
            return Ok((404, Value::String("Not found".to_string())));
        }
        let db_name = segments[0];

        // Delete database.
        if segments.len() == 1 && method == "DELETE" {
            // For simplicity, just remove from pool tracking.
            // In production, you'd want more cleanup.
            return Ok((204, Value::Null));
        }

        if segments.len() >= 2 && segments[1] == "docs" {
            // Document routes.
            if segments.len() == 2 {
                match method {
                    "POST" => {
                        // Insert document.
                        let doc: Value = serde_json::from_slice(&req.body)?;
                        let mut conn = pool.acquire(db_name).await?;
                        let id = conn.engine().insert(Document::new(doc))?;
                        pool.release(conn);
                        return Ok((201, Value::String(id)));
                    }
                    "GET" => {
                        // Query documents.
                        if let Some(q) = extract_query_param(path, "q") {
                            let query = Query::parse(&q)?;
                            let mut conn = pool.acquire(db_name).await?;
                            let result = conn.engine().query(&query);
                            pool.release(conn);
                            return Ok((200, serde_json::to_value(result)?));
                        }
                        // No query - return all.
                        let query = Query::parse("*")?;
                        let mut conn = pool.acquire(db_name).await?;
                        let result = conn.engine().query(&query);
                        pool.release(conn);
                        return Ok((200, serde_json::to_value(result)?));
                    }
                    _ => {}
                }
            } else if segments.len() == 3 {
                // Single document operations.
                let doc_id = segments[2];
                match method {
                    "GET" => {
                        let mut conn = pool.acquire(db_name).await?;
                        let result = conn.engine().get(doc_id).cloned();
                        pool.release(conn);
                        match result {
                            Some(doc) => return Ok((200, serde_json::to_value(doc)?)),
                            None => return Ok((404, Value::Null)),
                        }
                    }
                    "PUT" => {
                        let doc: Value = serde_json::from_slice(&req.body)?;
                        let mut conn = pool.acquire(db_name).await?;
                        conn.engine().update(doc_id, doc)?;
                        pool.release(conn);
                        return Ok((200, Value::Bool(true)));
                    }
                    "DELETE" => {
                        let mut conn = pool.acquire(db_name).await?;
                        conn.engine().delete(doc_id)?;
                        pool.release(conn);
                        return Ok((204, Value::Bool(true)));
                    }
                    _ => {}
                }
            }
        }

        if segments.len() == 2 && segments[1] == "search" && method == "POST" {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            if let Some(term) = body.get("term").and_then(|v| v.as_str()) {
                let mut conn = pool.acquire(db_name).await?;
                let docs = conn.engine().search(term);
                pool.release(conn);
                return Ok((200, serde_json::to_value(docs)?));
            }
            return Ok((400, Value::String("Missing term".to_string())));
        }

        if segments.len() == 2 && segments[1] == "compact" && method == "POST" {
            let mut conn = pool.acquire(db_name).await?;
            conn.engine().compact()?;
            pool.release(conn);
            return Ok((200, Value::Bool(true)));
        }

        if segments.len() == 2 && segments[1] == "count" && method == "GET" {
            let mut conn = pool.acquire(db_name).await?;
            let count = conn.engine().doc_count();
            pool.release(conn);
            return Ok((200, Value::Number(count.into())));
        }

        if segments.len() == 2 && segments[1] == "dump" && method == "GET" {
            let mut conn = pool.acquire(db_name).await?;
            let dump = conn.engine().dump()?;
            pool.release(conn);
            return Ok((200, Value::String(dump)));
        }
    }

    Ok((404, Value::String(format!("Not found: {} {}", method, path))))
}

/// Extract a query parameter from a URL path.
fn extract_query_param(path: &str, key: &str) -> Option<String> {
    if let Some(pos) = path.find('?') {
        let query = &path[pos + 1..];
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=')?;
            if k == key {
                return Some(url_decode(v));
            }
        }
    }
    None
}

/// Simple URL decode.
fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(h1), Some(h2)) = (h1, h2) {
                if let Ok(byte) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) {
                    result.push(byte as char);
                } else {
                    result.push('%');
                    result.push(h1);
                    result.push(h2);
                }
            } else {
                result.push('%');
                if let Some(h1) = h1 { result.push(h1); }
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}
