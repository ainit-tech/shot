//! Masking private data in a screenshot. The text recognition reads the words in it and where
//! each one is; a finder marks the private parts of their text, and the words of each part are
//! covered by a filled rectangle, one for the words next to each other on a line. The finder here
//! goes by rules: keys, tokens and passwords, card and bank account numbers, e-mail addresses and
//! phone numbers, which have shapes of their own.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use crate::geometry::Rect;

/// A word read in the picture: its text, and where it is in pixels of the picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub rect: Rect,
}

/// A line read in the picture: its text as the recognition put it together, and its words.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub words: Vec<Word>,
}

/// What kind of private data a part of the text is: the rules find the first four, OpenAI
/// Privacy Filter all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A key, a token or a password.
    Secret,
    /// A card or a bank account number.
    Account,
    Email,
    Phone,
    /// A person's name.
    Person,
    Address,
    Date,
    /// A link.
    Url,
    /// An IP address, of version 4 or 6.
    Ip,
}

impl Kind {
    /// The kind's name for the messages of the console.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Secret => "secret",
            Kind::Account => "account number",
            Kind::Email => "e-mail",
            Kind::Phone => "phone",
            Kind::Person => "name",
            Kind::Address => "address",
            Kind::Date => "date",
            Kind::Url => "link",
            Kind::Ip => "IP address",
        }
    }
}

/// A private part of the text: its bytes, and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub range: Range<usize>,
    pub kind: Kind,
}

/// The words read in a picture as one text, its lines one under another, with where each word is
/// in it and in the picture.
pub struct Page {
    pub text: String,
    words: Vec<Placed>,
}

/// A word of a page: its bytes in the text, where it is in the picture, and its line.
struct Placed {
    range: Range<usize>,
    rect: Rect,
    line: usize,
}

impl Page {
    pub fn new(lines: &[Line]) -> Page {
        let mut text = String::new();
        let mut words = Vec::new();
        for (number, line) in lines.iter().enumerate() {
            if number > 0 {
                text.push('\n');
            }
            let start = text.len();
            text.push_str(&line.text);
            // Each word is looked for in the line's text after the one before it; a word the
            // line doesn't have as such is added to it.
            let mut from = start;
            for word in &line.words {
                let at = match text[from..].find(&word.text) {
                    Some(at) => from + at,
                    None => {
                        text.push(' ');
                        text.push_str(&word.text);
                        text.len() - word.text.len()
                    }
                };
                from = at + word.text.len();
                words.push(Placed { range: at..from, rect: word.rect, line: number });
            }
        }
        Page { text, words }
    }

    /// The rectangles that cover the words of what was found, with the kind of what each covers:
    /// words of what was found next to each other on a line go under one rectangle. A rectangle
    /// is a little bigger than its words, so that the edges of the letters are covered too.
    pub fn cover(&self, found: &[Found]) -> Vec<(Rect, Kind)> {
        let kind_of = |word: &Placed| {
            found.iter().find(|f| f.range.start < word.range.end && word.range.start < f.range.end).map(|f| f.kind)
        };
        let mut covers: Vec<(Rect, Kind, usize)> = Vec::new();
        let mut last: Option<usize> = None;
        for (index, word) in self.words.iter().enumerate() {
            let Some(kind) = kind_of(word) else { continue };
            let next_to_last = last.is_some_and(|last| last + 1 == index);
            match covers.last_mut() {
                Some((rect, _, line)) if *line == word.line && next_to_last => *rect = rect.union(word.rect),
                _ => covers.push((word.rect, kind, word.line)),
            }
            last = Some(index);
        }
        covers
            .into_iter()
            .map(|(rect, kind, _)| {
                let room = ((rect.height() as f32 * 0.15).round() as i32).max(2);
                (rect.inflate(room), kind)
            })
            .collect()
    }
}

/// The covers with those that overlap joined into one: two readings of the same words leave one
/// cover.
pub fn joined(covers: Vec<(Rect, Kind)>) -> Vec<(Rect, Kind)> {
    let mut joined: Vec<(Rect, Kind)> = Vec::new();
    for (mut rect, kind) in covers {
        while let Some(i) = joined.iter().position(|(other, _)| !other.intersect(rect).is_empty()) {
            rect = rect.union(joined.remove(i).0);
        }
        joined.push((rect, kind));
    }
    joined
}

/// Finds the private parts of a text by their shapes.
pub fn find(text: &str) -> Vec<Found> {
    let mut found = Vec::new();
    let mut add = |range: Range<usize>, kind| found.push(Found { range, kind });
    for token in TOKENS.find_iter(text) {
        add(token.range(), Kind::Secret);
    }
    for labelled in [&*LABELLED, &*BEARER] {
        for captures in labelled.captures_iter(text) {
            if let Some(value) = captures.get(1) {
                add(value.range(), Kind::Secret);
            }
        }
    }
    for word in RANDOM.find_iter(text).filter(|word| random(word.as_str())) {
        add(word.range(), Kind::Secret);
    }
    for number in CARD.find_iter(text) {
        let digits: Vec<u8> = number.as_str().bytes().filter(u8::is_ascii_digit).collect();
        if (13..=19).contains(&digits.len()) && luhn(&digits) {
            add(number.range(), Kind::Account);
        }
    }
    for account in IBAN.find_iter(text) {
        if let Some(end) = iban_end(account.as_str()) {
            add(account.start()..account.start() + end, Kind::Account);
        }
    }
    for account in ACCOUNT.find_iter(text) {
        add(account.range(), Kind::Account);
    }
    for address in EMAIL.find_iter(text) {
        add(address.range(), Kind::Email);
    }
    for phone in PHONE.find_iter(text) {
        let digits = phone.as_str().bytes().filter(u8::is_ascii_digit).count();
        if (10..=15).contains(&digits) {
            add(phone.range(), Kind::Phone);
        }
    }
    for address in IPV4.find_iter(text).filter(|address| ipv4(address.as_str())) {
        add(address.range(), Kind::Ip);
    }
    for address in IPV6.find_iter(text).filter(|address| ipv6(address.as_str())) {
        add(address.range(), Kind::Ip);
    }
    found.sort_by_key(|f| f.range.start);
    found
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a pattern of the masking rules")
}

/// Keys and tokens of well-known services, by their prefixes.
static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    regex(concat!(
        r"\bsk-(?:proj-|ant-[a-z]+\d*-)?[A-Za-z0-9_\-]{20,}",
        r"|\bgh[pousr]_[A-Za-z0-9]{30,}|\bgithub_pat_[A-Za-z0-9_]{30,}",
        r"|\b(?:AKIA|ASIA)[A-Z0-9]{16}\b",
        r"|\bAIza[0-9A-Za-z_\-]{30,}",
        r"|\bxox[abposr]-[A-Za-z0-9\-]{10,}",
        r"|\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}",
        r"|\bglpat-[A-Za-z0-9_\-]{20,}",
        r"|\bhf_[A-Za-z0-9]{30,}",
        r"|\bnpm_[A-Za-z0-9]{36}\b",
        r"|\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
        r"|-----BEGIN[A-Z ]*PRIVATE KEY-----",
    ))
});

/// A value after a word that says it is secret: "password: …", "api_key=…", «пароль: …».
static LABELLED: LazyLock<Regex> = LazyLock::new(|| {
    regex(concat!(
        r"(?i)\b(?:password|passwd|pwd|passphrase|secret|token|api[_\- ]?key|apikey|access[_\- ]?key",
        r"|secret[_\- ]?key|private[_\- ]?key|client[_\- ]?secret|пароль|токен|ключ)",
        r#"["']?\s*[:=]\s*["']?([^\s"']{3,})"#,
    ))
});

static BEARER: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)\bbearer\s+([A-Za-z0-9._~+/\-]{8,}=*)"));

/// A long word of letters and digits, a key without a known prefix if it looks random.
static RANDOM: LazyLock<Regex> = LazyLock::new(|| regex(r"[A-Za-z0-9+/=_\-]{24,}"));

/// 13 to 19 digits, maybe in groups: a card number, if it passes the Luhn check.
static CARD: LazyLock<Regex> = LazyLock::new(|| regex(r"\b\d(?:[ \-]?\d){12,18}\b"));

/// An IBAN in groups of four, if it passes its check.
static IBAN: LazyLock<Regex> = LazyLock::new(|| regex(r"\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]{4}){2,7}(?: ?[A-Z0-9]{1,3})?\b"));

/// A bank account in Russia: 20 digits.
static ACCOUNT: LazyLock<Regex> = LazyLock::new(|| regex(r"\b\d{20}\b"));

static EMAIL: LazyLock<Regex> = LazyLock::new(|| regex(r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b"));

/// A phone number: international, with "+"; Russian, from 8; or American, with the area code
/// in brackets.
static PHONE: LazyLock<Regex> = LazyLock::new(|| {
    regex(concat!(
        r"\+\d{1,3}[ \-.]?\(?\d{1,4}\)?(?:[ \-.]?\d{2,4}){2,4}\b",
        r"|\b8[ \-]?\(?\d{3}\)?[ \-]?\d{3}[ \-]?\d{2}[ \-]?\d{2}\b",
        r"|\(\d{3}\) ?\d{3}[ \-]\d{4}\b",
    ))
});

/// An IP address of version 4, maybe with the length of its network, as the recognition reads it:
/// zeros may come as the letters O and Ø, ones as I and l, a space may stand by a dot, and one
/// dot may be read as a space.
static IPV4: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b[0-9OoØøОоIl]{1,3}(?:(?:\s?\.\s?|\s)[0-9OoØøОоIl]{1,3}){3}(?:/\d{1,2})?\b"));

/// An IP address of version 6, its groups of hexadecimal digits apart by colons, a space by a colon
/// as the recognition may read it.
static IPV6: LazyLock<Regex> = LazyLock::new(|| regex(r"\b(?:[0-9A-Fa-fØ]{0,4}\s?:\s?){2,7}[0-9A-Fa-fØ]{1,4}(?:/\d{1,3})?\b"));

/// The digit a letter that the recognition reads in place of a digit stands for.
fn digit(c: char) -> Option<u32> {
    match c {
        'O' | 'o' | 'Ø' | 'ø' | 'О' | 'о' => Some(0),
        'I' | 'l' => Some(1),
        c => c.to_digit(10),
    }
}

/// Whether what looks like an IP address of version 4 is one: four numbers up to 255, most of
/// whose characters are real digits, at least two of them after dots, so that a row of numbers
/// apart by spaces isn't one.
fn ipv4(text: &str) -> bool {
    let address = text.split('/').next().unwrap_or(text);
    let numbers: Vec<Option<u32>> = address
        .split(|c: char| c == '.' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| part.chars().try_fold(0, |number, c| Some(number * 10 + digit(c)?)))
        .collect();
    let real = address.chars().filter(char::is_ascii_digit).count();
    let dots = address.matches('.').count();
    numbers.len() == 4 && numbers.iter().all(|n| n.is_some_and(|n| n <= 255)) && real >= 4 && dots >= 2
}

/// Whether what looks like an IP address of version 6 is one, and not a time of day: it has a
/// double colon, or more than five groups, or letters among its digits.
fn ipv6(text: &str) -> bool {
    let address: String = text.split('/').next().unwrap_or(text).chars().filter(|c| !c.is_whitespace()).collect();
    let groups = address.split(':').count();
    let letters = address.chars().any(|c| c.is_ascii_alphabetic());
    let digits = address.chars().filter(char::is_ascii_hexdigit).count();
    (address.contains("::") || groups > 5 || letters) && groups >= 3 && digits >= 3
}

/// Whether a long word looks like a random key: it has capital and small letters and digits, and
/// its characters vary as random ones do.
fn random(word: &str) -> bool {
    let count = |test: fn(&char) -> bool| word.chars().filter(test).count();
    if count(char::is_ascii_uppercase) < 2 || count(char::is_ascii_lowercase) < 2 || count(char::is_ascii_digit) < 2 {
        return false;
    }
    let mut seen: HashMap<char, f32> = HashMap::new();
    for c in word.chars() {
        *seen.entry(c).or_default() += 1.0;
    }
    let length = word.chars().count() as f32;
    let entropy: f32 = seen.values().map(|n| -(n / length) * (n / length).log2()).sum();
    entropy >= 3.5
}

/// The Luhn check of a card number.
fn luhn(digits: &[u8]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            let d = u32::from(d - b'0');
            match i % 2 {
                1 if d * 2 > 9 => d * 2 - 9,
                1 => d * 2,
                _ => d,
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Where in the text an IBAN that passes its check ends: the longest that does, from the whole
/// text down by its groups, since a word in capitals after it may look like one more group.
fn iban_end(text: &str) -> Option<usize> {
    let mut ends: Vec<usize> = text.match_indices(' ').map(|(i, _)| i).chain([text.len()]).collect();
    while let Some(end) = ends.pop() {
        if iban_check(&text[..end]) {
            return Some(end);
        }
    }
    None
}

/// The check of an IBAN: its first four letters and digits go to its end, letters become
/// numbers from 10, and the number left after dividing by 97 is 1.
fn iban_check(text: &str) -> bool {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() < 15 || chars.len() > 34 {
        return false;
    }
    let mut rest = 0u32;
    for c in chars[4..].iter().chain(&chars[..4]) {
        let Some(value) = c.to_digit(36) else { return false };
        rest = if value >= 10 { (rest * 100 + value) % 97 } else { (rest * 10 + value) % 97 };
    }
    rest == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line of words standing 10 pixels apart, each 8 pixels a letter wide, 20 high.
    fn line(text: &str, y: i32) -> Line {
        let mut x = 0;
        let words = text
            .split(' ')
            .map(|word| {
                let width = 8 * word.chars().count() as i32;
                let rect = Rect::new(x, y, x + width, y + 20);
                x += width + 10;
                Word { text: word.into(), rect }
            })
            .collect();
        Line { text: text.into(), words }
    }

    fn kinds(text: &str) -> Vec<(String, Kind)> {
        find(text).into_iter().map(|f| (text[f.range].to_string(), f.kind)).collect()
    }

    #[test]
    fn keys_tokens_and_passwords_are_secrets() {
        assert_eq!(kinds("export OPENAI_API_KEY sk-proj-abcdefghijklmnopqrstuvwx1234"), [(
            "sk-proj-abcdefghijklmnopqrstuvwx1234".into(),
            Kind::Secret
        )]);
        assert_eq!(kinds("AKIAIOSFODNN7EXAMPLE is mine"), [("AKIAIOSFODNN7EXAMPLE".into(), Kind::Secret)]);
        assert_eq!(kinds("password: hunter2"), [("hunter2".into(), Kind::Secret)]);
        assert_eq!(kinds("Пароль = qwerty123"), [("qwerty123".into(), Kind::Secret)]);
        assert_eq!(kinds("Authorization: Bearer abc.def-ghi_jkl"), [("abc.def-ghi_jkl".into(), Kind::Secret)]);
        assert_eq!(kinds("key 9fK2xQ7mL4pZ8vB1nR6tY3wE5uI0oA"), [("9fK2xQ7mL4pZ8vB1nR6tY3wE5uI0oA".into(), Kind::Secret)]);
    }

    #[test]
    fn ordinary_text_has_nothing_private() {
        assert!(find("The file could not be saved: the disk is full. Try again in 5 minutes.").is_empty());
        assert!(find("Файл не удалось сохранить, 2026-10-25 14:05, версия 0.1.0").is_empty());
        assert!(find("commit 3f2a9c1b7e4d5f6a8b9c0d1e2f3a4b5c6d7e8f9a").is_empty());
        assert!(find("CamelCaseWordsWithoutAnyDigitsAtAll").is_empty());
    }

    #[test]
    fn numbers_of_cards_and_accounts_pass_their_checks() {
        assert_eq!(kinds("card 4111 1111 1111 1111 exp"), [("4111 1111 1111 1111".into(), Kind::Account)]);
        assert!(find("order 4111 1111 1111 1112").is_empty());
        assert_eq!(kinds("IBAN DE89 3704 0044 0532 0130 00 IS OURS"), [("DE89 3704 0044 0532 0130 00".into(), Kind::Account)]);
        assert_eq!(kinds("to GB29 NWBK 6016 1331 9268 19 ABCD"), [("GB29 NWBK 6016 1331 9268 19".into(), Kind::Account)]);
        assert!(find("GB29 NWBK 6016 1331 9268 18").is_empty());
        assert_eq!(iban_end("AT61 1904 3002 3457 3201 TEST"), Some(24));
        assert_eq!(kinds("счёт 40817810099910004312"), [("40817810099910004312".into(), Kind::Account)]);
    }

    #[test]
    fn e_mails_and_phones_are_found() {
        assert_eq!(kinds("write to anna.k@example.com today"), [("anna.k@example.com".into(), Kind::Email)]);
        assert_eq!(kinds("call +7 916 123-45-67 now"), [("+7 916 123-45-67".into(), Kind::Phone)]);
        assert_eq!(kinds("звоните 8 (916) 123-45-67"), [("8 (916) 123-45-67".into(), Kind::Phone)]);
        assert_eq!(kinds("tel. (212) 555-0123"), [("(212) 555-0123".into(), Kind::Phone)]);
    }

    /// IP addresses are found as the recognition reads them, with letters for some digits and
    /// spaces by dots and colons; numbers that only look like them aren't.
    #[test]
    fn ip_addresses_are_found_as_they_are_read() {
        let ip = |address: &str| vec![(address.to_string(), Kind::Ip)];
        assert_eq!(kinds("A example.com 203.0.113.10 300"), ip("203.0.113.10"));
        assert_eq!(kinds("v=spf1 ip4:203.0.113.0/24 ~all"), ip("203.0.113.0/24"));
        assert_eq!(kinds("198.51.1ØØ.23"), ip("198.51.1ØØ.23"));
        assert_eq!(kinds("api 203 .0. 113.77"), ip("203 .0. 113.77"));
        assert_eq!(kinds("ip4:2Ø3.Ø.113.Ø/24"), ip("2Ø3.Ø.113.Ø/24"));
        assert_eq!(kinds("docs 84 252.133.197"), ip("84 252.133.197"));
        assert!(find("pages 1 2 3 4 and 10 20 30.5").is_empty());
        assert_eq!(kinds("AAAA 2001:db8::1 3600"), ip("2001:db8::1"));
        assert_eq!(kinds("address 20Ø1:db8: : 1 here"), ip("20Ø1:db8: : 1"));
        assert_eq!(kinds("fe80::1ff:fe23:4567:890a"), ip("fe80::1ff:fe23:4567:890a"));
        for not in ["Windows 10.0.22000.1", "1.2.3 is out", "999.1.1.1", "at 14:05:33", "16:9", "O.O.O.O"] {
            assert!(find(not).is_empty(), "{not}");
        }
    }

    /// The words of what was found are covered: next to each other under one rectangle, on
    /// different lines or apart under their own.
    #[test]
    fn found_words_are_covered_line_by_line() {
        let lines = [line("Card: 4111 1111 1111 1111", 0), line("Mail anna@example.com or call", 30)];
        let page = Page::new(&lines);
        assert_eq!(page.text, "Card: 4111 1111 1111 1111\nMail anna@example.com or call");
        let covers = page.cover(&find(&page.text));
        let words = |line: usize, from: usize, to: usize| {
            let (first, last) = (&lines[line].words[from], &lines[line].words[to]);
            (first.rect.union(last.rect).inflate(3), if line == 0 { Kind::Account } else { Kind::Email })
        };
        assert_eq!(covers, [words(0, 1, 4), words(1, 1, 1)]);
    }

    /// Covers of two readings of the same words become one; covers apart stay apart.
    #[test]
    fn overlapping_covers_are_joined() {
        let covers = vec![
            (Rect::new(10, 10, 50, 30), Kind::Email),
            (Rect::new(100, 10, 140, 30), Kind::Phone),
            (Rect::new(12, 8, 60, 28), Kind::Email),
            (Rect::new(45, 12, 110, 26), Kind::Secret),
        ];
        assert_eq!(joined(covers.clone()), [(Rect::new(10, 8, 140, 30), Kind::Secret)]);
        assert_eq!(joined(covers[..3].to_vec()), [(Rect::new(100, 10, 140, 30), Kind::Phone), (Rect::new(10, 8, 60, 30), Kind::Email)]);
    }

    /// The recognition may read a line with words its text puts otherwise: each is found where it
    /// is, or added.
    #[test]
    fn words_are_found_in_the_text_of_their_line() {
        let mut odd = line("a b", 0);
        odd.words.push(Word { text: "c".into(), rect: Rect::new(40, 0, 48, 20) });
        let page = Page::new(&[odd]);
        assert_eq!(page.text, "a b c");
        let covers = page.cover(&[Found { range: 4..5, kind: Kind::Secret }]);
        assert_eq!(covers, [(Rect::new(37, -3, 51, 23), Kind::Secret)]);
    }
}
