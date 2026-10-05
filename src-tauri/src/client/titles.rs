//! Title names and descriptions from
//! `interfaces.pck/Interfaces/script/config/title_def_u.lua`.
//!
//! The client executes this as Lua and reads the `id`, `note` and `desc`
//! fields of every `title_definition` entry. We only lex the small subset of
//! Lua needed by those table assignments; no client script is executed.

use std::collections::HashMap;

use super::strings;

#[derive(Debug, Clone, PartialEq)]
pub struct Title {
    /// Game-formatted title, including an optional ^RRGGBB colour code.
    pub name: String,
    /// Game-formatted description; `\r` escapes have become line breaks.
    pub description: String,
}

#[derive(Debug, Default)]
pub struct TitleTable {
    titles: HashMap<u32, Title>,
}

#[derive(Debug, PartialEq)]
enum Token {
    Ident(String),
    Number(i64),
    String(String),
    Symbol(u8),
}

fn string(bytes: &[u8], mut i: usize, quote: u8) -> (String, usize) {
    let mut out = String::new();
    let mut start = i;
    while i < bytes.len() {
        if bytes[i] == quote {
            out.push_str(&String::from_utf8_lossy(&bytes[start..i]));
            return (out, i + 1);
        }
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        out.push_str(&String::from_utf8_lossy(&bytes[start..i]));
        i += 1;
        let Some(&escaped) = bytes.get(i) else { return (out, i) };
        match escaped {
            b'r' | b'n' => out.push('\n'),
            b't' => out.push('\t'),
            b'\\' => out.push('\\'),
            b'"' => out.push('"'),
            b'\'' => out.push('\''),
            b'\n' => {}
            b'0'..=b'9' => {
                let mut value = 0u16;
                let mut digits = 0;
                while digits < 3 && bytes.get(i).is_some_and(u8::is_ascii_digit) {
                    value = value * 10 + (bytes[i] - b'0') as u16;
                    digits += 1;
                    i += 1;
                }
                if let Some(c) = char::from_u32(value as u32) {
                    out.push(c);
                }
                start = i;
                continue;
            }
            other => out.push(other as char),
        }
        i += 1;
        start = i;
    }
    out.push_str(&String::from_utf8_lossy(&bytes[start..]));
    (out, i)
}

fn lex(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else if bytes[i] == b'-' && bytes.get(i + 1) == Some(&b'-') {
            if bytes.get(i + 2..i + 4) == Some(b"[[") {
                i += 4;
                while i + 1 < bytes.len() && &bytes[i..i + 2] != b"]]" {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            } else {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
        } else if matches!(bytes[i], b'"' | b'\'') {
            let quote = bytes[i];
            let (value, next) = string(bytes, i + 1, quote);
            out.push(Token::String(value));
            i = next;
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while bytes.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                i += 1;
            }
            out.push(Token::Ident(text[start..i].to_string()));
        } else if bytes[i].is_ascii_digit() || (bytes[i] == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let start = i;
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            if let Ok(value) = text[start..i].parse() {
                out.push(Token::Number(value));
            }
        } else {
            out.push(Token::Symbol(bytes[i]));
            i += 1;
        }
    }
    out
}

impl TitleTable {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let text = strings::decode(bytes);
        let tokens = lex(&text);
        let mut titles = HashMap::new();
        let mut i = 0;
        while i < tokens.len() {
            if !matches!(tokens.get(i), Some(Token::Ident(name)) if name == "title_definition") {
                i += 1;
                continue;
            }
            i += 1;
            // A record is either title_definition[...] = {...}, or the empty
            // initial title_definition = {}. Ignore method calls and reads.
            if matches!(tokens.get(i), Some(Token::Symbol(b'['))) {
                while i < tokens.len() && !matches!(tokens.get(i), Some(Token::Symbol(b']'))) {
                    i += 1;
                }
                i += usize::from(i < tokens.len());
            }
            if !matches!(tokens.get(i), Some(Token::Symbol(b'='))) || !matches!(tokens.get(i + 1), Some(Token::Symbol(b'{'))) {
                continue;
            }
            i += 2;
            let mut id = None;
            let mut name = None;
            let mut description = None;
            while i < tokens.len() && !matches!(tokens.get(i), Some(Token::Symbol(b'}'))) {
                if let (Some(Token::Ident(key)), Some(Token::Symbol(b'=')), Some(value)) = (tokens.get(i), tokens.get(i + 1), tokens.get(i + 2)) {
                    match (key.as_str(), value) {
                        ("id", Token::Number(value)) if *value >= 0 && *value <= u32::MAX as i64 => id = Some(*value as u32),
                        ("note", Token::String(value)) => name = Some(value.clone()),
                        ("desc", Token::String(value)) => description = Some(value.clone()),
                        _ => {}
                    }
                    i += 3;
                } else {
                    i += 1;
                }
            }
            if let (Some(id), Some(name)) = (id, name) {
                titles.insert(id, Title { name, description: description.unwrap_or_default() });
            }
            i += usize::from(i < tokens.len());
        }
        if titles.is_empty() {
            return Err("title_def_u.lua has no title definitions".into());
        }
        Ok(Self { titles })
    }

    pub fn get(&self, id: u32) -> Option<&Title> {
        self.titles.get(&id)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (u32, &Title)> {
        self.titles.iter().map(|(&id, title)| (id, title))
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.titles.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_title_definitions_without_executing_lua() {
        let lua = br#"
            title_definition = {}
            -- title_definition['ignored'] = {id = 9, note = "Wrong", desc = "Wrong"}
            title_definition['first'] = { desc = "Line 1\rLine 2", id = 1001, note = "^ffbc3cThe Pinnacle" }
            title_definition['quote'] = {id=1002,note="Hero's \"Title\"",desc="A\\B"}
            function title_definition:GetTitleDef() return self end
        "#;
        let titles = TitleTable::parse(lua).unwrap();
        assert_eq!(titles.len(), 2);
        assert_eq!(titles.get(1001).unwrap(), &Title { name: "^ffbc3cThe Pinnacle".into(), description: "Line 1\nLine 2".into() });
        assert_eq!(titles.get(1002).unwrap(), &Title { name: "Hero's \"Title\"".into(), description: "A\\B".into() });
        assert!(titles.get(9).is_none());
    }
}
