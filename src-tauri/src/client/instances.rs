//! Map (instance) names from `configs.pck` › `Configs/instance.txt` (`CECGameRun::LoadInstanceInfo`):
//! a UTF-16 script of blocks `"Name" { id zone "path" … }`, with `//` comments (also after values).

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

/// (map ID, name) in file order.
pub fn parse(bytes: &[u8]) -> Result<Vec<(i32, String)>, String> {
    let body = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
    if body.len() % 2 != 0 {
        return Err("instance.txt is not UTF-16".into());
    }
    let units: Vec<u16> = body.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    let text = String::from_utf16_lossy(&units);
    let lines: Vec<&str> = text.lines().map(|line| without_comment(line).trim()).filter(|line| !line.is_empty()).collect();
    let mut out = Vec::new();
    for window in lines.windows(3) {
        let [name, open, id] = window else { continue };
        let Some(name) = name.strip_prefix('"').and_then(|name| name.strip_suffix('"')) else { continue };
        if *open != "{" {
            continue;
        }
        if let Ok(id) = id.trim_end_matches(',').trim().parse::<i32>() {
            out.push((id, name.to_string()));
        }
    }
    if out.is_empty() {
        return Err("instance.txt has no maps".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_map_names() {
        // ForsakenJD's layout, then HDN's (comments after the values).
        let text = "//////\r\n\"Sunstream\"\r\n{\r\n1\r\n0\r\n\"Z1\"\r\n}\r\n\"Old Sunstream\"\t// old\r\n{\r\n60\t\t//  ID\r\n0\t\t//  Zone ID\r\n\"x//y\"\t// path\r\n}\r\n";
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(super::parse(&bytes).unwrap(), vec![(1, "Sunstream".to_string()), (60, "Old Sunstream".to_string())]);
    }

    #[test]
    fn real_clients_name_their_maps() {
        for root in ["E:/Games/ForsakenJD/element", "E:/Games/Elite Jade Dynasty - HDN/element"] {
            let path = std::path::Path::new(root).join("configs.pck");
            if !path.is_file() { continue; }
            let pck = super::super::pck::Pck::open(&path).unwrap();
            let maps = super::parse(&pck.read_path("configs/instance.txt").unwrap()).unwrap();
            assert!(maps.len() > 100, "{root}");
            assert!(maps.iter().any(|(id, name)| *id == 1 && name == "Sunstream"), "{root}");
        }
    }
}
