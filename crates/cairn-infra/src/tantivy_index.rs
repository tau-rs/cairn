//! Tantivy-backed `SearchIndex`: n-gram tokenized full-text search with BM25
//! ranking and snippet generation. Replaces `InMemoryIndex` in production.
//! In-memory (RamDirectory) today; an on-disk constructor can be added later
//! behind the same port via Tantivy's `Directory` seam.

use cairn_domain::{Note, NotePath};
use cairn_ports::{PortError, SearchHit, SearchIndex};
use tantivy::collector::TopDocs;
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
    path: Field,
    path_text: Field,
    body: Field,
}

fn adapt<E: std::fmt::Display>(e: E) -> PortError {
    PortError::Adapter(e.to_string())
}

impl TantivyIndex {
    /// Build an in-memory index (RamDirectory). Load notes via [`SearchIndex::reindex`].
    ///
    /// # Errors
    /// Returns [`PortError`] if the schema, tokenizer, or reader cannot be built.
    pub fn in_memory() -> Result<Self, PortError> {
        let mut sb = Schema::builder();
        let path = sb.add_text_field("path", STRING | STORED);
        let text_opts = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("ngram")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let path_text = sb.add_text_field("path_text", text_opts.clone());
        let body = sb.add_text_field("body", text_opts | STORED);
        let schema = sb.build();

        let index = Index::create_in_ram(schema);
        let ngram = NgramTokenizer::new(NGRAM_MIN, 3, false).map_err(adapt)?;
        let analyzer = TextAnalyzer::builder(ngram).filter(LowerCaser).build();
        index.tokenizers().register("ngram", analyzer);

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(adapt)?;
        Ok(Self {
            index,
            reader,
            path,
            path_text,
            body,
        })
    }

    fn writer(&self) -> Result<IndexWriter, PortError> {
        self.index.writer(WRITER_HEAP).map_err(adapt)
    }

    fn term(&self, path: &NotePath) -> Term {
        Term::from_field_text(self.path, path.as_str())
    }
}

impl SearchIndex for TantivyIndex {
    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError> {
        let mut w = self.writer()?;
        w.delete_all_documents().map_err(adapt)?;
        for n in notes {
            let p = n.path.as_str().to_string();
            w.add_document(doc!(
                self.path => p.clone(),
                self.path_text => p,
                self.body => n.body.clone(),
            ))
            .map_err(adapt)?;
        }
        w.commit().map_err(adapt)?;
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
        // match); strip embedded quotes so the parser can't break out.
        let parsed = parser
            .parse_query(&format!("\"{}\"", q.replace('"', " ")))
            .map_err(adapt)?;
        let collector = TopDocs::with_limit(SEARCH_LIMIT).order_by_score();
        let top = searcher.search(&*parsed, &collector).map_err(adapt)?;

        let mut sg = SnippetGenerator::create(&searcher, &*parsed, self.body).map_err(adapt)?;
        sg.set_max_num_chars(SNIPPET_MAX_CHARS);

        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let d: TantivyDocument = searcher.doc(addr).map_err(adapt)?;
            let path_str = d
                .get_first(self.path)
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let path = NotePath::new(path_str).map_err(adapt)?;
            let snip = sg.snippet_from_doc(&d);
            let highlights = snip
                .highlighted()
                .iter()
                .map(|r| (r.start as u32, r.end as u32))
                .collect();
            hits.push(SearchHit {
                path,
                score,
                snippet: snip.fragment().to_string(),
                highlights,
            });
        }
        Ok(hits)
    }

    fn upsert(&mut self, note: &Note) -> Result<(), PortError> {
        let mut w = self.writer()?;
        w.delete_term(self.term(&note.path));
        let p = note.path.as_str().to_string();
        w.add_document(doc!(
            self.path => p.clone(),
            self.path_text => p,
            self.body => note.body.clone(),
        ))
        .map_err(adapt)?;
        w.commit().map_err(adapt)?;
        self.reader.reload().map_err(adapt)
    }

    fn remove(&mut self, path: &NotePath) -> Result<(), PortError> {
        let mut w = self.writer()?;
        w.delete_term(self.term(path));
        w.commit().map_err(adapt)?;
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
}
