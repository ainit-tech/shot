//! Loading documentation sources: Markdown pages with front matter and other files (assets).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use pulldown_cmark::{CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, TextMergeStream, html};
use serde::Serialize;

use crate::decision;
use crate::glossary::{CARD_START, Glossary, Matcher};
use crate::site::{Language, Site};

/// Id of the version built from the working tree.
pub const LATEST: &str = "latest";

/// URL prefixes of one documentation version.
pub struct Urls {
    /// Prefix of page routes: empty for the latest version, `/v0.1.0` for a tagged one.
    pub pages: String,
    /// Where the source files (Markdown, diagrams) of the version are published: `/md/latest`.
    pub files: String,
}

impl Urls {
    pub fn for_version(version: &str) -> Urls {
        Urls {
            pages: if version == LATEST { String::new() } else { format!("/{version}") },
            files: format!("/md/{version}"),
        }
    }

    /// Site path of a page: `/en/` for the index, `/en/concept/overview` otherwise.
    pub fn route(&self, lang: &str, id: &str) -> String {
        if id == "index" { format!("{}/{lang}/", self.pages) } else { format!("{}/{lang}/{id}", self.pages) }
    }

    /// URL of a source file, `path` being relative to `docs/`.
    pub fn file(&self, path: &str) -> String {
        format!("{}/{path}", self.files)
    }
}

/// All documentation sources of one version: site settings, pages and assets of every language.
pub struct Docs {
    /// The folder that contains `docs/`: the repository, or an exported tagged version.
    pub root: PathBuf,
    pub site: Site,
    /// The term base from `docs/glossary.toml`, if the version has one.
    pub glossary: Option<Glossary>,
    pub urls: Urls,
    pub pages: Vec<Page>,
    pub assets: Vec<Asset>,
}

/// What a page needs to render: languages, URLs and the term base.
struct Env<'a> {
    languages: &'a [Language],
    codes: &'a [String],
    urls: &'a Urls,
    glossary: Option<&'a Glossary>,
    /// Terms of the page's language, for the hints on the site.
    terms: Option<&'a Matcher>,
}

/// A file in a language folder that is not a page, such as a diagram.
pub struct Asset {
    pub lang: String,
    /// Path inside the language folder, with `/` separators.
    pub file: String,
    pub source: PathBuf,
}

pub struct Page {
    pub lang: String,
    /// `index` for the language's README.md, the folder path for any other README.md,
    /// otherwise the file path without `.md`.
    pub id: String,
    /// Path inside the language folder, with `/` separators.
    pub file: String,
    /// The file with front matter, as published to AI agents: glossary markers are expanded.
    pub raw: String,
    /// The Markdown after the front matter, glossary markers expanded.
    pub body: String,
    pub meta: FrontMatter,
    /// Glossary markers the page contains, see [`crate::glossary::MARKERS`].
    pub glossary_markers: Vec<&'static str>,
    /// Text of the first `# ` heading.
    pub title: Option<String>,
    pub route: String,
    pub html: String,
    pub headings: Vec<Heading>,
    /// Plain text for search.
    pub text: String,
    /// Plain text of the paragraph right under the title, if the page starts with one.
    pub summary: Option<String>,
    /// Every section's text with its links, for comparing versions: (heading id, text).
    /// The part above the first `##` or `###` heading has the id `""`.
    pub sections: Vec<(String, String)>,
    pub links: Vec<Link>,
    /// The number of a decision record, from its file name: `decisions/007-….md` → 7.
    pub decision: Option<u32>,
    /// The page has the marker of the list of decisions, see [`decision::LIST_MARKER`].
    pub decision_list: bool,
}

/// Page metadata between `---` lines at the top of the file: flat `key: value` pairs.
#[derive(Default)]
pub struct FrontMatter {
    /// `draft`, `accepted`, `partial`, `living`, or `superseded` for a decision record.
    pub status: Option<String>,
    /// A sentence that clarifies the status.
    pub note: Option<String>,
    /// For translations: the language of the original.
    pub translation_of: Option<String>,
    /// For translations: when the text was last updated from the original, `YYYY-MM-DD`.
    pub updated: Option<String>,
    /// A shorter title for the menu.
    pub nav_title: Option<String>,
    /// Decision records: when the decision was made or, for a draft, proposed, `YYYY-MM-DD`.
    pub date: Option<String>,
    /// Decision records: the numbers of the decisions this one replaces, `3` or `3, 5`.
    pub supersedes: Option<String>,
    /// Decision records: the topic, one of `decision_topics` in `site.toml`.
    pub topic: Option<String>,
    /// Keys the engine does not know; reported by the checks.
    pub unknown: Vec<String>,
}

#[derive(Serialize)]
pub struct Heading {
    pub level: u8,
    pub id: String,
    pub text: String,
}

pub struct Link {
    /// The link as written in the source.
    pub target: String,
    pub resolved: Target,
}

pub enum Target {
    External,
    SamePage { fragment: String },
    Page { lang: String, id: String, fragment: Option<String> },
    /// Path inside `docs/`, such as `en/diagrams/l1-organizations.svg`.
    Asset { path: String },
    /// The link leaves the language folders.
    Outside,
}

impl Docs {
    pub fn load(root: &Path, urls: Urls) -> Result<Docs> {
        let docs_dir = root.join("docs");
        let site = Site::load(&docs_dir.join("site.toml"))?;
        let glossary = Glossary::load(&docs_dir.join("glossary.toml"))?;
        let languages: Vec<String> = site.languages.iter().map(|l| l.code.clone()).collect();
        let matchers: BTreeMap<&str, Matcher> = match &glossary {
            Some(glossary) => languages.iter().map(|l| (l.as_str(), Matcher::new(glossary, l))).collect(),
            None => BTreeMap::new(),
        };

        let mut pages = Vec::new();
        let mut assets = Vec::new();
        for lang in &languages {
            let dir = docs_dir.join(lang);
            if !dir.is_dir() {
                bail!("language folder {} is missing", dir.display());
            }
            let env = Env {
                languages: &site.languages,
                codes: &languages,
                urls: &urls,
                glossary: glossary.as_ref(),
                terms: matchers.get(lang.as_str()),
            };
            for (file, source) in walk(&dir)? {
                if file.ends_with(".md") {
                    let raw = fs::read_to_string(&source)
                        .with_context(|| format!("reading {}", source.display()))?;
                    let page = Page::parse(lang, &file, raw, &env)
                        .with_context(|| format!("parsing {}", source.display()))?;
                    pages.push(page);
                } else {
                    assets.push(Asset { lang: lang.clone(), file, source });
                }
            }
        }

        // The list of decisions needs every record of the language, so it is filled in last.
        for i in 0..pages.len() {
            if !pages[i].decision_list {
                continue;
            }
            let list = decision::list_markdown(&records(&pages, &pages[i].lang), &site.decision_topics, &pages[i].lang);
            let page = &mut pages[i];
            page.raw = page.raw.replace(decision::LIST_MARKER, list.trim_end());
            page.body = page.body.replace(decision::LIST_MARKER, list.trim_end());
        }
        Ok(Docs { root: root.to_path_buf(), site, glossary, urls, pages, assets })
    }

    pub fn page(&self, lang: &str, id: &str) -> Option<&Page> {
        self.pages.iter().find(|p| p.lang == lang && p.id == id)
    }

    /// The decision records of a language, by number.
    pub fn decisions(&self, lang: &str) -> Vec<&Page> {
        records(&self.pages, lang)
    }
}

/// A page parsed on its own, for tests elsewhere in the engine.
#[cfg(test)]
pub fn parse_for_test(lang: &str, file: &str, raw: &str) -> Page {
    let languages = vec![Language { code: lang.to_string(), name: lang.to_string() }];
    let codes = vec![lang.to_string()];
    let urls = Urls::for_version(LATEST);
    let env = Env { languages: &languages, codes: &codes, urls: &urls, glossary: None, terms: None };
    Page::parse(lang, file, raw.to_string(), &env).expect("a test page parses")
}

impl Page {
    fn parse(lang: &str, file: &str, raw: String, env: &Env) -> Result<Page> {
        let (meta, body) = front_matter(&raw)?;
        let front = &raw[..raw.len() - body.len()];
        let id = page_id(file);
        let number = decision::number(&id);
        let dir = parent(&format!("{lang}/{file}"));

        // Glossary markers become plain Markdown for the sources and styled cards for the site.
        let (markdown, glossary_markers) = match env.glossary {
            Some(glossary) => glossary.expand(body, lang, env.languages, false),
            None => (body.to_string(), Vec::new()),
        };
        let for_site = match env.glossary {
            Some(glossary) if !glossary_markers.is_empty() => glossary.expand(body, lang, env.languages, true).0,
            _ => markdown.clone(),
        };
        // The site fills the list of decisions in itself, from the manifest.
        let decision_list = body.contains(decision::LIST_MARKER);
        let for_site = for_site.replace(decision::LIST_MARKER, decision::LIST_HTML);
        let rendered = render(&for_site, &dir, env.codes, env.urls, env.terms);
        Ok(Page {
            lang: lang.to_string(),
            route: env.urls.route(lang, &id),
            id,
            file: file.to_string(),
            raw: format!("{front}{markdown}"),
            body: markdown,
            meta,
            glossary_markers,
            title: rendered.title,
            html: rendered.html,
            headings: rendered.headings,
            text: rendered.text,
            summary: rendered.summary,
            sections: rendered.sections,
            links: rendered.links,
            decision: number,
            decision_list,
        })
    }

    /// Where the page is in the sources, for messages: `en/glossary.md`.
    pub fn place(&self) -> String {
        format!("{}/{}", self.lang, self.file)
    }

    pub fn has_heading(&self, id: &str) -> bool {
        self.headings.iter().any(|h| h.id == id)
    }
}

fn page_id(file: &str) -> String {
    let stem = file.strip_suffix(".md").unwrap_or(file);
    if stem == "README" {
        "index".to_string()
    } else if let Some(folder) = stem.strip_suffix("/README") {
        folder.to_string()
    } else {
        stem.to_string()
    }
}

fn parent(path: &str) -> String {
    path.rsplit_once('/').map(|(dir, _)| dir.to_string()).unwrap_or_default()
}

/// Lists files under `dir` recursively as (path relative to `dir` with `/`, full path), sorted.
fn walk(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).with_context(|| format!("reading {}", folder.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(dir)?
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.push((relative, path));
        }
    }
    files.sort();
    Ok(files)
}

fn front_matter(raw: &str) -> Result<(FrontMatter, &str)> {
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return Ok((FrontMatter::default(), text));
    };
    let Some(end) = rest.find("\n---") else {
        bail!("front matter is not closed with `---`");
    };
    let after = &rest[end + "\n---".len()..];
    let body = after.strip_prefix("\r\n").or_else(|| after.strip_prefix('\n')).unwrap_or(after);

    let mut meta = FrontMatter::default();
    for line in rest[..end].lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            bail!("front matter line is not `key: value`: {line}");
        };
        let value = Some(unquote(value.trim()).to_string());
        match key.trim() {
            "status" => meta.status = value,
            "note" => meta.note = value,
            "translation_of" => meta.translation_of = value,
            "updated" => meta.updated = value,
            "nav_title" => meta.nav_title = value,
            "date" => meta.date = value,
            "supersedes" => meta.supersedes = value,
            "topic" => meta.topic = value,
            other => meta.unknown.push(other.to_string()),
        }
    }
    Ok((meta, body))
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value.strip_prefix(quote).and_then(|v| v.strip_suffix(quote)) {
            return inner;
        }
    }
    value
}

struct Rendered {
    title: Option<String>,
    html: String,
    headings: Vec<Heading>,
    text: String,
    summary: Option<String>,
    sections: Vec<(String, String)>,
    links: Vec<Link>,
}

/// Renders a Markdown fragment, such as a term's definition, to HTML. `dir` is its folder inside `docs/`.
pub fn render_html(markdown: &str, dir: &str, languages: &[String], urls: &Urls) -> String {
    render(markdown, dir, languages, urls, None).html
}

/// Renders Markdown to HTML: gives headings GitHub-style ids, rewrites links between source
/// files into site URLs and, with `terms`, marks glossary terms for the hints on the site.
/// `dir` is the page's folder inside `docs/`.
fn render(markdown: &str, dir: &str, languages: &[String], urls: &Urls, terms: Option<&Matcher>) -> Rendered {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let mut events: Vec<Event> = TextMergeStream::new(Parser::new_ext(markdown, options)).collect();

    let mut title = None;
    let mut headings = Vec::new();
    let mut used_ids = HashMap::new();
    let mut i = 0;
    while i < events.len() {
        if let Event::Start(Tag::Heading { level, .. }) = &events[i] {
            let level = *level as u8;
            let mut text = String::new();
            let mut end = i + 1;
            while end < events.len() && !matches!(events[end], Event::End(TagEnd::Heading(_))) {
                if let Event::Text(t) | Event::Code(t) = &events[end] {
                    text.push_str(t);
                }
                end += 1;
            }
            let id = unique(slugify(&text), &mut used_ids);
            if let Event::Start(Tag::Heading { id: slot, .. }) = &mut events[i] {
                *slot = Some(CowStr::from(id.clone()));
            }
            if level == 1 && title.is_none() {
                title = Some(text.clone());
            }
            headings.push(Heading { level, id, text });
            i = end;
        }
        i += 1;
    }

    let summary = summary(&events);
    let mut links = Vec::new();
    let mut text = String::new();
    let mut sections = vec![(String::new(), String::new())];
    for event in &mut events {
        match event {
            Event::Start(Tag::Heading { level: HeadingLevel::H2 | HeadingLevel::H3, id, .. }) => {
                sections.push((id.as_deref().unwrap_or_default().to_string(), String::new()));
            }
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                let target = dest_url.to_string();
                let resolved = resolve(dir, &target, languages);
                if let Some(url) = site_url(&resolved, urls) {
                    *dest_url = CowStr::from(url);
                }
                // A changed link is a change of its section too.
                if let Some((_, section)) = sections.last_mut() {
                    section.push_str(&format!(" <{target}> "));
                }
                links.push(Link { target, resolved });
            }
            Event::Text(t) | Event::Code(t) => {
                text.push_str(t);
                if let Some((_, section)) = sections.last_mut() {
                    section.push_str(t);
                }
            }
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::TableCell)
            | Event::SoftBreak
            | Event::HardBreak => {
                text.push(' ');
                if let Some((_, section)) = sections.last_mut() {
                    section.push(' ');
                }
            }
            _ => {}
        }
    }
    let sections = sections
        .into_iter()
        .map(|(id, text)| (id, text.split_whitespace().collect::<Vec<_>>().join(" ")))
        .collect();

    if let Some(matcher) = terms {
        events = mark_terms(events, matcher);
    }
    let mut html_out = String::new();
    html::push_html(&mut html_out, events.into_iter());
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Rendered { title, html: html_out, headings, text, summary, sections, links }
}

/// Plain text of the paragraph that comes right after the `# Title` heading, if any.
/// An image paragraph, such as a diagram under the title, does not count.
fn summary(events: &[Event]) -> Option<String> {
    let mut events = events.iter();
    if !matches!(events.next(), Some(Event::Start(Tag::Heading { level: HeadingLevel::H1, .. }))) {
        return None;
    }
    events.by_ref().find(|e| matches!(e, Event::End(TagEnd::Heading(_))));
    if !matches!(events.next(), Some(Event::Start(Tag::Paragraph))) {
        return None;
    }
    let mut text = String::new();
    let mut in_image = 0usize;
    for event in events {
        match event {
            Event::End(TagEnd::Paragraph) => break,
            Event::Start(Tag::Image { .. }) => in_image += 1,
            Event::End(TagEnd::Image) => in_image -= 1,
            Event::Text(t) | Event::Code(t) if in_image == 0 => text.push_str(t),
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            _ => {}
        }
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// The decision records of a language, by number.
fn records<'a>(pages: &'a [Page], lang: &str) -> Vec<&'a Page> {
    let mut records: Vec<&Page> = pages.iter().filter(|p| p.lang == lang && p.decision.is_some()).collect();
    records.sort_by_key(|p| p.decision);
    records
}

/// Wraps glossary terms in text into `<span class="term" data-term="…">` for the hints on the site.
/// Headings, links, images, tables and code stay as they are, and so does a glossary card's own
/// term. The first mention of a term in each section is underlined and can take keyboard focus.
fn mark_terms<'a>(events: Vec<Event<'a>>, matcher: &Matcher) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(events.len());
    let mut skip = 0usize;
    let mut underlined = HashSet::new();
    let mut card = None;
    for event in events {
        match &event {
            Event::Start(Tag::Heading { level, .. }) => {
                if (*level as u8) <= 2 {
                    underlined.clear();
                }
                skip += 1;
            }
            Event::Start(Tag::Link { .. } | Tag::Image { .. } | Tag::Table(_) | Tag::CodeBlock(_)) => skip += 1,
            Event::End(TagEnd::Heading(_) | TagEnd::Link | TagEnd::Image | TagEnd::Table | TagEnd::CodeBlock) => {
                skip -= 1;
            }
            Event::Html(html) => {
                let html = html.trim_start();
                if let Some(rest) = html.strip_prefix(CARD_START) {
                    card = rest.split('"').next().and_then(|id| matcher.term_index(id));
                } else if html.starts_with("</div>") {
                    card = None;
                }
            }
            _ => {}
        }
        let Event::Text(text) = &event else {
            out.push(event);
            continue;
        };
        let found: Vec<_> = if skip > 0 { Vec::new() } else { matcher.find(text) };
        let found: Vec<_> = found.into_iter().filter(|f| Some(f.term) != card).collect();
        if found.is_empty() {
            out.push(event);
            continue;
        }
        let mut rest = 0;
        for f in found {
            if f.start > rest {
                out.push(Event::Text(CowStr::from(text[rest..f.start].to_string())));
            }
            let id = matcher.id(f.term);
            let open = if underlined.insert(f.term) {
                format!("<span class=\"term term-first\" data-term=\"{id}\" tabindex=\"0\">")
            } else {
                format!("<span class=\"term\" data-term=\"{id}\">")
            };
            out.push(Event::InlineHtml(CowStr::from(open)));
            out.push(Event::Text(CowStr::from(text[f.start..f.end].to_string())));
            out.push(Event::InlineHtml(CowStr::from("</span>")));
            rest = f.end;
        }
        if rest < text.len() {
            out.push(Event::Text(CowStr::from(text[rest..].to_string())));
        }
    }
    out
}

fn resolve(dir: &str, target: &str, languages: &[String]) -> Target {
    if target.contains("://") || target.starts_with("mailto:") {
        return Target::External;
    }
    let (path, fragment) = match target.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment.to_string())),
        None => (target, None),
    };
    if path.is_empty() {
        return Target::SamePage { fragment: fragment.unwrap_or_default() };
    }

    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Target::Outside;
                }
            }
            other => parts.push(other),
        }
    }
    let Some((lang, rest)) = parts.split_first() else {
        return Target::Outside;
    };
    if rest.is_empty() || !languages.iter().any(|l| l == lang) {
        return Target::Outside;
    }
    let file = rest.join("/");
    if file.ends_with(".md") {
        Target::Page { lang: lang.to_string(), id: page_id(&file), fragment }
    } else {
        Target::Asset { path: parts.join("/") }
    }
}

fn site_url(target: &Target, urls: &Urls) -> Option<String> {
    match target {
        Target::Page { lang, id, fragment } => Some(match fragment {
            Some(fragment) => format!("{}#{fragment}", urls.route(lang, id)),
            None => urls.route(lang, id),
        }),
        Target::Asset { path } => Some(urls.file(path)),
        Target::External | Target::SamePage { .. } | Target::Outside => None,
    }
}

/// Heading id the way GitHub makes it: lowercase, punctuation removed, spaces become `-`.
pub fn slugify(text: &str) -> String {
    const PUNCTUATION: &[char] = &['«', '»', '“', '”', '„', '‘', '’', '—', '–', '…', '·', '।', '॥', '→'];
    let mut slug = String::new();
    for c in text.trim().chars().flat_map(char::to_lowercase) {
        if c.is_whitespace() {
            slug.push('-');
        } else if c == '-' || c == '_' || !(c.is_ascii_punctuation() || PUNCTUATION.contains(&c)) {
            slug.push(c);
        }
    }
    slug
}

/// Makes repeated ids unique the way GitHub does: `id`, `id-1`, `id-2`.
fn unique(slug: String, used: &mut HashMap<String, usize>) -> String {
    let count = used.entry(slug.clone()).or_insert(0);
    let id = if *count == 0 { slug } else { format!("{slug}-{count}") };
    *count += 1;
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn languages() -> Vec<String> {
        vec!["en".into(), "ru".into(), "hi".into()]
    }

    #[test]
    fn slugs_match_github() {
        assert_eq!(slugify("Term base"), "term-base");
        assert_eq!(slugify("Решения, которые нужны"), "решения-которые-нужны");
        assert_eq!(slugify("1. Organizations and links"), "1-organizations-and-links");
        assert_eq!(slugify("ज़रूरी निर्णय"), "ज़रूरी-निर्णय");
    }

    #[test]
    fn repeated_slugs_get_numbers() {
        let mut used = HashMap::new();
        assert_eq!(unique("a".into(), &mut used), "a");
        assert_eq!(unique("a".into(), &mut used), "a-1");
        assert_eq!(unique("a".into(), &mut used), "a-2");
    }

    #[test]
    fn page_ids() {
        assert_eq!(page_id("README.md"), "index");
        assert_eq!(page_id("diagrams/README.md"), "diagrams");
        assert_eq!(page_id("concept/overview.md"), "concept/overview");
    }

    #[test]
    fn links_resolve_to_pages_and_assets() {
        let langs = languages();
        match resolve("en/concept", "../glossary.md#term-base", &langs) {
            Target::Page { lang, id, fragment } => {
                assert_eq!((lang.as_str(), id.as_str()), ("en", "glossary"));
                assert_eq!(fragment.as_deref(), Some("term-base"));
            }
            _ => panic!("expected a page"),
        }
        match resolve("en/architecture", "../diagrams/l1-organizations.svg", &langs) {
            Target::Asset { path } => assert_eq!(path, "en/diagrams/l1-organizations.svg"),
            _ => panic!("expected an asset"),
        }
        assert!(matches!(resolve("en", "../README.md", &langs), Target::Outside));
        assert!(matches!(resolve("en", "https://example.com", &langs), Target::External));
        assert!(matches!(resolve("en", "#rules", &langs), Target::SamePage { .. }));
    }

    #[test]
    fn urls_depend_on_the_version() {
        let page = Target::Page { lang: "ru".into(), id: "glossary".into(), fragment: Some("x".into()) };
        let asset = Target::Asset { path: "ru/diagrams/a.svg".into() };

        let latest = Urls::for_version(LATEST);
        assert_eq!(site_url(&page, &latest).as_deref(), Some("/ru/glossary#x"));
        assert_eq!(site_url(&asset, &latest).as_deref(), Some("/md/latest/ru/diagrams/a.svg"));
        assert_eq!(latest.route("ru", "index"), "/ru/");

        let tagged = Urls::for_version("v0.1.0");
        assert_eq!(site_url(&page, &tagged).as_deref(), Some("/v0.1.0/ru/glossary#x"));
        assert_eq!(site_url(&asset, &tagged).as_deref(), Some("/md/v0.1.0/ru/diagrams/a.svg"));
    }

    #[test]
    fn terms_are_marked_outside_headings_links_and_their_own_cards() {
        let glossary: Glossary = toml::from_str(
            r#"
[labels.en]
code = "Code"
kind = "Kind"
status = "Status"
and = "and"

[statuses]
proposed = { en = "proposed" }

[[sections]]
id = "apps"
title = { en = "Apps" }

[[terms]]
id = "agent"
section = "apps"
code = ["Agent"]
status = "proposed"

[terms.en]
name = "Agent"
kind = "application"
text = "An agent."
forms = ["agent", "agents"]
"#,
        )
        .unwrap();
        let matcher = Matcher::new(&glossary, "en");
        let markdown = "# Agents\n\nAgents run. An [agent](x.md) and an agent.\n\n## Next\n\nOne agent.\n\n\
                        <div class=\"term-card\" data-term=\"agent\">\n\nThe agent itself.\n\n</div>\n";
        let html = render(markdown, "en", &languages(), &Urls::for_version(LATEST), Some(&matcher)).html;
        assert!(html.contains("<h1 id=\"agents\">Agents</h1>"));
        assert!(html.contains("<span class=\"term term-first\" data-term=\"agent\" tabindex=\"0\">Agents</span> run."));
        assert!(html.contains(">agent</a> and an <span class=\"term\" data-term=\"agent\">agent</span>."));
        assert!(html.contains("One <span class=\"term term-first\" data-term=\"agent\" tabindex=\"0\">agent</span>."));
        assert!(html.contains("<p>The agent itself.</p>"));
    }

    #[test]
    fn summary_is_the_paragraph_under_the_title() {
        let urls = Urls::for_version(LATEST);
        let summary = |markdown: &str| render(markdown, "en", &languages(), &urls, None).summary;
        assert_eq!(summary("# T

We take *Ansible*,
`not` Chef.

## Why

Text.
").as_deref(), Some("We take Ansible, not Chef."));
        assert_eq!(summary("# T

![Diagram](a.svg)

Text.
"), None);
        assert_eq!(summary("# T

## Section

Text.
"), None);
        assert_eq!(summary("Text.
"), None);
    }

    #[test]
    fn sections_split_at_second_and_third_level_headings() {
        let markdown = "# T

Intro.

## One

A [link](x.md).

### Two

B.

#### Deeper

C.
";
        let sections = render(markdown, "en", &languages(), &Urls::for_version(LATEST), None).sections;
        let ids: Vec<&str> = sections.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["", "one", "two"]);
        assert_eq!(sections[0].1, "T Intro.");
        assert_eq!(sections[1].1, "One A <x.md> link.");
        assert_eq!(sections[2].1, "Two B. Deeper C.");
    }

    #[test]
    fn front_matter_is_parsed() {
        let raw = "---\nstatus: draft\nnote: \"A: b\"\ndate: 2026-10-05\nsupersedes: 3, 5\n---\n\n# Title\n";
        let (meta, body) = front_matter(raw).unwrap();
        assert_eq!(meta.status.as_deref(), Some("draft"));
        assert_eq!(meta.note.as_deref(), Some("A: b"));
        assert_eq!(meta.date.as_deref(), Some("2026-10-05"));
        assert_eq!(meta.supersedes.as_deref(), Some("3, 5"));
        assert_eq!(body, "\n# Title\n");
    }
}
