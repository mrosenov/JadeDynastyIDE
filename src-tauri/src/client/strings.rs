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
//! Strings are double-quoted (no escapes) or bare words and begin on the line
//! of their number. Quoted strings can continue across physical lines, as the
//! skill descriptions do. A new numbered quoted entry terminates an unclosed
//! string so one broken row does not consume the rest of the table (Forsaken's
//! item_ext_desc.txt has a few).
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

/// Whether a physical line begins a numbered, quoted table entry.
///
/// This is also the recovery boundary for a malformed unclosed quote. Valid
/// multiline text can contain numbers, but an entry begins with a number and
/// then an opening quote.
fn starts_indexed_quoted_entry(text: &str) -> bool {
    let b = text.trim_start().as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let digits = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if i == digits || !b.get(i).is_some_and(u8::is_ascii_whitespace) {
        return false;
    }
    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    b.get(i) == Some(&b'"')
}

/// Quoted strings and bare words with their opening line, comments left out.
fn tokens(text: &str, recover_indexed_rows: bool) -> Vec<(usize, &str)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut line = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_ascii_whitespace() => i += 1,
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    line += usize::from(b[i] == b'\n');
                    i += 1;
                }
                i = (i + 2).min(b.len());
            }
            b'"' => {
                let start = i + 1;
                let opening_line = line;
                i = start;
                loop {
                    match b.get(i) {
                        Some(b'"') => {
                            out.push((opening_line, &text[start..i]));
                            i += 1;
                            break;
                        }
                        Some(b'\n') => {
                            let next_start = i + 1;
                            let next_end = text[next_start..].find('\n').map_or(b.len(), |p| next_start + p);
                            if recover_indexed_rows && starts_indexed_quoted_entry(&text[next_start..next_end]) {
                                out.push((opening_line, &text[start..i]));
                                break;
                            }
                            line += 1;
                            i += 1;
                        }
                        Some(_) => i += 1,
                        None => {
                            out.push((opening_line, &text[start..]));
                            break;
                        }
                    }
                }
            }
            _ => {
                let start = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'"' {
                    i += 1;
                }
                out.push((line, &text[start..i]));
            }
        }
    }
    out
}

impl StringTable {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let text = decode(bytes);
        let indexed = text
            .lines()
            .take_while(|line| !line.trim().eq_ignore_ascii_case("#_begin"))
            .any(|line| line.trim().eq_ignore_ascii_case("#_index"));
        let tokens = tokens(&text, indexed);
        let begin = tokens.iter().position(|(_, t)| t.eq_ignore_ascii_case("#_begin")).ok_or("not a string table (no #_begin line)")?;
        let mut strings = HashMap::new();
        let body = &tokens[begin + 1..];
        // Line breaks are written "\r" inside strings.
        let text = |t: &str| t.replace("\\r", "\n");
        if indexed {
            // A number, then its string on the same line (none: an empty one).
            let mut i = 0;
            while let Some(&(line, number)) = body.get(i) {
                i += 1;
                let value = match body.get(i) {
                    Some(&(l, value)) if l == line => {
                        i += 1;
                        value
                    }
                    _ => "",
                };
                if let Ok(n) = number.parse::<i64>() {
                    strings.entry(n as u32).or_insert_with(|| text(value));
                }
            }
        } else {
            for (n, &(_, value)) in body.iter().enumerate() {
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

/// Text prepared for display: colour codes removed and outer blank lines
/// discarded, while meaningful line breaks are preserved.
pub fn plain_text(text: &str) -> Option<String> {
    let stripped = strip_codes(text);
    let mut lines: Vec<&str> = stripped.lines().map(str::trim).collect();
    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Text prepared for a game-style display. Outer blank lines and surrounding
/// whitespace are discarded, while colour codes and meaningful line breaks
/// are preserved for the UI to render safely.
pub fn colored_text(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let first = lines.iter().position(|line| !strip_codes(line).trim().is_empty())?;
    let last = lines.iter().rposition(|line| !strip_codes(line).trim().is_empty())?;
    Some(lines[first..=last].join("\n"))
}

/// Everything after the first non-empty line, used by buff_str.txt where the
/// entry contains the coloured name followed by its description.
pub fn after_first_line(text: &str) -> Option<String> {
    let stripped = strip_codes(text);
    let mut lines = stripped.lines().map(str::trim).skip_while(|line| line.is_empty());
    lines.next()?;
    let rest = lines.collect::<Vec<_>>().join("\n");
    plain_text(&rest)
}

/// Everything after the first non-empty line, preserving colour codes.
pub fn colored_after_first_line(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let first = lines.iter().position(|line| !strip_codes(line).trim().is_empty())?;
    colored_text(&lines[first + 1..].join("\n"))
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
        let numbers: Vec<i64> = tokens(&text, false).iter().filter_map(|(_, t)| t.parse().ok()).collect();
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
        assert_eq!(after_first_line(t.get(10).unwrap()).as_deref(), Some("Burns"));
        assert_eq!(plain_text(" ^ff0000Line one^ffffff\n\n Line two ").as_deref(), Some("Line one\n\nLine two"));
        assert_eq!(colored_text(" ^ff0000Line one^ffffff\n\n ^11ff11Line two ").as_deref(), Some("^ff0000Line one^ffffff\n\n^11ff11Line two"));
        assert_eq!(colored_after_first_line("^ffffffName\n^ffcb4aDescription").as_deref(), Some("^ffcb4aDescription"));
        let plain = StringTable::parse(b"#_begin \"a\" \"b\"").unwrap();
        assert_eq!(plain.get(1), Some("b"));
        assert!(StringTable::parse(b"no header").is_err());
    }

    #[test]
    fn preserves_physical_lines_inside_quoted_strings() {
        let t = StringTable::parse(&utf16(
            "#_index\r\n#_begin\r\n2181  \"^ffffffHeavy Blow　^ffcb4a%s/6\r\n^ffcb4aStrike with all your might!\r\n\r\n^ffffffCast Time: 1 second\r\n^11ff11Deals bonus damage.^ffffff\"\r\n2182 \"Next entry\"",
        ))
        .unwrap();
        assert_eq!(
            plain_text(t.get(2181).unwrap()).as_deref(),
            Some("Heavy Blow　%s/6\nStrike with all your might!\n\nCast Time: 1 second\nDeals bonus damage.")
        );
        assert_eq!(t.get(2182), Some("Next entry"));
    }

    #[test]
    fn broken_lines_cost_one_entry() {
        // As in Forsaken's item_ext_desc.txt: quotes left open, a string
        // missing its closing quote, a number without a string.
        let t = StringTable::parse(&utf16(
            "#_index\r\n#_begin\r\n854472208\t\"\n12941  \"a\"\r\n1401661936\t\"\n12943  \"b\"\r\n12944\t\"c\n12945  \"d\"\r\n12946 \"e\"\r\n12947\r\n12948 \"f\"",
        ))
        .unwrap();
        assert_eq!([12941, 12943, 12944, 12945, 12946, 12948].map(|n| t.get(n)), [Some("a"), Some("b"), Some("c"), Some("d"), Some("e"), Some("f")]);
        assert_eq!(t.get(12947), None);
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
