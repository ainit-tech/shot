//! Page history and documentation versions from Git. Outside a Git repository the docs
//! still build: pages simply have no history and there is only the latest version.
//!
//! History and changes are read with one `git` call for the whole docs folder, not one per
//! page: in a container that reads the repository from the host, every call is slow.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Commit {
    /// Short hash.
    pub commit: String,
    /// Author date, `YYYY-MM-DD`.
    pub date: String,
    /// Author name. E-mail addresses are not published.
    pub author: String,
    pub subject: String,
}

/// A commit on the main line of history, the one `--first-parent` follows.
#[derive(Clone, Debug, PartialEq)]
pub struct LineCommit {
    pub hash: String,
    pub short: String,
    /// Author date, `YYYY-MM-DD`.
    pub date: String,
    /// The commit changed the folder that was asked about.
    pub touches: bool,
}

/// Field separator in `git log` output: the ASCII unit separator.
const SEPARATOR: char = '\u{1f}';

/// Starts every commit in the output of [`histories`]: the ASCII record separator.
const RECORD: char = '\u{1e}';

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    output.status.success().then_some(output.stdout)
}

fn git_text(root: &Path, args: &[&str]) -> Option<String> {
    git(root, args).map(|out| String::from_utf8_lossy(&out).into_owned())
}

/// Short hash and date of the commit `rev` points to.
pub fn commit_of(root: &Path, rev: &str) -> Option<(String, String)> {
    let text = git_text(root, &["log", "-1", "--format=%h%x1f%as", rev])?;
    let (commit, date) = text.trim().split_once(SEPARATOR)?;
    Some((commit.to_string(), date.to_string()))
}

/// The commits of the main line up to `HEAD`, oldest first, each marked whether it changed `dir`.
pub fn main_line(root: &Path, dir: &str) -> Vec<LineCommit> {
    let format = "--format=%H%x1f%h%x1f%as";
    let Some(all) = git_text(root, &["log", "--first-parent", "--reverse", format, "HEAD"]) else {
        return Vec::new();
    };
    let touched: HashSet<String> = git_text(root, &["log", "--first-parent", "--format=%H", "HEAD", "--", dir])
        .map(|text| text.lines().map(str::to_string).collect())
        .unwrap_or_default();
    parse_line(&all, &touched)
}

fn parse_line(text: &str, touched: &HashSet<String>) -> Vec<LineCommit> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split(SEPARATOR);
            let (hash, short, date) = (fields.next()?, fields.next()?, fields.next()?);
            Some(LineCommit {
                hash: hash.to_string(),
                short: short.to_string(),
                date: date.to_string(),
                touches: touched.contains(hash),
            })
        })
        .collect()
}

/// Version tags (`v*`) by the full hash of the commit they point to.
pub fn tags_by_commit(root: &Path) -> HashMap<String, Vec<String>> {
    // An annotated tag is an object of its own; `*objectname` is the commit it points to.
    let format = "--format=%(refname:short)%1f%(objectname)%1f%(*objectname)";
    let mut tags: HashMap<String, Vec<String>> = HashMap::new();
    for line in git_text(root, &["for-each-ref", format, "refs/tags/v*"]).unwrap_or_default().lines() {
        let mut fields = line.split(SEPARATOR);
        let (Some(name), Some(object)) = (fields.next(), fields.next()) else { continue };
        let commit = fields.next().filter(|peeled| !peeled.is_empty()).unwrap_or(object);
        tags.entry(commit.to_string()).or_default().push(name.to_string());
    }
    tags
}

/// For every file under `dir` (relative to the repository root): the commits up to `rev`
/// that changed it, newest first, keyed by the file's path at `rev`. Renames are followed,
/// as `git log --follow` does for a single file.
pub fn histories(root: &Path, rev: &str, dir: &str) -> HashMap<String, Vec<Commit>> {
    let format = "--format=%x1e%h%x1f%as%x1f%an%x1f%s";
    let args = ["-c", "core.quotePath=false", "log", "--name-status", "-M", format, rev, "--", dir];
    git_text(root, &args).map(|text| parse_histories(&text)).unwrap_or_default()
}

fn parse_histories(text: &str) -> HashMap<String, Vec<Commit>> {
    let mut histories: HashMap<String, Vec<Commit>> = HashMap::new();
    // The log runs from the newest commit back, so an older name of a renamed file is met
    // after its newer one. This maps every older name to the name at the newest commit.
    let mut newer_names: HashMap<String, String> = HashMap::new();
    for record in text.split(RECORD) {
        let mut lines = record.lines();
        let Some(header) = lines.next() else { continue };
        let mut fields = header.splitn(4, SEPARATOR).map(str::to_string);
        let (Some(commit), Some(date), Some(author), Some(subject)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let entry = Commit { commit, date, author, subject };
        for line in lines {
            let (path, older) = match line.split('\t').collect::<Vec<_>>()[..] {
                [status, old, new] if status.starts_with('R') => (new, Some(old)),
                [_, path] => (path, None),
                _ => continue,
            };
            let current = newer_names.get(path).cloned().unwrap_or_else(|| path.to_string());
            let commits = histories.entry(current.clone()).or_default();
            if commits.last().is_none_or(|last| last.commit != entry.commit) {
                commits.push(entry.clone());
            }
            if let Some(old) = older {
                newer_names.insert(old.to_string(), current);
            }
        }
    }
    histories
}

/// Files under `dir` with changes that are not committed yet, new files included.
pub fn uncommitted_files(root: &Path, dir: &str) -> HashSet<String> {
    let args = ["status", "--porcelain", "-z", "--untracked-files=all", "--", dir];
    git(root, &args).map(|out| parse_status(&String::from_utf8_lossy(&out))).unwrap_or_default()
}

/// Parses `git status --porcelain -z`: `XY path`, and after a rename or a copy, the old path.
fn parse_status(text: &str) -> HashSet<String> {
    let mut files = HashSet::new();
    let mut entries = text.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let Some((status, path)) = entry.split_at_checked(3) else { continue };
        files.insert(path.to_string());
        if status.contains(['R', 'C']) {
            entries.next();
        }
    }
    files
}

/// Writes the `docs` folder as it was at commit `hash` into `dest`, unless `dest` already
/// holds it: a commit's docs never change, so an export is made once.
pub fn export_docs(root: &Path, hash: &str, dest: &Path) -> Result<()> {
    let marker = dest.join(".commit");
    if fs::read_to_string(&marker).is_ok_and(|text| text.trim() == hash) {
        return Ok(());
    }
    let archive = git(root, &["archive", "--format=tar", hash, "--", "docs"])
        .with_context(|| format!("`git archive {hash}` failed"))?;
    if dest.exists() {
        fs::remove_dir_all(dest).with_context(|| format!("removing {}", dest.display()))?;
    }
    fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    tar::Archive::new(archive.as_slice())
        .unpack(dest)
        .with_context(|| format!("unpacking the docs of {hash}"))?;
    fs::write(&marker, hash).with_context(|| format!("writing {}", marker.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, subject: &str) -> Commit {
        Commit { commit: hash.into(), date: "2026-10-01".into(), author: "a".into(), subject: subject.into() }
    }

    #[test]
    fn histories_follow_renames() {
        let log = "\u{1e}c3\u{1f}2026-10-01\u{1f}a\u{1f}edit\n\nM\tdocs/en/new.md\nA\tdocs/en/other.md\n\
                   \u{1e}c2\u{1f}2026-10-01\u{1f}a\u{1f}move\n\nR100\tdocs/en/old.md\tdocs/en/new.md\n\
                   \u{1e}c1\u{1f}2026-10-01\u{1f}a\u{1f}start\n\nA\tdocs/en/old.md\n";
        let histories = parse_histories(log);
        assert_eq!(
            histories["docs/en/new.md"],
            vec![commit("c3", "edit"), commit("c2", "move"), commit("c1", "start")]
        );
        assert_eq!(histories["docs/en/other.md"], vec![commit("c3", "edit")]);
        assert!(!histories.contains_key("docs/en/old.md"));
    }

    #[test]
    fn main_line_marks_commits_that_touch_the_folder() {
        let text = "aaaa2026-09-30
bbbb2026-10-01
";
        let touched: HashSet<String> = ["bbb".to_string()].into();
        let line = parse_line(text, &touched);
        assert_eq!(line.len(), 2);
        assert!(!line[0].touches);
        assert_eq!((line[1].short.as_str(), line[1].date.as_str(), line[1].touches), ("b", "2026-10-01", true));
    }

    #[test]
    fn status_lists_changed_new_and_renamed_files() {
        let status = " M docs/en/a.md\0?? docs/en/b.md\0R  docs/en/c.md\0docs/en/old-c.md\0";
        let files = parse_status(status);
        let expected: HashSet<String> =
            ["docs/en/a.md", "docs/en/b.md", "docs/en/c.md"].into_iter().map(String::from).collect();
        assert_eq!(files, expected);
    }
}
