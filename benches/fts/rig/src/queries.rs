//! The deterministic query set, drawn from the corpus's own vocabulary.
//!
//! Query terms are restricted to pure ASCII `[a-z]{4,15}` tokens in a
//! mid-document-frequency band. That restriction is not cosmetic: it keeps
//! the two engines' tokenizers (Tantivy `default`, FTS5 `unicode61`) in
//! provable agreement on what a token is, so match sets are comparable
//! across engines. Diacritics, apostrophes, and numbers tokenize
//! differently between engines and would turn a tokenizer quirk into a
//! fake correctness failure.
//!
//! Four query classes. The first three have engine-independent answers
//! (a match set); the ranked class is each engine's own BM25 and is
//! recorded per engine, never cross-compared.

use crate::corpus::Doc;
use kyzo_bench_harness::{Seed, SplitMix64};
use std::collections::HashMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    Term(String),
    And(String, String),
    Or(String, String),
    Phrase(String, String),
}

/// A token acceptable as a query term in both engines' tokenizers.
fn plain_word(w: &str) -> bool {
    (4..=15).contains(&w.len()) && w.bytes().all(|b| b.is_ascii_lowercase())
}

/// Document frequencies of plain words, counted on lowercased
/// alphanumeric tokenization (both engines' behavior on ASCII).
fn doc_frequencies(docs: &[Doc]) -> HashMap<String, u32> {
    let mut df: HashMap<String, u32> = HashMap::new();
    for doc in docs {
        let lowered = doc.text.to_lowercase();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for tok in lowered.split(|c: char| !c.is_ascii_alphanumeric()) {
            if plain_word(tok) && seen.insert(tok) {
                *df.entry(tok.to_owned()).or_insert(0) += 1;
            }
        }
    }
    df
}

/// Adjacent plain-word pairs as both tokenizers would see them: two
/// qualifying words separated by a single space, cleanly delimited.
fn adjacent_pairs(docs: &[Doc], rng: &mut SplitMix64, want: usize) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut tries = 0;
    while pairs.len() < want && tries < want * 200 {
        tries += 1;
        let doc = &docs[(rng.next_u64() % docs.len() as u64) as usize];
        let words: Vec<&str> = doc.text.split(' ').collect();
        if words.len() < 2 {
            continue;
        }
        let i = (rng.next_u64() % (words.len() as u64 - 1)) as usize;
        let (a, b) = (words[i].to_lowercase(), words[i + 1].to_lowercase());
        if plain_word(&a) && plain_word(&b) && a != b {
            pairs.push((a, b));
        }
    }
    pairs
}

/// The full query set: 40 term / 30 and / 30 or / 20 phrase, all minted
/// from the seed and the corpus itself.
pub fn generate(seed: Seed, docs: &[Doc]) -> Vec<Query> {
    let mut rng = SplitMix64::new(seed);
    let df = doc_frequencies(docs);
    let n = docs.len() as u32;
    // Mid-frequency band: common enough to have real posting lists,
    // rare enough that match sets stay discriminating.
    let (lo, hi) = (n / 200, n / 20);
    let mut band: Vec<&String> = df
        .iter()
        .filter(|(_, c)| (lo..=hi).contains(*c))
        .map(|(w, _)| w)
        .collect();
    band.sort(); // HashMap order is not deterministic; the draw must be
    assert!(
        !band.is_empty(),
        "no query terms in the df band [{lo}, {hi}] over {n} docs; corpus too small or degenerate"
    );
    let draw = |rng: &mut SplitMix64| -> String {
        band[(rng.next_u64() % band.len() as u64) as usize].clone()
    };

    let mut queries = Vec::with_capacity(120);
    for _ in 0..40 {
        queries.push(Query::Term(draw(&mut rng)));
    }
    for _ in 0..30 {
        let (a, b) = (draw(&mut rng), draw(&mut rng));
        queries.push(Query::And(a, b));
    }
    for _ in 0..30 {
        let (a, b) = (draw(&mut rng), draw(&mut rng));
        queries.push(Query::Or(a, b));
    }
    for (a, b) in adjacent_pairs(docs, &mut rng, 20) {
        queries.push(Query::Phrase(a, b));
    }
    queries
}

/// Line format both runners parse: `qid<TAB>kind<TAB>term[<TAB>term]`.
pub fn to_file(queries: &[Query]) -> String {
    let mut out = String::new();
    for (i, q) in queries.iter().enumerate() {
        match q {
            Query::Term(a) => writeln!(out, "{i}\tterm\t{a}"),
            Query::And(a, b) => writeln!(out, "{i}\tand\t{a}\t{b}"),
            Query::Or(a, b) => writeln!(out, "{i}\tor\t{a}\t{b}"),
            Query::Phrase(a, b) => writeln!(out, "{i}\tphrase\t{a}\t{b}"),
        }
        .expect("string write");
    }
    out
}

/// Render the FTS5 side: match-set queries then ranked top-10, with the
/// output split between two files.
pub fn sqlite_query_script(
    queries: &[Query],
    matches_out: &std::path::Path,
    ranked_out: &std::path::Path,
    passes: u32,
) -> String {
    let mut s = String::from(".separator \"\\t\"\n");
    // Timing passes: the full set repeated with output discarded, so the
    // measured phase has enough work to time; the verified pass writes.
    s.push_str(".output /dev/null\n");
    for _ in 1..passes {
        push_sqlite_queries(&mut s, queries, false);
        push_sqlite_queries(&mut s, queries, true);
    }
    let _ = writeln!(s, ".output {}", matches_out.display());
    push_sqlite_queries(&mut s, queries, false);
    let _ = writeln!(s, ".output {}", ranked_out.display());
    push_sqlite_queries(&mut s, queries, true);
    s
}

fn push_sqlite_queries(s: &mut String, queries: &[Query], ranked: bool) {
    for (i, q) in queries.iter().enumerate() {
        let m = match q {
            Query::Term(a) => format!("\"{a}\""),
            Query::And(a, b) => format!("\"{a}\" AND \"{b}\""),
            Query::Or(a, b) => format!("\"{a}\" OR \"{b}\""),
            Query::Phrase(a, b) => format!("\"{a} {b}\""),
        };
        if ranked {
            let _ = writeln!(
                s,
                "SELECT {i}, rowid FROM d WHERE d MATCH '{m}' ORDER BY bm25(d), rowid LIMIT 10;"
            );
        } else {
            let _ = writeln!(s, "SELECT {i}, rowid FROM d WHERE d MATCH '{m}';");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs() -> Vec<Doc> {
        // Each `term<x><y>` word appears in exactly 2 docs, landing in the
        // mid-frequency band; the common words are everywhere (df = n,
        // above the band) and must never be drawn.
        (0..100u64)
            .map(|id| {
                let a = (b'a' + (id / 2 % 26) as u8) as char;
                let b = (b'a' + (id / 52) as u8) as char;
                Doc {
                    id,
                    text: format!("term{a}{b} common words appear here term{a}{b} ocean"),
                }
            })
            .collect()
    }

    #[test]
    fn generation_is_deterministic() {
        let d = docs();
        assert_eq!(generate(Seed(27_001), &d), generate(Seed(27_001), &d));
    }

    #[test]
    fn query_terms_are_plain_words() {
        for q in generate(Seed(27_001), &docs()) {
            let terms: Vec<&String> = match &q {
                Query::Term(a) => vec![a],
                Query::And(a, b) | Query::Or(a, b) | Query::Phrase(a, b) => vec![a, b],
            };
            for t in terms {
                assert!(plain_word(t), "unplain query term {t:?}");
            }
        }
    }
}
