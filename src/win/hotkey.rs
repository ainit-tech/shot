//! Keys by their names in the settings: the hotkey such as "Ctrl+~" or "PrintScreen", and the
//! letters of the query buttons; the hotkey taken in Windows, on the same key whatever the
//! keyboard layout; whether a hotkey is free; and how the key left of 1 is labelled.

use std::cell::Cell;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Globalization::LCIDToLocaleName;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, HKL, HOT_KEY_MODIFIERS, MAPVK_VK_TO_CHAR, MAPVK_VSC_TO_VK_EX, MOD_ALT,
    MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, MapVirtualKeyExW, RegisterHotKey, UnregisterHotKey, VK_0, VK_A,
    VK_DELETE, VK_END, VK_F1, VK_HOME, VK_INSERT, VK_NEXT, VK_OEM_3, VK_PAUSE, VK_PRIOR, VK_SNAPSHOT, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, KillTimer, SetTimer};

use crate::text::{Language, tr};

/// A hotkey: the modifier keys and the key itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: HOT_KEY_MODIFIERS,
    pub key: Key,
}

/// A key of a hotkey. Windows knows a key by the code the keyboard layout gives it. Letters,
/// digits and named keys keep their codes in every layout: a letter is the key with that Latin
/// letter on it, or, in a layout without Latin letters, the key where English has the letter. The
/// key left of 1 has a code of its own in each layout: ` in English and Ё in Russian are VK_OEM_3,
/// ^ in German is VK_OEM_5, ² in French VK_OEM_7, ` in British VK_OEM_8, 0 in Hungarian VK_0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Code(u16),
    LeftOfOne,
}

/// The scan code of the key left of 1, the same on every keyboard.
const LEFT_OF_ONE: u32 = 0x29;

impl Key {
    /// The key's code in a keyboard layout. On Japanese keyboards the key left of 1 switches the
    /// input method and types nothing; there the key with ` is taken, as in English.
    pub fn code(self, layout: HKL) -> u16 {
        match self {
            Key::Code(code) => code,
            Key::LeftOfOne => match unsafe { MapVirtualKeyExW(LEFT_OF_ONE, MAPVK_VSC_TO_VK_EX, layout) } {
                0 => VK_OEM_3,
                code if unsafe { MapVirtualKeyExW(code, MAPVK_VK_TO_CHAR, layout) } == 0 => VK_OEM_3,
                code => code as u16,
            },
        }
    }
}

/// Reads a hotkey written as keys joined by "+": "Z", "Ctrl+~", "Ctrl+Shift+S", "PrintScreen".
pub fn parse(text: &str) -> Result<Hotkey, String> {
    let mut modifiers = MOD_NOREPEAT;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "shift" => modifiers |= MOD_SHIFT,
            "alt" => modifiers |= MOD_ALT,
            "win" => modifiers |= MOD_WIN,
            name => {
                if key.is_some() {
                    let problem = tr("“{text}” has two keys, and one is needed, with Ctrl, Shift, Alt or Win.");
                    return Err(problem.replace("{text}", text));
                }
                let unknown = || {
                    let problem = tr("The key “{part}” in “{text}” is unknown.");
                    problem.replace("{part}", part).replace("{text}", text)
                };
                key = Some(key_named(name).ok_or_else(unknown)?);
            }
        }
    }
    let missing = || {
        tr("“{text}” has no key itself.").replace("{text}", text)
    };
    let key = key.ok_or_else(missing)?;
    Ok(Hotkey { modifiers, key })
}

/// A key by its name: a Latin letter or a digit, F1–F24, the key left of 1 ("~", "`" or "Ё"),
/// or a named key such as "PrintScreen".
fn key_named(name: &str) -> Option<Key> {
    let lower = name.to_lowercase();
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return match c {
            'a'..='z' => Some(Key::Code(VK_A + (c as u16 - 'a' as u16))),
            '0'..='9' => Some(Key::Code(VK_0 + (c as u16 - '0' as u16))),
            '~' | '`' | 'ё' => Some(Key::LeftOfOne),
            _ => None,
        };
    }
    if let Some(number) = lower.strip_prefix('f').and_then(|n| n.parse::<u16>().ok())
        && (1..=24).contains(&number)
    {
        return Some(Key::Code(VK_F1 + number - 1));
    }
    Some(Key::Code(match lower.as_str() {
        "printscreen" | "prtsc" | "print" => VK_SNAPSHOT,
        "space" => VK_SPACE,
        "pause" => VK_PAUSE,
        "insert" => VK_INSERT,
        "delete" => VK_DELETE,
        "home" => VK_HOME,
        "end" => VK_END,
        "pageup" => VK_PRIOR,
        "pagedown" => VK_NEXT,
        _ => return None,
    }))
}

/// How the key left of 1 is labelled on the user's keyboard, in the layout of `language` if the
/// user has one and in the first of their layouts otherwise: "Ё" in Russian, "^" in German, "²" in
/// French, and "~" where it types `, as in English. Nothing where it types nothing: on Japanese
/// keyboards it switches the input method.
pub fn left_of_one(language: Language) -> Option<String> {
    let layouts = layouts();
    let layout = layouts.iter().copied().find(|&layout| speaks(layout, language)).or(layouts.first().copied())?;
    let code = unsafe { MapVirtualKeyExW(LEFT_OF_ONE, MAPVK_VSC_TO_VK_EX, layout) };
    // The top bit marks a dead key, such as ^ in German, which types its letter with the next one.
    let typed = unsafe { MapVirtualKeyExW(code, MAPVK_VK_TO_CHAR, layout) } & 0x7FFF_FFFF;
    let typed = char::from_u32(typed).filter(|c| code != 0 && !c.is_control())?;
    Some(if typed == '`' { "~".into() } else { typed.to_uppercase().collect() })
}

/// Whether a keyboard layout is one of a language.
fn speaks(layout: HKL, language: Language) -> bool {
    // LOCALE_NAME_MAX_LENGTH: no name of a language is longer.
    let mut name = [0u16; 85];
    let len = unsafe { LCIDToLocaleName(layout as usize as u32 & 0xFFFF, name.as_mut_ptr(), name.len() as i32, 0) };
    len > 1 && Language::from_tag(&String::from_utf16_lossy(&name[..len as usize - 1])) == language
}

/// The hotkey's number among the window's hotkeys.
const ID: i32 = 1;
/// The number a hotkey is tried with, to learn whether another program has it.
const TRIED: i32 = 2;

/// Whether no other program has a hotkey: the program takes it for a moment and lets it go. The
/// program's own is free only while it is held back.
pub fn free(window: HWND, hotkey: Hotkey) -> bool {
    let code = hotkey.key.code(layout_in_front());
    let took = unsafe { RegisterHotKey(window, TRIED, hotkey.modifiers, code.into()) } != 0;
    if took {
        unsafe { UnregisterHotKey(window, TRIED) };
    }
    took
}
/// The timer that looks at the layout in front while the layouts give the hotkey's key different
/// codes, and how often, in milliseconds.
pub const TIMER: usize = 0x484B;
const FOLLOW_TICK: u32 = 200;

/// The hotkey taken, with the code its key was taken with.
#[derive(Clone, Copy)]
struct Taken {
    hotkey: Hotkey,
    code: u16,
    /// A code another program has: it isn't tried again until the layout in front changes the
    /// key's code once more.
    refused: Option<u16>,
    /// Whether the timer looks at the layout in front.
    following: bool,
}

thread_local! {
    static TAKEN: Cell<Option<Taken>> = const { Cell::new(None) };
}

/// Takes the hotkey for the window, with the code its key has in the layout of the window in
/// front, instead of the one taken before; false if another program has it.
pub fn take(window: HWND, hotkey: Hotkey) -> bool {
    release(window);
    let code = hotkey.key.code(layout_in_front());
    if unsafe { RegisterHotKey(window, ID, hotkey.modifiers, code.into()) } == 0 {
        return false;
    }
    TAKEN.set(Some(Taken { hotkey, code, refused: None, following: false }));
    follow(window);
    true
}

/// Lets the hotkey go.
pub fn release(window: HWND) {
    let Some(taken) = TAKEN.take() else { return };
    unsafe {
        UnregisterHotKey(window, ID);
        if taken.following {
            KillTimer(window, TIMER);
        }
    }
}

/// Keeps the hotkey on its key as the layout changes. Windows matches a hotkey by the code the
/// layout of the window in front gives the key pressed, so the key left of 1 is taken again with
/// its code in that layout. This runs with the settings' tick, every second, and five times a
/// second while the layouts the user has give the key different codes, as English and German
/// do: then switching between them moves the hotkey at once.
pub fn follow(window: HWND) {
    let Some(mut taken) = TAKEN.get() else { return };
    if taken.hotkey.key != Key::LeftOfOne {
        return;
    }
    let code = taken.hotkey.key.code(layout_in_front());
    if code != taken.code && taken.refused != Some(code) {
        unsafe { UnregisterHotKey(window, ID) };
        if unsafe { RegisterHotKey(window, ID, taken.hotkey.modifiers, code.into()) } != 0 {
            (taken.code, taken.refused) = (code, None);
        } else {
            unsafe { RegisterHotKey(window, ID, taken.hotkey.modifiers, taken.code.into()) };
            taken.refused = Some(code);
        }
    }
    let following = !same_everywhere(taken.hotkey.key, &layouts());
    if following != taken.following {
        unsafe {
            if following {
                SetTimer(window, TIMER, FOLLOW_TICK, None);
            } else {
                KillTimer(window, TIMER);
            }
        }
        taken.following = following;
    }
    TAKEN.set(Some(taken));
}

/// Whether a key has the same code in all these layouts.
fn same_everywhere(key: Key, layouts: &[HKL]) -> bool {
    layouts.windows(2).all(|pair| key.code(pair[0]) == key.code(pair[1]))
}

/// The keyboard layouts the user has.
fn layouts() -> Vec<HKL> {
    let count = unsafe { GetKeyboardLayoutList(0, null_mut()) };
    let mut layouts = vec![null_mut(); count.max(0) as usize];
    let count = unsafe { GetKeyboardLayoutList(layouts.len() as i32, layouts.as_mut_ptr()) };
    layouts.truncate(count.max(0) as usize);
    layouts
}

/// The keyboard layout of the window in front, or of this thread when no window is.
fn layout_in_front() -> HKL {
    unsafe { GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), null_mut())) }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::MAPVK_VK_TO_VSC;

    use super::*;

    #[test]
    fn hotkeys_are_read_by_name() {
        assert_eq!(parse("Z"), Ok(Hotkey { modifiers: MOD_NOREPEAT, key: Key::Code(VK_A + 25) }));
        assert_eq!(parse("Ctrl+~"), Ok(Hotkey { modifiers: MOD_NOREPEAT | MOD_CONTROL, key: Key::LeftOfOne }));
        assert_eq!(parse("Alt+Ё").map(|h| h.key), Ok(Key::LeftOfOne));
        assert_eq!(parse("ctrl + shift + F12").map(|h| h.key), Ok(Key::Code(VK_F1 + 11)));
        assert_eq!(parse("PrintScreen").map(|h| h.key), Ok(Key::Code(VK_SNAPSHOT)));
    }

    #[test]
    fn a_wrong_hotkey_is_explained() {
        assert!(parse("Ctrl").is_err());
        assert!(parse("Ctrl+A+B").is_err());
        assert!(parse("Ctrl+Hyper").is_err());
    }

    /// The key left of 1 has a label in the language of every layout the user has, unless it
    /// switches the input method there: one letter or sign, and ~ for `.
    #[test]
    fn the_key_left_of_1_has_a_label() {
        for layout in layouts() {
            let language = Language::all().find(|&language| speaks(layout, language)).unwrap_or(Language::EN);
            if let Some(label) = left_of_one(language) {
                assert_eq!(label.chars().count(), 1, "{layout:?}: {label}");
                assert!(label != "`" && !label.chars().any(char::is_control), "{layout:?}: {label}");
            }
        }
    }

    /// In every layout the user has, the code taken for "~" is that of the key left of 1, or of
    /// the key with ` where the key left of 1 types nothing; the other keys keep their codes.
    #[test]
    fn the_key_left_of_1_is_found_in_every_layout() {
        let layouts = layouts();
        assert!(!layouts.is_empty());
        for layout in layouts {
            let code = Key::LeftOfOne.code(layout);
            let place = unsafe { MapVirtualKeyExW(code.into(), MAPVK_VK_TO_VSC, layout) };
            assert!(place == LEFT_OF_ONE || code == VK_OEM_3, "{layout:?}: {code:#x} is at {place:#x}");
            assert_eq!(Key::Code(VK_A + 25).code(layout), VK_A + 25);
        }
    }
}
