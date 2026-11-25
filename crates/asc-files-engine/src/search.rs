// SPDX-License-Identifier: MIT
//! Search: walks folders for names matching a query, with no index, as legacy Files did. It
//! never follows links or crosses onto another drive, skips hidden entries unless asked, and
//! prunes the trees programs generate (caches, `node_modules`, `target`, …) and what
//! `.gitignore` files leave out. Each word of the query is matched fuzzily against an entry's
//! name, or failing that its path below the root, in fzf's syntax as legacy took it: `!word`
//! leaves out what matches it, `^word` and `word$` anchor it, `'word` asks for it exactly. A match in
//! the name outranks one in the path only, and an exact name ranks first. The walk stops at its
//! limits: results kept, entries looked at, depth and time. Folders the user leaves out (by name
//! or by path) are never walked.

use std::{
    cmp::Reverse,
    collections::BinaryHeap,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

use frizbee::{CaseMatching, Config, Matcher, Pattern};
use unicode_normalization::UnicodeNormalization;

use crate::ops::Cancel;

/// Trees programs generate, pruned so a search of Home stays quick; tools' own settings in them
/// stay searchable elsewhere.
const GENERATED_TREES: [&str; 14] = [
    "!**/.cache/",
    "!**/.cargo/registry/",
    "!**/.cargo/git/",
    "!**/.rustup/downloads/",
    "!**/.gradle/caches/",
    "!**/.gradle/wrapper/dists/",
    "!**/.m2/repository/",
    "!**/.npm/_cacache/",
    "!**/.bun/install/cache/",
    "!**/go/pkg/mod/",
    "!**/node_modules/",
    "!**/target/",
    "!**/.venv/",
    "!**/__pycache__/",
];

/// A name match outranks any path-only match.
const NAME_TERM_TIER: i64 = 1 << 32;
const EXACT_NAME_BONUS: i64 = 1 << 31;

/// How far a search goes, and where it doesn't.
#[derive(Clone, Debug)]
pub struct Limits {
    /// The best this many are kept.
    pub results: usize,
    pub entries: usize,
    pub depth: usize,
    pub time: Duration,
    /// Folders the user leaves out of every search.
    pub exclusions: Exclusions,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            results: 100,
            entries: 200_000,
            depth: 64,
            time: Duration::from_secs(10),
            exclusions: Exclusions::default(),
        }
    }
}

/// Folders left out of searches, as legacy kept them: a folder name (`cache`) leaves out every
/// folder of that name, whatever its case, and all below it, but not files so named; a path
/// (`/srv/backups`, `~/Private`) leaves out that folder and all below it, case and all.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Exclusions {
    names: Vec<String>,
    folders: Vec<PathBuf>,
}

impl Exclusions {
    /// A rule as it is kept (`~/` spelled out against `home`, the path tidied), or why it can't
    /// be one: empty, `.` or `..`, a path not from `/` or `~/`, one with `..` in it, or the
    /// whole of `/` or Home.
    pub fn normalize(raw: &str, home: &Path) -> Result<String, &'static str> {
        let input = raw.trim();
        if input.is_empty() {
            return Err("Enter a folder name or a folder's path.");
        }
        if input.contains('\0') {
            return Err("A rule cannot hold a NUL character.");
        }
        let trimmed = input.trim_end_matches('/');
        if trimmed.is_empty() || trimmed == "~" {
            return Err("The whole computer or Home cannot be left out.");
        }
        if !(trimmed.starts_with('~') || trimmed.contains('/')) {
            if matches!(trimmed, "." | "..") {
                return Err("Enter a folder name, not . or ..");
            }
            return Ok(trimmed.to_owned());
        }
        let path = if let Some(rest) = trimmed.strip_prefix("~/") {
            home.join(rest.trim_start_matches('/'))
        } else if trimmed.starts_with('/') {
            PathBuf::from(trimmed)
        } else {
            return Err("A folder's path starts with / or ~/");
        };
        // Taking .. out by reading would change what a path through a link means.
        if path.components().any(|part| part == Component::ParentDir) {
            return Err("A folder's path cannot hold ..");
        }
        let path: PathBuf = path.components().collect();
        if path == Path::new("/") || path == home {
            return Err("The whole computer or Home cannot be left out.");
        }
        Ok(path.to_string_lossy().into_owned())
    }

    /// The rules among `raw` that can be; the others are left out.
    pub fn new(raw: &[String], home: &Path) -> Self {
        let mut exclusions = Self::default();
        for rule in raw {
            let Ok(rule) = Self::normalize(rule, home) else {
                continue;
            };
            if Path::new(&rule).is_absolute() {
                exclusions.folders.push(PathBuf::from(rule));
            } else {
                exclusions.names.push(rule.to_ascii_lowercase());
            }
        }
        exclusions.names.sort();
        exclusions.names.dedup();
        exclusions.folders.sort();
        exclusions.folders.dedup();
        exclusions
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && self.folders.is_empty()
    }

    /// Whether `path` (named `name`) is left out, by a rule on it or on a folder above it.
    pub fn is_excluded(&self, path: &Path, name: &str, is_folder: bool) -> bool {
        self.folders.iter().any(|folder| path.starts_with(folder))
            || (is_folder && self.names_folder(name))
            || (!self.names.is_empty()
                && path
                    .ancestors()
                    .skip(1)
                    .any(|parent| parent.file_name().is_some_and(|name| self.names_folder(&name.to_string_lossy()))))
    }

    fn names_folder(&self, name: &str) -> bool {
        self.names.iter().any(|rule| name.eq_ignore_ascii_case(rule))
    }
}

/// One entry that matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub is_folder: bool,
    pub size: u64,
    pub modified: i64,
    pub score: i64,
}

/// How a search ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// The best matches, best first.
    pub best: Vec<Found>,
    /// Entries that matched in all, kept or not.
    pub matched: usize,
    pub entries: usize,
    /// It stopped at its entry, depth or time limit before walking everything.
    pub truncated: bool,
    pub cancelled: bool,
}

/// Lower case, composed: how names and queries are compared.
pub fn fold(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    text.to_lowercase().nfc().collect()
}

/// A query's words, compiled once for every entry.
struct Query {
    terms: Vec<Matcher>,
    exclusions: Vec<Matcher>,
    text: String,
}

impl Query {
    fn parse(text: &str) -> Self {
        let folded = fold(text.trim());
        let config = Config::default().casing(CaseMatching::Respect);
        let (exclusions, terms): (Vec<_>, Vec<_>) = Pattern::parse_query(&folded)
            .into_iter()
            .partition(|pattern| pattern.negated);
        let compile = |patterns: Vec<Pattern>| -> Vec<Matcher> {
            patterns
                .into_iter()
                .map(|pattern| Matcher::new(pattern.negated(false), &config))
                .collect()
        };
        Self {
            terms: compile(terms),
            exclusions: compile(exclusions),
            text: folded,
        }
    }

    /// The score of a folded relative `path` whose name starts at `name_start`, if it matches.
    fn score(&mut self, path: &str, name_start: usize) -> Option<i64> {
        if self.terms.is_empty() {
            return None;
        }
        if self
            .exclusions
            .iter_mut()
            .any(|matcher| matcher.match_one(path, 0).is_some())
        {
            return None;
        }
        let name = &path[name_start..];
        let mut score = 0i64;
        for matcher in &mut self.terms {
            if let Some(found) = matcher.match_one(name, 0) {
                score += NAME_TERM_TIER + i64::from(found.score);
            } else {
                score += i64::from(matcher.match_one(path, 0)?.score);
            }
        }
        if name == self.text {
            score += EXACT_NAME_BONUS;
        }
        Some(score)
    }
}

/// Ranks by score, then by shorter path, then by path, so equal results come in a fixed order.
#[derive(PartialEq, Eq)]
struct Ranked(Found);

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .score
            .cmp(&other.0.score)
            .then_with(|| {
                other
                    .0
                    .path
                    .as_os_str()
                    .len()
                    .cmp(&self.0.path.as_os_str().len())
            })
            .then_with(|| other.0.path.cmp(&self.0.path))
    }
}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Searches `roots` for `query`. `found` gets each batch of new matches as the walk goes (every
/// 50 ms at most), for a client to show at once; the summary holds the best, ranked.
pub fn search(
    roots: &[PathBuf],
    query: &str,
    hidden: bool,
    limits: Limits,
    cancel: &Cancel,
    mut found: impl FnMut(Vec<Found>),
) -> Summary {
    use std::os::unix::fs::MetadataExt;
    let started = Instant::now();
    let mut query = Query::parse(query);
    let mut summary = Summary::default();
    // The best results kept so far, worst on top.
    let mut best: BinaryHeap<Reverse<Ranked>> = BinaryHeap::new();
    let mut batch = Vec::new();
    let mut sent = Instant::now();
    'roots: for root in roots {
        let mut overrides = ignore::overrides::OverrideBuilder::new(root);
        for glob in GENERATED_TREES {
            let _ = overrides.add(glob);
        }
        let walk = ignore::WalkBuilder::new(root)
            .follow_links(false)
            .standard_filters(true)
            // `standard_filters` turns hidden-entry filtering on; this sets it as asked.
            .hidden(!hidden)
            .require_git(false)
            .same_file_system(true)
            .max_depth(Some(limits.depth))
            // What the user leaves out goes, with all below it; the walk never looks inside.
            .filter_entry({
                let exclusions = limits.exclusions.clone();
                move |entry| {
                    exclusions.is_empty()
                        || entry.depth() == 0
                        || !exclusions.is_excluded(
                            entry.path(),
                            &entry.file_name().to_string_lossy(),
                            entry.file_type().is_some_and(|kind| kind.is_dir()),
                        )
                }
            })
            .overrides(
                overrides
                    .build()
                    .unwrap_or_else(|_| ignore::overrides::Override::empty()),
            )
            .build();
        for entry in walk {
            if cancel.is_cancelled() {
                summary.cancelled = true;
                break 'roots;
            }
            if summary.entries >= limits.entries || started.elapsed() >= limits.time {
                summary.truncated = true;
                break 'roots;
            }
            let Ok(entry) = entry else { continue };
            if entry.depth() == 0 {
                continue;
            }
            summary.entries += 1;
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
            let folded = fold(&relative);
            let name_start = folded.rfind('/').map_or(0, |slash| slash + 1);
            let Some(score) = query.score(&folded, name_start) else {
                continue;
            };
            summary.matched += 1;
            let meta = entry.metadata().ok();
            let hit = Found {
                path: path.to_path_buf(),
                is_folder: entry.file_type().is_some_and(|kind| kind.is_dir()),
                size: meta.as_ref().map_or(0, |meta| meta.len()),
                modified: meta.as_ref().map_or(0, |meta| meta.mtime()),
                score,
            };
            let keeps = best.len() < limits.results
                || best
                    .peek()
                    .is_some_and(|worst| Ranked(hit.clone()) > worst.0);
            if keeps {
                best.push(Reverse(Ranked(hit.clone())));
                if best.len() > limits.results {
                    best.pop();
                }
                batch.push(hit);
            }
            if !batch.is_empty() && sent.elapsed() >= Duration::from_millis(50) {
                found(std::mem::take(&mut batch));
                sent = Instant::now();
            }
        }
    }
    if !batch.is_empty() {
        found(batch);
    }
    let mut ranked: Vec<Found> = best
        .into_iter()
        .map(|Reverse(Ranked(found))| found)
        .collect();
    ranked.sort_by(|a, b| Ranked(b.clone()).cmp(&Ranked(a.clone())));
    summary.best = ranked;
    summary
}

/// Whether a name a listing holds matches a filter typed over a folder: every word is in it,
/// fuzzily, and no `!word` is.
pub struct Filter {
    query: Query,
}

impl Filter {
    pub fn new(text: &str) -> Self {
        Self {
            query: Query::parse(text),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.query.terms.is_empty() && self.query.exclusions.is_empty()
    }

    pub fn matches(&mut self, name: &str) -> bool {
        if self.query.terms.is_empty() {
            let folded = fold(name);
            return !self
                .query
                .exclusions
                .iter_mut()
                .any(|matcher| matcher.match_one(&folded, 0).is_some());
        }
        self.query.score(&fold(name), 0).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(found: &[Found], root: &Path) -> Vec<String> {
        found
            .iter()
            .map(|found| {
                found
                    .path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn names_outrank_paths_and_an_exact_name_comes_first() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        std::fs::create_dir_all(dir.join("report/old")).unwrap();
        std::fs::create_dir_all(dir.join("music")).unwrap();
        for file in [
            "report/old/notes.txt",
            "music/report-2026.pdf",
            "reports.md",
            "music/song.flac",
        ] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        let summary = search(
            &[dir.to_path_buf()],
            "report",
            false,
            Limits::default(),
            &Cancel::new(),
            |_| {},
        );
        let ranked = names(&summary.best, dir);
        assert_eq!(
            ranked.first().map(String::as_str),
            Some("report"),
            "{ranked:?}"
        );
        assert!(ranked.contains(&"music/report-2026.pdf".to_owned()));
        assert!(ranked.contains(&"reports.md".to_owned()));
        // A path-only match ranks after every name match.
        let path_only = ranked
            .iter()
            .position(|name| name == "report/old/notes.txt")
            .unwrap();
        let name_match = ranked
            .iter()
            .position(|name| name == "music/report-2026.pdf")
            .unwrap();
        assert!(name_match < path_only, "{ranked:?}");
        assert!(!ranked.contains(&"music/song.flac".to_owned()));
    }

    #[test]
    fn hidden_generated_and_ignored_trees_are_left_out() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        std::fs::create_dir_all(dir.join(".secret")).unwrap();
        std::fs::create_dir_all(dir.join("app/node_modules/pkg")).unwrap();
        std::fs::create_dir_all(dir.join("app/build")).unwrap();
        for file in [
            ".secret/plan.txt",
            "app/node_modules/pkg/plan.js",
            "app/build/plan.o",
            "app/plan.md",
        ] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        std::fs::write(dir.join("app/.gitignore"), b"build/\n").unwrap();
        let summary = search(
            &[dir.to_path_buf()],
            "plan",
            false,
            Limits::default(),
            &Cancel::new(),
            |_| {},
        );
        assert_eq!(names(&summary.best, dir), ["app/plan.md"]);
        let with_hidden = search(
            &[dir.to_path_buf()],
            "plan",
            true,
            Limits::default(),
            &Cancel::new(),
            |_| {},
        );
        assert!(names(&with_hidden.best, dir).contains(&".secret/plan.txt".to_owned()));
    }

    #[test]
    fn exclusions_limits_and_cancel() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        for index in 0..50 {
            std::fs::write(dir.join(format!("photo-{index:02}.jpg")), b"x").unwrap();
        }
        std::fs::write(dir.join("photo-draft.png"), b"x").unwrap();
        let excluded = search(
            &[dir.to_path_buf()],
            "photo !jpg",
            false,
            Limits::default(),
            &Cancel::new(),
            |_| {},
        );
        assert_eq!(names(&excluded.best, dir), ["photo-draft.png"]);

        let limits = Limits {
            results: 5,
            ..Limits::default()
        };
        let mut streamed = 0;
        let few = search(
            &[dir.to_path_buf()],
            "photo",
            false,
            limits,
            &Cancel::new(),
            |batch| streamed += batch.len(),
        );
        assert_eq!(few.best.len(), 5);
        assert_eq!(few.matched, 51);
        assert!(streamed >= 5);

        let stopped = search(
            &[dir.to_path_buf()],
            "photo",
            false,
            Limits {
                entries: 10,
                ..Limits::default()
            },
            &Cancel::new(),
            |_| {},
        );
        assert!(stopped.truncated);
        let cancel = Cancel::new();
        cancel.cancel();
        assert!(
            search(
                &[dir.to_path_buf()],
                "photo",
                false,
                Limits::default(),
                &cancel,
                |_| {}
            )
            .cancelled
        );
    }

    #[test]
    fn a_filter_matches_names_fuzzily() {
        let mut filter = Filter::new("rpt");
        assert!(filter.matches("Report.PDF"));
        assert!(!filter.matches("notes.txt"));
        let mut excluding = Filter::new("!tmp");
        assert!(excluding.matches("notes.txt"));
        assert!(!excluding.matches("notes.tmp"));
        assert!(Filter::new("  ").is_empty());
        // Accents and case fold alike.
        assert!(Filter::new("CAFÉ").matches("café.txt"));
    }

    // Legacy's exclusion tests (`services/search/exclusions/tests.rs`), with Home given.
    #[test]
    fn folder_rules_match_ancestors_but_not_same_named_files() {
        let exclusions = Exclusions::new(&["cache".into()], Path::new("/home/someone"));
        for (path, folder, excluded) in [
            ("/project/cache", true, true),
            ("/other/CACHE", true, true),
            ("/project/cache/nested", true, true),
            ("/project/cache/nested/file.py", false, true),
            ("/project/cache", false, false),
            ("/project/caches", true, false),
            ("/project/file.py", false, false),
        ] {
            let path = Path::new(path);
            let name = path.file_name().unwrap().to_str().unwrap();
            assert_eq!(exclusions.is_excluded(path, name, folder), excluded, "{path:?}");
        }
    }

    #[test]
    fn folder_paths_take_home_and_keep_to_whole_names_and_case() {
        let home = Path::new("/home/someone");
        let exclusions = Exclusions::new(&["~/Secret/./nested//".into()], home);
        for (below, excluded) in [
            ("Secret/nested", true),
            ("Secret/nested/file.txt", true),
            ("Secret/nested-other/file.txt", false),
            ("Other/Secret/nested", false),
            ("secret/nested/file.txt", false),
        ] {
            let path = home.join(below);
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            assert_eq!(exclusions.is_excluded(&path, &name, true), excluded, "{below}");
        }
    }

    #[test]
    fn the_same_rule_written_twice_is_one_and_bad_rules_are_left_out() {
        let home = Path::new("/home/someone");
        let exclusions = Exclusions::new(
            &[
                " Cache/ ".into(),
                "cache".into(),
                "~/Secret//./nested".into(),
                "/home/someone/Secret/nested".into(),
                "relative/sub".into(),
                "~someone".into(),
                "/".into(),
                "~".into(),
                "/var/../".into(),
                ".".into(),
                "..".into(),
                "bad\0name".into(),
                "/home/someone".into(),
            ],
            home,
        );
        assert_eq!(exclusions, Exclusions::new(&["cache".into(), "~/Secret/nested".into()], home));
        assert!(!exclusions.is_excluded(Path::new("/var/other"), "other", true));
        assert_eq!(Exclusions::normalize("~", home), Err("The whole computer or Home cannot be left out."));
        assert_eq!(Exclusions::normalize(" Build/ ", home), Ok("Build".to_owned()));
    }

    #[test]
    fn a_search_never_walks_what_is_left_out() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        for folder in ["work/Backups/old", "work/notes", "private"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        for file in ["work/Backups/old/plan.txt", "work/notes/plan.txt", "private/plan.txt", "work/backups"] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        let limits = Limits {
            exclusions: Exclusions::new(
                &["backups".into(), dir.join("private").to_string_lossy().into_owned()],
                Path::new("/home/someone"),
            ),
            ..Limits::default()
        };
        let summary = search(&[dir.to_path_buf()], "plan", false, limits.clone(), &Cancel::new(), |_| {});
        assert_eq!(names(&summary.best, dir), ["work/notes/plan.txt"]);
        // A file named as a folder rule stays.
        let backups = search(&[dir.to_path_buf()], "backups", false, limits, &Cancel::new(), |_| {});
        assert_eq!(names(&backups.best, dir), ["work/backups"]);
    }
}
