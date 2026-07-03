//! kyzo#27 — the Tantivy opponent runner.
//!
//! Two externally timed phases against a persistent index directory:
//!
//! - `--phase index` — build the index from `docs.tsv` (`id<TAB>text`),
//!   default tokenizer, positions indexed (FTS5's default detail=full
//!   also stores positions), multithreaded writer as shipped.
//! - `--phase query` — run the query set `--passes` times single-threaded;
//!   the last pass writes match sets to `--matches` and Tantivy's own
//!   BM25 top-10 to `--ranked`.
//!
//! Queries are built programmatically (TermQuery/BooleanQuery/
//! PhraseQuery), not through the query parser, so the semantics under
//! test are exact and match the rig's definition of each query class.

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;
use tantivy::collector::{DocSetCollector, TopDocs};
use tantivy::query::{BooleanQuery, Occur, PhraseQuery, Query, TermQuery};
use tantivy::schema::{
    FAST, INDEXED, IndexRecordOption, STORED, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::{Index, Term, doc};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tantivy-runner error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

struct Args {
    docs: Option<PathBuf>,
    index: PathBuf,
    phase: String,
    queries: Option<PathBuf>,
    matches: Option<PathBuf>,
    ranked: Option<PathBuf>,
    passes: u32,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        docs: None,
        index: PathBuf::new(),
        phase: String::new(),
        queries: None,
        matches: None,
        ranked: None,
        passes: 1,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut next = |f: &str| it.next().ok_or(format!("{f} needs a value"));
        match flag.as_str() {
            "--docs" => a.docs = Some(PathBuf::from(next("--docs")?)),
            "--index" => a.index = PathBuf::from(next("--index")?),
            "--phase" => a.phase = next("--phase")?,
            "--queries" => a.queries = Some(PathBuf::from(next("--queries")?)),
            "--matches" => a.matches = Some(PathBuf::from(next("--matches")?)),
            "--ranked" => a.ranked = Some(PathBuf::from(next("--ranked")?)),
            "--passes" => {
                a.passes = next("--passes")?
                    .parse()
                    .map_err(|_| "bad --passes".to_owned())?;
            }
            other => return Err(format!("unknown flag {other:?}")),
        }
    }
    if a.index.as_os_str().is_empty() || a.phase.is_empty() {
        return Err("--index and --phase are required".into());
    }
    Ok(a)
}

fn schema() -> (Schema, tantivy::schema::Field, tantivy::schema::Field) {
    let mut b = Schema::builder();
    let id = b.add_u64_field("id", INDEXED | STORED | FAST);
    let body = b.add_text_field(
        "body",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        ),
    );
    (b.build(), id, body)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let t = Instant::now();
    match args.phase.as_str() {
        "index" => phase_index(&args)?,
        "query" => phase_query(&args)?,
        other => return Err(format!("unknown phase {other:?}").into()),
    }
    eprintln!(
        "tantivy: phase={} took {:.3}s",
        args.phase,
        t.elapsed().as_secs_f64()
    );
    Ok(())
}

fn phase_index(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let docs_path = args.docs.as_ref().ok_or("--docs required for index")?;
    let (schema, id_f, body_f) = schema();
    std::fs::create_dir_all(&args.index)?;
    let index = Index::create_in_dir(&args.index, schema)?;
    // Writer as shipped: default thread count, 1 GiB heap budget. Auto
    // merging is disabled so the explicit force-merge below owns the
    // segment set; total work stays inside this timed phase either way.
    let mut writer = index.writer(1 << 30)?;
    writer.set_merge_policy(Box::new(tantivy::merge_policy::NoMergePolicy));
    for line in BufReader::new(std::fs::File::open(docs_path)?).lines() {
        let line = line?;
        let (id, text) = line.split_once('\t').ok_or("docs.tsv line missing tab")?;
        let id: u64 = id.parse()?;
        writer.add_document(doc!(id_f => id, body_f => text))?;
    }
    writer.commit()?;
    // Force-merge to one segment (standard practice for a static corpus,
    // and what tantivy's own search benchmark does). This also makes
    // result order deterministic across runs: with multiple segments the
    // layout varies with writer threading and BM25 ties at the top-k
    // cutoff resolve differently run to run. The merge cost stays inside
    // the timed index phase.
    let segments = index.searchable_segment_ids()?;
    if segments.len() > 1 {
        writer.merge(&segments).wait()?;
    }
    writer.wait_merging_threads()?;
    Ok(())
}

enum Parsed {
    Term(String),
    And(String, String),
    Or(String, String),
    Phrase(String, String),
}

fn parse_queries(path: &std::path::Path) -> Result<Vec<Parsed>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for line in BufReader::new(std::fs::File::open(path)?).lines() {
        let line = line?;
        let parts: Vec<&str> = line.split('\t').collect();
        let q = match (parts.get(1).copied(), parts.get(2), parts.get(3)) {
            (Some("term"), Some(a), None) => Parsed::Term((*a).to_owned()),
            (Some("and"), Some(a), Some(b)) => Parsed::And((*a).to_owned(), (*b).to_owned()),
            (Some("or"), Some(a), Some(b)) => Parsed::Or((*a).to_owned(), (*b).to_owned()),
            (Some("phrase"), Some(a), Some(b)) => Parsed::Phrase((*a).to_owned(), (*b).to_owned()),
            _ => return Err(format!("bad query line {line:?}").into()),
        };
        out.push(q);
    }
    Ok(out)
}

fn phase_query(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let queries_path = args.queries.as_ref().ok_or("--queries required")?;
    let matches_path = args.matches.as_ref().ok_or("--matches required")?;
    let ranked_path = args.ranked.as_ref().ok_or("--ranked required")?;
    let (_, id_f, body_f) = schema();
    let index = Index::open_in_dir(&args.index)?;
    let reader = index.reader()?;
    let searcher = reader.searcher();
    let queries = parse_queries(queries_path)?;

    let term = |w: &str| Term::from_field_text(body_f, w);
    let build = |q: &Parsed| -> Box<dyn Query> {
        match q {
            Parsed::Term(a) => Box::new(TermQuery::new(term(a), IndexRecordOption::Basic)),
            Parsed::And(a, b) => Box::new(BooleanQuery::new(vec![
                (
                    Occur::Must,
                    Box::new(TermQuery::new(term(a), IndexRecordOption::Basic)) as Box<dyn Query>,
                ),
                (
                    Occur::Must,
                    Box::new(TermQuery::new(term(b), IndexRecordOption::Basic)),
                ),
            ])),
            Parsed::Or(a, b) => Box::new(BooleanQuery::new(vec![
                (
                    Occur::Should,
                    Box::new(TermQuery::new(term(a), IndexRecordOption::Basic)) as Box<dyn Query>,
                ),
                (
                    Occur::Should,
                    Box::new(TermQuery::new(term(b), IndexRecordOption::Basic)),
                ),
            ])),
            Parsed::Phrase(a, b) => Box::new(PhraseQuery::new(vec![term(a), term(b)])),
        }
    };

    let doc_id = |seg_doc: tantivy::DocAddress| -> Result<u64, Box<dyn std::error::Error>> {
        let stored: tantivy::TantivyDocument = searcher.doc(seg_doc)?;
        stored
            .get_first(id_f)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "doc missing id".into())
    };

    // Timing passes with output discarded, then the verified pass writes.
    for _ in 1..args.passes {
        for q in &queries {
            let _ = searcher.search(&*build(q), &DocSetCollector)?;
            let _ = searcher.search(&*build(q), &TopDocs::with_limit(10).order_by_score())?;
        }
    }

    let mut matches = BufWriter::new(std::fs::File::create(matches_path)?);
    let mut ranked = BufWriter::new(std::fs::File::create(ranked_path)?);
    for (qid, q) in queries.iter().enumerate() {
        let hits = searcher.search(&*build(q), &DocSetCollector)?;
        let mut ids: Vec<u64> = hits.into_iter().map(&doc_id).collect::<Result<_, _>>()?;
        ids.sort_unstable();
        for id in ids {
            writeln!(matches, "{qid}\t{id}")?;
        }
        // Ranked: Tantivy's own BM25. Fetch past the cutoff, then break
        // score ties by doc id so the published top-10 is a stable fact.
        let top = searcher.search(&*build(q), &TopDocs::with_limit(50).order_by_score())?;
        let mut scored: Vec<(f32, u64)> = top
            .into_iter()
            .map(|(score, addr)| doc_id(addr).map(|id| (score, id)))
            .collect::<Result<_, _>>()?;
        scored.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
        for (rank, (_, id)) in scored.iter().take(10).enumerate() {
            writeln!(ranked, "{qid}\t{rank}\t{id}")?;
        }
    }
    matches.flush()?;
    ranked.flush()?;
    Ok(())
}
