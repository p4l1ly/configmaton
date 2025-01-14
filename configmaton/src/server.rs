use std::{net::SocketAddr, sync::{Arc, RwLock}};

use configmaton::{blob::state::build::U8BuildConfig, keyval_nfa::{Cmd, Msg, Parser}};
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::{body::{Body, Bytes}, server::conn::http1, service::service_fn, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio_rusqlite::Connection;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    let db_path = &args[1];

    let db = Connection::open(db_path).await?;
    db.call(|conn| {
        conn.execute("
            CREATE TABLE IF NOT EXISTS config
                (
                    name TEXT,
                    version INTEGER,
                    body BLOB,
                    body_hash TEXT,
                    PRIMARY KEY (name, version)
                )
        ", [])?;
        conn.execute("
            CREATE INDEX IF NOT EXISTS config_hash ON config (body_hash)
        ", [])?;
        conn.execute("
            CREATE TABLE IF NOT EXISTS automaton
                (
                    config_name TEXT,
                    config_version INTEGER,
                    automaton_version INTEGER,
                    automaton BLOB,
                    automaton_hash TEXT,
                    heatmap_snapshot BLOB,
                    live_heatmap BLOB,
                    backmap BLOB,
                    algorithm TEXT,
                    algorithm_version TEXT,
                    algorithm_params TEXT,
                    PRIMARY KEY (config_name, config_version, automaton_version)
                )
        ", [])?;
        conn.execute("
            CREATE INDEX IF NOT EXISTS automaton_hash ON automaton (automaton_hash)
        ", [])?;

        Ok(())
    }).await?;

    let app = Arc::new(RwLock::new(App { db }));

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    let listener = TcpListener::bind(addr).await?;

    // We start a loop to continuously accept incoming connections
    loop {
        let (stream, _) = listener.accept().await?;

        // Use an adapter to access something implementing `tokio::io` traits as if they implement
        // `hyper::rt` IO traits.
        let io = TokioIo::new(stream);

        // Spawn a tokio task to serve multiple connections concurrently
        let app = app.clone();
        tokio::task::spawn(async move {
            // Finally, we bind the incoming connection to our `hello` service
            let app = app.clone();
            if let Err(err) = http1::Builder::new()
                // `service_fn` converts our function in a `Service`
                .serve_connection(io, service_fn(move |req| {
                    let app = app.clone();
                    async move {
                        handle(app, req).await
                    }
                }))
                .await
            {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}

struct App {
    db: Connection,
}

pub struct BuildConfig;
impl U8BuildConfig for BuildConfig {
    fn guard_size_keep(&self) -> u32 { 10 }
    fn hashmap_cap_power_fn(&self, _len: usize) -> usize { 3 }
    fn dense_guard_count(&self) -> usize { 15 }
}

pub fn json_to_automaton_matchrun(json: &str)
    -> Result<Msg, serde_json::Error>
{
    let config: Vec<Cmd> = serde_json::from_str(json)?;
    let (parser, init) = Parser::parse(config);
    Ok(Msg::serialize(&parser, &init, &BuildConfig))
}

async fn handle(app: Arc<RwLock<App>>, req: Request<hyper::body::Incoming>)
    -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error>
{
    match (req.method(), req.uri().path()) {
        (&hyper::Method::GET, "/ping") => {
            Ok(Response::new(full("pong")))
        }
        (&hyper::Method::POST, "/config") => {
            // Get name from the request parameters, then get the body from the request
            // and store it in the database. Immediately generate an automaton from the body.
            // Heatmaps and backmaps are still unimplemented - they conatin empty data. Use the
            // matchrun algorithm, version 1.0.0, use hashxx.

            // Protect our server from massive bodies.
            let upper = req.body().size_hint().upper().unwrap_or(u64::MAX);
            if upper > 1024 * 64 {
                let mut resp = Response::new(full("Body too big"));
                *resp.status_mut() = hyper::StatusCode::PAYLOAD_TOO_LARGE;
                return Ok(resp);
            }

            // Parse query parameters
            let query = req.uri().query().unwrap_or("");
            let params: std::collections::HashMap<_, _> =
                url::form_urlencoded::parse(query.as_bytes()).collect();

            let name = match params.get("name") {
                Some(n) => n.to_string(),
                None => return Ok(Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .body(full("Missing name parameter"))
                    .unwrap())
            };

            let force = match params.get("force") {
                Some(n) => n == "true",
                None => false,
            };

            // Await the whole body to be collected into a single `Bytes`...
            let body_bytes = req.collect().await?.to_bytes();
            let body_str = String::from_utf8_lossy(&body_bytes);

            // Calculate body hash using xxhash
            let body_hash = format!("{:016x}", xxhash_rust::xxh64::xxh64(body_bytes.as_ref(), 0));

            // Generate automaton
            let automaton = match json_to_automaton_matchrun(&body_str) {
                Ok(msg) => msg,
                Err(e) => return Ok(Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .body(full(format!("Invalid configuration format {} {:?}", e, &body_bytes)))
                    .unwrap())
            };

            let automaton_bytes = unsafe {
                std::slice::from_raw_parts(automaton.data, automaton.data_len())
            };
            let automaton_hash = format!("{:016x}", xxhash_rust::xxh64::xxh64(automaton_bytes, 0));

            // Empty heatmaps and backmap
            let empty_heatmap = vec![];
            let empty_backmap = vec![];

            let version = {
                let db = app.read().unwrap().db.clone();
                db.call(move |conn| {
                    let tx = conn.transaction()?;

                    if !force {
                        // Check if the configuration already exists
                        let exists: bool = tx.query_row(
                            "SELECT EXISTS(SELECT 1 FROM config WHERE name = ? AND body_hash = ?)",
                            [&name, &body_hash],
                            |row| row.get(0)
                        )?;
                        if exists {
                            return Ok(-1);
                        }
                    }

                    // Get the next version number
                    let current_max: i64 = tx.query_row(
                        "SELECT COALESCE(MAX(version), 0) FROM config WHERE name = ?",
                        [&name],
                        |row| row.get(0)
                    )?;
                    let next_version = current_max + 1;

                    // Store config
                    tx.execute(
                        "INSERT INTO config (name, version, body, body_hash) VALUES (?, ?, ?, ?)",
                        tokio_rusqlite::params![&name, next_version, body_bytes.as_ref(), &body_hash]
                    )?;

                    // Store automaton
                    tx.execute(
                        "INSERT INTO automaton (
                            config_name, config_version, automaton_version,
                            automaton, automaton_hash,
                            heatmap_snapshot, live_heatmap, backmap,
                            algorithm, algorithm_version, algorithm_params
                        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                        (
                            &name, next_version, 1,
                            automaton_bytes, &automaton_hash,
                            empty_heatmap.as_slice(), empty_heatmap.as_slice(), empty_backmap.as_slice(),
                            "matchrun", "1.0.0", "hashxx"
                        )
                    )?;

                    tx.commit()?;

                    Ok(next_version)
                }).await
            };

            let version = match version {
                Ok(version) => {version}
                Err(e) => {
                    eprintln!("Error storing configuration: {:?}", e);
                    return Ok(Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .body(full("Error storing configuration"))
                        .unwrap());
                }
            };

            Ok(Response::builder()
                .status(hyper::StatusCode::CREATED)
                .body(full(version.to_string()))
                .unwrap())
        }
        _ => {
            let mut not_found = Response::new(empty());
            *not_found.status_mut() = StatusCode::NOT_FOUND;
            Ok(not_found)
        }
    }
}

// We create some utility functions to make Empty and Full bodies
// fit our broadened Response body type.
fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}
