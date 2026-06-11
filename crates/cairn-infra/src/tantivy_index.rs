//! Tantivy-backed `SearchIndex`: n-gram tokenized full-text search with BM25
//! ranking and snippet generation. Replaces `InMemoryIndex` in production.
//! Supports both in-memory (RamDirectory) via [`TantivyIndex::in_memory`] and
//! persistent on-disk (MmapDirectory) via [`TantivyIndex::open_at`], both
//! behind the same port seam.

use std::path::Path;

use cairn_domain::{Note, NotePath};
use cairn_ports::{AdapterError, PortError, SearchHit, SearchIndex};
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING,
};
use tantivy::snippet::SnippetGenerator;
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

const WRITER_HEAP: usize = 15_000_000;
const SEARCH_LIMIT: usize = 50;
const SNIPPET_MAX_CHARS: usize = 160;
/// Smallest n-gram (and minimum useful query length).
const NGRAM_MIN: usize = 2;

/// A Tantivy full-text index over note bodies and paths.
pub struct TantivyIndex {
    index: Index,
    reader: IndexReader,
    writer: IndexWriter,
    path: Field,
    path_text: Field,
    body: Field,
}

fn adapt<E: std::error::Error + Send + Sync + 'static>(e: E) -> PortError {
    PortError::Adapter(AdapterError::new(e))
}

fn open_or_rebuild(dir: &Path, schema: &Schema) -> Result<Index, PortError> {
    std::fs::create_dir_all(dir).map_err(adapt)?;
    let open = |schema: Schema| -> tantivy::Result<Index> {
        let mmap = MmapDirectory::open(dir)?;
        Index::open_or_create(mmap, schema)
    };
    match open(schema.clone()) {
        Ok(index) => Ok(index),
        Err(_) => {
            // Corrupt or schema-mismatched index: wipe the directory and recreate.
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        let _ = std::fs::remove_dir_all(&p);
                    } else {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
            open(schema.clone()).map_err(adapt)
        }
    }
}

impl TantivyIndex {
    fn schema() -> (Schema, Field, Field, Field) {
        let mut sb = Schema::builder();
        let path = sb.add_text_field("path", STRING | STORED);
        let text_opts = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("ngram")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let path_text = sb.add_text_field("path_text", text_opts.clone());
        let body = sb.add_text_field("body", text_opts | STORED);
        (sb.build(), path, path_text, body)
    }

    fn finish(index: Index, path: Field, path_text: Field, body: Field) -> Result<Self, PortError> {
        let ngram = NgramTokenizer::new(NGRAM_MIN, 3, false).map_err(adapt)?;
        let analyzer = TextAnalyzer::builder(ngram).filter(LowerCaser).build();
        index.tokenizers().register("ngram", analyzer);
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(adapt)?;
        let writer = index.writer(WRITER_HEAP).map_err(adapt)?;
        Ok(Self {
            index,
            reader,
            writer,
            path,
            path_text,
            body,
        })
    }

    /// Build an in-memory index (RamDirectory). Load notes via [`SearchIndex::reindex`].
    ///
    /// # Errors
    /// Returns [`PortError`] if the schema, tokenizer, writer, or reader cannot be built.
    pub fn in_memory() -> Result<Self, PortError> {
        let (schema, path, path_text, body) = Self::schema();
        let index = Index::create_in_ram(schema);
        Self::finish(index, path, path_text, body)
    }

    /// Open (or create) a persistent index under `dir` (a `MmapDirectory`). The
    /// caller holds the exclusive writer for the value's lifetime.
    ///
    /// # Errors
    /// Returns [`PortError`] if the directory can't be created/opened even after a
    /// rebuild attempt, or the writer lock is held by another process.
    pub fn open_at(dir: &Path) -> Result<Self, PortError> {
        let (schema, path, path_text, body) = Self::schema();
        let index = open_or_rebuild(dir, &schema)?;
        Self::finish(index, path, path_text, body)
    }

    fn term(&self, path: &NotePath) -> Term {
        Term::from_field_text(self.path, path.as_str())
    }
}

impl SearchIndex for TantivyIndex {
    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError> {
        self.writer.delete_all_documents().map_err(adapt)?;
        for n in notes {
            let p = n.path.as_str().to_string();
            self.writer
                .add_document(doc!(
                    self.path => p.clone(),
                    self.path_text => p,
                    self.body => n.body.clone(),
                ))
                .map_err(adapt)?;
        }
        self.writer.commit().map_err(adapt)?;
        self.reader.reload().map_err(adapt)
    }

    fn search(&self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        let q = query.trim();
        if q.chars().count() < NGRAM_MIN {
            return Ok(Vec::new());
        }
        let searcher = self.reader.searcher();
        let mut parser = QueryParser::for_index(&self.index, vec![self.body, self.path_text]);
        parser.set_conjunction_by_default();
        // Phrase-quote the query so its n-grams must be adjacent (substring
        // match); strip embedded quotes and backslashes so the parser can't
        // break out or trigger Tantivy's escape grammar.
        let sanitized = q.replace(['"', '\\'], " ");
        let parsed = parser
            .parse_query(&format!("\"{sanitized}\""))
            .map_err(adapt)?;
        let collector = TopDocs::with_limit(SEARCH_LIMIT).order_by_score();
        let top = searcher.search(&*parsed, &collector).map_err(adapt)?;

        // Snippet generation is best-effort: if the generator can't be built,
        // results still return with empty snippets (spec: non-fatal).
        let mut sg = SnippetGenerator::create(&searcher, &*parsed, self.body).ok();
        if let Some(g) = sg.as_mut() {
            g.set_max_num_chars(SNIPPET_MAX_CHARS);
        }

        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let d: TantivyDocument = searcher.doc(addr).map_err(adapt)?;
            let path_str = d
                .get_first(self.path)
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let path = NotePath::new(path_str).map_err(adapt)?;
            let (snippet, highlights) = match sg.as_ref() {
                Some(g) => {
                    let snip = g.snippet_from_doc(&d);
                    let hl = snip
                        .highlighted()
                        .iter()
                        .map(|r| (r.start as u32, r.end as u32))
                        .collect();
                    (snip.fragment().to_string(), hl)
                }
                None => (String::new(), Vec::new()),
            };
            hits.push(SearchHit {
                path,
                score,
                snippet,
                highlights,
            });
        }
        Ok(hits)
    }

    fn upsert(&mut self, note: &Note) -> Result<(), PortError> {
        let term = self.term(&note.path);
        self.writer.delete_term(term);
        let p = note.path.as_str().to_string();
        self.writer
            .add_document(doc!(
                self.path => p.clone(),
                self.path_text => p,
                self.body => note.body.clone(),
            ))
            .map_err(adapt)?;
        self.writer.commit().map_err(adapt)?;
        self.reader.reload().map_err(adapt)
    }

    fn remove(&mut self, path: &NotePath) -> Result<(), PortError> {
        let term = self.term(path);
        self.writer.delete_term(term);
        self.writer.commit().map_err(adapt)?;
        self.reader.reload().map_err(adapt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(path: &str, body: &str) -> Note {
        Note {
            path: NotePath::new(path).unwrap(),
            frontmatter: None,
            body: body.into(),
        }
    }

    #[test]
    fn ranks_and_matches_substring() {
        let mut idx = TantivyIndex::in_memory().unwrap();
        idx.reindex(&[
            note("a.md", "the borrow checker enforces ownership"),
            note("b.md", "ownership ownership ownership rules"),
        ])
        .unwrap();

        let hits = idx.search("ownership").unwrap();
        assert!(!hits.is_empty());
        // b.md mentions the term more → should rank at/near the top.
        assert_eq!(hits[0].path.as_str(), "b.md");
        // snippet for the top hit contains the matched term.
        assert!(hits[0].snippet.to_lowercase().contains("ownership"));
        // every highlight range lies within its snippet.
        for h in &hits {
            for (s, e) in &h.highlights {
                assert!(*s <= *e && (*e as usize) <= h.snippet.len());
            }
        }

        // mid-word substring still matches via n-grams.
        let sub = idx.search("nersh").unwrap();
        assert!(sub.iter().any(|h| h.path.as_str() == "a.md"));

        // sub-2-char query returns nothing.
        assert!(idx.search("o").unwrap().is_empty());
        assert!(idx.search("  ").unwrap().is_empty());
    }

    #[test]
    fn matches_by_path() {
        let mut idx = TantivyIndex::in_memory().unwrap();
        idx.reindex(&[note("rust-notes.md", "unrelated body text")])
            .unwrap();
        assert!(idx
            .search("rust")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "rust-notes.md"));
    }

    #[test]
    fn query_with_backslash_does_not_error() {
        let mut idx = TantivyIndex::in_memory().unwrap();
        idx.reindex(&[note("a.md", "ownership rules")]).unwrap();
        // A query containing/ending with a backslash must not return Err.
        assert!(idx.search("rules\\").is_ok());
        assert!(idx.search("c:\\notes\\").is_ok());
        // Sanity: a normal substring still matches after sanitization.
        assert!(idx
            .search("owners")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "a.md"));
    }

    #[test]
    fn upsert_replaces_and_remove_deletes() {
        let mut idx = TantivyIndex::in_memory().unwrap();
        idx.upsert(&note("a.md", "hello target")).unwrap();
        assert!(idx
            .search("target")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "a.md"));

        idx.upsert(&note("a.md", "changed now")).unwrap();
        assert!(idx.search("target").unwrap().is_empty());
        assert!(!idx.search("changed").unwrap().is_empty());

        idx.remove(&NotePath::new("a.md").unwrap()).unwrap();
        assert!(idx.search("changed").unwrap().is_empty());
    }

    #[test]
    fn open_at_persists_across_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("index");
        {
            let mut idx = TantivyIndex::open_at(&dir).unwrap();
            idx.reindex(&[note("a.md", "the borrow checker enforces ownership")])
                .unwrap();
            assert!(idx
                .search("ownership")
                .unwrap()
                .iter()
                .any(|h| h.path.as_str() == "a.md"));
        } // drop releases the writer lock

        let idx2 = TantivyIndex::open_at(&dir).unwrap();
        assert!(idx2
            .search("nersh")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "a.md"));
    }

    #[test]
    fn open_at_rebuilds_on_corruption() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("index");
        {
            let _ = TantivyIndex::open_at(&dir).unwrap();
        }
        std::fs::write(dir.join("meta.json"), "not valid json").unwrap();
        let idx = TantivyIndex::open_at(&dir).unwrap();
        assert!(idx.search("anything").unwrap().is_empty());
    }
}
