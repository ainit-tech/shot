//! Documentation versions. Every commit on the main line that changes the docs is a version.
//! A milestone is a Git tag `vX.Y.Z` set by hand; every other commit takes the next patch
//! number after the version before it and needs no tag: after `v0.3.0` come `v0.3.1`, `v0.3.2`.

use std::collections::{HashMap, HashSet};

use crate::git::LineCommit;

/// A version of the docs, before it is built.
#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    pub id: String,
    /// Full hash of the commit.
    pub hash: String,
    /// Short hash of the commit.
    pub commit: String,
    pub date: String,
    /// The number comes from a tag, not from counting.
    pub tagged: bool,
}

/// Numbers the versions along the main line, oldest first. `tags` maps a commit's full hash
/// to the version tags on it.
pub fn number(line: &[LineCommit], tags: &HashMap<String, Vec<String>>) -> Vec<Planned> {
    let tag_names: HashSet<&str> = tags.values().flatten().map(String::as_str).collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut current: Option<[u64; 3]> = None;
    let mut versions = Vec::new();
    for commit in line {
        let tag = tags.get(&commit.hash).and_then(|names| names.iter().filter_map(|n| parse(n).map(|v| (v, n))).max());
        let (id, tagged) = if let Some((number, name)) = tag {
            current = Some(number);
            (name.clone(), true)
        } else if commit.touches {
            let mut number = current.map_or([0, 0, 1], |[major, minor, patch]| [major, minor, patch + 1]);
            // A counted number never takes a name that a tag or an earlier version already has.
            while tag_names.contains(name(number).as_str()) || used.contains(&name(number)) {
                number[2] += 1;
            }
            current = Some(number);
            (name(number), false)
        } else {
            continue;
        };
        used.insert(id.clone());
        versions.push(Planned {
            id,
            hash: commit.hash.clone(),
            commit: commit.short.clone(),
            date: commit.date.clone(),
            tagged,
        });
    }
    versions
}

fn name([major, minor, patch]: [u64; 3]) -> String {
    format!("v{major}.{minor}.{patch}")
}

/// `v1.2.3` → `[1, 2, 3]`; `None` for any other tag.
fn parse(tag: &str) -> Option<[u64; 3]> {
    let mut parts = tag.strip_prefix('v')?.split('.').map(|part| part.parse::<u64>().ok());
    let number = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, touches: bool) -> LineCommit {
        LineCommit { hash: hash.into(), short: hash.into(), date: "2026-10-05".into(), touches }
    }

    #[test]
    fn commits_that_change_the_docs_count_patches_after_tags() {
        let line = [
            commit("c1", true),
            commit("c2", true),
            commit("c3", true),
            commit("c4", true),
            commit("c5", false),
            commit("c6", true),
            commit("c7", false),
            commit("c8", true),
        ];
        let tags: HashMap<String, Vec<String>> =
            [("c3".to_string(), vec!["v0.1.0".to_string()]), ("c7".to_string(), vec!["v0.2.0".to_string()])].into();
        let ids: Vec<(String, bool)> = number(&line, &tags).into_iter().map(|v| (v.id, v.tagged)).collect();
        let expected = [
            ("v0.0.1", false),
            ("v0.0.2", false),
            ("v0.1.0", true),
            ("v0.1.1", false),
            ("v0.1.2", false),
            ("v0.2.0", true),
            ("v0.2.1", false),
        ];
        assert_eq!(ids, expected.map(|(id, tagged)| (id.to_string(), tagged)));
    }

    #[test]
    fn counted_numbers_skip_names_of_tags() {
        let line = [commit("c1", true), commit("c2", true), commit("c3", true)];
        let tags: HashMap<String, Vec<String>> =
            [("c1".to_string(), vec!["v0.3.0".to_string()]), ("c3".to_string(), vec!["v0.3.1".to_string()])].into();
        let ids: Vec<String> = number(&line, &tags).into_iter().map(|v| v.id).collect();
        assert_eq!(ids, ["v0.3.0", "v0.3.2", "v0.3.1"]);
    }

    #[test]
    fn only_plain_version_tags_count() {
        assert_eq!(parse("v0.3.10"), Some([0, 3, 10]));
        assert_eq!(parse("v1.0"), None);
        assert_eq!(parse("v1.0.0-rc1"), None);
        assert_eq!(parse("release-1"), None);
    }
}
