//! Reading the text in a screenshot with the text recognition built into Windows (decision 2):
//! on the computer, without the internet, in the languages of the user's Windows.

use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::System::UserProfile::GlobalizationPreferences;

use crate::image::Image;
use crate::text::tr;

/// The text in the picture, its lines one under another; empty if there is none. It runs for a
/// moment, so it is called away from the window's thread.
pub fn read(picture: &Image) -> Result<String, String> {
    let (engine, english) = engines()?;
    let largest = OcrEngine::MaxImageDimension().unwrap_or(2600);
    let longer = picture.width.max(picture.height);
    // Small letters on the screen are read better twice as big; a picture too big for the
    // recognition is made smaller.
    let picture = if longer * 2 <= largest {
        resized(picture, 2, 1)
    } else if longer > largest {
        resized(picture, 1, longer.div_ceil(largest))
    } else {
        picture.clone()
    };
    let failed = |error: windows_core::Error| {
        format!("{} {}", tr("The text couldn't be read:"), error.message())
    };
    let pixels = CryptographicBuffer::CreateFromByteArray(&picture.pixels).map_err(failed)?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &pixels,
        BitmapPixelFormat::Bgra8,
        picture.width as i32,
        picture.height as i32,
        BitmapAlphaMode::Ignore,
    )
    .map_err(failed)?;
    let lines = read_lines(&engine, &bitmap).map_err(failed)?;
    let Some(english) = english else {
        return Ok(lines.into_iter().map(|line| line.text).collect::<Vec<_>>().join("\n"));
    };
    // A line mostly in Latin letters is English: the English recognition reads it better.
    let english = read_lines(&english, &bitmap).map_err(failed)?;
    let mixed: Vec<String> = lines
        .into_iter()
        .map(|line| {
            if !mostly_latin(&line.text) {
                return line.text;
            }
            let same: Vec<&str> = english.iter().filter(|other| line.has(other)).map(|other| other.text.as_str()).collect();
            if same.is_empty() { line.text } else { same.join(" ") }
        })
        .collect();
    Ok(mixed.join("\n"))
}

/// A line read in the picture: its text and where its words are.
struct Line {
    text: String,
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
}

/// The lines that a recognition reads in the picture.
fn read_lines(engine: &OcrEngine, bitmap: &SoftwareBitmap) -> windows_core::Result<Vec<Line>> {
    let result = engine.RecognizeAsync(bitmap)?.join()?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        let mut found = Line { text: line.Text()?.to_string(), left: f32::MAX, top: f32::MAX, right: f32::MIN, bottom: f32::MIN };
        for word in line.Words()? {
            let r = word.BoundingRect()?;
            found.left = found.left.min(r.X);
            found.top = found.top.min(r.Y);
            found.right = found.right.max(r.X + r.Width);
            found.bottom = found.bottom.max(r.Y + r.Height);
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
