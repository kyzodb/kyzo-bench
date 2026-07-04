//! kyzo#71 gate 4/5 — the SurrealDB subject for the kyzo#25 vector rig.
//!
//! Same method as `kyzo-vector-runner` (the "database" class in this bench,
//! distinct from hnswlib/FAISS's "raw embedded library" class): one query,
//! one round trip through the query-language door, per test vector — not
//! hnswlib/FAISS's single batched native call over all 10k queries. That
//! difference is real and stated, not hidden: a database's normal
//! operational shape is "one call per request," and both database subjects
//! in this bench (KyzoDB, SurrealDB) are measured that way, while both
//! embedded-library subjects are measured through their own designed
//! batch API. Load raw vectors first (untimed, chunked `INSERT`), then
//! `DEFINE INDEX` over the fully-loaded table as the timed "build" phase —
//! without `CONCURRENTLY`, `DEFINE INDEX` blocks until the graph is fully
//! built (confirmed against SurrealDB's own docs), so its wall time alone
//! is the build measurement, the same shape as hnswlib's/FAISS's single
//! blocking `add_items`/`add` call.
//!
//! Usage:
//!   surrealdb-runner vector --index hnsw|diskann --flat <dir> --store <dir> \
//!       [--runs 3] [--sweep 10,20,40,80,120,200,400,800] [--land]
//!       [--results-dir <dir>]

use kyzo_bench_harness::envelope;
use kyzo_bench_harness::subject::{Opponent, Provenance, Subject};
use kyzo_bench_harness::FlatVectors;
use serde_json::json;
use surrealdb::engine::local::RocksDb;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;
use std::path::PathBuf;
use std::time::Instant;

const K: usize = 10;
const LOAD_CHUNK_ROWS: usize = 1_000;
/// SurrealDB's own docs: HNSW's graph is meant to live comfortably in
/// memory ("keep in mind the in-memory nature of HNSW"); the default
/// 256 MiB `SURREAL_HNSW_CACHE_SIZE` is sized for much smaller graphs than
/// SIFT1M's ~512 MiB of raw f32 vectors alone. Configuring HNSW's own
/// intended-fit cache below its own working set would not be "the way its
/// own documentation recommends" (gate 3) — it would be a manufactured
/// cache-thrash penalty. 2 GiB comfortably holds SIFT1M's vectors plus
/// M0=32 graph edges with headroom; verified generous, not tuned to a
/// number that happens to flatter the result.
const HNSW_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// DiskANN's whole design point is a *bounded* cache with the KV store as
/// source of truth (SurrealDB's own architecture, confirmed in
/// surrealdb/surrealdb#7337) — so its documented default (256 MiB) is what
/// "configured the way its own docs recommend" means here, not a number we
/// pick. Set explicitly anyway (not left to an undocumented default) per
/// the same issue's finding that this env var was undocumented and several
/// users were unknowingly running without it.
const DISKANN_CACHE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexKind {
    Hnsw,
    DiskAnn,
}

impl IndexKind {
    fn as_str(self) -> &'static str {
        match self {
            IndexKind::Hnsw => "hnsw",
            IndexKind::DiskAnn => "diskann",
        }
    }
}

#[derive(SurrealValue)]
struct PointRow {
    idx: i64,
    v: Vec<f32>,
}

#[derive(Debug, SurrealValue)]
struct IdxRow {
    idx: i64,
}

#[derive(Debug, SurrealValue)]
struct BuildingStatus {
    status: String,
}

#[derive(Debug, SurrealValue)]
struct IndexInfo {
    building: BuildingStatus,
}

struct Args {
    index: IndexKind,
    flat: PathBuf,
    store: PathBuf,
    runs: usize,
    sweep: Vec<usize>,
    land: bool,
    results_dir: PathBuf,
}

fn parse_args(args: &mut impl Iterator<Item = String>) -> Result<Args, String> {
    let mut index = None;
    let mut flat = None;
    let mut store = None;
    let mut runs = 3usize;
    let mut sweep = vec![10, 20, 40, 80, 120, 200, 400, 800];
    let mut land = false;
    let mut results_dir = PathBuf::from("results");
    while let Some(a) = args.next() {
        let mut next = |flag: &str| args.next().ok_or(format!("{flag} needs a value"));
        match a.as_str() {
            "--index" => {
                index = Some(match next("--index")?.as_str() {
                    "hnsw" => IndexKind::Hnsw,
                    "diskann" => IndexKind::DiskAnn,
                    other => return Err(format!("unknown --index {other:?}; want hnsw|diskann")),
                })
            }
            "--flat" => flat = Some(PathBuf::from(next("--flat")?)),
            "--store" => store = Some(PathBuf::from(next("--store")?)),
            "--runs" => runs = next("--runs")?.parse().map_err(|_| "bad --runs")?,
            "--sweep" => {
                sweep = next("--sweep")?
                    .split(',')
                    .map(|s| s.parse().map_err(|_| format!("bad sweep point {s:?}")))
                    .collect::<Result<_, _>>()?
            }
            "--land" => land = true,
            "--results-dir" => results_dir = PathBuf::from(next("--results-dir")?),
            other => return Err(format!("unknown flag {other:?}")),
        }
    }
    Ok(Args {
        index: index.ok_or("--index hnsw|diskann is required")?,
        flat: flat.ok_or("--flat is required")?,
        store: store.ok_or("--store is required")?,
        runs,
        sweep,
        land,
        results_dir,
    })
}

pub async fn run(
    args: &mut impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let parsed = parse_args(args).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let flat = FlatVectors::read(&parsed.flat)?;
    eprintln!(
        "dataset: n={} dim={} q={} k_truth={}",
        flat.n, flat.dim, flat.q, flat.k_truth
    );

    // Sound despite the tokio worker threads already existing at this point:
    // nothing reads the environment concurrently with this write — no query
    // has been issued yet, and SurrealDB itself only reads these vars later,
    // sequenced after this call completes via `.await` on this same task.
    unsafe {
        match parsed.index {
            IndexKind::Hnsw => {
                std::env::set_var("SURREAL_HNSW_CACHE_SIZE", HNSW_CACHE_BYTES.to_string())
            }
            IndexKind::DiskAnn => {
                std::env::set_var("SURREAL_DISKANN_CACHE_SIZE", DISKANN_CACHE_BYTES.to_string())
            }
        }
    }

    if parsed.store.exists() {
        std::fs::remove_dir_all(&parsed.store)?;
    }
    let store_path = parsed.store.to_str().ok_or("non-utf8 --store path")?;
    let db = Surreal::new::<RocksDb>(store_path).await?;
    db.use_ns("kyzo_bench").use_db("vector").await?;

    let t_load = Instant::now();
    let mut row = 0usize;
    while row < flat.n {
        let end = (row + LOAD_CHUNK_ROWS).min(flat.n);
        let chunk: Vec<PointRow> = (row..end)
            .map(|i| PointRow {
                idx: i as i64,
                v: flat.train[i * flat.dim..(i + 1) * flat.dim].to_vec(),
            })
            .collect();
        let _: Vec<IdxRow> = db.insert("pts").content(chunk).await?;
        row = end;
    }
    let load_seconds = t_load.elapsed().as_secs_f64();
    eprintln!("load: {load_seconds:.3}s");

    // CONCURRENTLY, polled via INFO FOR INDEX, not the plain blocking form:
    // a single-transaction blocking DEFINE INDEX over 1M rows hits RocksDB's
    // optimistic-transaction conflict check once the transaction has lived
    // long enough that its start snapshot's memtable history has been
    // flushed out from under it ("Transaction could not check for conflicts
    // ... MemTable only contains changes newer than SequenceNumber ...") —
    // reproduced on this exact workload before this code path existed.
    // SurrealDB's own docs name `CONCURRENTLY` as the mechanism for exactly
    // this case ("building indexes can be lengthy and may time out before
    // they're completed"), so this is the documented way to build at this
    // scale, not a workaround chosen to flatter the number.
    let define_sql = match parsed.index {
        IndexKind::Hnsw => format!(
            "DEFINE INDEX v_idx ON pts FIELDS v HNSW DIMENSION {} DIST EUCLIDEAN TYPE F32 EFC 200 M 16 CONCURRENTLY",
            flat.dim
        ),
        IndexKind::DiskAnn => format!(
            "DEFINE INDEX v_idx ON pts FIELDS v DISKANN DIMENSION {} DIST EUCLIDEAN TYPE F32 CONCURRENTLY",
            flat.dim
        ),
    };
    let t_build = Instant::now();
    db.query(&define_sql).await?.check()?;
    loop {
        let mut info = db.query("INFO FOR INDEX v_idx ON pts").await?.check()?;
        let status: Option<IndexInfo> = info.take(0)?;
        let status = status.ok_or("INFO FOR INDEX returned nothing")?;
        eprintln!("build ({}) status: {}", parsed.index.as_str(), status.building.status);
        if status.building.status == "ready" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
    let build_seconds = t_build.elapsed().as_secs_f64();
    eprintln!("build ({}): {build_seconds:.3}s", parsed.index.as_str());

    // Untimed warm-up: a full, discarded pass over every test query before
    // the timed sweep starts. Measured directly, twice: a 50-query warm-up
    // (this comment used to describe that) left the first *timed* pass
    // 289x slower than the next two at the identical ef (3435s vs ~11s;
    // median-of-3 silently absorbed it into a clean-looking number without
    // fixing what produced it), and reopening an idle, hours-settled copy
    // of the same store dropped per-query latency another order of
    // magnitude below even those "fast" passes (~150µs vs ~1.2ms). Both
    // point to the same cause: SurrealDB's HNSW graph is lazily paged in
    // from RocksDB rather than made fully resident when `CONCURRENTLY`
    // reports `ready` (its own docs describe HNSW as in-memory once
    // built, not eagerly loaded at that moment), so the first real access
    // pattern pays for pulling the whole graph off disk once. hnswlib and
    // FAISS get the equivalent for free because their build call already
    // leaves the graph hot in the same process's memory; a full untimed
    // pass is that same warm path made explicit here instead of left to
    // silently contaminate whichever timed pass happens to run first.
    {
        let warm_query = format!("SELECT idx FROM pts WHERE v <|{K}, {K}|> $q");
        for qi in 0..flat.q {
            let qv: Vec<f32> = flat.test[qi * flat.dim..(qi + 1) * flat.dim].to_vec();
            let mut r = db.query(&warm_query).bind(("q", qv)).await?.check()?;
            let _: Vec<IdxRow> = r.take(0)?;
        }
    }

    let mut curve = Vec::with_capacity(parsed.sweep.len());
    for &point in &parsed.sweep {
        let effort = point.max(K);
        let query = format!("SELECT idx FROM pts WHERE v <|{K}, {effort}|> $q");
        let mut passes: Vec<f64> = Vec::with_capacity(parsed.runs);
        let mut hits = 0usize;
        for pass in 0..parsed.runs {
            let t0 = Instant::now();
            let mut pass_hits = 0usize;
            for qi in 0..flat.q {
                let qv: Vec<f32> = flat.test[qi * flat.dim..(qi + 1) * flat.dim].to_vec();
                let mut result = db.query(&query).bind(("q", qv)).await?.check()?;
                let rows: Vec<IdxRow> = result.take(0)?;
                let truth = &flat.neighbors[qi * flat.k_truth..qi * flat.k_truth + K];
                for r in &rows {
                    if truth.contains(&r.idx) {
                        pass_hits += 1;
                    }
                }
            }
            passes.push(t0.elapsed().as_secs_f64());
            hits = pass_hits; // deterministic search: identical each pass
            eprintln!(
                "{}={effort} pass={pass} wall={:.3}s",
                if parsed.index == IndexKind::Hnsw { "ef_search" } else { "effort" },
                passes.last().expect("just pushed")
            );
        }
        passes.sort_by(|a, b| a.partial_cmp(b).expect("no NaN walls"));
        let median = passes[passes.len() / 2];
        let point_key = if parsed.index == IndexKind::Hnsw { "ef_search" } else { "effort" };
        curve.push(json!({
            (point_key): point,
            "recall_at_10": hits as f64 / (K * flat.q) as f64,
            "qps": flat.q as f64 / median,
        }));
    }

    let peak_rss_kib = peak_rss_kib()?;
    let cache_bytes = match parsed.index {
        IndexKind::Hnsw => HNSW_CACHE_BYTES,
        IndexKind::DiskAnn => DISKANN_CACHE_BYTES,
    };
    let dataset_sha = std::fs::read_to_string(
        parsed
            .flat
            .parent()
            .unwrap_or(&parsed.flat)
            .join("sift-128-euclidean.hdf5.sha256"),
    )
    .ok()
    .and_then(|s| s.split_whitespace().next().map(str::to_owned));

    let metrics = json!({
        "dataset": {
            "name": "ann-benchmarks/sift-128-euclidean",
            "sha256": dataset_sha,
            "base_vectors": flat.n,
            "dim": flat.dim,
            "queries": flat.q,
            "metric": "euclidean",
        },
        "scope_note": format!(
            "ann-benchmarks method, database class (see kyzo-vector-runner, not \
             hnswlib/FAISS's batched-library class): recall@10 vs QPS with one \
             query, one SurrealQL round trip, per test vector — no batched \
             multi-query API used because SurrealDB, like KyzoDB, does not \
             expose one for its normal query door. {} tuned per SurrealDB's own \
             docs (surrealdb.com/docs/reference/query-language/statements/define/indexes); \
             `{}` set to {} bytes ({}), not left at whatever undocumented default \
             the process would otherwise pick up. Build ({} index) times \
             `DEFINE INDEX ... CONCURRENTLY` from issuance to `INFO FOR INDEX` \
             first reporting `building.status = 'ready'`, polled every 1s — \
             not the plain blocking form, which this workload's scale drove \
             into a real RocksDB optimistic-transaction-conflict failure \
             (`Transaction could not check for conflicts ... MemTable only \
             contains changes newer than SequenceNumber ...`) after running \
             for wall-clock minutes as one transaction; `CONCURRENTLY` is \
             SurrealDB's own documented mechanism for exactly this case \
             (\"building indexes can be lengthy\").             A full, untimed, throwaway pass over every test query runs \
             after build and before the first timed sweep point: \
             SurrealDB's HNSW graph is lazily paged in from RocksDB rather \
             than made fully resident when `CONCURRENTLY` reports `ready`, \
             so the first real access pattern pays a one-off cost to pull \
             the whole graph off disk. Measured directly: with only a \
             50-query warm-up, the first timed pass ran 289x slower than \
             the next two passes at the identical ef (3435s vs a median of \
             ~11s) — a real cost that a 3-pass median absorbs into a \
             clean-looking number without fixing what produced it, which \
             is not the same thing as it not happening. A full pass is the \
             smallest warm-up directly confirmed sufficient. Each sweep \
             point is the median of {} full {}-query \
             passes, one query in flight at a time (SurrealDB's tokio \
             runtime is necessarily multi-threaded per its own embedding \
             docs; nothing here issues concurrent queries).",
            parsed.index.as_str(),
            match parsed.index { IndexKind::Hnsw => "SURREAL_HNSW_CACHE_SIZE", IndexKind::DiskAnn => "SURREAL_DISKANN_CACHE_SIZE" },
            cache_bytes,
            match parsed.index {
                IndexKind::Hnsw => "sized to comfortably hold SIFT1M's working set, not left at the 256 MiB default sized for far smaller graphs",
                IndexKind::DiskAnn => "SurrealDB's own documented default, its bounded-cache-over-KV design is the point being tested",
            },
            parsed.index.as_str(),
            parsed.runs,
            flat.q,
        ),
        "config": match parsed.index {
            IndexKind::Hnsw => json!({"index": "hnsw", "dist": "euclidean", "type": "F32", "efc": 200, "m": 16}),
            IndexKind::DiskAnn => json!({"index": "diskann", "dist": "euclidean", "type": "F32", "degree": 64, "l_build": 100, "alpha": 1.2}),
        },
        "cache_bytes": cache_bytes,
        "load_seconds": load_seconds,
        "build_seconds": build_seconds,
        "query_threads": 1,
        "curve": curve,
        "peak_rss_kib": peak_rss_kib,
    });

    let env = envelope::build(
        "vector",
        "kyzo#25",
        &Subject::Opponent(Opponent {
            name: format!("surrealdb-{}", parsed.index.as_str()),
            version: "3.2.0".into(),
            provenance: Provenance::Package {
                ecosystem: "cargo".into(),
                package: "surrealdb".into(),
                version: "3.2.0".into(),
            },
        }),
        metrics,
    );
    envelope::emit(env, parsed.land, &parsed.results_dir, "sift1m", None)?;
    Ok(())
}

fn peak_rss_kib() -> std::io::Result<u64> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    Ok(status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|l| l.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0))
}
