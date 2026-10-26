//! The program's site, shot.ainit.tech, and its source code. The site speaks the six languages of
//! the UN: English at its root and the other five under their codes, so that a page has an address
//! in each, "/support" and "/ru/support".

use crate::text::Language;

/// The site's own address.
const ORIGIN: &str = "https://shot.ainit.tech";
/// The overview of the program.
pub const OVERVIEW: &str = "/";
/// The page where people support the program.
pub const SUPPORT: &str = "/support";
/// The program's source code: its mirror on GitHub, which people know and can open anywhere. The
/// repository itself lives on SourceCraft.
pub const SOURCE: &str = "https://github.com/ainit-tech/shot";

/// A page of the site in a language of the program: in that language where the site speaks it,
/// in English otherwise. `path` starts with a slash; the overview is "/".
pub fn page(language: Language, path: &str) -> String {
    match Language::UN.contains(&language) && language != Language::EN {
        true => format!("{ORIGIN}/{}{path}", language.code()),
        false => format!("{ORIGIN}{path}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_in_the_programs_language_where_the_site_speaks_it() {
        assert_eq!(page(Language::RU, "/support"), "https://shot.ainit.tech/ru/support");
        assert_eq!(page(Language::ZH, "/"), "https://shot.ainit.tech/zh/");
        assert_eq!(page(Language::AR, "/support"), "https://shot.ainit.tech/ar/support");
        assert_eq!(page(Language::EN, "/support"), "https://shot.ainit.tech/support");
        assert_eq!(page(Language::EN, "/"), "https://shot.ainit.tech/");
        for code in ["de", "hi", "uk", "fa"] {
            assert_eq!(page(Language::from_code(code).unwrap(), "/support"), "https://shot.ainit.tech/support", "{code}");
        }
    }
}
