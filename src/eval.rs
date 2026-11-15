//! `search-eval`: how often search finds the page it should.
//!
//! The question file holds one question per line: the query, a tab, and the
//! paths that count as the right answer, separated by commas. Blank lines and
//! lines starting with `#` are skipped. The file lives in the memory folder
//! (`search-eval.tsv` by default), never in this repository, because the
//! questions and answers are the user's own memory.

use crate::store::Store;

/// How many of the first files a search returns are looked at.
const TOP: usize = 5;

/// One question that did not find its page, for the report.
struct Miss {
    query: String,
    wanted: Vec<String>,
    got: Vec<String>,
}

/// Run every question in `questions` against `store` and report the misses
/// and the score. A question is searched the way the gate would: working
/// memory first, and the deeper layer when working memory does not answer.
pub fn run(store: &Store, questions: &str) -> Result<String, String> {
    let mut asked = 0;
    let mut misses = Vec::new();
    for (n, line) in questions.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (query, wanted) = line
            .split_once('\t')
            .ok_or(format!("line {}: query and paths must be separated by a tab", n + 1))?;
        let wanted: Vec<String> = wanted
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if wanted.is_empty() {
            return Err(format!("line {}: no expected path", n + 1));
        }
        asked += 1;
        let got = top_files(store, query)?;
        if !got.iter().any(|path| wanted.contains(path)) {
            misses.push(Miss {
                query: query.to_string(),
                wanted,
                got,
            });
        }
    }
    if asked == 0 {
        return Err("no questions in the file".into());
    }
    let mut report = String::new();
    for miss in &misses {
        report.push_str(&format!(
            "miss: {}\n  wanted: {}\n  got: {}\n",
            miss.query,
            miss.wanted.join(", "),
            if miss.got.is_empty() {
                "nothing".to_string()
            } else {
                miss.got.join(", ")
            }
        ));
    }
    report.push_str(&format!(
        "found {} of {asked} in the top {TOP}\n",
        asked - misses.len()
    ));
    Ok(report)
}

/// The first distinct files a search returns, in order, as the gate would
/// search: working memory, or everything when working memory has no answer.
fn top_files(store: &Store, query: &str) -> Result<Vec<String>, String> {
    let deep = !store.working_answers(query, false).map_err(|e| e.to_string())?;
    let hits = store
        .search(query, "", 200, deep, false)
        .map_err(|e| e.to_string())?;
    let mut files: Vec<String> = Vec::new();
    for hit in hits {
        if !files.contains(&hit.path) {
            files.push(hit.path);
            if files.len() == TOP {
                break;
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::Temp;
    use std::fs;

    #[test]
    fn counts_questions_found_in_the_top_five_and_lists_misses() {
        let t = Temp::new();
        let s = t.store();
        fs::create_dir_all(t.0.join("wiki")).unwrap();
        fs::write(t.0.join("wiki/zoo.md"), "The zebra lives here.\n").unwrap();
        fs::create_dir_all(t.0.join("archive")).unwrap();
        fs::write(t.0.join("archive/old.md"), "An okapi once.\n").unwrap();
        let questions = "# animals\n\
                         zebra\twiki/zoo.md\n\
                         okapi\tarchive/old.md, wiki/other.md\n\
                         giraffe\twiki/zoo.md\n";
        let report = run(&s, questions).unwrap();
        assert!(report.ends_with("found 2 of 3 in the top 5\n"), "{report}");
        assert!(report.contains("miss: giraffe\n  wanted: wiki/zoo.md\n  got: nothing\n"));
        assert!(run(&s, "zebra wiki/zoo.md\n").is_err());
        assert!(run(&s, "# only a comment\n").is_err());
    }
}
