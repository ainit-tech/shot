//! Writes the site data into `tools/docs/ui/public`. Three kinds of readers get three
//! kinds of URLs:
//!
//! - people: the site itself, `/<lang>/<page>` and `/<version>/<lang>/<page>`;
//! - AI agents: `llms.txt`, `llms-full.txt` and Markdown sources under `/md/<version>/`;
//! - programs: JSON under `/api/`:
//!   - `api/versions.json` — documentation versions: the working tree and every commit that
//!     changed the docs, see [`crate::versions`];
//!   - `api/<version>/manifest.json` — languages, menu and metadata of every page;
//!   - `api/<version>/pages/<lang>/<id>.json` — one page: metadata, history, Markdown, HTML,
//!     and for a decision record the pages that link to it;
//!   - `api/<version>/search/<lang>.json` — search index;
//!   - `api/<version>/terms/<lang>.json` — glossary terms, for the hints over terms in texts.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::decision;
use crate::git::{self, Commit};
use crate::glossary::Glossary;
use crate::page::{self, Docs, Heading, LATEST, Page, Target};
use crate::site::{Language, NavSection, Topic};

/// Version of the JSON formats below; raise it when a field is removed or changes meaning.
const SCHEMA_VERSION: u32 = 1;

/// One documentation version: the working tree (`latest`) or a commit that changed the docs.
pub struct Version {
    pub id: String,
    pub commit: Option<String>,
    pub date: Option<String>,
    /// Git revision that page history is read up to.
    pub rev: String,
    /// The version this one is compared with to mark what changed.
    pub previous: Option<String>,
    /// A milestone: the number comes from a Git tag.
    pub tagged: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Meta<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    nav_title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    translation_of: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest<'a> {
    schema_version: u32,
    version: &'a str,
    name: &'a str,
    summary: &'a str,
    default_language: &'a str,
    languages: &'a [Language],
    nav: &'a [NavSection],
    /// Topics of decision records, in display order.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    decision_topics: &'a [Topic],
    /// Short hash of the version's commit; none for docs outside Git.
    #[serde(skip_serializing_if = "Option::is_none")]
    commit: Option<&'a str>,
    /// The version the pages' `changes` are counted from.
    #[serde(skip_serializing_if = "Option::is_none")]
    previous: Option<&'a str>,
    /// Which engine wrote these files: a version is written again when the engine changes.
    build: &'a str,
    /// Language → page id → page.
    pages: BTreeMap<&'a str, BTreeMap<&'a str, PageEntry<'a>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PageEntry<'a> {
    title: &'a str,
    #[serde(flatten)]
    meta: Meta<'a>,
    route: &'a str,
    api_url: String,
    markdown_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_change: Option<&'a Commit>,
    uncommitted_changes: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<DecisionEntry<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    changes: Option<PageChanges>,
}

/// How a page differs from the same page in the previous version.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PageChanges {
    /// `added` for a page the previous version didn't have, otherwise `changed`.
    status: &'static str,
    /// New and changed sections by heading id; `""` is the part above the first heading.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sections: Vec<SectionChange>,
    /// Titles of the sections the previous version had and this one doesn't.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    removed: Vec<String>,
}

#[derive(Clone, Serialize)]
struct SectionChange {
    id: String,
    /// `added` or `changed`.
    status: &'static str,
}

/// What the site shows about a decision record in lists, hints and the record's own header.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DecisionEntry<'a> {
    number: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<&'a str>,
    /// The decision in a sentence or two: the paragraph under the record's title.
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<&'a str>,
    /// Decisions this one replaces.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    supersedes: Vec<u32>,
    /// Decisions that replace this one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    superseded_by: Vec<u32>,
    /// The topic: one of the manifest's `decisionTopics`.
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<&'a str>,
}

/// A page that links to a decision record.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Reference<'a> {
    id: &'a str,
    title: &'a str,
    route: &'a str,
    /// The number, when the linking page is a decision record too.
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PageData<'a> {
    schema_version: u32,
    version: &'a str,
    lang: &'a str,
    id: &'a str,
    title: &'a str,
    #[serde(flatten)]
    meta: Meta<'a>,
    route: &'a str,
    api_url: String,
    markdown_url: String,
    /// Language → API URL of the same page in that language.
    translations: BTreeMap<&'a str, String>,
    headings: &'a [Heading],
    /// Commits that changed the page, newest first.
    history: &'a [Commit],
    /// The working tree has changes to this page that are not committed yet.
    uncommitted_changes: bool,
    /// Markdown without the front matter.
    markdown: &'a str,
    html: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<DecisionEntry<'a>>,
    /// Decision records only: the pages of the same language that link to the record.
    #[serde(skip_serializing_if = "Option::is_none")]
    referenced_by: Option<Vec<Reference<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    changes: Option<PageChanges>,
}

#[derive(Serialize)]
struct SearchEntry<'a> {
    id: &'a str,
    title: &'a str,
    route: &'a str,
    headings: &'a [Heading],
    text: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TermsFile<'a> {
    schema_version: u32,
    version: &'a str,
    lang: &'a str,
    /// Route of the glossary page.
    glossary: String,
    /// Term id → the term.
    terms: BTreeMap<&'a str, TermEntry<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TermEntry<'a> {
    name: &'a str,
    code: &'a [String],
    kind: &'a str,
    status: String,
    status_name: String,
    /// The definition as HTML.
    html: String,
    /// The term's card on the glossary page.
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionsFile<'a> {
    schema_version: u32,
    /// Newest first; `latest` is always the first.
    versions: Vec<VersionEntry<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionEntry<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<&'a str>,
    /// A milestone: the number comes from a Git tag.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    tagged: bool,
    /// The version this one is compared with.
    #[serde(skip_serializing_if = "Option::is_none")]
    previous: Option<&'a str>,
    manifest: String,
}

/// Where the site data goes: `tools/docs/ui/public`. A running site reads these files while a
/// build writes them, so every file is written beside its place and then renamed into it, and the
/// files an earlier build wrote and this one did not are removed only at the end, by
/// [`Output::prune`]. The site never finds the data half-written or missing.
pub struct Output {
    public: PathBuf,
    written: HashSet<PathBuf>,
    /// Identifies the engine that writes; see [`fingerprint`].
    build: String,
}

impl Output {
    pub fn new(public: &Path, build: String) -> Output {
        Output { public: public.to_path_buf(), written: HashSet::new(), build }
    }

    /// Keeps a version written by an earlier build of the same commit with the same engine:
    /// a commit's docs never change, so its files would come out the same. Returns whether
    /// the version is kept.
    pub fn reuse(&mut self, version: &str, commit: &str, previous: Option<&str>) -> bool {
        let manifest = self.path(&format!("/api/{version}/manifest.json"));
        let Some(json) = fs::read_to_string(&manifest).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        else {
            return false;
        };
        let expected_previous = previous.map_or(serde_json::Value::Null, serde_json::Value::from);
        if json["build"] != self.build.as_str() || json["commit"] != commit || json["previous"] != expected_previous {
            return false;
        }
        for dir in [format!("/api/{version}"), format!("/md/{version}")] {
            let mut files = Vec::new();
            collect_files(&self.path(&dir), &mut files);
            self.written.extend(files);
        }
        true
    }

    /// The file behind a site URL such as `/api/latest/manifest.json`.
    fn path(&self, url: &str) -> PathBuf {
        let mut path = self.public.clone();
        path.extend(url.split('/').filter(|part| !part.is_empty()));
        path
    }

    fn write(&mut self, url: &str, bytes: &[u8]) -> Result<()> {
        let path = self.path(url);
        create_parent(&path)?;
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".tmp");
        let temporary = path.with_file_name(name);
        fs::write(&temporary, bytes).with_context(|| format!("writing {}", temporary.display()))?;
        fs::rename(&temporary, &path).with_context(|| format!("replacing {}", path.display()))?;
        self.written.insert(path);
        Ok(())
    }

    fn write_json(&mut self, url: &str, value: &impl Serialize) -> Result<()> {
        let json = serde_json::to_string_pretty(value)?;
        self.write(url, json.as_bytes())
    }

    /// Removes what earlier builds wrote and this one did not, and the folders left empty.
    pub fn prune(&self) -> Result<()> {
        for dir in ["api", "md", "data"] {
            prune(&self.public.join(dir), &self.written)?;
        }
        Ok(())
    }
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

/// Identifies the running engine: a hash of its program file and of the data format. The
/// files of every version are written again when it changes.
pub fn fingerprint() -> String {
    use std::hash::{Hash, Hasher};
    let program = std::env::current_exe().and_then(fs::read).unwrap_or_default();
    let mut hasher = std::hash::DefaultHasher::new();
    program.hash(&mut hasher);
    format!("{:016x}-{SCHEMA_VERSION}", hasher.finish())
}

/// Removes the files under `dir` that are not in `keep`; returns whether `dir` is left empty.
fn prune(dir: &Path, keep: &HashSet<PathBuf>) -> Result<bool> {
    if !dir.is_dir() {
        return Ok(true);
    }
    let mut empty = true;
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            if prune(&path, keep)? {
                fs::remove_dir(&path).with_context(|| format!("removing {}", path.display()))?;
            } else {
                empty = false;
            }
        } else if keep.contains(&path) {
            empty = false;
        } else {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    Ok(empty)
}

/// Writes the API files and Markdown sources of one version. `repo` is where Git history is read;
/// `previous` is the version this one is compared with.
pub fn write_version(docs: &Docs, version: &Version, repo: &Path, out: &mut Output, previous: Option<&Docs>) -> Result<()> {
    let v = version.id.as_str();
    let api = format!("/api/{v}");

    let mut histories = git::histories(repo, &version.rev, "docs");
    let changed = if v == LATEST { git::uncommitted_files(repo, "docs") } else { HashSet::new() };
    let history: Vec<Vec<Commit>> = docs
        .pages
        .iter()
        .map(|page| histories.remove(&source_path(page)).unwrap_or_default())
        .collect();
    let uncommitted: Vec<bool> = docs.pages.iter().map(|page| changed.contains(&source_path(page))).collect();
    let superseded_by = superseded_by(docs);
    let mut references = references(docs);

    let mut pages: BTreeMap<&str, BTreeMap<&str, PageEntry>> = BTreeMap::new();
    let mut search: BTreeMap<&str, Vec<SearchEntry>> = BTreeMap::new();
    for ((page, history), &uncommitted) in docs.pages.iter().zip(&history).zip(&uncommitted) {
        let title = title(page);
        let api_url = page_api_url(v, &page.lang, &page.id);
        let markdown_url = docs.urls.file(&format!("{}/{}", page.lang, page.file));
        let translations = docs
            .site
            .languages
            .iter()
            .filter(|l| docs.page(&l.code, &page.id).is_some())
            .map(|l| (l.code.as_str(), page_api_url(v, &l.code, &page.id)))
            .collect();

        let decision = page.decision.map(|number| DecisionEntry {
            number,
            date: page.meta.date.as_deref(),
            summary: page.summary.as_deref(),
            supersedes: page.meta.supersedes.as_deref().and_then(decision::parse_numbers).unwrap_or_default(),
            superseded_by: superseded_by.get(&(page.lang.as_str(), number)).cloned().unwrap_or_default(),
            topic: page.meta.topic.as_deref(),
        });
        let referenced_by = page.decision.map(|_| {
            references
                .remove(&(page.lang.as_str(), page.id.as_str()))
                .unwrap_or_default()
                .into_iter()
                .map(|p| Reference { id: &p.id, title: self::title(p), route: &p.route, decision: p.decision })
                .collect()
        });

        // Without a previous version there is nothing to compare with, so nothing is marked.
        let changes = previous.and_then(|before| page_changes(page, before.page(&page.lang, &page.id)));

        let data = PageData {
            schema_version: SCHEMA_VERSION,
            version: v,
            lang: &page.lang,
            id: &page.id,
            title,
            meta: meta(page),
            route: &page.route,
            api_url: api_url.clone(),
            markdown_url: markdown_url.clone(),
            translations,
            headings: &page.headings,
            history,
            uncommitted_changes: uncommitted,
            markdown: page.body.trim_start(),
            html: &page.html,
            decision: decision.clone(),
            referenced_by,
            changes: changes.clone(),
        };
        out.write_json(&api_url, &data)?;
        out.write(&markdown_url, page.raw.as_bytes())?;

        pages.entry(&page.lang).or_default().insert(
            &page.id,
            PageEntry {
                title,
                meta: meta(page),
                route: &page.route,
                api_url,
                markdown_url,
                last_change: history.first(),
                uncommitted_changes: uncommitted,
                decision,
                changes,
            },
        );
        search.entry(&page.lang).or_default().push(SearchEntry {
            id: &page.id,
            title,
            route: &page.route,
            headings: &page.headings,
            text: &page.text,
        });
    }

    for asset in &docs.assets {
        let url = docs.urls.file(&format!("{}/{}", asset.lang, asset.file));
        let bytes = fs::read(&asset.source).with_context(|| format!("reading {}", asset.source.display()))?;
        out.write(&url, &bytes)?;
    }
    for (lang, entries) in &search {
        out.write_json(&format!("{api}/search/{lang}.json"), entries)?;
    }
    if let Some(glossary) = &docs.glossary {
        for language in &docs.site.languages {
            let lang = language.code.as_str();
            out.write_json(&format!("{api}/terms/{lang}.json"), &terms_file(docs, glossary, v, lang))?;
        }
    }

    let build = out.build.clone();
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        version: v,
        name: &docs.site.name,
        summary: &docs.site.summary,
        default_language: &docs.site.default_language,
        languages: &docs.site.languages,
        nav: &docs.site.nav,
        decision_topics: &docs.site.decision_topics,
        commit: version.commit.as_deref(),
        previous: version.previous.as_deref(),
        build: &build,
        pages,
    };
    out.write_json(&format!("{api}/manifest.json"), &manifest)
}

/// How `page` differs from `before`, the same page in the previous version; `None` if it doesn't.
fn page_changes(page: &Page, before: Option<&Page>) -> Option<PageChanges> {
    let Some(before) = before else {
        return Some(PageChanges { status: "added", sections: Vec::new(), removed: Vec::new() });
    };
    let old: HashMap<&str, &str> = before.sections.iter().map(|(id, text)| (id.as_str(), text.as_str())).collect();
    let sections: Vec<SectionChange> = page
        .sections
        .iter()
        .filter_map(|(id, text)| match old.get(id.as_str()) {
            None => Some(SectionChange { id: id.clone(), status: "added" }),
            Some(old_text) if old_text != text => Some(SectionChange { id: id.clone(), status: "changed" }),
            Some(_) => None,
        })
        .collect();
    let current: HashSet<&str> = page.sections.iter().map(|(id, _)| id.as_str()).collect();
    let removed: Vec<String> = before
        .sections
        .iter()
        .filter(|(id, _)| !current.contains(id.as_str()))
        .filter_map(|(id, _)| before.headings.iter().find(|h| &h.id == id).map(|h| h.text.clone()))
        .collect();
    let meta_changed = page.title != before.title || page.meta.status != before.meta.status || page.meta.note != before.meta.note;
    if sections.is_empty() && removed.is_empty() && !meta_changed {
        return None;
    }
    Some(PageChanges { status: "changed", sections, removed })
}

/// (language, decision number) → the numbers of the decisions that replace it.
fn superseded_by(docs: &Docs) -> HashMap<(&str, u32), Vec<u32>> {
    let mut map: HashMap<(&str, u32), Vec<u32>> = HashMap::new();
    for page in &docs.pages {
        let (Some(number), Some(value)) = (page.decision, page.meta.supersedes.as_deref()) else { continue };
        for old in decision::parse_numbers(value).unwrap_or_default() {
            map.entry((page.lang.as_str(), old)).or_default().push(number);
        }
    }
    map
}

/// (language, decision record id) → the pages of that language that link to the record:
/// pages that describe the system first, then other decision records by number.
fn references(docs: &Docs) -> HashMap<(&str, &str), Vec<&Page>> {
    let mut map: HashMap<(&str, &str), Vec<&Page>> = HashMap::new();
    for page in &docs.pages {
        for link in &page.links {
            let Target::Page { lang, id, .. } = &link.resolved else { continue };
            if decision::number(id).is_none() || (lang == &page.lang && id == &page.id) {
                continue;
            }
            let sources = map.entry((lang.as_str(), id.as_str())).or_default();
            if !sources.iter().any(|p| std::ptr::eq(*p, page)) {
                sources.push(page);
            }
        }
    }
    for sources in map.values_mut() {
        sources.sort_by_key(|p| (p.decision.is_some(), p.decision, p.id.clone()));
    }
    map
}

/// The glossary terms of one language with their definitions and links to their cards.
fn terms_file<'a>(docs: &'a Docs, glossary: &'a Glossary, version: &'a str, lang: &'a str) -> TermsFile<'a> {
    let codes: Vec<String> = docs.site.languages.iter().map(|l| l.code.clone()).collect();
    let route = docs.urls.route(lang, "glossary");
    let page = docs.page(lang, "glossary");
    let terms = glossary
        .terms
        .iter()
        .filter_map(|term| {
            let text = term.text.get(lang)?;
            let anchor = page
                .and_then(|p| p.headings.iter().find(|h| h.level == 3 && h.text == text.name))
                .map_or_else(|| page::slugify(&text.name), |h| h.id.clone());
            let (status, status_name) = glossary.status(term, lang);
            let entry = TermEntry {
                name: &text.name,
                code: &term.code,
                kind: &text.kind,
                status,
                status_name,
                html: page::render_html(&text.text, lang, &codes, &docs.urls),
                url: format!("{route}#{anchor}"),
            };
            Some((term.id.as_str(), entry))
        })
        .collect();
    TermsFile { schema_version: SCHEMA_VERSION, version, lang, glossary: route, terms }
}

pub fn write_versions(versions: &[Version], out: &mut Output) -> Result<()> {
    let file = VersionsFile {
        schema_version: SCHEMA_VERSION,
        versions: versions
            .iter()
            .map(|v| VersionEntry {
                id: &v.id,
                commit: v.commit.as_deref(),
                date: v.date.as_deref(),
                tagged: v.tagged,
                previous: v.previous.as_deref(),
                manifest: format!("/api/{}/manifest.json", v.id),
            })
            .collect(),
    };
    out.write_json("/api/versions.json", &file)
}

/// `llms.txt` (https://llmstxt.org) and `llms-full.txt` for the latest version.
pub fn write_llms(docs: &Docs, out: &mut Output) -> Result<()> {
    let site = &docs.site;
    let source_language = site
        .languages
        .iter()
        .find(|l| l.code == site.default_language)
        .map_or(site.default_language.as_str(), |l| l.name.as_str());

    let mut text = format!("# {}\n\n> {}\n\n", site.name, site.summary);
    text.push_str(&format!(
        "The documentation is written in {source_language} and translated into the other languages below. \
         The links lead to the Markdown sources; relative links inside them work as they are. \
         All {source_language} pages in one file: [/llms-full.txt](/llms-full.txt).\n\n\
         JSON API: [/api/versions.json](/api/versions.json) lists documentation versions, \
         `/api/<version>/manifest.json` lists the pages of a version with their metadata, \
         `/api/<version>/pages/<lang>/<page>.json` is one page with its history, Markdown and HTML, and \
         `/api/<version>/terms/<lang>.json` holds the glossary terms with their definitions.\n"
    ));
    for language in &site.languages {
        text.push_str(&format!("\n## {}\n\n", language.name));
        for page in in_menu_order(docs, &language.code) {
            let status = page.meta.status.as_deref().map(|s| format!(": status {s}")).unwrap_or_default();
            let url = docs.urls.file(&format!("{}/{}", page.lang, page.file));
            text.push_str(&format!("- [{}]({url}){status}\n", title(page)));
        }
    }
    out.write("/llms.txt", text.as_bytes())?;

    let full: Vec<&str> = in_menu_order(docs, &site.default_language).map(|p| p.body.trim()).collect();
    out.write("/llms-full.txt", format!("{}\n", full.join("\n\n")).as_bytes())
}

/// Pages in menu order; decision records follow the decisions page, by number.
fn in_menu_order<'a>(docs: &'a Docs, lang: &'a str) -> impl Iterator<Item = &'a Page> {
    docs.site.nav.iter().flat_map(move |section| {
        let pages = section.all_pages().filter_map(move |id| docs.page(lang, id));
        let records = if section.index.as_deref() == Some(decision::FOLDER) { docs.decisions(lang) } else { Vec::new() };
        pages.chain(records)
    })
}

fn page_api_url(version: &str, lang: &str, id: &str) -> String {
    format!("/api/{version}/pages/{lang}/{id}.json")
}

/// Path of the page's source file relative to the repository root.
fn source_path(page: &Page) -> String {
    format!("docs/{}/{}", page.lang, page.file)
}

fn title(page: &Page) -> &str {
    page.title.as_deref().unwrap_or(&page.id)
}

fn meta(page: &Page) -> Meta<'_> {
    let m = &page.meta;
    Meta {
        nav_title: m.nav_title.as_deref(),
        status: m.status.as_deref(),
        note: m.note.as_deref(),
        translation_of: m.translation_of.as_deref(),
        updated: m.updated.as_deref(),
    }
}

fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_replaces_files_and_removes_stale_ones() {
        let public = std::env::temp_dir().join(format!("docs-output-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&public);
        fs::create_dir_all(public.join("api").join("v0.0.1").join("pages")).unwrap();
        fs::write(public.join("api").join("v0.0.1").join("pages").join("x.json"), "{}").unwrap();
        fs::create_dir_all(public.join("api").join("latest")).unwrap();
        fs::write(public.join("api").join("latest").join("manifest.json"), "old").unwrap();

        let mut out = Output::new(&public, "test".to_string());
        out.write("/api/latest/manifest.json", b"new").unwrap();
        out.write("/md/latest/en/README.md", b"# Docs").unwrap();
        out.prune().unwrap();

        let manifest = public.join("api").join("latest").join("manifest.json");
        assert_eq!(fs::read_to_string(&manifest).unwrap(), "new");
        assert!(public.join("md").join("latest").join("en").join("README.md").is_file());
        assert!(!public.join("api").join("v0.0.1").exists());
        assert!(!public.join("api").join("latest").join("manifest.json.tmp").exists());
        fs::remove_dir_all(&public).unwrap();
    }
}
