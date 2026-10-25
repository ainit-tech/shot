//! Colours as red, green, blue and opacity, and the way the settings write them.

pub type Rgba = [u8; 4];

/// Reads a colour written as `#RRGGBB`, or `#RRGGBBAA` with its opacity.
pub fn parse(text: &str) -> Option<Rgba> {
    let hex = text.trim().strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let alpha = if hex.len() == 8 { byte(6)? } else { 255 };
    Some([byte(0)?, byte(2)?, byte(4)?, alpha])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_read_with_and_without_opacity() {
        assert_eq!(parse("#E53935"), Some([0xE5, 0x39, 0x35, 255]));
        assert_eq!(parse("#00000080"), Some([0, 0, 0, 0x80]));
        assert_eq!(parse("E53935"), None);
        assert_eq!(parse("#E5393"), None);
        assert_eq!(parse("#GG3935"), None);
    }
}
