//! The term base from `docs/glossary.toml`. The glossary page takes its term cards and term base
//! table from here, and every page gets hints over the terms in its text.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::site::Language;

/// Markers a page puts where the generated parts go, as `<!-- glossary: terms -->`.
pub const MARKERS: [&str; 2] = ["term base", "terms"];

/// How a term card starts in the Markdown rendered for the site; followed by the term id.
pub const CARD_START: &str = "<div class=\"term-card\" data-term=\"";

#[derive(Deserialize)]
pub struct Glossary {
    /// Words of a card's meta line, per language.
    pub labels: BTreeMap<String, Labels>,
    /// Status id → its name per language.
    pub statuses: BTreeMap<String, BTreeMap<String, String>>,
    /// Groups of terms, in display order.
    pub sections: Vec<Section>,
    pub terms: Vec<Term>,
}

#[derive(Deserialize)]
pub struct Labels {
    pub code: String,
    pub kind: String,
    pub status: String,
    /// Joins the last two code names: "`managing` and `managed`".
    pub and: String,
}

#[derive(Deserialize)]
pub struct Section {
    pub id: String,
    pub title: BTreeMap<String, String>,
    /// A line under the title, per language.
    #[serde(default)]
    pub intro: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub struct Term {
    pub id: String,
    pub section: String,
    /// Code names; most terms have one.
    pub code: Vec<String>,
    pub status: String,
    /// Language → the term in that language.
    #[serde(flatten)]
    pub text: BTreeMap<String, TermText>,
}

#[derive(Deserialize)]
pub struct TermText {
    pub name: String,
    pub kind: String,
    /// The definition: one paragraph of Markdown.
    pub text: String,
    /// Bullet points under the definition: the pair, what not to confuse the term with, why it is open.
    #[serde(default)]
    pub notes: Vec<String>,
    /// Word forms that mark the term in page texts; letter case does not matter.
    pub forms: Vec<String>,
    /// Phrases in which a form means something else, such as "guest agent".
    #[serde(default)]
    pub avoid: Vec<String>,
    /// The status in this language when it differs from the term's: a word may still be chosen.
    #[serde(default)]
    pub status: Option<String>,
}

impl Glossary {
    /// Reads `docs/glossary.toml`; `None` if there is no such file, as in older versions.
    pub fn load(path: &Path) -> Result<Option<Glossary>> {
        if !path.is_file() {
            return Ok(None);
        }
        let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let glossary = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(glossary))
    }

    /// The term's status id and its name in `lang`.
    pub fn status(&self, term: &Term, lang: &str) -> (String, String) {
        let id = term.text.get(lang).and_then(|t| t.status.clone()).unwrap_or_else(|| term.status.clone());
        let name = self.statuses.get(&id).and_then(|names| names.get(lang)).cloned().unwrap_or_else(|| id.clone());
        (id, name)
    }

    /// What is wrong in the file, as messages.
    pub fn problems(&self, languages: &[Language]) -> Vec<String> {
        let mut problems = Vec::new();
        let codes: Vec<&str> = languages.iter().map(|l| l.code.as_str()).collect();

        for &lang in &codes {
            if !self.labels.contains_key(lang) {
                problems.push(format!("no `labels.{lang}`"));
            }
            for (status, names) in &self.statuses {
                if !names.contains_key(lang) {
                    problems.push(format!("status `{status}` has no `{lang}` name"));
                }
            }
            for section in &self.sections {
                if !section.title.contains_key(lang) {
                    problems.push(format!("section `{}` has no `{lang}` title", section.id));
                }
            }
        }

        let mut ids = BTreeSet::new();
        let mut owners: BTreeMap<(&str, String), &str> = BTreeMap::new();
        for term in &self.terms {
            let id = term.id.as_str();
            if !ids.insert(id) {
                problems.push(format!("term `{id}` is defined twice"));
            }
            if !self.sections.iter().any(|s| s.id == term.section) {
                problems.push(format!("term `{id}`: unknown section `{}`", term.section));
            }
            if term.code.is_empty() {
                problems.push(format!("term `{id}` has no code name"));
            }
            for lang in term.text.keys().filter(|l| !codes.contains(&l.as_str())) {
                problems.push(format!("term `{id}`: `{lang}` is not a language of the site"));
            }
            for &lang in &codes {
                let Some(text) = term.text.get(lang) else {
                    problems.push(format!("term `{id}` has no `{lang}` text"));
                    continue;
                };
                for (field, value) in [("name", &text.name), ("kind", &text.kind), ("text", &text.text)] {
                    if value.trim().is_empty() {
                        problems.push(format!("term `{id}`: `{lang}.{field}` is empty"));
                    }
                }
                if text.forms.is_empty() {
                    problems.push(format!("term `{id}`: `{lang}.forms` is empty"));
                }
                for form in &text.forms {
                    if let Some(other) = owners.insert((lang, form.to_lowercase()), id) {
                        problems.push(format!("form `{form}` ({lang}) belongs to both `{other}` and `{id}`"));
                    }
                }
            }
            for status in term.text.values().filter_map(|t| t.status.as_ref()).chain([&term.status]) {
                if !self.statuses.contains_key(status) {
                    problems.push(format!("term `{id}`: unknown status `{status}`"));
                }
            }
        }
        problems
    }

    /// Replaces the markers in a page's Markdown with generated Markdown and returns the markers
    /// found. With `html`, every card gets a wrapper and a meta line in HTML that the site styles;
    /// without it, the Markdown stays plain, for the sources published to AI agents.
    pub fn expand(&self, markdown: &str, lang: &str, languages: &[Language], html: bool) -> (String, Vec<&'static str>) {
        let mut out = markdown.to_string();
        let mut found = Vec::new();
        for marker in MARKERS {
            let tag = format!("<!-- glossary: {marker} -->");
            if !out.contains(&tag) {
                continue;
            }
            found.push(marker);
            let generated = match marker {
                "term base" => self.term_base(lang, languages),
                _ => self.cards(lang, html),
            };
            out = out.replace(&tag, generated.trim_end());
        }
        (out, found)
    }

    fn term_base(&self, lang: &str, languages: &[Language]) -> String {
        let code = self.labels.get(lang).map_or("Code", |l| l.code.as_str());
        let names: Vec<&str> = languages.iter().map(|l| l.name.as_str()).collect();
        let mut out = format!("| {} | {code} |\n|{}---|\n", names.join(" | "), "---|".repeat(names.len()));
        for term in &self.terms {
            let cells: Vec<&str> =
                languages.iter().map(|l| term.text.get(&l.code).map_or("", |t| t.name.as_str())).collect();
            let codes: Vec<String> = term.code.iter().map(|c| format!("`{c}`")).collect();
            out.push_str(&format!("| {} | {} |\n", cells.join(" | "), codes.join(", ")));
        }
        out
    }

    fn cards(&self, lang: &str, html: bool) -> String {
        let sections: Vec<String> = self
            .sections
            .iter()
            .map(|section| {
                let mut out = format!("## {}\n\n", section.title.get(lang).map_or("", String::as_str));
                if let Some(intro) = section.intro.get(lang).filter(|i| !i.is_empty()) {
                    out.push_str(&format!("{intro}\n\n"));
                }
                for term in self.terms.iter().filter(|t| t.section == section.id) {
                    out.push_str(&self.card(term, lang, html));
                }
                out
            })
            .collect();
        sections.join("---\n\n")
    }

    fn card(&self, term: &Term, lang: &str, html: bool) -> String {
        let Some(text) = term.text.get(lang) else { return String::new() };
        let labels = self.labels.get(lang);
        let label = |pick: fn(&Labels) -> &str, fallback: &'static str| labels.map_or(fallback, pick).to_string();
        let (code, kind, status_label, and) = (
            label(|l| l.code.as_str(), "Code"),
            label(|l| l.kind.as_str(), "Kind"),
            label(|l| l.status.as_str(), "Status"),
            label(|l| l.and.as_str(), "and"),
        );
        let (status, status_name) = self.status(term, lang);
        let codes = |wrap: &dyn Fn(&str) -> String| join_names(&term.code.iter().map(|c| wrap(c)).collect::<Vec<_>>(), &and);

        let mut out = String::new();
        if html {
            out.push_str(&format!("{CARD_START}{}\">\n\n", escape(&term.id)));
        }
        out.push_str(&format!("### {}\n\n", text.name));
        if html {
            out.push_str(&format!(
                "<p class=\"term-meta\"><span>{}: {}</span><span>{}: {}</span><span>{}: <span class=\"status status-{}\">{}</span></span></p>\n\n",
                escape(&code),
                codes(&|c| format!("<code>{}</code>", escape(c))),
                escape(&kind),
                escape(&text.kind),
                escape(&status_label),
                escape(&status),
                escape(&status_name),
            ));
        } else {
            out.push_str(&format!(
                "{code}: {} · {kind}: {} · {status_label}: {status_name}\n\n",
                codes(&|c| format!("`{c}`")),
                text.kind,
            ));
        }
        out.push_str(&format!("{}\n\n", text.text));
        if !text.notes.is_empty() {
            for note in &text.notes {
                out.push_str(&format!("- {note}\n"));
            }
            out.push('\n');
        }
        if html {
            out.push_str("</div>\n\n");
        }
        out
    }
}

/// "A", "A and B", "A, B and C".
fn join_names(names: &[String], and: &str) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} {and} {last}", rest.join(", ")),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Finds the terms of one language in text.
pub struct Matcher {
    ids: Vec<String>,
    /// Forms in lower case, longest first, with the index of their term.
    forms: Vec<(Vec<char>, usize)>,
    avoid: Vec<Vec<char>>,
}

/// A term found in text, as byte offsets.
#[derive(Debug, PartialEq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub term: usize,
}

impl Matcher {
    pub fn new(glossary: &Glossary, lang: &str) -> Matcher {
        let lowered = |s: &str| s.chars().map(lower).collect::<Vec<char>>();
        let mut forms = Vec::new();
        let mut avoid = Vec::new();
        for (index, term) in glossary.terms.iter().enumerate() {
            if let Some(text) = term.text.get(lang) {
                forms.extend(text.forms.iter().map(|f| (lowered(f), index)));
                avoid.extend(text.avoid.iter().map(|a| lowered(a)));
            }
        }
        forms.sort_by_key(|(form, _)| std::cmp::Reverse(form.len()));
        Matcher { ids: glossary.terms.iter().map(|t| t.id.clone()).collect(), forms, avoid }
    }

    pub fn id(&self, term: usize) -> &str {
        &self.ids[term]
    }

    pub fn term_index(&self, id: &str) -> Option<usize> {
        self.ids.iter().position(|i| i == id)
    }

    /// Whole-word occurrences of term forms, left to right, the longest form winning.
    pub fn find(&self, text: &str) -> Vec<Found> {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let lowered: Vec<char> = chars.iter().map(|&(_, c)| lower(c)).collect();
        let n = lowered.len();
        let matches_at = |i: usize, word: &[char]| {
            i + word.len() <= n
                && lowered[i..i + word.len()] == *word
                && (i == 0 || !is_word(lowered[i - 1]))
                && (i + word.len() == n || !is_word(lowered[i + word.len()]))
        };

        let mut blocked = vec![false; n];
        for phrase in &self.avoid {
            for i in 0..n {
                if matches_at(i, phrase) {
                    blocked[i..i + phrase.len()].fill(true);
                }
            }
        }

        let byte = |i: usize| if i == n { text.len() } else { chars[i].0 };
        let mut found = Vec::new();
        let mut i = 0;
        while i < n {
            let hit = self
                .forms
                .iter()
                .find(|(form, _)| matches_at(i, form) && !blocked[i..i + form.len()].contains(&true));
            match hit {
                Some((form, term)) => {
                    found.push(Found { start: byte(i), end: byte(i + form.len()), term: *term });
                    i += form.len();
                }
                None => i += 1,
            }
        }
        found
    }
}

fn lower(c: char) -> char {
    let mut lowered = c.to_lowercase();
    match (lowered.next(), lowered.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Letters, digits, the marks of Devanagari and joiners belong to words, and so do `-` and `_`:
/// "qemu-guest-agent" is one word, not "agent". The danda `।` and `॥` end sentences.
fn is_word(c: char) -> bool {
    if matches!(c, '\u{0964}' | '\u{0965}') {
        return false;
    }
    c.is_alphanumeric() || matches!(c, '-' | '_' | '\u{0900}'..='\u{097F}' | '\u{200C}' | '\u{200D}')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[labels.en]
code = "Code"
kind = "Kind"
status = "Status"
and = "and"

[statuses]
proposed = { en = "proposed" }
open = { en = "open" }

[[sections]]
id = "apps"
title = { en = "1. Applications" }
intro = { en = "What runs." }

[[terms]]
id = "agent"
section = "apps"
code = ["Agent"]
status = "proposed"

[terms.en]
name = "Agent"
kind = "application"
text = "Carries out operations."
notes = ["Pair: a perimeter — its own agent."]
forms = ["agent", "agents"]
avoid = ["guest agent"]

[[terms]]
id = "control-server"
section = "apps"
code = ["ControlServer", "Server"]
status = "open"

[terms.en]
name = "Control server"
kind = "application"
text = "Decides what to do."
forms = ["control server", "server"]
"#;

    fn sample() -> Glossary {
        toml::from_str(SAMPLE).unwrap()
    }

    fn languages() -> Vec<Language> {
        vec![Language { code: "en".into(), name: "English".into() }]
    }

    fn words(text: &str) -> Vec<(String, String)> {
        let glossary = sample();
        let matcher = Matcher::new(&glossary, "en");
        matcher
            .find(text)
            .into_iter()
            .map(|f| (text[f.start..f.end].to_string(), matcher.id(f.term).to_string()))
            .collect()
    }

    #[test]
    fn finds_whole_words_ignoring_case() {
        assert_eq!(
            words("Agents talk to the Control Server; an agent's key."),
            vec![
                ("Agents".into(), "agent".into()),
                ("Control Server".into(), "control-server".into()),
                ("agent".into(), "agent".into()),
            ]
        );
        assert!(words("agency, reagent, qemu-guest-agent").is_empty());
    }

    #[test]
    fn a_danda_ends_a_word() {
        let mut glossary = sample();
        glossary.terms[0].text.get_mut("en").unwrap().forms.push("एजेंट".into());
        let matcher = Matcher::new(&glossary, "en");
        let text = "यह एजेंट।";
        let found = matcher.find(text);
        assert_eq!(found.len(), 1);
        assert_eq!(&text[found[0].start..found[0].end], "एजेंट");
    }

    #[test]
    fn prefers_the_longest_form_and_skips_avoided_phrases() {
        assert_eq!(words("the control server"), vec![("control server".into(), "control-server".into())]);
        assert_eq!(words("a guest agent and an agent"), vec![("agent".into(), "agent".into())]);
    }

    #[test]
    fn expands_markers_into_cards_and_a_table() {
        let glossary = sample();
        let page = "# Glossary\n\n<!-- glossary: term base -->\n\n<!-- glossary: terms -->\n";
        let (plain, markers) = glossary.expand(page, "en", &languages(), false);
        assert_eq!(markers, vec!["term base", "terms"]);
        assert!(plain.contains("| English | Code |\n|---|---|\n| Agent | `Agent` |"));
        assert!(plain.contains("## 1. Applications\n\nWhat runs.\n\n### Agent\n\nCode: `Agent` · Kind: application · Status: proposed"));
        assert!(plain.contains("Code: `ControlServer` and `Server` · Kind: application · Status: open"));
        assert!(!plain.contains("<div"));

        let (html, _) = glossary.expand(page, "en", &languages(), true);
        assert!(html.contains("<div class=\"term-card\" data-term=\"agent\">\n\n### Agent"));
        assert!(html.contains("<span class=\"status status-proposed\">proposed</span>"));
    }

    #[test]
    fn reports_problems() {
        let mut glossary = sample();
        glossary.terms[1].text.get_mut("en").unwrap().forms.push("agents".into());
        glossary.terms[1].status = "unknown".into();
        let problems = glossary.problems(&languages());
        assert!(problems.iter().any(|p| p.contains("belongs to both `agent` and `control-server`")));
        assert!(problems.iter().any(|p| p.contains("unknown status `unknown`")));
    }
}
