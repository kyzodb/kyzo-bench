//! kyzo#71 — the SurrealDB opponent: embedded via `kv-rocksdb`, driven by
//! argv the same way every other subprocess-timed opponent in this repo is.
//!
//! This binary is the *one* place SurrealDB is wired into kyzo-bench — every
//! bench that onboards it (vector first, then fts, oltp, ...) adds a mode
//! here rather than re-pinning or re-embedding the crate per bench. Today
//! it only proves the pin: that `surrealdb = "=3.2.0"` with `kv-rocksdb`
//! embeds, opens a durable store at a given path, and round-trips a record
//! through SurrealQL. The per-bench modes (vector HNSW/DiskANN sweep, fts
//! BM25 match set, oltp mixed op stream) land as this story's later commits.
//!
//! Usage: surrealdb-runner smoke-test --store <dir>

use surrealdb::engine::local::RocksDb;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;

mod graph_recursion_probe;
mod vector;

#[derive(Debug, SurrealValue)]
struct Ping {
    ok: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or("usage: surrealdb-runner <mode> [args...]")?;
    match mode.as_str() {
        "smoke-test" => smoke_test(&mut args).await,
        "vector" => vector::run(&mut args).await,
        other => Err(format!("unknown mode {other:?}; want smoke-test|vector").into()),
    }
}

async fn smoke_test(
    args: &mut impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut store: Option<String> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--store" => store = Some(args.next().ok_or("--store needs a value")?),
            other => return Err(format!("unknown flag {other:?}").into()),
        }
    }
    let store = store.ok_or("--store is required")?;
    if std::path::Path::new(&store).exists() {
        std::fs::remove_dir_all(&store)?;
    }

    let db = Surreal::new::<RocksDb>(store.as_str()).await?;
    db.use_ns("kyzo_bench").use_db("smoke").await?;
    db.query("DEFINE TABLE ping SCHEMALESS").await?.check()?;
    db.query("CREATE ping:one SET ok = true").await?.check()?;
    let mut result = db.query("SELECT ok FROM ping:one").await?.check()?;
    let rows: Vec<Ping> = result.take(0)?;
    let ok = rows.first().map(|p| p.ok).unwrap_or(false);
    if !ok {
        return Err("round-trip through RocksDB-backed SurrealDB did not return ok=true".into());
    }
    println!("{{\"pin_verified\": true, \"engine\": \"kv-rocksdb\", \"version\": \"3.2.0\"}}");
    Ok(())
}
