//! A keyword index of the memory folder, so search does not read every file.
//!
//! The Markdown files stay the memory. The index is a copy in
//! `<root>/.lucid/index/`, built from them and refreshed before each search
//! from file sizes and modified times; deleting it loses nothing. Each file is
//! cut into pieces at headings and about every 300 words. A piece is indexed
//! twice: stemmed ("trading" finds "trade") and as raw words that keep paths,
//! ids and `#541` whole. Only one process writes the index; a process that
//! cannot get the writer's lock searches by scanning instead.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde_json::{json, Value as Json};
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, PhraseQuery, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, INDEXED, STORED,
    STRING,
};
use tantivy::tokenizer::TextAnalyzer;
use tantivy::{Index as Tantivy, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

/// Bump when the schema or the way files are cut changes; the index is then
/// rebuilt from the files.
const VERSION: u64 = 1;
/// About how many words go in one piece.
const PIECE_WORDS: usize = 300;
/// How many pieces a search looks at before it ranks files.
const CANDIDATES: usize = 2000;
/// Memory the writer may use before it flushes, in bytes.
const WRITER_MEMORY: usize = 30_000_000;

/// How long a search may trust the last look at the files. Changes the
/// server makes itself are seen at once; changes from outside (dream, an
/// editor) within this long.
#[cfg(not(test))]
const LOOK_EVERY: Duration = Duration::from_secs(2);
#[cfg(test)]
const LOOK_EVERY: Duration = Duration::ZERO;

/// How a refused writer's lock is reported.
const BUSY: &str = "another process writes the index; stop the server to rebuild it";

/// Counts the server's own writes, so the next search looks at the files.
static WRITES: AtomicU64 = AtomicU64::new(0);

/// The server wrote a file: the next search looks for changes.
pub fn changed() {
    WRITES.fetch_add(1, Ordering::Relaxed);
}

/// Characters that separate raw words, besides whitespace.
const RAW_SPLIT: &[char] = &[
    ',', ';', '(', ')', '[', ']', '{', '}', '"', '\'', '`', '!', '?', '<', '>', '*', '|',
];
/// Characters trimmed from the ends of a raw word.
const RAW_TRIM: &[char] = &['.', ':'];

/// Each indexed file's size and modified time (nanoseconds), by path.
type Manifest = BTreeMap<String, (u64, u128)>;

#[derive(Clone, Copy)]
struct Fields {
    path: Field,
    folder: Field,
    line: Field,
    deep: Field,
    record: Field,
    raw: Field,
    stem: Field,
    text: Field,
}

/// A piece of a file that a search found, with its score.
pub struct Found {
    pub path: String,
    pub line: usize,
    pub text: String,
    pub score: f32,
}

/// What a search may look at.
pub struct Scope<'a> {
    /// Leave out the archive, logs and messages.
    pub working_only: bool,
    /// Leave out session records.
    pub sealed: bool,
    /// Only files under this folder (relative, forward slashes), if any.
    pub folder: Option<&'a str>,
}

pub struct Index {
    root: PathBuf,
    fields: Fields,
    reader: IndexReader,
    stemmer: Mutex<TextAnalyzer>,
    /// The writer and the manifest (path to size and modified time), behind
    /// one lock so refreshes never overlap.
    state: Mutex<(IndexWriter, Manifest)>,
    /// When the files were last looked at, and the write count then.
    looked: Mutex<Option<(Instant, u64)>>,
}

/// The index of each memory root this process has opened, or `None` when it
/// could not be opened, so a failure is not retried on every search.
fn opened() -> &'static Mutex<HashMap<PathBuf, Option<Arc<Index>>>> {
    static OPENED: OnceLock<Mutex<HashMap<PathBuf, Option<Arc<Index>>>>> = OnceLock::new();
    OPENED.get_or_init(Default::default)
}

/// The index for `root`, opened once per process. `None` means search scans.
pub fn for_root(root: &Path) -> Option<Arc<Index>> {
    let mut opened = opened().lock().unwrap_or_else(|e| e.into_inner());
    opened
        .entry(root.to_path_buf())
        .or_insert_with(|| match Index::open(root) {
            Ok(index) => Some(Arc::new(index)),
            Err(e) => {
                // Another process writing the index is normal (a server and a
                // stdio server on one folder); only a real failure is reported.
                if !e.starts_with(BUSY) {
                    eprintln!("lucid-dream: search index not used, scanning instead: {e}");
                }
                None
            }
        })
        .clone()
}

/// Delete and rebuild the index for `root` from its files.
pub fn rebuild(root: &Path, files: &[(String, PathBuf)]) -> Result<usize, String> {
    // Never delete an index a running server is writing: take its lock first.
    drop(Index::open(root)?);
    opened()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(root);
    let dir = index_dir(root);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let index = Index::open(root)?;
    index.refresh(files)?;
    Ok(index.reader.searcher().num_docs() as usize)
}

fn index_dir(root: &Path) -> PathBuf {
    root.join(".lucid").join("index")
}

fn schema() -> (Schema, Fields) {
    let mut s = Schema::builder();
    let words = |tokenizer: &str| {
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(tokenizer)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
    };
    let fields = Fields {
        path: s.add_text_field("path", STRING | STORED),
        folder: s.add_text_field("folder", STRING),
        line: s.add_u64_field("line", STORED),
        deep: s.add_u64_field("deep", INDEXED),
        record: s.add_u64_field("record", INDEXED),
        raw: s.add_text_field("raw", words("whitespace")),
        stem: s.add_text_field("stem", words("en_stem")),
        text: s.add_text_field("text", STORED),
    };
    (s.build(), fields)
}

/// Lowercased words split the raw way: whitespace and `RAW_SPLIT`, with
/// `RAW_TRIM` taken off the ends, so `#541`, `src/store.rs` and `0.19.0`
/// stay whole.
pub fn raw_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| c.is_whitespace() || RAW_SPLIT.contains(&c))
        .map(|w| w.trim_matches(RAW_TRIM))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// A term is also searched stemmed only when it is made of plain words.
fn plain(term: &str) -> bool {
    term.chars().all(|c| c.is_alphanumeric() || c == ' ')
}

/// The file cut into pieces: a new piece at every Markdown heading and after
/// about `PIECE_WORDS` words. Each piece comes with its first line number.
pub fn pieces(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut current = String::new();
    let mut start = 1;
    let mut words = 0;
    for (i, line) in text.lines().enumerate() {
        let heading = line.trim_start().starts_with('#');
        if !current.is_empty() && (heading || words >= PIECE_WORDS) {
            out.push((start, std::mem::take(&mut current)));
            words = 0;
        }
        if current.is_empty() {
            start = i + 1;
        }
        current.push_str(line);
        current.push('\n');
        words += line.split_whitespace().count();
    }
    if !current.trim().is_empty() {
        out.push((start, current));
    }
    out
}

impl Index {
    pub(crate) fn open(root: &Path) -> Result<Index, String> {
        let dir = index_dir(root);
        let (schema, fields) = schema();
        let manifest_path = dir.join("manifest.json");
        let manifest = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Json>(&raw).ok());
        let current = manifest
            .as_ref()
            .is_some_and(|m| m["version"].as_u64() == Some(VERSION));
        if dir.exists() && !current {
            // An old or broken index: start again from the files.
            fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let opened = MmapDirectory::open(&dir)
            .map_err(|e| e.to_string())
            .and_then(|d| Tantivy::open_or_create(d, schema).map_err(|e| e.to_string()));
        let tantivy = match opened {
            Ok(index) => index,
            Err(_) => {
                // Unreadable files: wipe once and create it fresh.
                fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let (schema, _) = self::schema();
                let d = MmapDirectory::open(&dir).map_err(|e| e.to_string())?;
                Tantivy::open_or_create(d, schema).map_err(|e| e.to_string())?
            }
        };
        // Taking the writer takes tantivy's lock: a second process fails here
        // and scans instead.
        let writer: IndexWriter = tantivy
            .writer_with_num_threads(1, WRITER_MEMORY)
            .map_err(|e| format!("{BUSY} ({e})"))?;
        let reader = tantivy
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e: tantivy::TantivyError| e.to_string())?;
        let stemmer = tantivy
            .tokenizers()
            .get("en_stem")
            .ok_or("no English stemmer")?;
        let files = if current {
            manifest
                .as_ref()
                .and_then(|m| m["files"].as_object())
                .map(|files| {
                    files
                        .iter()
                        .filter_map(|(path, v)| {
                            let size = v[0].as_u64()?;
                            let time = v[1].as_str()?.parse().ok()?;
                            Some((path.clone(), (size, time)))
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        Ok(Index {
            root: root.to_path_buf(),
            fields,
            reader,
            stemmer: Mutex::new(stemmer),
            state: Mutex::new((writer, files)),
            looked: Mutex::new(None),
        })
    }

    /// True when it is time to look at the files again; the caller then
    /// calls `refresh`.
    pub fn due(&self) -> bool {
        let mut looked = self.looked.lock().unwrap_or_else(|e| e.into_inner());
        let writes = WRITES.load(Ordering::Relaxed);
        let due = looked.is_none_or(|(at, seen)| seen != writes || at.elapsed() >= LOOK_EVERY);
        if due {
            *looked = Some((Instant::now(), writes));
        }
        due
    }

    /// Bring the index up to date with `files` (relative path, real path):
    /// index new and changed files, drop the ones that are gone.
    pub fn refresh(&self, files: &[(String, PathBuf)]) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (writer, known) = &mut *state;
        let f = self.fields;
        let mut changed = false;
        let mut seen = Manifest::new();
        for (rel, real) in files {
            let Ok(meta) = fs::metadata(real) else {
                continue;
            };
            let time = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            let stamp = (meta.len(), time);
            seen.insert(rel.clone(), stamp);
            if known.get(rel) == Some(&stamp) {
                continue;
            }
            changed = true;
            writer.delete_term(Term::from_field_text(f.path, rel));
            // Unreadable or non-UTF-8 files are left out, as the scan does.
            let Ok(text) = fs::read_to_string(real) else {
                continue;
            };
            let deep = u64::from(crate::store::is_deep(rel));
            let record = u64::from(rel.to_lowercase().contains(".record.md"));
            let folders: Vec<String> = rel
                .match_indices('/')
                .map(|(i, _)| rel[..i].to_string())
                .collect();
            for (line, body) in pieces(&text) {
                let mut doc = TantivyDocument::default();
                doc.add_text(f.path, rel);
                for folder in &folders {
                    doc.add_text(f.folder, folder);
                }
                doc.add_u64(f.line, line as u64);
                doc.add_u64(f.deep, deep);
                doc.add_u64(f.record, record);
                doc.add_text(f.raw, raw_words(&body).join(" "));
                doc.add_text(f.stem, &body);
                doc.add_text(f.text, &body);
                writer.add_document(doc).map_err(|e| e.to_string())?;
            }
        }
        for rel in known.keys() {
            if !seen.contains_key(rel) {
                changed = true;
                writer.delete_term(Term::from_field_text(f.path, rel));
            }
        }
        if !changed {
            return Ok(());
        }
        writer.commit().map_err(|e| e.to_string())?;
        self.reader.reload().map_err(|e| e.to_string())?;
        *known = seen;
        let manifest = json!({
            "version": VERSION,
            "files": known
                .iter()
                .map(|(path, (size, time))| (path.clone(), json!([size, time.to_string()])))
                .collect::<serde_json::Map<_, _>>(),
        });
        let path = index_dir(&self.root).join("manifest.json");
        fs::write(&path, manifest.to_string()).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The stems of `text`, as the stemmed field holds them.
    pub fn stems(&self, text: &str) -> Vec<String> {
        let mut stemmer = self.stemmer.lock().unwrap_or_else(|e| e.into_inner());
        let mut stream = stemmer.token_stream(text);
        let mut out = Vec::new();
        while stream.advance() {
            out.push(stream.token().text.clone());
        }
        out
    }

    /// The pieces that hold any of `terms` within `scope`, best first.
    pub fn search(&self, terms: &[String], scope: &Scope) -> Result<Vec<Found>, String> {
        let f = self.fields;
        let words = |field: Field, tokens: Vec<String>| -> Option<Box<dyn Query>> {
            let mut terms: Vec<Term> = tokens
                .iter()
                .map(|t| Term::from_field_text(field, t))
                .collect();
            match terms.len() {
                0 => None,
                1 => Some(Box::new(TermQuery::new(
                    terms.remove(0),
                    IndexRecordOption::WithFreqs,
                ))),
                _ => Some(Box::new(PhraseQuery::new(terms))),
            }
        };
        let mut any: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for term in terms {
            if let Some(q) = words(f.raw, raw_words(term)) {
                any.push((Occur::Should, Box::new(BoostQuery::new(q, 2.0))));
            }
            if plain(term) {
                if let Some(q) = words(f.stem, self.stems(term)) {
                    any.push((Occur::Should, q));
                }
            }
        }
        if any.is_empty() {
            return Ok(Vec::new());
        }
        let flag = |field: Field, value: u64| -> (Occur, Box<dyn Query>) {
            let term = Term::from_field_u64(field, value);
            (Occur::Must, Box::new(TermQuery::new(term, IndexRecordOption::Basic)))
        };
        let mut all: Vec<(Occur, Box<dyn Query>)> =
            vec![(Occur::Must, Box::new(BooleanQuery::new(any)))];
        if scope.working_only {
            all.push(flag(f.deep, 0));
        }
        if scope.sealed {
            all.push(flag(f.record, 0));
        }
        if let Some(folder) = scope.folder {
            let term = Term::from_field_text(f.folder, folder);
            all.push((Occur::Must, Box::new(TermQuery::new(term, IndexRecordOption::Basic))));
        }
        let query = BooleanQuery::new(all);
        let searcher = self.reader.searcher();
        let top = searcher
            .search(&query, &TopDocs::with_limit(CANDIDATES).order_by_score())
            .map_err(|e| e.to_string())?;
        let mut found = Vec::with_capacity(top.len());
        for (score, address) in top {
            let doc: TantivyDocument = searcher.doc(address).map_err(|e| e.to_string())?;
            let text = |field| doc.get_first(field).and_then(|v| v.as_str()).map(String::from);
            let (Some(path), Some(text)) = (text(f.path), text(f.text)) else {
                continue;
            };
            let line = doc.get_first(f.line).and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            found.push(Found {
                path,
                line,
                text,
                score,
            });
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_start_at_headings_and_after_many_words() {
        let text = "intro\n# One\na b c\n## Two\nd\n";
        let cut = pieces(text);
        let starts: Vec<usize> = cut.iter().map(|(line, _)| *line).collect();
        assert_eq!(starts, [1, 2, 4]);
        assert_eq!(cut[1].1, "# One\na b c\n");
        let long = "word ".repeat(200) + "\n" + &"word ".repeat(200) + "\nlast\n";
        let starts: Vec<usize> = pieces(&long).iter().map(|(line, _)| *line).collect();
        assert_eq!(starts, [1, 3]);
        assert!(pieces("").is_empty());
    }

    #[test]
    fn raw_words_keep_paths_ids_and_versions_whole() {
        assert_eq!(
            raw_words("See PR #541, in src/store.rs: v0.19.0."),
            ["see", "pr", "#541", "in", "src/store.rs", "v0.19.0"]
        );
    }
}
