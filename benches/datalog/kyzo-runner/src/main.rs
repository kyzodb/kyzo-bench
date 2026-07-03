//! kyzo#22 — the KyzoDB subject for the recursive-Datalog rig.
//!
//! Mirrors the Souffle invocation exactly: facts arrive as TSV files on
//! disk, the answer leaves as a TSV file on disk, and the whole process is
//! timed externally by the rig. Inside, everything goes through the
//! engine's one public front door (`Db::run_script`): a fresh store, facts
//! loaded with `:create`/`:put` mutations, the query run, rows written out.
//! No private seams, no pre-warmed store — a database that persists its
//! facts competes as a database, and the internal load/query split is
//! printed to stderr for the record's notes.
//!
//! Usage:
//!   kyzo-runner --facts <dir> --relations edge:2,parent:2 \
//!               --program <file.kz> --output <file.tsv> [--store <dir>]

use kyzo::{DataValue, Db, Num, new_fjall_storage};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

/// Rows per `:put` script. Large enough to amortize parsing, small enough
/// to keep each script's literal well under the parser's nesting limits.
const LOAD_CHUNK_ROWS: usize = 5_000;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kyzo-runner error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

struct Args {
    facts: PathBuf,
    relations: Vec<(String, usize)>,
    program: PathBuf,
    output: PathBuf,
    store: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut facts = None;
    let mut relations = None;
    let mut program = None;
    let mut output = None;
    let mut store = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut next = |flag: &str| it.next().ok_or(format!("{flag} needs a value"));
        match a.as_str() {
            "--facts" => facts = Some(PathBuf::from(next("--facts")?)),
            "--relations" => {
                let spec = next("--relations")?;
                let mut rels = Vec::new();
                for part in spec.split(',') {
                    let (name, arity) = part
                        .split_once(':')
                        .ok_or(format!("bad relation spec {part:?}, want name:arity"))?;
                    let arity: usize = arity
                        .parse()
                        .map_err(|_| format!("bad arity in {part:?}"))?;
                    rels.push((name.to_owned(), arity));
                }
                relations = Some(rels);
            }
            "--program" => program = Some(PathBuf::from(next("--program")?)),
            "--output" => output = Some(PathBuf::from(next("--output")?)),
            "--store" => store = Some(PathBuf::from(next("--store")?)),
            other => return Err(format!("unknown flag {other:?}")),
        }
    }
    Ok(Args {
        facts: facts.ok_or("--facts is required")?,
        relations: relations.ok_or("--relations is required")?,
        program: program.ok_or("--program is required")?,
        output: output.ok_or("--output is required")?,
        store: store.unwrap_or_else(|| PathBuf::from(".kyzo-store")),
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;

    // A fresh universe per invocation, exactly like Souffle starts empty.
    if args.store.exists() {
        std::fs::remove_dir_all(&args.store)?;
    }
    let storage = new_fjall_storage(&args.store)?;
    let db = Db::new(storage)?;
    let no_params = BTreeMap::<String, DataValue>::new();

    let t_load = Instant::now();
    for (name, arity) in &args.relations {
        load_relation(&db, &args.facts, name, *arity)?;
    }
    let load = t_load.elapsed();

    let script = std::fs::read_to_string(&args.program)?;
    let t_query = Instant::now();
    let rows = db.run_script(&script, no_params)?;
    let query = t_query.elapsed();

    let t_write = Instant::now();
    let mut w = BufWriter::new(std::fs::File::create(&args.output)?);
    for row in &rows.rows {
        let mut line = String::new();
        for (i, v) in row.iter().enumerate() {
            if i > 0 {
                line.push('\t');
            }
            write_value(&mut line, v)?;
        }
        line.push('\n');
        w.write_all(line.as_bytes())?;
    }
    w.flush()?;
    let write = t_write.elapsed();

    eprintln!(
        "kyzo-split: load={:.3}s query={:.3}s write={:.3}s rows={}",
        load.as_secs_f64(),
        query.as_secs_f64(),
        write.as_secs_f64(),
        rows.rows.len()
    );
    Ok(())
}

fn load_relation(
    db: &Db<kyzo::FjallStorage>,
    facts_dir: &std::path::Path,
    name: &str,
    arity: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let cols: Vec<String> = (0..arity).map(|i| format!("c{i}")).collect();
    let head = cols.join(", ");
    let no_params = BTreeMap::<String, DataValue>::new();

    db.run_script(
        &format!("?[{head}] <- [] :create {name} {{{head}}}"),
        no_params.clone(),
    )?;

    let file = std::fs::File::open(facts_dir.join(format!("{name}.facts")))?;
    let mut batch: Vec<String> = Vec::with_capacity(LOAD_CHUNK_ROWS);
    let flush = |batch: &mut Vec<String>| -> Result<(), Box<dyn std::error::Error>> {
        if batch.is_empty() {
            return Ok(());
        }
        let script = format!(
            "?[{head}] <- [{rows}] :put {name} {{{head}}}",
            rows = batch.join(",")
        );
        db.run_script(&script, no_params.clone())?;
        batch.clear();
        Ok(())
    };
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != arity {
            return Err(format!(
                "{name}.facts row has {} fields, relation declared {arity}",
                fields.len()
            )
            .into());
        }
        // Facts are integers; refuse anything else rather than guess.
        for f in &fields {
            f.parse::<i64>()
                .map_err(|_| format!("non-integer fact value {f:?} in {name}.facts"))?;
        }
        batch.push(format!("[{}]", fields.join(",")));
        if batch.len() == LOAD_CHUNK_ROWS {
            flush(&mut batch)?;
        }
    }
    flush(&mut batch)?;
    Ok(())
}

fn write_value(out: &mut String, v: &DataValue) -> Result<(), std::fmt::Error> {
    match v {
        DataValue::Num(Num::Int(i)) => write!(out, "{i}"),
        DataValue::Num(Num::Float(f)) => write!(out, "{f}"),
        DataValue::Str(s) => write!(out, "{s}"),
        DataValue::Bool(b) => write!(out, "{b}"),
        other => write!(out, "{other:?}"),
    }
}
