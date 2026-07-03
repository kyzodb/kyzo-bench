//! kyzo#26 — the KyzoDB subject for the embedded-OLTP rig.
//!
//! Replays one phase of the deterministic op stream against a persistent
//! fjall store, through the engine's one public front door
//! (`Db::run_script`), exactly as the SQLite side replays the same stream
//! through the `sqlite3` CLI against a persistent database file:
//!
//! - `--phase load`  — create the `item` relation, bulk-load in batches of
//!   1000 rows per script (the SQLite side uses 1000-row transactions).
//! - `--phase mixed` — one script per op, reads append `idx\tgrp\tval`
//!   lines to `--output` in op order.
//! - `--phase dump`  — every surviving row as `id\tgrp\tval`, ordered by
//!   id, to `--output`.
//!
//! The rig times each phase invocation externally; internal timings go to
//! stderr for the record's notes.
//!
//! Usage:
//!   kyzo-oltp-runner --stream <file> --phase <load|mixed|dump> \
//!                    --store <dir> [--output <file>]

use kyzo::{DataValue, Db, Num, new_fjall_storage};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

const LOAD_BATCH: usize = 1_000;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kyzo-oltp-runner error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

struct Args {
    stream: PathBuf,
    phase: String,
    store: PathBuf,
    output: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut stream = None;
    let mut phase = None;
    let mut store = None;
    let mut output = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut next = |flag: &str| it.next().ok_or(format!("{flag} needs a value"));
        match a.as_str() {
            "--stream" => stream = Some(PathBuf::from(next("--stream")?)),
            "--phase" => phase = Some(next("--phase")?),
            "--store" => store = Some(PathBuf::from(next("--store")?)),
            "--output" => output = Some(PathBuf::from(next("--output")?)),
            other => return Err(format!("unknown flag {other:?}")),
        }
    }
    Ok(Args {
        stream: stream.ok_or("--stream is required")?,
        phase: phase.ok_or("--phase is required")?,
        store: store.ok_or("--store is required")?,
        output,
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let storage = new_fjall_storage(&args.store)?;
    let db = Db::new(storage)?;
    let no_params = BTreeMap::<String, DataValue>::new();
    let t = Instant::now();

    match args.phase.as_str() {
        "load" => phase_load(&db, &args)?,
        "mixed" => phase_mixed(&db, &args)?,
        "dump" => phase_dump(&db, &args)?,
        other => return Err(format!("unknown phase {other:?}").into()),
    }

    let _ = no_params;
    eprintln!(
        "kyzo-oltp: phase={} took {:.3}s",
        args.phase,
        t.elapsed().as_secs_f64()
    );
    Ok(())
}

type Engine = Db<kyzo::FjallStorage>;

fn script(db: &Engine, s: &str) -> Result<kyzo::NamedRows, Box<dyn std::error::Error>> {
    Ok(db.run_script(s, BTreeMap::new())?)
}

fn phase_load(db: &Engine, args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    script(db, "?[id, grp, val] <- [] :create item {id => grp, val}")?;
    let mut batch: Vec<String> = Vec::with_capacity(LOAD_BATCH);
    let flush = |batch: &mut Vec<String>| -> Result<(), Box<dyn std::error::Error>> {
        if batch.is_empty() {
            return Ok(());
        }
        script(
            db,
            &format!(
                "?[id, grp, val] <- [{}] :put item {{id => grp, val}}",
                batch.join(",")
            ),
        )?;
        batch.clear();
        Ok(())
    };
    for line in BufReader::new(std::fs::File::open(&args.stream)?).lines() {
        let line = line?;
        let Some(rest) = line.strip_prefix("L ") else {
            continue;
        };
        let [id, grp, val] = fields3(rest)?;
        batch.push(format!("[{id},{grp},{val}]"));
        if batch.len() == LOAD_BATCH {
            flush(&mut batch)?;
        }
    }
    flush(&mut batch)?;
    Ok(())
}

fn phase_mixed(db: &Engine, args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let out_path = args.output.as_ref().ok_or("--output required for mixed")?;
    let mut out = BufWriter::new(std::fs::File::create(out_path)?);
    let mut idx: u64 = 0;
    for line in BufReader::new(std::fs::File::open(&args.stream)?).lines() {
        let line = line?;
        let (tag, rest) = line
            .split_once(' ')
            .ok_or(format!("bad op line {line:?}"))?;
        match tag {
            "L" => continue, // load phase rows, replayed by --phase load
            "R" => {
                let id: u64 = rest.trim().parse()?;
                let rows = script(
                    db,
                    &format!("?[grp, val] := id = {id}, *item{{id, grp, val}}"),
                )?;
                for row in &rows.rows {
                    writeln!(out, "{idx}\t{}\t{}", int(&row[0])?, int(&row[1])?)?;
                }
            }
            "U" => {
                let id: u64 = rest.trim().parse()?;
                script(
                    db,
                    &format!(
                        "?[id, grp, val] := id = {id}, *item{{id, grp, val: v}}, val = v + 1 \
                         :put item {{id => grp, val}}"
                    ),
                )?;
            }
            "I" => {
                let [id, grp, val] = fields3(rest)?;
                script(
                    db,
                    &format!(
                        "?[id, grp, val] <- [[{id},{grp},{val}]] :put item {{id => grp, val}}"
                    ),
                )?;
            }
            "D" => {
                let id: u64 = rest.trim().parse()?;
                script(db, &format!("?[id] <- [[{id}]] :rm item {{id}}"))?;
            }
            other => return Err(format!("unknown op tag {other:?}").into()),
        }
        if tag != "L" {
            idx += 1;
        }
    }
    out.flush()?;
    Ok(())
}

fn phase_dump(db: &Engine, args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let out_path = args.output.as_ref().ok_or("--output required for dump")?;
    let rows = script(db, "?[id, grp, val] := *item{id, grp, val}")?;
    // Ordered by key: the SQLite side pays for its ORDER BY inside the
    // measured window, so the sort happens inside ours too.
    let mut dumped: Vec<(i64, i64, i64)> = Vec::with_capacity(rows.rows.len());
    for row in &rows.rows {
        dumped.push((int(&row[0])?, int(&row[1])?, int(&row[2])?));
    }
    dumped.sort_unstable();
    let mut out = BufWriter::new(std::fs::File::create(out_path)?);
    for (id, grp, val) in dumped {
        writeln!(out, "{id}\t{grp}\t{val}")?;
    }
    out.flush()?;
    Ok(())
}

fn fields3(rest: &str) -> Result<[i64; 3], Box<dyn std::error::Error>> {
    let mut it = rest.split_whitespace();
    let mut next = || -> Result<i64, Box<dyn std::error::Error>> {
        Ok(it.next().ok_or("short op line")?.parse::<i64>()?)
    };
    Ok([next()?, next()?, next()?])
}

fn int(v: &DataValue) -> Result<i64, Box<dyn std::error::Error>> {
    match v {
        DataValue::Num(Num::Int(i)) => Ok(*i),
        other => Err(format!("expected integer, got {other:?}").into()),
    }
}
