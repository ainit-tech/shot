//! Checks of the documentation sources. Errors stop the build; warnings do not.

use std::collections::{BTreeMap, BTreeSet};

use crate::decision;
use crate::glossary::MARKERS;
use crate::page::{Docs, Page, Target};

pub struct Problem {
    pub error: bool,
    /// Where the problem is: a file inside `docs/`.
    pub place: String,
    pub message: String,
}

const STATUSES: &[&str] = &["draft", "accepted", "partial", "living", "superseded"];

pub fn run(docs: &Docs) -> Vec<Problem> {
    let mut problems = Vec::new();
    check_languages(docs, &mut problems);
    check_translations_exist(docs, &mut problems);
    for page in &docs.pages {
        check_front_matter(docs, page, &mut problems);
        check_links(docs, page, &mut problems);
    }
    check_nav(docs, &mut problems);
    check_topics(docs, &mut problems);
    check_glossary(docs, &mut problems);
    check_decisions(docs, &mut problems);
    problems
}

fn error(place: impl Into<String>, message: impl Into<String>) -> Problem {
    Problem { error: true, place: place.into(), message: message.into() }
}

fn warning(place: impl Into<String>, message: impl Into<String>) -> Problem {
    Problem { error: false, place: place.into(), message: message.into() }
}

fn check_languages(docs: &Docs, problems: &mut Vec<Problem>) {
    let default = &docs.site.default_language;
    if !docs.site.languages.iter().any(|l| &l.code == default) {
        problems.push(error("site.toml", format!("default language `{default}` is not in `languages`")));
    }
}

/// Every file of the default language must exist in every other language, and nothing more.
fn check_translations_exist(docs: &Docs, problems: &mut Vec<Problem>) {
    let default = &docs.site.default_language;
    let originals = files(docs, default);
    for language in docs.site.languages.iter().filter(|l| &l.code != default) {
        let lang = &language.code;
        let translations = files(docs, lang);
        for file in originals.difference(&translations) {
            problems.push(error(format!("{lang}/{file}"), format!("missing translation of {default}/{file}")));
        }
        for file in translations.difference(&originals) {
            problems.push(error(format!("{lang}/{file}"), format!("there is no {default}/{file} to translate from")));
        }
    }
}

fn files<'a>(docs: &'a Docs, lang: &str) -> BTreeSet<&'a str> {
    let pages = docs.pages.iter().filter(|p| p.lang == lang).map(|p| p.file.as_str());
    let assets = docs.assets.iter().filter(|a| a.lang == lang).map(|a| a.file.as_str());
    pages.chain(assets).collect()
}

fn check_front_matter(docs: &Docs, page: &Page, problems: &mut Vec<Problem>) {
    let place = page.place();
    let meta = &page.meta;
    for key in &meta.unknown {
        problems.push(error(&place, format!("unknown front matter key `{key}`")));
    }
    if page.title.is_none() {
        problems.push(error(&place, "the page has no `# Title` heading"));
    }
    if let Some(status) = &meta.status
        && !STATUSES.contains(&status.as_str())
    {
        problems.push(error(&place, format!("unknown status `{status}`, expected one of {}", STATUSES.join(", "))));
    }
    check_decision_meta(docs, page, problems);

    let default = &docs.site.default_language;
    if &page.lang == default {
        if meta.translation_of.is_some() || meta.updated.is_some() {
            problems.push(error(&place, "`translation_of` and `updated` are only for translations"));
        }
        return;
    }
    if meta.translation_of.as_ref() != Some(default) {
        problems.push(error(&place, format!("a translation needs `translation_of: {default}`")));
    }
    if !meta.updated.as_deref().is_some_and(is_date) {
        problems.push(error(&place, "a translation needs `updated: YYYY-MM-DD`"));
    }
    let Some(original) = docs.page(default, &page.id) else { return };
    let fields = [
        ("status", &original.meta.status, &meta.status),
        ("date", &original.meta.date, &meta.date),
        ("supersedes", &original.meta.supersedes, &meta.supersedes),
        ("topic", &original.meta.topic, &meta.topic),
    ];
    for (key, theirs, ours) in fields {
        if theirs != ours {
            problems.push(error(
                &place,
                format!(
                    "`{key}` is `{}`, but the original's is `{}`",
                    ours.as_deref().unwrap_or("none"),
                    theirs.as_deref().unwrap_or("none")
                ),
            ));
        }
    }
}

/// A decision record has a status of its own, a date and a summary under the title; other pages
/// have none of these.
fn check_decision_meta(docs: &Docs, page: &Page, problems: &mut Vec<Problem>) {
    let place = page.place();
    let meta = &page.meta;
    if page.decision.is_none() {
        if meta.status.as_deref() == Some("superseded") {
            problems.push(error(&place, "only a decision record can be `superseded`"));
        }
        if meta.date.is_some() || meta.supersedes.is_some() || meta.topic.is_some() {
            problems.push(error(&place, "`date`, `supersedes` and `topic` are only for decision records"));
        }
        return;
    }
    if !meta.status.as_deref().is_some_and(|s| decision::STATUSES.contains(&s)) {
        problems.push(error(
            &place,
            format!("a decision record needs a status: {}", decision::STATUSES.join(", ")),
        ));
    }
    if !meta.date.as_deref().is_some_and(is_date) {
        problems.push(error(&place, "a decision record needs `date: YYYY-MM-DD`: when it was decided or proposed"));
    }
    if let Some(value) = &meta.supersedes
        && decision::parse_numbers(value).is_none()
    {
        problems.push(error(&place, format!("`supersedes` lists decision numbers, such as `3` or `3, 5`, not `{value}`")));
    }
    if page.summary.is_none() {
        problems.push(error(&place, "a decision record starts with a paragraph right under the title that states the decision"));
    }
    let topics: Vec<&str> = docs.site.decision_topics.iter().map(|t| t.id.as_str()).collect();
    if !topics.is_empty() {
        match meta.topic.as_deref() {
            None => problems.push(error(&place, format!("a decision record needs a `topic`: {}", topics.join(", ")))),
            Some(topic) if !topics.contains(&topic) => {
                problems.push(error(&place, format!("unknown topic `{topic}`, expected one of {}", topics.join(", "))))
            }
            Some(_) => {}
        }
    }
}

fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| if i == 4 || i == 7 { *b == b'-' } else { b.is_ascii_digit() })
}

fn check_links(docs: &Docs, page: &Page, problems: &mut Vec<Problem>) {
    let place = page.place();
    for link in &page.links {
        let broken = match &link.resolved {
            Target::External => None,
            Target::Outside => Some("it leads outside the language folders".to_string()),
            Target::SamePage { fragment } => {
                (!page.has_heading(fragment)).then(|| format!("there is no heading `#{fragment}` on this page"))
            }
            Target::Page { lang, id, fragment } => match docs.page(lang, id) {
                None => Some("the page does not exist".to_string()),
                Some(target) => fragment
                    .as_ref()
                    .filter(|f| !target.has_heading(f))
                    .map(|f| format!("there is no heading `#{f}` on {}", target.place())),
            },
            Target::Asset { path } => {
                (!docs.root.join("docs").join(path).is_file()).then(|| "the file does not exist".to_string())
            }
        };
        if let Some(reason) = broken {
            problems.push(error(&place, format!("broken link `{}`: {reason}", link.target)));
        }

        let target_lang = match &link.resolved {
            Target::Page { lang, .. } => Some(lang.as_str()),
            Target::Asset { path } => path.split('/').next(),
            _ => None,
        };
        if target_lang.is_some_and(|lang| lang != page.lang) {
            problems.push(warning(&place, format!("link `{}` points to another language", link.target)));
        }
    }
}

fn check_nav(docs: &Docs, problems: &mut Vec<Problem>) {
    let mut in_nav = BTreeSet::new();
    for section in &docs.site.nav {
        if let (Some(index), Some(_)) = (&section.index, &section.title) {
            problems.push(error(
                "site.toml",
                format!("menu section `{index}` has both `index` and `title`; the index page gives the title"),
            ));
        }
        if let Some(titles) = &section.title {
            for language in &docs.site.languages {
                if !titles.contains_key(&language.code) {
                    problems.push(error("site.toml", format!("a menu section has no `{}` title", language.code)));
                }
            }
        }
        for id in section.all_pages() {
            if !in_nav.insert(id.as_str()) {
                problems.push(error("site.toml", format!("page `{id}` is in the menu twice")));
            }
            for language in &docs.site.languages {
                if docs.page(&language.code, id).is_none() {
                    problems.push(error("site.toml", format!("menu page `{id}` does not exist in `{}`", language.code)));
                }
            }
        }
    }
    // Decision records are listed on the decisions page instead of the menu.
    let records_listed = in_nav.contains(decision::FOLDER);
    for page in docs.pages.iter().filter(|p| p.lang == docs.site.default_language) {
        if page.decision.is_some() && records_listed {
            continue;
        }
        if !in_nav.contains(page.id.as_str()) {
            problems.push(warning(page.place(), "the page is not in the menu (docs/site.toml)"));
        }
    }
}

/// Every topic of decision records has its own id and a title in every language.
fn check_topics(docs: &Docs, problems: &mut Vec<Problem>) {
    let mut ids = BTreeSet::new();
    for topic in &docs.site.decision_topics {
        if !ids.insert(topic.id.as_str()) {
            problems.push(error("site.toml", format!("decision topic `{}` is listed twice", topic.id)));
        }
        for language in &docs.site.languages {
            if !topic.title.contains_key(&language.code) {
                problems.push(error("site.toml", format!("decision topic `{}` has no `{}` title", topic.id, language.code)));
            }
        }
    }
}

/// The term base is complete in every language, and every glossary page shows it.
fn check_glossary(docs: &Docs, problems: &mut Vec<Problem>) {
    let Some(glossary) = &docs.glossary else { return };
    for message in glossary.problems(&docs.site.languages) {
        problems.push(error("glossary.toml", message));
    }
    for language in &docs.site.languages {
        let Some(page) = docs.page(&language.code, "glossary") else { continue };
        for marker in MARKERS.iter().filter(|m| !page.glossary_markers.contains(m)) {
            problems.push(error(page.place(), format!("no `<!-- glossary: {marker} -->` marker")));
        }
    }
}

/// Decision records are named `NNN-name.md` with unique numbers, a replaced decision is marked
/// `superseded` by the one that replaces it, and the list of decisions is on the decisions page.
fn check_decisions(docs: &Docs, problems: &mut Vec<Problem>) {
    // Translations have the same files and front matter, so the records are checked once.
    let default = docs.site.default_language.as_str();
    let mut numbers: BTreeMap<u32, &Page> = BTreeMap::new();
    for page in docs.pages.iter().filter(|p| p.lang == default && decision::in_folder(&p.id)) {
        let Some(number) = page.decision else {
            problems.push(error(page.place(), "a decision record is named `NNN-name.md`: three digits, a hyphen, then lowercase words"));
            continue;
        };
        if let Some(other) = numbers.insert(number, page) {
            problems.push(error(page.place(), format!("decision {number} is also {}", other.place())));
        }
    }

    let mut replaced = BTreeSet::new();
    for page in numbers.values() {
        let supersedes = page.meta.supersedes.as_deref().and_then(decision::parse_numbers).unwrap_or_default();
        for number in supersedes {
            replaced.insert(number);
            match numbers.get(&number) {
                None => problems.push(error(page.place(), format!("decision {number} in `supersedes` does not exist"))),
                Some(old) if old.meta.status.as_deref() != Some("superseded") => problems.push(error(
                    old.place(),
                    format!("decision {} replaces this one, so its status is `superseded`", page.decision.unwrap_or_default()),
                )),
                Some(_) => {}
            }
        }
    }
    for (number, page) in &numbers {
        if page.meta.status.as_deref() == Some("superseded") && !replaced.contains(number) {
            problems.push(error(page.place(), "the decision is `superseded`, but no decision lists it in `supersedes`"));
        }
    }

    for page in &docs.pages {
        if page.decision_list && page.id != decision::FOLDER {
            problems.push(error(page.place(), format!("the `{}` marker belongs on the decisions page only", decision::LIST_MARKER)));
        }
        if page.id == decision::FOLDER && !page.decision_list {
            problems.push(error(page.place(), format!("no `{}` marker", decision::LIST_MARKER)));
        }
    }
}
