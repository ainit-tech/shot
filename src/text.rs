//! The languages of the program: the six official languages of the UN and those of most other
//! countries. The texts are written in English where they are used, `tr("…")`; every other
//! language gives them from its catalog, `src/locales/<code>.json`: the program's own texts by
//! their English, and the texts of the pages of its windows by the names the pages give them. A
//! text a catalog lacks stays in English.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::de::{Error, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// A language the program speaks: its place among [`LANGUAGES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Language(u8);

/// The language the settings choose: that of Windows, or one of the program's own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Choice {
    #[default]
    Auto,
    Own(Language),
}

/// What the program knows of a language: its code, its name in itself, and its catalog.
struct Known {
    code: &'static str,
    native: &'static str,
    catalog: Option<&'static str>,
}

macro_rules! known {
    ($code:literal, $native:literal) => {
        Known { code: $code, native: $native, catalog: Some(include_str!(concat!("locales/", $code, ".json"))) }
    };
}

/// The languages: the six of the UN first, by how many people speak them, then Hindi, a language
/// of the documentation, then the others.
const LANGUAGES: [Known; 50] = [
    Known { code: "en", native: "English", catalog: None },
    known!("zh", "中文"),
    known!("es", "Español"),
    known!("ar", "العربية"),
    known!("fr", "Français"),
    known!("ru", "Русский"),
    known!("hi", "हिन्दी"),
    known!("pt", "Português"),
    known!("bn", "বাংলা"),
    known!("ja", "日本語"),
    known!("de", "Deutsch"),
    known!("id", "Bahasa Indonesia"),
    known!("ur", "اردو"),
    known!("tr", "Türkçe"),
    known!("vi", "Tiếng Việt"),
    known!("ko", "한국어"),
    known!("it", "Italiano"),
    known!("fa", "فارسی"),
    known!("th", "ไทย"),
    known!("pl", "Polski"),
    known!("uk", "Українська"),
    known!("ms", "Bahasa Melayu"),
    known!("fil", "Filipino"),
    known!("sw", "Kiswahili"),
    known!("am", "አማርኛ"),
    known!("my", "မြန်မာ"),
    known!("uz", "Oʻzbek"),
    known!("nl", "Nederlands"),
    known!("ro", "Română"),
    known!("kk", "Қазақ тілі"),
    known!("az", "Azərbaycan"),
    known!("el", "Ελληνικά"),
    known!("hu", "Magyar"),
    known!("cs", "Čeština"),
    known!("sv", "Svenska"),
    known!("sr", "Српски"),
    known!("he", "עברית"),
    known!("bg", "Български"),
    known!("da", "Dansk"),
    known!("fi", "Suomi"),
    known!("sk", "Slovenčina"),
    known!("nb", "Norsk"),
    known!("ka", "ქართული"),
    known!("hr", "Hrvatski"),
    known!("hy", "Հայերեն"),
    known!("sq", "Shqip"),
    known!("lt", "Lietuvių"),
    known!("sl", "Slovenščina"),
    known!("lv", "Latviešu"),
    known!("et", "Eesti"),
];

impl Language {
    pub const EN: Language = Language(0);
    pub const ZH: Language = Language(1);
    pub const ES: Language = Language(2);
    pub const AR: Language = Language(3);
    pub const FR: Language = Language(4);
    pub const RU: Language = Language(5);
    #[cfg(test)]
    pub const HI: Language = Language(6);
    /// The six official languages of the UN, by how many people speak them.
    pub const UN: [Language; 6] = [Language::EN, Language::ZH, Language::ES, Language::AR, Language::FR, Language::RU];

    /// Every language the program speaks.
    pub fn all() -> impl Iterator<Item = Language> {
        (0..LANGUAGES.len() as u8).map(Language)
    }

    fn known(self) -> &'static Known {
        &LANGUAGES[self.0 as usize]
    }

    /// The language's code: in the settings file and for the pages.
    pub fn code(self) -> &'static str {
        self.known().code
    }

    /// The language's name in itself.
    pub fn native(self) -> &'static str {
        self.known().native
    }

    /// Whether the language is written from right to left.
    pub fn right_to_left(self) -> bool {
        matches!(self.code(), "ar" | "fa" | "he" | "ur")
    }

    /// The language's code for Google, which names a few languages its own way.
    pub fn google(self) -> &'static str {
        match self.code() {
            "zh" => "zh-CN",
            "pt" => "pt-BR",
            "he" => "iw",
            "nb" => "no",
            "fil" => "tl",
            code => code,
        }
    }

    pub fn from_code(code: &str) -> Option<Language> {
        Language::all().find(|language| language.code() == code)
    }

    /// The program's language for a language tag of Windows, such as "ru-RU": English for a
    /// language it doesn't speak.
    pub fn from_tag(tag: &str) -> Language {
        let code = tag.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
        // Norwegian is written in Bokmål here, and Tagalog is Filipino.
        let code = match code.as_str() {
            "no" | "nn" => "nb",
            "tl" => "fil",
            code => code,
        };
        Language::from_code(code).unwrap_or(Language::EN)
    }
}

impl Choice {
    /// The language chosen, with that of Windows for "auto".
    pub fn resolve(self, windows: Language) -> Language {
        match self {
            Choice::Auto => windows,
            Choice::Own(language) => language,
        }
    }

    /// The choice's name in the settings file.
    pub fn name(self) -> &'static str {
        match self {
            Choice::Auto => "auto",
            Choice::Own(language) => language.code(),
        }
    }
}

impl Serialize for Choice {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Choice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        if name == "auto" {
            return Ok(Choice::Auto);
        }
        Language::from_code(&name)
            .map(Choice::Own)
            .ok_or_else(|| D::Error::invalid_value(Unexpected::Str(&name), &"\"auto\" or a language's code, such as \"en\""))
    }
}

/// The language in force. Russian until the program sets the one it starts in, so that the tests,
/// which never set it, read the texts they were written against.
static CURRENT: AtomicU8 = AtomicU8::new(Language::RU.0);

pub fn set(language: Language) {
    CURRENT.store(language.0, Ordering::Relaxed);
}

pub fn current() -> Language {
    Language(CURRENT.load(Ordering::Relaxed))
}

/// A text of the program in the language in force: its translation, or the English itself.
pub fn tr(english: &'static str) -> &'static str {
    tr_in(current(), english)
}

/// A text of the program in a language.
pub fn tr_in(language: Language, english: &'static str) -> &'static str {
    catalog(language).and_then(|catalog| catalog.program.get(english)).copied().unwrap_or(english)
}

/// A text of the program in the language in force, by its English, where the language has it.
pub fn translation(english: &str) -> Option<&'static str> {
    catalog(current()).and_then(|catalog| catalog.program.get(english)).copied()
}

/// The texts of a page of the program's windows in the language in force, by their names; the
/// page has its English itself.
pub fn page(name: &str) -> Value {
    catalog(current()).and_then(|catalog| catalog.pages.get(name)).cloned().unwrap_or(Value::Object(Map::new()))
}

/// The translations into a language.
struct Catalog {
    program: HashMap<String, &'static str>,
    pages: Map<String, Value>,
}

/// A language's catalog, read when it is first needed: the program's texts live as long as the
/// program.
fn catalog(language: Language) -> Option<&'static Catalog> {
    static CATALOGS: [OnceLock<Option<Catalog>>; LANGUAGES.len()] = [const { OnceLock::new() }; LANGUAGES.len()];
    CATALOGS[language.0 as usize].get_or_init(|| read(language.known().catalog?)).as_ref()
}

fn read(json: &str) -> Option<Catalog> {
    let Ok(Value::Object(mut sections)) = serde_json::from_str::<Value>(json) else { return None };
    let program = match sections.remove("program") {
        Some(Value::Object(texts)) => texts
            .into_iter()
            .filter_map(|(english, text)| {
                let text: &'static str = Box::leak(text.as_str()?.to_owned().into_boxed_str());
                Some((english, text))
            })
            .collect(),
        _ => HashMap::new(),
    };
    Some(Catalog { program, pages: sections })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;

    use super::*;

    #[test]
    fn windows_languages_become_the_programs_own_or_english() {
        assert_eq!(Language::from_tag("ru-RU"), Language::RU);
        assert_eq!(Language::from_tag("zh-Hans-CN"), Language::ZH);
        assert_eq!(Language::from_tag("ar-SA"), Language::AR);
        assert_eq!(Language::from_tag("en-GB"), Language::EN);
        assert_eq!(Language::from_tag("de-DE").code(), "de");
        assert_eq!(Language::from_tag("pt-PT").code(), "pt");
        assert_eq!(Language::from_tag("sr-Latn-RS").code(), "sr");
        assert_eq!(Language::from_tag("nn-NO").code(), "nb");
        assert_eq!(Language::from_tag("fil-PH").code(), "fil");
        assert_eq!(Language::from_tag("kl-GL"), Language::EN);
        assert_eq!(Choice::Auto.resolve(Language::HI), Language::HI);
        assert_eq!(Choice::Own(Language::FR).resolve(Language::EN), Language::FR);
        assert_eq!(Choice::Own(Language::ES).name(), "es");
    }

    #[test]
    fn the_languages_are_each_once_with_the_uns_first() {
        let codes: Vec<&str> = Language::all().map(Language::code).collect();
        assert_eq!(Language::UN.map(Language::code), ["en", "zh", "es", "ar", "fr", "ru"]);
        assert_eq!(Language::HI.code(), "hi");
        assert_eq!(codes.iter().collect::<HashSet<_>>().len(), codes.len(), "{codes:?}");
        let natives: HashSet<&str> = Language::all().map(Language::native).collect();
        assert_eq!(natives.len(), codes.len());
        let right_to_left: Vec<&str> = Language::all().filter(|l| l.right_to_left()).map(Language::code).collect();
        assert_eq!(right_to_left, ["ar", "ur", "fa", "he"]);
    }

    /// The texts the program writes as `tr("…")` in its code, and the comments of the settings
    /// file.
    fn program_texts() -> Vec<String> {
        let mut texts: Vec<String> = crate::settings::notes().map(String::from).into();
        let mut folders = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("text.rs") {
                    let code = fs::read_to_string(&path).unwrap().replace("tr_in(language, \"", "tr(\"");
                    // `tr("` alone, not the end of a longer name such as `push_str("`.
                    let calls = code.match_indices("tr(\"").filter(|(at, _)| {
                        code[..*at].chars().next_back().is_none_or(|c| !c.is_alphanumeric() && c != '_')
                    });
                    for (at, _) in calls {
                        let mut text = String::new();
                        let mut chars = code[at + 4..].chars();
                        while let Some(c) = chars.next() {
                            match c {
                                '"' => break,
                                '\\' => match chars.next() {
                                    Some('n') => text.push('\n'),
                                    Some(other) => text.push(other),
                                    None => {}
                                },
                                c => text.push(c),
                            }
                        }
                        texts.push(text);
                    }
                }
            }
        }
        texts
    }

    /// The placeholders of a text, such as `{name}` and `{}`, in order of their names.
    fn placeholders(text: &str) -> Vec<&str> {
        let mut found: Vec<&str> = text
            .match_indices('{')
            .filter_map(|(at, _)| {
                let end = at + text[at..].find('}')?;
                text[at + 1..end].chars().all(|c| c.is_ascii_alphabetic()).then(|| &text[at..=end])
            })
            .collect();
        found.sort_unstable();
        found
    }

    #[test]
    fn every_language_has_every_text() {
        let texts = program_texts();
        assert!(texts.len() > 100, "{}", texts.len());
        let reference = catalog(Language::RU).unwrap();
        for language in Language::all().filter(|&l| l != Language::EN) {
            let catalog = catalog(language).unwrap_or_else(|| panic!("no catalog for {}", language.code()));
            let missing: Vec<&String> = texts.iter().filter(|text| !catalog.program.contains_key(text.as_str())).collect();
            assert!(missing.is_empty(), "{} lacks {missing:#?}", language.code());
            let stale: Vec<&String> = catalog.program.keys().filter(|text| !texts.contains(text)).collect();
            assert!(stale.is_empty(), "{} has texts the program no longer has: {stale:#?}", language.code());
            for (english, text) in &catalog.program {
                assert_eq!(placeholders(text), placeholders(english), "{}: {text}", language.code());
            }
            for (page, names) in &reference.pages {
                let theirs = catalog.pages.get(page).and_then(Value::as_object);
                for (name, example) in names.as_object().unwrap() {
                    let text = theirs.and_then(|t| t.get(name)).and_then(Value::as_str);
                    let text = text.unwrap_or_else(|| panic!("{} lacks {page}.{name}", language.code()));
                    assert_eq!(placeholders(text), placeholders(example.as_str().unwrap()), "{} {page}.{name}: {text}", language.code());
                }
            }
        }
    }
}
