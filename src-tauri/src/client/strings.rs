//! The client's string tables in configs.pck: item descriptions
//! (item_ext_desc.txt), skill and buff texts (skillstr.txt, buff_str.txt),
//! addon texts (addon_str.txt), monster descriptions (monster_desc.txt), and
//! item name colours (item_color.txt with the palette in item_desc.txt).
//!
//! Tables are read the way CECStringTab::LoadWideStrings does:
//!
//! ```text
//! // comments (also /* block comments */)
//! #_index          optional: every string has an explicit number
//! #_begin          the strings follow
//! 1   "text"       with #_index: a number, then a (quoted) string
//! "text"           without it: strings numbered 0, 1, 2, …
//! ```
//!
//! Strings are double-quoted (no escapes, newlines allowed) or bare words.
//! The files are UTF-16LE with a BOM; UTF-8 and GBK are accepted too. When a
//! number repeats, the first string wins, as in the client. Line breaks
//! inside strings are written `\r` (backslash, r) and come out as "\n".

use std::collections::HashMap;

use encoding_rs::{GBK, UTF_16BE, UTF_16LE};

#[derive(Debug, Default)]
pub struct StringTable {
    pub strings: HashMap<u32, String>,
}

/// The text of a table file, with "\n" line breaks.
fn decode(bytes: &[u8]) -> String {
    let text = if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        UTF_16LE.decode_without_bom_handling(rest).0.into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        UTF_16BE.decode_without_bom_handling(rest).0.into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if let Ok(utf8) = std::str::from_utf8(bytes) {
        utf8.to_string()
    } else {
        GBK.decode(bytes).0.into_owned()
    };
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Quoted strings and bare words, comments left out.
fn tokens(text: &str) -> Vec<&str> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            c if c.is_ascii_whitespace() => i += 1,
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i = (i + 2).min(b.len());
            }
            b'"' => {
                let start = i + 1;
                let end = text[start..].find('"').map_or(b.len(), |p| start + p);
                out.push(&text[start..end]);
                i = end + 1;
            }
            _ => {
                let start = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'"' {
                    i += 1;
                }
                out.push(&text[start..i]);
            }
        }
    }
    out
}

impl StringTable {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let text = decode(bytes);
        let tokens = tokens(&text);
        let begin = tokens.iter().position(|t| t.eq_ignore_ascii_case("#_begin")).ok_or("not a string table (no #_begin line)")?;
        let indexed = tokens[..begin].iter().any(|t| t.eq_ignore_ascii_case("#_index"));
        let mut strings = HashMap::new();
        let body = &tokens[begin + 1..];
        // Line breaks are written "\r" inside strings.
        let text = |t: &str| t.replace("\\r", "\n");
        if indexed {
            for pair in body.chunks(2) {
                let [number, value] = pair else { break };
                if let Ok(n) = number.parse::<i64>() {
                    strings.entry(n as u32).or_insert_with(|| text(value));
                }
            }
        } else {
            for (n, value) in body.iter().enumerate() {
                strings.insert(n as u32, text(value));
            }
        }
        Ok(Self { strings })
    }

    pub fn get(&self, n: u32) -> Option<&str> {
        self.strings.get(&n).map(String::as_str).filter(|s| !s.trim().is_empty())
    }
}

/// The first line with text, without colour codes: a skill's or buff's name.
pub fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(|l| strip_codes(l).trim().to_string())
        .find(|l| !l.is_empty())
}

/// A text without its ^RRGGBB colour codes.
pub fn strip_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '^' && text.get(i + 1..i + 7).is_some_and(|h| h.chars().all(|c| c.is_ascii_hexdigit())) {
            for _ in 0..6 {
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// item_desc.txt entry of the first name colour (ITEMDESC_COL_WHITE).
const FIRST_COLOUR_ENTRY: u32 = 5;
/// The colours shipped in item_desc.txt (white, purple, blue, light blue,
/// green, red, dark gold, grey, yellow, cyan), used when it lacks one.
const DEFAULT_PALETTE: [&str; 10] = ["#ffffff", "#aa32ff", "#0000ff", "#8080ff", "#6cfb4b", "#ff0000", "#ff6000", "#909090", "#ffdc50", "#80ffff"];

/// Item name colours: item_color.txt pairs "item ID  colour index", the
/// colours themselves from item_desc.txt. Items not listed are white.
#[derive(Debug, Default)]
pub struct ItemColors {
    index: HashMap<u32, u8>,
    palette: Vec<String>,
}

impl ItemColors {
    pub fn parse(item_color: &[u8], item_desc: Option<&StringTable>) -> Self {
        let text = decode(item_color);
        let mut index = HashMap::new();
        let numbers: Vec<i64> = tokens(&text).iter().filter_map(|t| t.parse().ok()).collect();
        for pair in numbers.chunks(2) {
            if let [id, colour] = pair {
                index.entry(*id as u32).or_insert(*colour as u8);
            }
        }
        let palette = (0..10)
            .map(|i| {
                item_desc
                    .and_then(|t| t.strings.get(&(FIRST_COLOUR_ENTRY + i)))
                    .and_then(|e| {
                        let at = e.find('^')?;
                        let hex = e.get(at + 1..at + 7)?;
                        hex.chars().all(|c| c.is_ascii_hexdigit()).then(|| format!("#{}", hex.to_lowercase()))
                    })
                    .unwrap_or_else(|| DEFAULT_PALETTE[i as usize].to_string())
            })
            .collect();
        Self { index, palette }
    }

    /// The name colour of an item, when it is not plain white.
    pub fn get(&self, id: u32) -> Option<&str> {
        let i = *self.index.get(&id)? as usize;
        (1..=9).contains(&i).then(|| self.palette[i].as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        let mut out = vec![0xff, 0xfe];
        out.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
        out
    }

    #[test]
    fn parses_indexed_tables() {
        let t = StringTable::parse(&utf16("// skills\r\n#_index\r\n#_begin\r\n10 \"^ff0000Fire\\rBurns\"\r\n10 \"dupe\"\r\n/* block */ 11 word\r\n")).unwrap();
        assert_eq!(t.get(10), Some("^ff0000Fire\nBurns"));
        assert_eq!(t.get(11), Some("word"));
        assert_eq!(first_line(t.get(10).unwrap()).as_deref(), Some("Fire"));
        let plain = StringTable::parse(b"#_begin \"a\" \"b\"").unwrap();
        assert_eq!(plain.get(1), Some("b"));
        assert!(StringTable::parse(b"no header").is_err());
    }

    #[test]
    fn item_colours_from_pairs_and_palette() {
        let desc = StringTable::parse(&utf16("#_begin \"0\" \"1\" \"2\" \"3\" \"4\" \"^ffffff\" \"^aa32ff\"")).unwrap();
        let c = ItemColors::parse(b"// x\n1728\t1\n2698\t0\n", Some(&desc));
        assert_eq!(c.get(1728), Some("#aa32ff"));
        assert_eq!(c.get(2698), None);
        assert_eq!(c.get(5), None);
    }
}
