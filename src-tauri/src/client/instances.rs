//! Maps (instances) from `configs.pck` › `Configs/instance.txt` (`CECInstance::Load`): a UTF-16 script
//! of blocks `"Name" { id zone "path" "data path" "detail texture" rows, cols … }`, with `//` comments
//! (also after values). `path` names the client's map images (`Surfaces\MidMaps\<path>.dds`); servers
//! name their map folders by the data path (`z12` is `e12` there).

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: i32,
    pub name: String,
    /// Empty when the block ends early.
    pub path: String,
    pub data_path: String,
    /// Map size in 1024-unit cells (0 when missing).
    pub rows: i32,
    pub cols: i32,
}

/// A line without its `//` comment (outside quotes).
fn without_comment(line: &str) -> &str {
    let mut quoted = false;
    let bytes = line.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'"' => quoted = !quoted,
            b'/' if !quoted && bytes.get(index + 1) == Some(&b'/') => return &line[..index],
            _ => {}
        }
    }
    line
}

#[derive(Debug, PartialEq)]
enum Token {
    Text(String),
    Word(String),
}

/// Quoted strings and words; whitespace and commas separate them.
fn tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    for line in text.lines().map(without_comment) {
        let mut chars = line.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == ',' {
                chars.next();
            } else if c == '"' {
                chars.next();
                let text: String = chars.by_ref().take_while(|&c| c != '"').collect();
                out.push(Token::Text(text));
            } else {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == ',' || c == '"' {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                out.push(Token::Word(word));
            }
        }
    }
    out
}

/// Maps in file order.
pub fn parse(bytes: &[u8]) -> Result<Vec<Instance>, String> {
    let body = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
    if body.len() % 2 != 0 {
        return Err("instance.txt is not UTF-16".into());
    }
    let units: Vec<u16> = body.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    let tokens = tokens(&String::from_utf16_lossy(&units));
    let word = |at: usize| match tokens.get(at) {
        Some(Token::Word(word)) => Some(word.as_str()),
        _ => None,
    };
    let text = |at: usize| match tokens.get(at) {
        Some(Token::Text(text)) => Some(text.clone()),
        _ => None,
    };
    let number = |at: usize| word(at).and_then(|word| word.parse::<i32>().ok());
    let mut out = Vec::new();
    for at in 0..tokens.len() {
        let (Some(name), Some("{"), Some(id)) = (text(at), word(at + 1), number(at + 2)) else { continue };
        // id, zone, path, data path, detail texture, rows, cols
        let path = text(at + 4).unwrap_or_default();
        let data_path = text(at + 5).unwrap_or_default();
        let (rows, cols) = if text(at + 6).is_some() { (number(at + 7).unwrap_or(0), number(at + 8).unwrap_or(0)) } else { (0, 0) };
        out.push(Instance { id, name, path, data_path, rows, cols });
    }
    if out.is_empty() {
        return Err("instance.txt has no maps".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::Instance;

    fn utf16(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }

    #[test]
    fn reads_maps() {
        // ForsakenJD's layout, then HDN's (comments after the values), then a block that ends early.
        let text = "//////\r\n\"Sunstream\"\r\n{\r\n401\r\n0\r\n\"x1\"\r\n\"x1\"\r\n\"Textures\\Maps\\detail\\z1.dds\"\r\n2,2\r\n37,79,85\r\n1000,\r\n1\r\n-1024 0 0 1024 \"\" \"Sky\"\r\n}\r\n\
            \"Foxhill\"\t// old\r\n{\r\n612\t\t//  ID\r\n0\t\t//  Zone ID\r\n\"z12\"\t// path\r\n\"e12\" // data path\r\n\"Textures\\Maps\\detail\\Z12.dds\" // detail texture\r\n1, 1  //  row, column\r\n}\r\n\
            \"Old\"\r\n{\r\n60\r\n0\r\n\"x//y\"\r\n}\r\n";
        let maps = super::parse(&utf16(text)).unwrap();
        let map = |id, name: &str, path: &str, data_path: &str, rows, cols| Instance { id, name: name.into(), path: path.into(), data_path: data_path.into(), rows, cols };
        assert_eq!(maps, vec![map(401, "Sunstream", "x1", "x1", 2, 2), map(612, "Foxhill", "z12", "e12", 1, 1), map(60, "Old", "x//y", "", 0, 0)]);
    }

    #[test]
    fn real_clients_describe_their_maps() {
        for root in ["E:/Games/ForsakenJD/element", "E:/Games/Elite Jade Dynasty - HDN/element"] {
            let path = std::path::Path::new(root).join("configs.pck");
            if !path.is_file() { continue; }
            let pck = super::super::pck::Pck::open(&path).unwrap();
            let maps = super::parse(&pck.read_path("configs/instance.txt").unwrap()).unwrap();
            assert!(maps.len() > 100, "{root}");
            assert!(maps.iter().any(|map| map.id == 1 && map.name == "Sunstream"), "{root}");
            let foxhill = maps.iter().find(|map| map.id == 612).unwrap();
            assert_eq!((foxhill.path.as_str(), foxhill.data_path.as_str(), foxhill.rows, foxhill.cols), ("z12", "e12", 1, 1), "{root}");
            assert!(maps.iter().filter(|map| map.rows > 0).count() > maps.len() * 9 / 10, "{root}");
        }
    }
}
