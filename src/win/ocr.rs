//! Reading the text in a screenshot with the text recognition built into Windows (decision 2):
//! on the computer, without the internet, in the languages of the user's Windows.

use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::System::UserProfile::GlobalizationPreferences;

use crate::geometry::Rect;
use crate::image::Image;
use crate::mask;
use crate::text::tr;

/// The text in the picture, its lines one under another; empty if there is none. It runs for a
/// moment, so it is called away from the window's thread.
pub fn read(picture: &Image) -> Result<String, String> {
    let (lines, english) = recognize(picture, Preparation::BEST)?;
    let text = |lines: Vec<Line>| lines.into_iter().map(|line| line.text).collect::<Vec<_>>().join("\n");
    let Some(english) = english else { return Ok(text(lines)) };
    // A line mostly in Latin letters is English: the English recognition reads it better.
    let mixed = lines
        .into_iter()
        .map(|line| {
            if !mostly_latin(&line.text) {
                return line;
            }
            let same: Vec<&Line> = english.iter().filter(|other| line.has(other)).collect();
            if same.is_empty() { line } else { Line::joined(&same) }
        })
        .collect();
    Ok(text(mixed))
}

/// The lines in the picture with their words, and where each word is in it, for masking: as the
/// recognition of the user's language reads them, and as the English one does when it reads too.
/// Each may misread what the other reads right, as the Russian one reads the digits of a card
/// number as Cyrillic letters, so masking looks at both.
pub fn read_words(picture: &Image) -> Result<Vec<Vec<mask::Line>>, String> {
    read_words_prepared(picture, Preparation::BEST)
}

/// The same, with the picture prepared for the recognition as `how` says.
pub fn read_words_prepared(picture: &Image, how: Preparation) -> Result<Vec<Vec<mask::Line>>, String> {
    let (lines, english) = recognize(picture, how)?;
    let words = |lines: Vec<Line>| lines.into_iter().map(|line| mask::Line { text: line.text, words: line.words }).collect();
    Ok([Some(lines), english].into_iter().flatten().map(words).collect())
}

/// How a picture is made ready for the recognition, which reads big dark letters on a light
/// ground best.
#[derive(Clone, Copy, Debug)]
pub struct Preparation {
    /// How many times bigger a small picture is made, at most.
    pub up: u32,
    /// Whether a picture made bigger is smoothed, or each of its pixels becomes a square.
    pub smooth: bool,
    /// Whether it is made grey, without the colours that ClearType gives the edges of letters.
    pub grey: bool,
    /// Whether light letters on a dark ground, as in terminals, become dark letters on light.
    pub light: bool,
}

impl Preparation {
    /// Twice as big and smoothed: on the drawn records of the masking bench it read 5 and 4
    /// addresses of 7 exactly, while each pixel made a square read none; grey, a light ground,
    /// or three or four times bigger read no more.
    pub const BEST: Preparation = Preparation { up: 2, smooth: true, grey: false, light: false };
}

/// The lines in the picture as the recognition of the user's language reads them, and as the
/// English one does, when it reads too.
fn recognize(picture: &Image, how: Preparation) -> Result<(Vec<Line>, Option<Vec<Line>>), String> {
    let (engine, english) = engines()?;
    let largest = OcrEngine::MaxImageDimension().unwrap_or(2600);
    let (read, scale) = prepared(picture, how, largest);
    let failed = |error: windows_core::Error| {
        format!("{} {}", tr("The text couldn't be read:"), error.message())
    };
    let pixels = CryptographicBuffer::CreateFromByteArray(&read.pixels).map_err(failed)?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &pixels,
        BitmapPixelFormat::Bgra8,
        read.width as i32,
        read.height as i32,
        BitmapAlphaMode::Ignore,
    )
    .map_err(failed)?;
    let lines = read_lines(&engine, &bitmap, scale).map_err(failed)?;
    let english = english.map(|english| read_lines(&english, &bitmap, scale)).transpose().map_err(failed)?;
    Ok((lines, english))
}

/// A line read in the picture: its text, its words, and where they are, in pixels of the picture.
#[derive(Clone)]
struct Line {
    text: String,
    words: Vec<mask::Word>,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Line {
    /// Whether another reading's line is this one: its middle is level with this line, and they
    /// overlap side to side.
    fn has(&self, other: &Line) -> bool {
        let middle = (other.top + other.bottom) / 2.0;
        (self.top..=self.bottom).contains(&middle) && other.left < self.right && other.right > self.left
    }

    /// Lines of another reading as one line.
    fn joined(lines: &[&Line]) -> Line {
        let text = lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>().join(" ");
        let words = lines.iter().flat_map(|line| line.words.iter().cloned()).collect();
        let left = lines.iter().map(|line| line.left).fold(f32::MAX, f32::min);
        let top = lines.iter().map(|line| line.top).fold(f32::MAX, f32::min);
        let right = lines.iter().map(|line| line.right).fold(f32::MIN, f32::max);
        let bottom = lines.iter().map(|line| line.bottom).fold(f32::MIN, f32::max);
        Line { text, words, left, top, right, bottom }
    }
}

/// The lines that a recognition reads in the picture, with their places times `scale`: the
/// picture read may be bigger or smaller than the one given.
fn read_lines(engine: &OcrEngine, bitmap: &SoftwareBitmap, scale: f32) -> windows_core::Result<Vec<Line>> {
    let result = engine.RecognizeAsync(bitmap)?.join()?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        let mut found = Line {
            text: line.Text()?.to_string(),
            words: Vec::new(),
            left: f32::MAX,
            top: f32::MAX,
            right: f32::MIN,
            bottom: f32::MIN,
        };
        for word in line.Words()? {
            let r = word.BoundingRect()?;
            let (left, top, right, bottom) = (r.X * scale, r.Y * scale, (r.X + r.Width) * scale, (r.Y + r.Height) * scale);
            found.left = found.left.min(left);
            found.top = found.top.min(top);
            found.right = found.right.max(right);
            found.bottom = found.bottom.max(bottom);
            let rect = Rect::new(left.floor() as i32, top.floor() as i32, right.ceil() as i32, bottom.ceil() as i32);
            found.words.push(mask::Word { text: word.Text()?.to_string(), rect });
        }
        lines.push(found);
    }
    Ok(lines)
}

/// Whether most of the letters of the text are Latin ones.
fn mostly_latin(text: &str) -> bool {
    let letters = text.chars().filter(|c| c.is_alphabetic());
    let (latin, other) = letters.fold((0, 0), |(latin, other), c| match c {
        'a'..='z' | 'A'..='Z' | '\u{C0}'..='\u{24F}' => (latin + 1, other),
        _ => (latin, other + 1),
    });
    latin > other
}

/// The recognition to read with: of the first of the user's languages that isn't English, if
/// Windows can read it, since such a recognition reads English letters as well, while the English
/// one reads no others; of English otherwise. When that language isn't written in Latin letters,
/// it reads English words with letters of its own, so the English recognition comes too, for the
/// lines in English.
fn engines() -> Result<(OcrEngine, Option<OcrEngine>), String> {
    let readable = |tag: &windows_core::HSTRING| {
        let language = Language::CreateLanguage(tag).ok()?;
        OcrEngine::IsLanguageSupported(&language).unwrap_or(false).then_some(language)
    };
    let other = GlobalizationPreferences::Languages()
        .ok()
        .into_iter()
        .flatten()
        .filter(|tag| !tag.to_string().starts_with("en"))
        .find_map(|tag| readable(&tag));
    let Some(language) = other else {
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()
            .map_err(|_| {
                tr("Windows has no text recognition for the languages chosen in it.")
                .to_string()
            })?;
        return Ok((engine, None));
    };
    let engine = OcrEngine::TryCreateFromLanguage(&language).map_err(|error| error.message())?;
    let latin = language.Script().is_ok_and(|script| script == "Latn");
    let english = (!latin)
        .then(|| OcrEngine::AvailableRecognizerLanguages().ok())
        .flatten()
        .into_iter()
        .flatten()
        .find(|language| language.LanguageTag().is_ok_and(|tag| tag.to_string().starts_with("en")))
        .and_then(|language| OcrEngine::TryCreateFromLanguage(&language).ok());
    Ok((engine, english))
}

/// The picture prepared for the recognition, and how many of its pixels make one of the one read:
/// a small picture made bigger, a picture too big for the recognition made smaller.
fn prepared(picture: &Image, how: Preparation, largest: u32) -> (Image, f32) {
    let longer = picture.width.max(picture.height);
    let up = (1..=how.up.max(1)).rev().find(|up| longer * up <= largest).unwrap_or(1);
    let (mut read, scale) = if longer > largest {
        let down = longer.div_ceil(largest);
        (resized(picture, 1, down), down as f32)
    } else if up > 1 && how.smooth {
        (smoothed(picture, up), 1.0 / up as f32)
    } else if up > 1 {
        (resized(picture, up, 1), 1.0 / up as f32)
    } else {
        (picture.clone(), 1.0)
    };
    if how.grey || how.light {
        toned(&mut read, how.grey, how.light);
    }
    (read, scale)
}

/// The picture `up` times bigger, smoothed: each new pixel mixes the four old ones nearest to it.
fn smoothed(picture: &Image, up: u32) -> Image {
    let (width, height) = (picture.width * up, picture.height * up);
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let near = |at: u32, of: u32| {
        let from = ((at as f32 + 0.5) / up as f32 - 0.5).max(0.0);
        let first = (from as u32).min(of - 1);
        (first, (first + 1).min(of - 1), from - first as f32)
    };
    for y in 0..height {
        let (y0, y1, fy) = near(y, picture.height);
        for x in 0..width {
            let (x0, x1, fx) = near(x, picture.width);
            let at = |x: u32, y: u32, c: usize| f32::from(picture.pixels[(y * picture.width + x) as usize * 4 + c]);
            for c in 0..4 {
                let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
                let bottom = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
                pixels[(y * width + x) as usize * 4 + c] = (top * (1.0 - fy) + bottom * fy).round() as u8;
            }
        }
    }
    Image { width, height, pixels }
}

/// Makes the picture grey, and its letters dark on a light ground if the ground is dark.
fn toned(picture: &mut Image, grey: bool, light: bool) {
    let brightness = |p: &[u8]| (u32::from(p[2]) * 299 + u32::from(p[1]) * 587 + u32::from(p[0]) * 114) / 1000;
    let count = picture.pixels.len() as u64 / 4;
    let total: u64 = picture.pixels.chunks_exact(4).map(|p| u64::from(brightness(p))).sum();
    let invert = light && count > 0 && total / count < 128;
    for pixel in picture.pixels.chunks_exact_mut(4) {
        if grey {
            let value = brightness(pixel) as u8;
            pixel[..3].fill(value);
        }
        if invert {
            for channel in &mut pixel[..3] {
                *channel = 255 - *channel;
            }
        }
    }
}

/// The picture `up` times bigger, each pixel a square of them, or `down` times smaller, each
/// pixel the first of its square.
fn resized(picture: &Image, up: u32, down: u32) -> Image {
    let (width, height) = (picture.width * up / down, picture.height * up / down);
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        let row = (y * down / up) as usize * picture.stride();
        for x in 0..width {
            let at = row + (x * down / up) as usize * 4;
            pixels.extend_from_slice(&picture.pixels[at..at + 4]);
        }
    }
    Image { width, height, pixels }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

    use super::super::gdi;
    use super::*;

    /// A screenshot of lines of text, black on white, at the size of the screen's letters.
    fn screenshot(lines: &[&str]) -> Image {
        let (width, height) = (640, 40 + 28 * lines.len() as i32);
        let mut buffer = gdi::Buffer::new(width as u32, height as u32).unwrap();
        buffer.frame().pixels.fill(255);
        let font = gdi::Font::new(15, false).unwrap();
        for (i, line) in lines.iter().enumerate() {
            let text: Vec<u16> = line.encode_utf16().collect();
            gdi::draw_text(buffer.dc(), &font, &text, 20, 20 + 28 * i as i32, [0, 0, 0, 255]);
        }
        let mut pixels = buffer.frame().pixels.to_vec();
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Image { width: width as u32, height: height as u32, pixels }
    }

    /// Reads a screenshot with a line of English and a line of Russian. It needs the text
    /// recognition of Windows for the user's languages, so it runs only when asked:
    /// `cargo test -p ainitshot reads_text -- --ignored`.
    #[test]
    #[ignore]
    fn reads_text_in_english_and_russian() {
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        let text = read(&screenshot(&["Error: the file could not be saved", "Ошибка: файл не удалось сохранить"])).unwrap();
        println!("{text}");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].contains("could not be saved"), "{text}");
        assert!(lines[1].contains("не удалось сохранить"), "{text}");
    }

    /// Reads the words of a screenshot with where each one is, and masks what is private in it:
    /// the covers hide the words of the e-mail, the card number, the password and the phone
    /// number, and touch no other word. It needs the text recognition of Windows:
    /// `cargo test -p ainitshot masks_what -- --ignored`.
    #[test]
    #[ignore]
    fn masks_what_it_reads_where_it_is() {
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        let picture = screenshot(&[
            "Write to anna.k@example.com today",
            "Card 4242 4242 4242 4242 is mine",
            "password: hunter2xyz",
            "Phone +7 916 123-45-67 is mine",
        ]);
        let readings = read_words(&picture).unwrap();
        let mut covers = Vec::new();
        for lines in &readings {
            let page = mask::Page::new(lines);
            println!("{}", page.text);
            covers.extend(page.cover(&mask::find(&page.text)));
        }
        let covers: Vec<Rect> = mask::joined(covers).into_iter().map(|(rect, _)| rect).collect();
        println!("{covers:?}");
        assert_eq!(covers.len(), 4);
        let private = |word: &str| word.contains('@') || word.starts_with(['4', '+', '9', '1']) || word == "hunter2xyz";
        for word in readings.iter().flatten().flat_map(|line| &line.words) {
            let hidden = covers.iter().any(|cover| cover.intersect(word.rect) == word.rect);
            let touched = covers.iter().any(|cover| !cover.intersect(word.rect).is_empty());
            assert!(if private(&word.text) { hidden } else { !touched }, "{word:?}");
        }
    }

    #[test]
    fn small_pictures_are_doubled_and_big_ones_shrunk() {
        let picture = Image { width: 2, height: 1, pixels: vec![1, 2, 3, 4, 5, 6, 7, 8] };
        let doubled = resized(&picture, 2, 1);
        assert_eq!((doubled.width, doubled.height), (4, 2));
        assert_eq!(&doubled.pixels[..16], &[1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8, 5, 6, 7, 8]);
        let shrunk = resized(&doubled, 1, 2);
        assert_eq!(shrunk.pixels, picture.pixels);
    }
}
