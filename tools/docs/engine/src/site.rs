//! Site settings from `docs/site.toml`: languages and the menu.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct Site {
    pub name: String,
    /// One-paragraph description of the project, used in `llms.txt`.
    pub summary: String,
    /// The language of the source texts; the others are translations.
    pub default_language: String,
    pub languages: Vec<Language>,
    /// Menu sections in display order.
    pub nav: Vec<NavSection>,
    /// Topics of decision records, in display order: every record names its topic in its front
    /// matter, and the decisions page groups the records by topic.
    #[serde(default)]
    pub decision_topics: Vec<Topic>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Language {
    pub code: String,
    pub name: String,
}

/// A menu section. With neither `index` nor `title`, its pages are shown at the top level.
#[derive(Debug, Deserialize, Serialize)]
pub struct NavSection {
    /// The section's own page (its folder's README.md): the section takes its title
    /// from that page and links to it, so the page has no separate menu entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<String>,
    /// Section title per language, for a section without an `index` page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<BTreeMap<String, String>>,
    /// Page ids, see [`crate::page::Page::id`].
    #[serde(default)]
    pub pages: Vec<String>,
}

/// A topic of decision records, such as the resource model or trust and access.
#[derive(Debug, Deserialize, Serialize)]
pub struct Topic {
    pub id: String,
    /// Title per language.
    pub title: BTreeMap<String, String>,
}

impl NavSection {
    /// Every page of the section in menu order, the `index` page first.
    pub fn all_pages(&self) -> impl Iterator<Item = &String> {
        self.index.iter().chain(&self.pages)
    }
}

impl Site {
    pub fn load(path: &Path) -> Result<Site> {
        let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}
