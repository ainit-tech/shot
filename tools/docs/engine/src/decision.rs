//! Decision records: `docs/<lang>/decisions/NNN-name.md`. Each one records a single decision:
//! the question, the options considered, why this one was chosen and what follows from it.
//! Pages that describe the system state the result and link to its record; the decisions
//! page (`decisions/README.md`) lists them all.

use crate::page::Page;
use crate::site::Topic;

/// The folder of decision records in every language; its README.md is the decisions page.
pub const FOLDER: &str = "decisions";

/// Where the list of all decisions goes on the decisions page.
pub const LIST_MARKER: &str = "<!-- decisions: list -->";

/// What the marker becomes on the site, which fills the list in from the manifest.
pub const LIST_HTML: &str = "<div class=\"decision-list\"></div>";

/// Statuses a decision record can have. A superseded decision stays as it was written;
/// the decision that replaces it lists its number in `supersedes`.
pub const STATUSES: &[&str] = &["draft", "accepted", "superseded"];

/// The number of a decision record from its page id: `decisions/007-ansible` → 7.
/// `None` for any other page, the decisions page itself included.
pub fn number(id: &str) -> Option<u32> {
    let name = id.strip_prefix(FOLDER)?.strip_prefix('/')?;
    let (digits, slug) = name.split_once('-')?;
    let named = !slug.is_empty() && slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if digits.len() != 3 || !digits.bytes().all(|b| b.is_ascii_digit()) || !named {
        return None;
    }
    digits.parse().ok()
}

/// Whether the page is inside the decisions folder; the decisions page itself is not.
pub fn in_folder(id: &str) -> bool {
    id.strip_prefix(FOLDER).is_some_and(|rest| rest.starts_with('/'))
}

/// Decision numbers from a `supersedes` value, `3` or `3, 5`; `None` if one of them is not a number.
pub fn parse_numbers(value: &str) -> Option<Vec<u32>> {
    value.split(',').map(|part| part.trim().parse().ok()).collect()
}

/// The list of decisions for the Markdown sources: one line per record, grouped by topic in the
/// order of `topics`, with records of no known topic last. The links are relative to the
/// decisions page.
pub fn list_markdown(records: &[&Page], topics: &[Topic], lang: &str) -> String {
    let line = |page: &&Page| {
        let name = page.file.strip_prefix(&format!("{FOLDER}/")).unwrap_or(&page.file);
        let title = page.title.as_deref().unwrap_or(&page.id);
        let status = page.meta.status.as_deref().unwrap_or("");
        let date = page.meta.date.as_deref().unwrap_or("");
        format!("- [{:03}. {title}]({name}) — {status}, {date}\n", page.decision.unwrap_or_default())
    };
    let mut out = String::new();
    for topic in topics {
        let group: Vec<&&Page> = records.iter().filter(|p| p.meta.topic.as_deref() == Some(topic.id.as_str())).collect();
        if group.is_empty() {
            continue;
        }
        let title = topic.title.get(lang).unwrap_or(&topic.id);
        out.push_str(&format!("**{title}**\n\n"));
        out.extend(group.into_iter().map(line));
        out.push('\n');
    }
    let known = |page: &&&Page| topics.iter().any(|t| page.meta.topic.as_deref() == Some(t.id.as_str()));
    out.extend(records.iter().filter(|p| !known(p)).map(line));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_come_from_record_names() {
        assert_eq!(number("decisions/007-ansible-for-machines"), Some(7));
        assert_eq!(number("decisions/120-x"), Some(120));
        assert_eq!(number("decisions"), None);
        assert_eq!(number("decisions/7-ansible"), None);
        assert_eq!(number("decisions/007-Ansible"), None);
        assert_eq!(number("decisions/007-"), None);
        assert_eq!(number("decisions/007"), None);
        assert_eq!(number("concept/007-ansible"), None);
    }

    #[test]
    fn records_are_inside_the_folder() {
        assert!(in_folder("decisions/007-ansible"));
        assert!(in_folder("decisions/notes"));
        assert!(!in_folder("decisions"));
        assert!(!in_folder("decisions-old/x"));
    }

    #[test]
    fn the_list_is_grouped_by_topic() {
        let page = |file: &str, topic: Option<&str>| {
            let raw = format!(
                "---\nstatus: accepted\ndate: 2026-10-01\n{}---\n\n# {file}\n\nSummary.\n",
                topic.map(|t| format!("topic: {t}\n")).unwrap_or_default()
            );
            crate::page::parse_for_test("en", &format!("decisions/{file}.md"), &raw)
        };
        let a = page("001-a", Some("agents"));
        let b = page("002-b", Some("trust"));
        let c = page("003-c", Some("agents"));
        let d = page("004-d", None);
        let topic = |id: &str, title: &str| Topic { id: id.into(), title: [("en".to_string(), title.to_string())].into() };
        let topics = [topic("trust", "Trust"), topic("agents", "Agents")];
        let list = list_markdown(&[&a, &b, &c, &d], &topics, "en");
        assert_eq!(
            list,
            "**Trust**\n\n- [002. 002-b](002-b.md) — accepted, 2026-10-01\n\n\
             **Agents**\n\n- [001. 001-a](001-a.md) — accepted, 2026-10-01\n- [003. 003-c](003-c.md) — accepted, 2026-10-01\n\n\
             - [004. 004-d](004-d.md) — accepted, 2026-10-01\n"
        );
    }

    #[test]
    fn supersedes_lists_numbers() {
        assert_eq!(parse_numbers("3"), Some(vec![3]));
        assert_eq!(parse_numbers("3, 5"), Some(vec![3, 5]));
        assert_eq!(parse_numbers("three"), None);
        assert_eq!(parse_numbers(""), None);
    }
}
