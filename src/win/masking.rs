//! Masking a picture (decision 15): the text read in it, what the rules and OpenAI Privacy Filter
//! find there, and the rectangles that cover it. The frozen screen masks its selection with it; a
//! test runs it on pictures of its own and shows what was read and found in each.

use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use super::{models, ocr};
use crate::geometry::Rect;
use crate::image::Image;
#[cfg(test)]
use crate::mask::Found;
use crate::mask::{self, Page};

/// What masking did: the rectangles that cover the private data, in pixels of the picture, and
/// what was read and found, for the console and for looking into it.
pub struct Masking {
    pub covers: Vec<Rect>,
    pub report: String,
}

/// Masks a picture. It reads and looks for a moment, the model loaded the first time for a few
/// seconds, so it is called away from the window's thread.
pub fn mask(picture: &Image) -> Result<Masking, String> {
    let readings = ocr::read_words(picture)?;
    let mut covers = Vec::new();
    let mut report = String::new();
    for (number, lines) in readings.iter().enumerate() {
        let page = Page::new(lines);
        let _ = writeln!(report, "Reading {}:\n{}", number + 1, page.text);
        let mut found = mask::find(&page.text);
        for f in &found {
            let _ = writeln!(report, "  rules: {} {:?}", f.kind.name(), &page.text[f.range.clone()]);
        }
        let started = Instant::now();
        match models::find(&page.text) {
            Some(Ok(more)) => {
                for f in &more {
                    let _ = writeln!(report, "  model: {} {:?}", f.kind.name(), &page.text[f.range.clone()]);
                }
                let _ = writeln!(report, "  the model took {:.2} s", started.elapsed().as_secs_f32());
                found.extend(more);
            }
            Some(Err(problem)) => {
                let _ = writeln!(report, "  the model failed: {problem}");
            }
            None => {
                let _ = writeln!(report, "  the model isn't downloaded");
            }
        }
        covers.extend(page.cover(&found));
    }
    let covers = mask::joined(covers).into_iter().map(|(rect, _)| rect).collect();
    Ok(Masking { covers, report })
}

/// Saves into `folder`, under `name`, the picture, the picture with its covers drawn through,
/// and what was read and found: for looking into masking that went wrong.
pub fn keep(folder: &Path, name: &str, picture: &Image, masking: &Masking) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let save = |suffix: &str, bytes: &[u8]| std::fs::write(folder.join(format!("{name}{suffix}")), bytes).map_err(|e| e.to_string());
    save(".png", &picture.to_png()?)?;
    save(".masked.png", &marked(picture, &masking.covers).to_png()?)?;
    save(".txt", masking.report.as_bytes())
}

/// The picture with the covers drawn through: red over what they cover, the text still showing,
/// and a red frame round each.
fn marked(picture: &Image, covers: &[Rect]) -> Image {
    let mut marked = picture.clone();
    let stride = marked.stride();
    for cover in covers {
        let cover = cover.intersect(marked.bounds());
        for y in cover.top..cover.bottom {
            for x in cover.left..cover.right {
                let at = y as usize * stride + x as usize * 4;
                let edge = x == cover.left || x == cover.right - 1 || y == cover.top || y == cover.bottom - 1;
                let pixel = &mut marked.pixels[at..at + 3];
                let (strength, red): (f32, [u8; 3]) = if edge { (1.0, [40, 40, 230]) } else { (0.35, [60, 60, 230]) };
                for (channel, value) in pixel.iter_mut().zip(red) {
                    *channel = (f32::from(*channel) * (1.0 - strength) + f32::from(value) * strength) as u8;
                }
            }
        }
    }
    marked
}

/// The texts among `wanted` that the covers don't hide: a text is hidden when every word read
/// that holds a part of it is under a cover, in one of the readings at least. A text is looked for
/// as the recognition may read it: without spaces, and with the letters it reads for some digits.
#[cfg(test)]
fn missed(picture: &Image, covers: &[Rect], wanted: &[String]) -> Result<Vec<String>, String> {
    let readings = ocr::read_words(picture)?;
    let plain = |c: char| match c {
        'O' | 'o' | 'Ø' | 'ø' | 'О' | 'о' => '0',
        'I' | 'l' => '1',
        c => c,
    };
    let hidden = |text: &str| {
        let wanted: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).map(plain).collect();
        readings.iter().any(|lines| {
            let page = Page::new(lines);
            // The page's characters without spaces, as the wanted text is, with where each begins.
            let read: Vec<(char, usize)> = page.text.char_indices().filter(|(_, c)| !c.is_whitespace()).map(|(at, c)| (plain(c), at)).collect();
            let Some(first) = read.windows(wanted.len()).position(|window| window.iter().map(|(c, _)| *c).eq(wanted.iter().copied())) else {
                return false;
            };
            let (start, (last, at)) = (read[first].1, read[first + wanted.len() - 1]);
            let found = Found { range: start..at + last.len_utf8(), kind: mask::Kind::Secret };
            let words = page.cover(&[found]);
            !words.is_empty() && words.iter().all(|(word, _)| covers.iter().any(|cover| cover.intersect(*word) == *word))
        })
    };
    Ok(wanted.iter().filter(|text| !hidden(text)).cloned().collect())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

    use super::super::gdi;
    use super::*;

    /// A picture of rows of cells, each drawn from the left edge of its column: black on white in
    /// the font of Windows, as in a web page, or light grey on black in that of terminals.
    fn drawn(rows: &[Vec<&str>], columns: &[i32], terminal: bool) -> Image {
        let (width, line) = (1000, if terminal { 22 } else { 30 });
        let height = 40 + line * rows.len() as i32;
        let mut buffer = gdi::Buffer::new(width as u32, height as u32).unwrap();
        buffer.frame().pixels.fill(if terminal { 12 } else { 255 });
        let font = if terminal { gdi::Font::consolas(15) } else { gdi::Font::new(15, false) }.unwrap();
        let ink = if terminal { [204, 204, 204, 255] } else { [0, 0, 0, 255] };
        for (i, row) in rows.iter().enumerate() {
            for (cell, x) in row.iter().zip(columns) {
                let text: Vec<u16> = cell.encode_utf16().collect();
                gdi::draw_text(buffer.dc(), &font, &text, *x, 20 + line * i as i32, ink);
            }
        }
        let mut pixels = buffer.frame().pixels.to_vec();
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Image { width: width as u32, height: height as u32, pixels }
    }

    /// The addresses in the drawn records, which masking must hide wherever they stand.
    const ADDRESSES: [&str; 5] = ["203.0.113.0/24", "203.0.113.10", "198.51.100.23", "192.0.2.45", "203.0.113.77"];
    /// The addresses of version 6 in them, which the recognition of Windows doesn't read, or reads
    /// a piece of: masking can't hide them, and the bench only shows that.
    const UNREADABLE: [&str; 2] = ["2001:db8:85a3::8a2e:370:7334", "2001:db8::1"];

    /// DNS records of every kind, mixed, as a registrar's panel shows them.
    fn panel() -> Image {
        let rows = [
            ["Type", "Name", "Content", "TTL"],
            ["CNAME", "www", "example.com", "3600"],
            ["TXT", "example.com", "v=spf1 ip4:203.0.113.0/24 include:_spf.google.com ~all", "3600"],
            ["A", "example.com", "203.0.113.10", "300"],
            ["NS", "example.com", "ns1.registrar.net", "86400"],
            ["MX", "example.com", "10 mail.example.com", "3600"],
            ["AAAA", "example.com", "2001:db8:85a3::8a2e:370:7334", "3600"],
            ["A", "mail", "198.51.100.23", "3600"],
            ["CAA", "example.com", "0 issue \"letsencrypt.org\"", "3600"],
            ["SRV", "_sip._tcp", "10 60 5060 sip.example.com", "3600"],
            ["TXT", "_dmarc", "v=DMARC1; p=quarantine; rua=mailto:dmarc@example.com", "3600"],
            ["A", "vpn", "192.0.2.45", "600"],
            ["CNAME", "shop", "shops.myshopify.com", "3600"],
            ["AAAA", "www", "2001:db8::1", "3600"],
            ["NS", "example.com", "ns2.registrar.net", "86400"],
            ["TXT", "google._domainkey", "v=DKIM1; k=rsa; p=MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQ", "3600"],
            ["A", "api", "203.0.113.77", "300"],
            ["PTR", "77.113.0.203.in-addr.arpa", "api.example.com", "3600"],
        ];
        drawn(&rows.map(Vec::from), &[20, 110, 330, 900], false)
    }

    /// The same records, mixed otherwise, as dig shows them in a terminal.
    fn dig() -> Image {
        let lines = [
            "$ dig example.com ANY +noall +answer",
            "example.com.          3600   IN  TXT    \"v=spf1 ip4:203.0.113.0/24 ~all\"",
            "example.com.          300    IN  A      203.0.113.10",
            "www.example.com.      3600   IN  CNAME  example.com.",
            "example.com.          86400  IN  NS     ns1.registrar.net.",
            "mail.example.com.     3600   IN  A      198.51.100.23",
            "example.com.          3600   IN  AAAA   2001:db8:85a3::8a2e:370:7334",
            "example.com.          3600   IN  MX     10 mail.example.com.",
            "vpn.example.com.      600    IN  A      192.0.2.45",
            "_dmarc.example.com.   3600   IN  TXT    \"v=DMARC1; p=quarantine\"",
            "api.example.com.      300    IN  A      203.0.113.77",
            "shop.example.com.     3600   IN  CNAME  shops.myshopify.com.",
            "www.example.com.      3600   IN  AAAA   2001:db8::1",
        ];
        drawn(&lines.map(|line| vec![line]), &[16], true)
    }

    /// Compares the ways of preparing a picture for the recognition on the drawn DNS records: how
    /// many of their addresses each way reads, exactly and with the spaces the recognition puts
    /// after dots left out, in either reading. It needs the recognition of Windows:
    /// `cargo test -p ainitshot preparations -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn preparations_compared() {
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        let way = |up, smooth, grey, light| ocr::Preparation { up, smooth, grey, light };
        let ways = [
            ("2x, squares (as now)", way(2, false, false, false)),
            ("2x, smooth", way(2, true, false, false)),
            ("2x, smooth, grey, light", way(2, true, true, true)),
            ("3x, squares, grey, light", way(3, false, true, true)),
            ("3x, smooth", way(3, true, false, false)),
            ("3x, smooth, grey, light", way(3, true, true, true)),
            ("4x, smooth, grey, light", way(4, true, true, true)),
        ];
        for (name, picture) in [("panel", panel()), ("dig", dig())] {
            for (way, how) in ways {
                let started = std::time::Instant::now();
                let readings = ocr::read_words_prepared(&picture, how).unwrap();
                let texts: Vec<String> = readings.iter().map(|lines| Page::new(lines).text).collect();
                let read = |text: &dyn Fn(&String) -> String| ADDRESSES.iter().filter(|a| texts.iter().any(|t| text(t).contains(*a))).count();
                let exact = read(&|t| t.clone());
                let spaced = read(&|t| t.replace(". ", ".").replace(": ", ":"));
                let time = started.elapsed().as_secs_f32();
                println!("{name:>5} | {way:<26} | exact {exact}/5, spaces out {spaced}/5 | {time:.2} s");
            }
        }
    }

    /// A bench for masking. It masks pictures, says what was read, found and missed in each, and
    /// saves each one masked into `SCREENSHOT_MASKING_OUT`, or the system's folder of temporary
    /// files: the DNS records drawn here, and every picture in the folder `SCREENSHOT_MASKING`
    /// with the texts that must be hidden in a file of the same name with ".txt", one a line. It
    /// needs the recognition of Windows, and the model for names and addresses:
    /// `cargo test -p ainitshot masking_bench -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn masking_bench() {
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        let out = std::env::var_os("SCREENSHOT_MASKING_OUT").map_or_else(|| std::env::temp_dir().join("ainitshot-masking"), PathBuf::from);
        println!("The model is used: {}", models::use_downloaded());
        let addresses: Vec<String> = ADDRESSES.map(String::from).into();
        let mut cases = vec![("dns-panel".to_string(), panel(), addresses.clone()), ("dns-dig".to_string(), dig(), addresses)];
        if let Some(folder) = std::env::var_os("SCREENSHOT_MASKING") {
            let mut files: Vec<PathBuf> = std::fs::read_dir(folder).unwrap().flatten().map(|e| e.path()).collect();
            files.sort();
            for path in files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "png") && !p.to_string_lossy().contains(".masked.")) {
                let picture = Image::from_png(&std::fs::read(&path).unwrap()).unwrap();
                let wanted = std::fs::read_to_string(path.with_extension("txt")).unwrap_or_default();
                let wanted = wanted.lines().map(str::trim).filter(|line| !line.is_empty()).map(String::from).collect();
                cases.push((path.file_stem().unwrap().to_string_lossy().into(), picture, wanted));
            }
        }
        let mut missing = 0;
        for (name, picture, wanted) in &cases {
            let masking = mask(picture).unwrap();
            let missed = missed(picture, &masking.covers, wanted).unwrap();
            keep(&out, name, picture, &masking).unwrap();
            println!("=== {name}: {} covers; missed {} of {}: {missed:?}\n{}", masking.covers.len(), missed.len(), wanted.len(), masking.report);
            if name.starts_with("dns-") {
                let unreadable: Vec<String> = UNREADABLE.map(String::from).into();
                println!("    unread by Windows: {:?}", self::missed(picture, &masking.covers, &unreadable).unwrap());
            }
            missing += missed.len();
        }
        println!("The pictures, masked, are in {}", out.display());
        assert_eq!(missing, 0, "some private data stayed in sight");
    }
}
