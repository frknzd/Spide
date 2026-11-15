//! Position encodings. Spide stores 1-based lines and 0-based UTF-8 byte columns; LSP servers
//! speak UTF-16 code units unless `utf-8`/`utf-32` was negotiated (`positionEncoding`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
    Utf32,
}

impl Encoding {
    pub fn parse(s: &str) -> Encoding {
        match s {
            "utf-8" => Encoding::Utf8,
            "utf-32" => Encoding::Utf32,
            _ => Encoding::Utf16,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf16 => "utf-16",
            Encoding::Utf32 => "utf-32",
        }
    }
}

/// LSP column (in `enc` units) on `line_text` → UTF-8 byte column (clamped to the line).
pub fn to_byte_col(line_text: &str, col: u32, enc: Encoding) -> u32 {
    let mut units = 0u32;
    for (i, ch) in line_text.char_indices() {
        if units >= col {
            return i as u32;
        }
        units += match enc {
            Encoding::Utf8 => ch.len_utf8() as u32,
            Encoding::Utf16 => ch.len_utf16() as u32,
            Encoding::Utf32 => 1,
        };
    }
    line_text.len() as u32
}

/// UTF-8 byte column → LSP column in `enc` units.
pub fn from_byte_col(line_text: &str, byte: u32, enc: Encoding) -> u32 {
    let mut b = (byte as usize).min(line_text.len());
    while !line_text.is_char_boundary(b) {
        b -= 1;
    }
    let s = &line_text[..b];
    match enc {
        Encoding::Utf8 => s.len() as u32,
        Encoding::Utf16 => s.chars().map(|c| c.len_utf16() as u32).sum(),
        Encoding::Utf32 => s.chars().count() as u32,
    }
}

/// The text of 0-based line `n`.
pub fn line(text: &str, n: u32) -> &str {
    text.split('\n').nth(n as usize).map(|l| l.trim_end_matches('\r')).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        let l = "a🦀é=x";
        // 🦀 is 4 bytes, 2 UTF-16 units, 1 code point; é 2 bytes, 1 unit
        assert_eq!(to_byte_col(l, 3, Encoding::Utf16), 5);
        assert_eq!(to_byte_col(l, 2, Encoding::Utf32), 5);
        assert_eq!(to_byte_col(l, 5, Encoding::Utf8), 5);
        assert_eq!(from_byte_col(l, 7, Encoding::Utf16), 4);
        assert_eq!(from_byte_col(l, 7, Encoding::Utf32), 3);
        assert_eq!(from_byte_col(l, 6, Encoding::Utf16), 3, "inside a char: clamp back");
        assert_eq!(to_byte_col(l, 99, Encoding::Utf16), l.len() as u32);
        assert_eq!(line("a\r\nb", 1), "b");
    }
}
