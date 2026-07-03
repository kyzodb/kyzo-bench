//! Corpus preparation: Gutenberg books → paragraph documents.
//!
//! Deterministic and engine-neutral: header/footer boilerplate stripped
//! (the license text is identical across books and would pollute term
//! statistics), paragraphs split on blank lines, whitespace collapsed,
//! short fragments dropped. Doc ids are assigned in (book, paragraph)
//! order, so the same corpus directory always yields the same documents.

use std::path::Path;

pub struct Doc {
    pub id: u64,
    pub text: String,
}

pub fn load(corpus_dir: &Path) -> std::io::Result<Vec<Doc>> {
    let mut books: Vec<std::path::PathBuf> = std::fs::read_dir(corpus_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("pg") && n.ends_with(".txt"))
        })
        .collect();
    books.sort();
    if books.is_empty() {
        return Err(std::io::Error::other(format!(
            "no pg*.txt in {}; run benches/fts/fetch-corpus.sh",
            corpus_dir.display()
        )));
    }

    let mut docs = Vec::new();
    let mut id = 0u64;
    for book in books {
        // Gutenberg plain text uses CRLF; normalize before splitting.
        let raw = std::fs::read_to_string(&book)?.replace("\r\n", "\n");
        for para in paragraphs(strip_gutenberg(&raw)) {
            docs.push(Doc { id, text: para });
            id += 1;
        }
    }
    Ok(docs)
}

/// The body between the `*** START OF …` and `*** END OF …` markers.
fn strip_gutenberg(raw: &str) -> &str {
    let body_start = raw
        .find("*** START OF")
        .and_then(|i| raw[i..].find('\n').map(|j| i + j + 1))
        .unwrap_or(0);
    let body_end = raw[body_start..]
        .find("*** END OF")
        .map(|i| body_start + i)
        .unwrap_or(raw.len());
    &raw[body_start..body_end]
}

/// Blank-line-separated paragraphs, whitespace collapsed to single
/// spaces, fragments under 200 chars dropped.
fn paragraphs(body: &str) -> impl Iterator<Item = String> + '_ {
    body.split("\n\n").filter_map(|p| {
        let joined = p.split_whitespace().collect::<Vec<_>>().join(" ");
        (joined.len() >= 200).then_some(joined)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boilerplate_is_stripped_and_paragraphs_split() {
        let raw = "junk header\n*** START OF THE PROJECT GUTENBERG EBOOK X ***\n\
                   first  paragraph line one\nline two of the same paragraph, padded out to reach \
                   the two hundred character minimum for a document, because short fragments are \
                   dropped as noise rather than indexed as documents by either engine under test.\n\
                   \n\
                   short\n\
                   \n\
                   *** END OF THE PROJECT GUTENBERG EBOOK X ***\nlicense junk";
        let paras: Vec<String> = paragraphs(strip_gutenberg(raw)).collect();
        assert_eq!(paras.len(), 1);
        assert!(paras[0].starts_with("first paragraph line one line two"));
        assert!(!paras[0].contains("junk"));
    }
}
