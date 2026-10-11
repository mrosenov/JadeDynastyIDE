//! Server Lua scripts (gamed, gamed/script), read for the Lua viewers. gs runs them in ten environments set up by
//! gs.conf `[Script] GlobalScript` (global_script.lua `Init(index)`: lottery, talisman, pets, player, summon, magic,
//! transform, misc, rune, astrology); a script that fails to load stops gs with -11. Most are GBK with CRLF, a few
//! UTF-8. The viewers only read them.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuaFile {
    pub path: String,
    /// `utf-8`, `utf-8-bom` or `gbk`.
    pub encoding: &'static str,
    pub size: usize,
    pub text: String,
}

/// UTF-8 (with or without a byte order mark) when it decodes as such, else GBK (the designers' Chinese Windows).
pub fn decode(bytes: &[u8]) -> (&'static str, String) {
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return ("utf-8-bom", String::from_utf8_lossy(rest).into_owned());
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => ("utf-8", text.to_string()),
        Err(_) => ("gbk", encoding_rs::GBK.decode_without_bom_handling(bytes).0.into_owned()),
    }
}

pub fn read(path: &Path) -> Result<LuaFile, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(format!("{} is {} MB; the viewer reads scripts up to 32 MB", path.display(), bytes.len() / 1024 / 1024));
    }
    let (encoding, text) = decode(&bytes);
    Ok(LuaFile { path: path.display().to_string(), encoding, size: bytes.len(), text })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample servers' scripts decode: GBK files give Chinese comments, UTF-8 files stay as they are.
    #[test]
    fn sample_scripts_decode() {
        let (mut gbk, mut utf8) = (0, 0);
        for dir in ["E:/Game Dev/JD/1559/gamed/script", "E:/Game Dev/JD/zxserver/zgame/gs/script"] {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten().filter(|entry| entry.path().extension().is_some_and(|ext| ext == "lua")) {
                let file = read(&entry.path()).unwrap();
                match file.encoding {
                    "gbk" => gbk += 1,
                    _ => utf8 += 1,
                }
                // A few files have bytes no encoding reads (damaged characters in comments).
                let bad = file.text.matches('\u{fffd}').count();
                assert!(bad * 1000 <= file.text.chars().count(), "{}: {bad} unreadable characters", file.path);
            }
        }
        if gbk + utf8 > 0 {
            assert!(gbk > 0);
            let lottery = read(Path::new("E:/Game Dev/JD/1559/gamed/script/lottery.lua")).unwrap();
            assert!(lottery.text.contains("出现几率正态化"));
        }
    }
}

