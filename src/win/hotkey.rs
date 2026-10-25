//! Keys by their names in the settings: the hotkey such as "Ctrl+~" or "PrintScreen", and the
//! letters of the query buttons.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_0, VK_A, VK_DELETE, VK_END, VK_F1,
    VK_HOME, VK_INSERT, VK_NEXT, VK_OEM_3, VK_PAUSE, VK_PRIOR, VK_SNAPSHOT, VK_SPACE,
};

use crate::text::tr;

/// A hotkey: the modifier keys and the key itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: HOT_KEY_MODIFIERS,
    pub key: u16,
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
                key = Some(key_code(name).ok_or_else(unknown)?);
            }
        }
    }
    let missing = || {
        tr("“{text}” has no key itself.").replace("{text}", text)
    };
    let key = key.ok_or_else(missing)?;
    Ok(Hotkey { modifiers, key })
}

/// The code of a key by its name: a Latin letter or a digit, F1–F24, the key left of 1
/// ("~", "`" or "Ё"), or a named key such as "PrintScreen".
pub fn key_code(name: &str) -> Option<u16> {
    let lower = name.to_lowercase();
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return match c {
            'a'..='z' => Some(VK_A + (c as u16 - 'a' as u16)),
            '0'..='9' => Some(VK_0 + (c as u16 - '0' as u16)),
            '~' | '`' | 'ё' => Some(VK_OEM_3),
            _ => None,
        };
    }
    if let Some(number) = lower.strip_prefix('f').and_then(|n| n.parse::<u16>().ok())
        && (1..=24).contains(&number)
    {
        return Some(VK_F1 + number - 1);
    }
    Some(match lower.as_str() {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_are_read_by_name() {
        assert_eq!(parse("Z"), Ok(Hotkey { modifiers: MOD_NOREPEAT, key: VK_A + 25 }));
        assert_eq!(parse("Ctrl+~"), Ok(Hotkey { modifiers: MOD_NOREPEAT | MOD_CONTROL, key: VK_OEM_3 }));
        assert_eq!(parse("ctrl + shift + F12").map(|h| h.key), Ok(VK_F1 + 11));
        assert_eq!(parse("PrintScreen").map(|h| h.key), Ok(VK_SNAPSHOT));
    }

    #[test]
    fn a_wrong_hotkey_is_explained() {
        assert!(parse("Ctrl").is_err());
        assert!(parse("Ctrl+A+B").is_err());
        assert!(parse("Ctrl+Hyper").is_err());
    }
}
