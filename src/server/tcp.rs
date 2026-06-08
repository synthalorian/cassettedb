//! TCP server with custom binary protocol for CassetteDB.
//!
//! Protocol format:
//!   Request:  [4 bytes: payload length (big-endian)] [payload: JSON-encoded command]
//!   Response: [4 bytes: payload length (big-endian)] [payload: JSON-encoded response]
//!
//! Commands:
//!   {"cmd":"insert","db":"name","doc":{...}}
//!   {"cmd":"get","db":"name","id":"doc-id"}
//!   {"cmd":"delete","db":"name","id":"doc-id"}
//!   {"cmd":"update","db":"name","id":"doc-id","doc":{...}}
//!   {"cmd":"query","db":"name","q":"expression"}
//!   {"cmd":"search","db":"name","term":"text"}
//!   {"cmd":"dump","db":"name"}
//!   {"cmd":"compact","db":"name"}
//!   {"cmd":"count","db":"name"}
//!   {"cmd":"auth","token":"secret"}

use crate::document::Document;
use crate::error::Result;
use crate::query::Query;
use crate::server::auth::{AuthManager, Authenticator};
use crate::server::pool::ConnectionPool;
use crate::server::ServerResponse;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// TCP protocol command.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "cmd")]
pub enum TcpCommand {
    #[serde(rename = "auth")]
    Auth { token: String },
    #[serde(rename = "insert")]
    Insert { db: String, doc: Value },
    #[serde(rename = "get")]
    Get { db: String, id: String },
    #[serde(rename = "delete")]
    Delete { db: String, id: String },
    #[serde(rename = "update")]
    Update { db: String, id: String, doc: Value },
    #[serde(rename = "query")]
    Query { db: String, q: String },
    #[serde(rename = "search")]
    Search { db: String, term: String },
    #[serde(rename = "dump")]
    Dump { db: String },
    #[serde(rename = "compact")]
    Compact { db: String },
    #[serde(rename = "count")]
    Count { db: String },
    #[serde(rename = "list_dbs")]
    ListDbs,
    #[serde(rename = "create_db")]
    CreateDb { db: String },
}

/// TCP server state.
pub struct TcpServer {
    pool: Arc<ConnectionPool>,
    auth: Arc<AuthManager>,
}

impl TcpServer {
    /// Create a new TCP server.
    pub fn new(pool: Arc<ConnectionPool>, auth: Arc<AuthManager>) -> Self {
        Self { pool, auth }
    }

    /// Run the TCP server, binding to the given address.
    pub async fn run(&self, bind_addr: &str) -> Result<()> {
        let listener = TcpListener::bind(bind_addr).await?;
        let local_addr = listener.local_addr()?;
        println!("TCP server listening on {}", local_addr);

        loop {
            let (stream, addr) = listener.accept().await?;
            let pool = self.pool.clone();
            let auth = self.auth.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_client(stream, pool, auth).await {
                    eprintln!("Client {} error: {}", addr, e);
                }
            });
        }
    }
}

/// Helper function to run TCP server.
pub async fn run_tcp_server(pool: Arc<ConnectionPool>, auth: Arc<AuthManager>, bind_addr: &str) -> Result<()> {
    let server = TcpServer::new(pool, auth);
    server.run(bind_addr).await
}

/// Handle a single TCP client connection.
async fn handle_client(
    mut stream: TcpStream,
    pool: Arc<ConnectionPool>,
    auth: Arc<AuthManager>,
) -> Result<()> {
    let mut authenticated = !auth.is_enabled();

    loop {
        // Read 4-byte length prefix.
        let mut len_buf = [0u8; 4];
        match stream.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 10_000_000 {
            // 10MB max request size.
            return Err(crate::error::CassetteError::Io(
                std::io::Error::new(std::io::ErrorKind::InvalidData, "request too large")
            ));
        }

        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).await?;

        let cmd: TcpCommand = match serde_json::from_slice(&payload) {
            Ok(c) => c,
            Err(e) => {
                let resp = ServerResponse::<Value>::err(format!("Invalid command: {}", e));
                send_response(&mut stream, &resp).await?;
                continue;
            }
        };

        // Auth command is always allowed.
        if let TcpCommand::Auth { token } = &cmd {
            if auth.validate(token) {
                authenticated = true;
                let resp = ServerResponse::ok(Value::String("authenticated".to_string()));
                send_response(&mut stream, &resp).await?;
            } else {
                authenticated = false;
                let resp = ServerResponse::<Value>::err("Invalid token");
                send_response(&mut stream, &resp).await?;
            }
            continue;
        }

        if !authenticated {
            let resp = ServerResponse::<Value>::err("Not authenticated");
            send_response(&mut stream, &resp).await?;
            continue;
        }

        let result = execute_command(cmd, &pool).await;
        match result {
            Ok(value) => {
                let resp = ServerResponse::ok(value);
                send_response(&mut stream, &resp).await?;
            }
            Err(e) => {
                let resp = ServerResponse::<Value>::err(e.to_string());
                send_response(&mut stream, &resp).await?;
            }
        }
    }

    Ok(())
}

/// Execute a TCP command and return the result.
async fn execute_command(cmd: TcpCommand, pool: &ConnectionPool) -> Result<Value> {
    match cmd {
        TcpCommand::Auth { .. } => {
            // Handled separately.
            Ok(Value::Null)
        }
        TcpCommand::Insert { db, doc } => {
            let mut conn = pool.acquire(&db).await?;
            let id = conn.engine().insert(Document::new(doc))?;
            pool.release(conn);
            Ok(Value::String(id))
        }
        TcpCommand::Get { db, id } => {
            let mut conn = pool.acquire(&db).await?;
            let result = conn.engine().get(&id).cloned();
            pool.release(conn);
            match result {
                Some(doc) => Ok(serde_json::to_value(doc)?),
                None => Ok(Value::Null),
            }
        }
        TcpCommand::Delete { db, id } => {
            let mut conn = pool.acquire(&db).await?;
            conn.engine().delete(&id)?;
            pool.release(conn);
            Ok(Value::Bool(true))
        }
        TcpCommand::Update { db, id, doc } => {
            let mut conn = pool.acquire(&db).await?;
            conn.engine().update(&id, doc)?;
            pool.release(conn);
            Ok(Value::Bool(true))
        }
        TcpCommand::Query { db, q } => {
            let mut conn = pool.acquire(&db).await?;
            let query = Query::parse(&q)?;
            let result = conn.engine().query(&query);
            pool.release(conn);
            Ok(serde_json::to_value(result)?)
        }
        TcpCommand::Search { db, term } => {
            let mut conn = pool.acquire(&db).await?;
            let docs = conn.engine().search(&term);
            pool.release(conn);
            Ok(serde_json::to_value(docs)?)
        }
        TcpCommand::Dump { db } => {
            let mut conn = pool.acquire(&db).await?;
            let dump = conn.engine().dump()?;
            pool.release(conn);
            Ok(Value::String(dump))
        }
        TcpCommand::Compact { db } => {
            let mut conn = pool.acquire(&db).await?;
            conn.engine().compact()?;
            pool.release(conn);
            Ok(Value::Bool(true))
        }
        TcpCommand::Count { db } => {
            let mut conn = pool.acquire(&db).await?;
            let count = conn.engine().doc_count();
            pool.release(conn);
            Ok(Value::Number(count.into()))
        }
        TcpCommand::ListDbs => {
            let dbs = pool.list_databases();
            Ok(serde_json::to_value(dbs)?)
        }
        TcpCommand::CreateDb { db } => {
            pool.create_database(&db)?;
            Ok(Value::Bool(true))
        }
    }
}

/// Send a JSON response with a 4-byte length prefix.
async fn send_response<T: Serialize>(stream: &mut TcpStream, resp: &ServerResponse<T>) -> Result<()> {
    let payload = serde_json::to_vec(resp)?;
    let len = payload.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(())
}
