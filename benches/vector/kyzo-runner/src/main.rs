//! kyzo#25 — the KyzoDB subject for the ann-benchmarks vector rig.
//!
//! Same method as the hnswlib/FAISS baselines: load the SIFT1M base
//! vectors, build an HNSW index (M=16, efConstruction=200, L2), then sweep
//! ef over the full 10k-query set, single-threaded, reporting recall@10
//! against the exact ground truth and QPS as the median of the passes.
//! Everything goes through the engine's one public front door
//! (`Db::run_script`); query vectors cross the boundary as parameters, not
//! script text, so the clock measures search, not script parsing — the
//! same courtesy the Python baselines get from their array APIs.
//!
//! Usage:
//!   kyzo-vector-runner --flat <dir> --store <dir> --runs 3 \
//!       --ef 10,20,40,80,120,200,400,800
//!
//! Output: one JSON-ish line per ef point on stdout
//! (`ef recall qps build_seconds`), timings on stderr.
//!
//! `--n <count>` caps the loaded base-vector count to the first `count`
//! rows of the same fetched flat dataset, and `--build-only` skips the
//! query sweep entirely (load + build, print `build_seconds`, exit) — the
//! shape needed for a build-time-vs-scale sweep (kyzo/kyzo#76) without
//! fetching a family of differently-sized datasets.

use kyzo::{DataValue, Db, Num, Vector, new_fjall_storage};
use kyzo_bench_harness::FlatVectors;
use ndarray::Array1;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

const K: usize = 10;
/// Rows per `:put` script during load; each row's vector is passed as a
/// parameter list, so this bounds parameters per script, not text size.
const LOAD_CHUNK_ROWS: usize = 1_000;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kyzo-vector-runner error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

struct Args {
    flat: PathBuf,
    store: PathBuf,
    runs: usize,
    ef_sweep: Vec<usize>,
    /// Caps the loaded base-vector count to the first `n` rows of the flat
    /// dataset — a scale study (build time vs. n) on one fetched dataset
    /// rather than a family of separately fetched ones. `None` loads all of
    /// `flat.n`.
    n_cap: Option<usize>,
    /// Skip the query sweep entirely: load + build, print build_seconds,
    /// exit. For the build-time-vs-scale study, where the query curve at
    /// small n isn't the question being asked.
    build_only: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut flat = None;
    let mut store = None;
    let mut runs = 3usize;
    let mut ef_sweep = vec![10, 20, 40, 80, 120, 200, 400, 800];
    let mut n_cap = None;
    let mut build_only = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut next = |flag: &str| it.next().ok_or(format!("{flag} needs a value"));
        match a.as_str() {
            "--flat" => flat = Some(PathBuf::from(next("--flat")?)),
            "--store" => store = Some(PathBuf::from(next("--store")?)),
            "--runs" => {
                runs = next("--runs")?
                    .parse()
                    .map_err(|_| "bad --runs".to_owned())?
            }
            "--ef" => {
                ef_sweep = next("--ef")?
                    .split(',')
                    .map(|s| s.parse().map_err(|_| format!("bad ef {s:?}")))
                    .collect::<Result<_, _>>()?
            }
            "--n" => {
                n_cap = Some(
                    next("--n")?
                        .parse()
                        .map_err(|_| "bad --n".to_owned())?,
                )
            }
            "--build-only" => build_only = true,
            other => return Err(format!("unknown flag {other:?}")),
        }
    }
    Ok(Args {
        flat: flat.ok_or("--flat is required")?,
        store: store.ok_or("--store is required")?,
        runs,
        ef_sweep,
        n_cap,
        build_only,
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let mut flat = FlatVectors::read(&args.flat)?;
    if let Some(n) = args.n_cap
        && n < flat.n
    {
        flat.train.truncate(n * flat.dim);
        flat.n = n;
    }
    eprintln!(
        "dataset: n={} dim={} q={} k_truth={}",
        flat.n, flat.dim, flat.q, flat.k_truth
    );

    if args.store.exists() {
        std::fs::remove_dir_all(&args.store)?;
    }
    let storage = new_fjall_storage(&args.store)?;
    let db = Db::new(storage)?;
    let no_params = BTreeMap::<String, DataValue>::new();

    // Load: id => vector, chunked puts, vectors passed as parameters.
    let t_load = Instant::now();
    db.run_script(
        &format!(
            "?[id, v] <- [] :create item {{id: Int => v: <F32; {}>}}",
            flat.dim
        ),
        no_params.clone(),
    )?;
    let mut row = 0usize;
    while row < flat.n {
        let end = (row + LOAD_CHUNK_ROWS).min(flat.n);
        let mut params = BTreeMap::new();
        let mut lines = Vec::with_capacity(end - row);
        for (slot, i) in (row..end).enumerate() {
            let vec = Array1::from(flat.train[i * flat.dim..(i + 1) * flat.dim].to_vec());
            params.insert(format!("v{slot}"), DataValue::Vec(Vector::F32(vec)));
            lines.push(format!("[{i}, $v{slot}]"));
        }
        db.run_script(
            &format!("?[id, v] <- [{}] :put item {{id => v}}", lines.join(",")),
            params,
        )?;
        row = end;
    }
    let load = t_load.elapsed();
    eprintln!("load: {:.3}s", load.as_secs_f64());

    // Build the HNSW index; this is the timed "build" the record reports.
    let t_build = Instant::now();
    db.run_script(
        &format!(
            "::hnsw create item:emb {{fields: [v], dim: {}, m: 16, ef_construction: 200, \
             distance: L2}}",
            flat.dim
        ),
        no_params.clone(),
    )?;
    let build_seconds = t_build.elapsed().as_secs_f64();
    eprintln!("build: {build_seconds:.3}s");

    if args.build_only {
        println!(
            "{{\"n\": {}, \"dim\": {}, \"build_seconds\": {build_seconds}, \"load_seconds\": {}}}",
            flat.n,
            flat.dim,
            load.as_secs_f64()
        );
        return Ok(());
    }

    // Sweep: full query set per pass, median pass for QPS, recall@10
    // against the exact ground truth.
    let script = format!(
        "?[dist, id] := ~item:emb{{id | query: $q, k: {K}, ef: $ef, bind_distance: dist}} \
         :sort dist :limit {K}"
    );
    for &ef in &args.ef_sweep {
        let ef = ef.max(K);
        let mut passes: Vec<f64> = Vec::with_capacity(args.runs);
        let mut hits = 0usize;
        for pass in 0..args.runs {
            let t0 = Instant::now();
            let mut pass_hits = 0usize;
            for qi in 0..flat.q {
                let qv = Array1::from(flat.test[qi * flat.dim..(qi + 1) * flat.dim].to_vec());
                let mut params = BTreeMap::new();
                params.insert("q".to_owned(), DataValue::Vec(Vector::F32(qv)));
                params.insert("ef".to_owned(), DataValue::Num(Num::Int(ef as i64)));
                let rows = db.run_script(&script, params)?;
                let truth = &flat.neighbors[qi * flat.k_truth..qi * flat.k_truth + K];
                for r in &rows.rows {
                    if let DataValue::Num(Num::Int(id)) = &r[1]
                        && truth.contains(id)
                    {
                        pass_hits += 1;
                    }
                }
            }
            passes.push(t0.elapsed().as_secs_f64());
            hits = pass_hits; // deterministic search: identical each pass
            eprintln!(
                "ef={ef} pass={pass} wall={:.3}s",
                passes.last().expect("just pushed")
            );
        }
        passes.sort_by(|a, b| a.partial_cmp(b).expect("no NaN walls"));
        let median = passes[passes.len() / 2];
        let recall = hits as f64 / (K * flat.q) as f64;
        let qps = flat.q as f64 / median;
        println!("{{\"ef_search\": {ef}, \"recall_at_10\": {recall}, \"qps\": {qps}, \"build_seconds\": {build_seconds}, \"load_seconds\": {}}}", load.as_secs_f64());
    }
    Ok(())
}
