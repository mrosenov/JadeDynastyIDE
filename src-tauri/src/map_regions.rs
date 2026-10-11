//! A map's precinct.clt / region.clt (client, UTF-16 text written by the map editor) and their server copies
//! precinct.sev / region.sev (binary). ZCommon EL_Precinct.cpp / EL_Region.cpp read both (`_ELEMENTCLIENT` text,
//! otherwise binary; the servers' template/el_precinct.cpp and el_region.cpp are the binary half); the map editor's
//! ZElementEditor AIGenExportor.cpp writes all four with one timestamp.
//!
//! ```text
//! precinct.clt  version v · [v≥5] timestamp · precincts: "name" · [v≥2] id, points, marks, priority, return city
//!               instance, musics, music interval, loop, [v≥4] source instance, [v≥6] domain · city x, y, z ·
//!               [v≥7] rebirths, max level, min level, kick-out city · kick-out x, y, z · points · marks ("name" x y z)
//!               · "sound" · musics · [v≥3] "night sound" · [v≥8] "minimap"
//! precinct.sev  u32 version, i32 count, [v≥5] u32 timestamp; per precinct with a non-zero ID: i32 points, priority,
//!               return city, [v≥4] source, [v≥6] domain, f32×3 city, [v≥7] i32×4 + f32×3 kick-out, f32×3 points
//! region.clt    version v · [v≥4] timestamp · [region] "name" type points … · [trans] target, [v≥3] source,
//!               [v≥5] level, [v≥6] line, [v≥7] controller · position · half extents · target position ·
//!               [GuildBuilding] template, width, height · position · direction · up
//! region.sev    u32 version, i32 regions, [v≥2] i32 portals, [v≥4] u32 timestamp, [v≥8] i32 buff regions; then
//!               items: i32 0 + region points, 1 + portal, 2 + template, monster, points (buff regions: server only)
//! ```
//!
//! The client sends the server the index of the portal it walked into (`c2s_CmdEnterInstance`), so portals must be in
//! the same order in both files. On entering a map the server sends both timestamps and the client shows an instance
//! data error when they differ from its own (`CECWorld::CheckOutInst`). Precincts with ID 0 are client only (the
//! exporter skips them for the server). A file the client cannot read is dropped whole (`glb_ErrorOutput`).

use std::path::Path;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    pub name: String,
    pub pos: Vec3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Precinct {
    pub name: String,
    /// 0 = client only (not exported to the server).
    pub id: u32,
    /// Overlapping precincts: the lowest number wins (the later one on a tie).
    pub priority: i32,
    /// Instance of the return city.
    pub city_instance: i32,
    pub music_interval: i32,
    pub music_loop: i32,
    /// The instance this precinct belongs to (a map folder can hold several).
    pub instance: i32,
    pub domain: i32,
    pub city: Vec3,
    pub rebirths: i32,
    pub level_max: i32,
    pub level_min: i32,
    pub kick_city: i32,
    pub kick_pos: Vec3,
    pub points: Vec<Vec3>,
    pub marks: Vec<Mark>,
    pub sound: String,
    pub music: Vec<String>,
    pub sound_night: String,
    pub minimap: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RegionItem {
    /// `type` 1 = safe zone, 0 = PK region (-1 when read from a .sev, which does not store it).
    Region { name: String, region_type: i32, points: Vec<Vec3> },
    /// A portal box: walking in moves the player to `target_pos` in `target`.
    Portal { target: i32, instance: i32, level: i32, line: i32, controller: i32, pos: Vec3, extents: Vec3, target_pos: Vec3 },
    /// Client only.
    GuildBuilding { template: i32, width: i32, height: i32, pos: Vec3, dir: Vec3, up: Vec3 },
}

/// Server only (region.sev version 8): a region with a buff template (`city_region::GetBuffArea`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuffRegion {
    /// Regions and portals before it in the server file (kept so an unchanged file is written back as it was).
    #[serde(default)]
    pub after: usize,
    pub template: i32,
    pub monster: i32,
    pub points: Vec<Vec3>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MapFile {
    Precinct { version: u32, timestamp: u32, precincts: Vec<Precinct> },
    /// `buffs` come from the server file (the client file cannot hold them).
    Region { version: u32, timestamp: u32, items: Vec<RegionItem>, buffs: Vec<BuffRegion> },
}

impl MapFile {
    pub fn kind(&self) -> &'static str {
        match self {
            MapFile::Precinct { .. } => "precinct",
            MapFile::Region { .. } => "region",
        }
    }
    pub fn version(&self) -> u32 {
        match self {
            MapFile::Precinct { version, .. } | MapFile::Region { version, .. } => *version,
        }
    }
    pub fn timestamp(&self) -> u32 {
        match self {
            MapFile::Precinct { timestamp, .. } | MapFile::Region { timestamp, .. } => *timestamp,
        }
    }
    fn set_timestamp(&mut self, value: u32) {
        match self {
            MapFile::Precinct { timestamp, .. } | MapFile::Region { timestamp, .. } => *timestamp = value,
        }
    }
}

pub const MAX_VERSION: u32 = 8;

// ---------------------------------------------------------------- client text

struct Token {
    text: String,
    line: usize,
}

/// Tokens as AWScriptFile reads them: separated by white space and commas, `"…"` texts on one line, `//` comments.
fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        let line_no = index + 1;
        let mut chars = line.char_indices().peekable();
        while let Some((at, c)) = chars.next() {
            if c.is_whitespace() || c == ',' || c == '\u{feff}' {
                continue;
            }
            if c == '/' && line[at + 1..].starts_with('/') {
                break;
            }
            if c == '"' {
                let rest = &line[at + 1..];
                let end = rest.find('"').ok_or_else(|| format!("Line {line_no}: a text has no closing quote"))?;
                out.push(Token { text: rest[..end].to_string(), line: line_no });
                for _ in 0..rest[..end].chars().count() + 1 {
                    chars.next();
                }
                continue;
            }
            let start = at;
            let mut end = line.len();
            while let Some(&(next_at, next)) = chars.peek() {
                if next.is_whitespace() || next == ',' || next == '"' {
                    end = next_at;
                    break;
                }
                chars.next();
            }
            let word = &line[start..end];
            if word.contains('"') {
                return Err(format!("Line {line_no}: a quote inside “{word}”"));
            }
            out.push(Token { text: word.to_string(), line: line_no });
        }
    }
    Ok(out)
}

struct Reader {
    tokens: Vec<Token>,
    at: usize,
}

impl Reader {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }
    fn last_line(&self) -> usize {
        self.at.checked_sub(1).and_then(|index| self.tokens.get(index)).map_or(0, |token| token.line)
    }
    /// The next token; `same_line` as the client's `GetNextToken(false)`: it must follow on the previous token's line.
    fn next(&mut self, same_line: bool, what: &str) -> Result<&Token, String> {
        let line = self.last_line();
        let token = self.tokens.get(self.at).ok_or_else(|| format!("The file ends where {what} should be (after line {line})"))?;
        if same_line && token.line != line {
            return Err(format!("Line {line}: {what} is missing at the end of the line (the game does not read the next line for it)"));
        }
        // The editor starts every group on a new line; a value left on the line is one too many for this version.
        if !same_line && self.at > 0 && token.line == line {
            return Err(format!("Line {line}: “{}” is a value too many at the end of the line ({what} should start the next line; is the file version right?)", token.text));
        }
        self.at += 1;
        Ok(&self.tokens[self.at - 1])
    }
    fn text(&mut self, same_line: bool, what: &str) -> Result<String, String> {
        Ok(self.next(same_line, what)?.text.clone())
    }
    fn int(&mut self, same_line: bool, what: &str) -> Result<i32, String> {
        let token = self.next(same_line, what)?;
        token.text.parse::<i32>().map_err(|_| format!("Line {}: {what} should be a whole number, not “{}”", token.line, token.text))
    }
    fn float(&mut self, same_line: bool, what: &str) -> Result<f32, String> {
        let token = self.next(same_line, what)?;
        token.text.parse::<f32>().map_err(|_| format!("Line {}: {what} should be a number, not “{}”", token.line, token.text))
    }
    fn vec3(&mut self, same_line: bool, what: &str) -> Result<Vec3, String> {
        Ok(Vec3 { x: self.float(same_line, what)?, y: self.float(true, what)?, z: self.float(true, what)? })
    }
    fn count(&mut self, same_line: bool, what: &str) -> Result<usize, String> {
        let line = self.last_line();
        let value = self.int(same_line, what)?;
        usize::try_from(value).ok().filter(|count| *count <= 100_000).ok_or_else(|| format!("Line {line}: {value} is not a valid {what}"))
    }
}

fn decode_text(bytes: &[u8]) -> Result<String, String> {
    let body = bytes.strip_prefix(&[0xff, 0xfe]).ok_or("Not a client region/precinct file: it does not start as UTF-16 text (the game reads only UTF-16 files with a byte order mark)")?;
    if body.len() % 2 != 0 {
        return Err("The UTF-16 text has an odd number of bytes".into());
    }
    let units: Vec<u16> = body.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    String::from_utf16(&units).map_err(|_| "The file is not valid UTF-16 text".to_string())
}

fn read_header(reader: &mut Reader, stamp_from: u32) -> Result<(u32, u32), String> {
    let first = reader.next(false, "“version”")?;
    if first.text != "version" {
        return Err(format!("Line {}: the file must start with “version” (found “{}”)", first.line, first.text));
    }
    let version = reader.int(true, "the version")?;
    if version < 1 || version as u32 > MAX_VERSION {
        return Err(format!("Version {version} is not supported (the game reads 1–{MAX_VERSION})"));
    }
    let version = version as u32;
    let timestamp = if version >= stamp_from { reader.int(false, "the timestamp")? as u32 } else { 0 };
    Ok((version, timestamp))
}

fn read_points(reader: &mut Reader, count: usize, what: &str) -> Result<Vec<Vec3>, String> {
    (0..count).map(|_| reader.vec3(false, what)).collect()
}

pub fn parse_precinct_clt(text: &str) -> Result<MapFile, String> {
    let mut reader = Reader { tokens: tokenize(text)?, at: 0 };
    let (version, timestamp) = read_header(&mut reader, 5)?;
    let mut precincts = Vec::new();
    while reader.peek().is_some() {
        let name = reader.text(false, "a precinct name")?;
        let (id, points) = if version >= 2 { (reader.int(false, "the precinct ID")? as u32, reader.count(true, "point count")?) } else { (0, reader.count(false, "point count")?) };
        let marks = reader.count(true, "mark count")?;
        let priority = reader.int(true, "the priority")?;
        let city_instance = reader.int(true, "the return city instance")?;
        let musics = reader.count(true, "music count")?;
        let music_interval = reader.int(true, "the music interval")?;
        let music_loop = reader.int(true, "the music loop")?;
        let instance = if version >= 4 { reader.int(true, "the source instance")? } else { 1 };
        let domain = if version >= 6 { reader.int(true, "the domain ID")? } else { 0 };
        let city = reader.vec3(false, "the return city position")?;
        let (rebirths, level_max, level_min, kick_city, kick_pos) = if version >= 7 {
            (reader.int(false, "the rebirth count")?, reader.int(true, "the maximum level")?, reader.int(true, "the minimum level")?, reader.int(true, "the kick-out city")?, reader.vec3(false, "the kick-out position")?)
        } else {
            (0, 0, 0, 1, Vec3::default())
        };
        let points = read_points(&mut reader, points, "a point")?;
        let marks = (0..marks).map(|_| Ok(Mark { name: reader.text(false, "a mark name")?, pos: reader.vec3(true, "a mark position")? })).collect::<Result<Vec<_>, String>>()?;
        let sound = reader.text(false, "the sound file")?;
        let music = (0..musics).map(|_| reader.text(false, "a music file")).collect::<Result<Vec<_>, String>>()?;
        let sound_night = if version >= 3 { reader.text(false, "the night sound file")? } else { String::new() };
        let minimap = if version >= 8 { reader.text(false, "the minimap")? } else { String::new() };
        precincts.push(Precinct { name, id, priority, city_instance, music_interval, music_loop, instance, domain, city, rebirths, level_max, level_min, kick_city, kick_pos, points, marks, sound, music, sound_night, minimap });
    }
    Ok(MapFile::Precinct { version, timestamp, precincts })
}

pub fn parse_region_clt(text: &str) -> Result<MapFile, String> {
    let mut reader = Reader { tokens: tokenize(text)?, at: 0 };
    let (version, timestamp) = read_header(&mut reader, 4)?;
    let mut items = Vec::new();
    while let Some(token) = reader.peek() {
        let tag = if version == 1 { "[region]".to_string() } else { token.text.to_lowercase() };
        let line = token.line;
        if version >= 2 {
            reader.at += 1;
        }
        match tag.as_str() {
            "[region]" => {
                let name = reader.text(false, "a region name")?;
                let region_type = reader.int(false, "the region type")?;
                let count = reader.count(true, "point count")?;
                items.push(RegionItem::Region { name, region_type, points: read_points(&mut reader, count, "a point")? });
            }
            "[trans]" => {
                let target = reader.int(false, "the target instance")?;
                let instance = if version >= 3 { reader.int(true, "the source instance")? } else { 1 };
                let level = if version >= 5 { reader.int(true, "the level limit")? } else { 1 };
                let line = if version >= 6 { reader.int(true, "the line")? } else { 0 };
                let controller = if version >= 7 { reader.int(true, "the controller")? } else { 0 };
                let pos = reader.vec3(false, "the portal position")?;
                let extents = reader.vec3(false, "the portal size")?;
                let target_pos = reader.vec3(false, "the target position")?;
                items.push(RegionItem::Portal { target, instance, level, line, controller, pos, extents, target_pos });
            }
            "[guildbuilding]" => {
                let template = reader.int(false, "the building template")?;
                let width = reader.int(true, "the width")?;
                let height = reader.int(true, "the height")?;
                items.push(RegionItem::GuildBuilding { template, width, height, pos: reader.vec3(false, "the position")?, dir: reader.vec3(false, "the direction")?, up: reader.vec3(false, "the up direction")? });
            }
            other => return Err(format!("Line {line}: “{other}” where [region], [trans] or [GuildBuilding] should start (a value too many or too few above?)")),
        }
    }
    Ok(MapFile::Region { version, timestamp, items, buffs: Vec::new() })
}

/// `%f` as the editor's fwprintf writes it.
fn f(value: f32) -> String {
    format!("{:.6}", value as f64)
}

fn check_text(text: &str, what: &str) -> Result<(), String> {
    if text.contains('"') || text.contains('\n') || text.contains('\r') {
        return Err(format!("{what} “{text}” has a quote or a line break, which the file cannot hold"));
    }
    Ok(())
}

/// The client file in the map editor's layout for its version (`AIGenExportor`).
pub fn write_clt(file: &MapFile) -> Result<Vec<u8>, String> {
    let mut out = String::from("\u{feff}");
    match file {
        MapFile::Precinct { version, timestamp, precincts } => {
            let v = *version;
            out.push_str("//  Element pricinct file (client version)\n\n");
            out.push_str(&format!("version  {v}\n"));
            if v >= 5 {
                out.push_str(&format!("{}\n", *timestamp as i32));
            }
            out.push('\n');
            for (index, p) in precincts.iter().enumerate() {
                let what = format!("Precinct {} name", index + 1);
                check_text(&p.name, &what)?;
                out.push_str(&format!("\"{}\"\n", p.name));
                let mut numbers = Vec::new();
                if v >= 2 {
                    numbers.push(p.id.to_string());
                }
                numbers.extend([p.points.len(), p.marks.len()].map(|n| n.to_string()));
                numbers.extend([p.priority, p.city_instance].map(|n| n.to_string()));
                numbers.push(p.music.len().to_string());
                numbers.extend([p.music_interval, p.music_loop].map(|n| n.to_string()));
                if v >= 4 {
                    numbers.push(p.instance.to_string());
                }
                if v >= 6 {
                    numbers.push(p.domain.to_string());
                }
                out.push_str(&numbers.join("  "));
                out.push('\n');
                out.push_str(&format!("{}, {}, {}\n", f(p.city.x), f(p.city.y), f(p.city.z)));
                if v >= 7 {
                    out.push_str(&format!("{}  {}  {}  {}\n", p.rebirths, p.level_max, p.level_min, p.kick_city));
                    out.push_str(&format!("{}, {}, {}\n", f(p.kick_pos.x), f(p.kick_pos.y), f(p.kick_pos.z)));
                }
                for point in &p.points {
                    out.push_str(&format!("{}  {}  {}\n", f(point.x), f(point.y), f(point.z)));
                }
                out.push('\n');
                for mark in &p.marks {
                    check_text(&mark.name, "A mark name")?;
                    out.push_str(&format!("\"{}\"  {}  {}  {}\n", mark.name, f(mark.pos.x), f(mark.pos.y), f(mark.pos.z)));
                }
                out.push('\n');
                let mut texts = vec![&p.sound];
                texts.extend(&p.music);
                if v >= 3 {
                    texts.push(&p.sound_night);
                }
                if v >= 8 {
                    texts.push(&p.minimap);
                }
                for text in texts {
                    check_text(text, "A sound or picture file")?;
                    out.push_str(&format!("\"{text}\"\n"));
                }
                out.push('\n');
            }
        }
        MapFile::Region { version, timestamp, items, .. } => {
            let v = *version;
            out.push_str("//  Element region file (client version)\n\n");
            out.push_str(&format!("version  {v}\n"));
            if v >= 4 {
                out.push_str(&format!("{}\n", *timestamp as i32));
            }
            out.push('\n');
            let comma = |p: &Vec3| format!("{}, {}, {}\n", f(p.x), f(p.y), f(p.z));
            for item in items {
                match item {
                    RegionItem::Region { name, region_type, points } => {
                        check_text(name, "A region name")?;
                        if v >= 2 {
                            out.push_str("[region]\n");
                        }
                        out.push_str(&format!("\"{name}\"\n{region_type}  {}\n", points.len()));
                        for point in points {
                            out.push_str(&format!("{}  {}  {}\n", f(point.x), f(point.y), f(point.z)));
                        }
                        out.push('\n');
                    }
                    RegionItem::Portal { target, instance, level, line, controller, pos, extents, target_pos } => {
                        if v < 2 {
                            return Err("Version 1 region files cannot hold portals".into());
                        }
                        let mut numbers = vec![target.to_string()];
                        if v >= 3 {
                            numbers.push(instance.to_string());
                        }
                        if v >= 5 {
                            numbers.push(level.to_string());
                        }
                        if v >= 6 {
                            numbers.push(line.to_string());
                        }
                        if v >= 7 {
                            numbers.push(controller.to_string());
                        }
                        out.push_str(&format!("[trans]\n{}\n{}{}{}\n", numbers.join("  "), comma(pos), comma(extents), comma(target_pos)));
                    }
                    RegionItem::GuildBuilding { template, width, height, pos, dir, up } => {
                        if v < 2 {
                            return Err("Version 1 region files cannot hold guild buildings".into());
                        }
                        out.push_str(&format!("[GuildBuilding]\n{template}  {width}  {height}\n{}{}{}\n", comma(pos), comma(dir), comma(up)));
                    }
                }
            }
        }
    }
    let mut bytes = Vec::with_capacity(out.len() * 2);
    for unit in out.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(bytes)
}

// ---------------------------------------------------------------- server binary

struct Bin<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bin<'_> {
    fn i32(&mut self) -> Result<i32, String> {
        let bytes = self.data.get(self.at..self.at + 4).ok_or_else(|| format!("The file ends early (offset {})", self.at))?;
        self.at += 4;
        Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.i32()? as u32))
    }
    fn vec3(&mut self) -> Result<Vec3, String> {
        Ok(Vec3 { x: self.f32()?, y: self.f32()?, z: self.f32()? })
    }
    fn count(&mut self, what: &str) -> Result<usize, String> {
        let value = self.i32()?;
        usize::try_from(value).ok().filter(|count| *count <= (self.data.len() - self.at) / 4 + 1).ok_or_else(|| format!("{value} is not a valid {what} (offset {})", self.at - 4))
    }
    fn points(&mut self) -> Result<Vec<Vec3>, String> {
        let count = self.count("point count")?;
        (0..count).map(|_| self.vec3()).collect()
    }
}

/// A server file (names and client-only fields are empty; regions have type -1; precincts get IDs 1, 2, … as the
/// file stores none and every server precinct has one).
pub fn parse_sev(data: &[u8], kind: &str) -> Result<MapFile, String> {
    let mut bin = Bin { data, at: 0 };
    let version = bin.i32()? as u32;
    if version == 0 || version > MAX_VERSION {
        return Err(format!("Version {version} is not supported (the server reads 1–{MAX_VERSION})"));
    }
    let file = if kind == "precinct" {
        let count = bin.count("precinct count")?;
        let timestamp = if version >= 5 { bin.i32()? as u32 } else { 0 };
        let mut precincts = Vec::with_capacity(count);
        for index in 0..count {
            let points = bin.count("point count")?;
            let priority = bin.i32()?;
            let city_instance = bin.i32()?;
            let instance = if version >= 4 { bin.i32()? } else { 1 };
            let domain = if version >= 6 { bin.i32()? } else { 0 };
            let city = bin.vec3()?;
            let (rebirths, level_max, level_min, kick_city, kick_pos) = if version >= 7 { (bin.i32()?, bin.i32()?, bin.i32()?, bin.i32()?, bin.vec3()?) } else { (0, 0, 0, 1, Vec3::default()) };
            let points = (0..points).map(|_| bin.vec3()).collect::<Result<Vec<_>, _>>()?;
            precincts.push(Precinct { name: String::new(), id: index as u32 + 1, priority, city_instance, music_interval: 0, music_loop: 0, instance, domain, city, rebirths, level_max, level_min, kick_city, kick_pos, points, marks: Vec::new(), sound: String::new(), music: Vec::new(), sound_night: String::new(), minimap: String::new() });
        }
        MapFile::Precinct { version, timestamp, precincts }
    } else {
        let regions = bin.count("region count")?;
        let portals = if version >= 2 { bin.count("portal count")? } else { 0 };
        let timestamp = if version >= 4 { bin.i32()? as u32 } else { 0 };
        let buff_count = if version >= 8 { bin.count("buff region count")? } else { 0 };
        let (mut items, mut buffs) = (Vec::new(), Vec::new());
        let (mut r, mut t) = (0, 0);
        while r < regions || t < portals || buffs.len() < buff_count {
            match bin.i32()? {
                0 => {
                    items.push(RegionItem::Region { name: String::new(), region_type: -1, points: bin.points()? });
                    r += 1;
                }
                1 => {
                    let target = bin.i32()?;
                    let instance = if version >= 3 { bin.i32()? } else { 1 };
                    let level = if version >= 5 { bin.i32()? } else { 1 };
                    let line = if version >= 6 { bin.i32()? } else { 0 };
                    let controller = if version >= 7 { bin.i32()? } else { 0 };
                    items.push(RegionItem::Portal { target, instance, level, line, controller, pos: bin.vec3()?, extents: bin.vec3()?, target_pos: bin.vec3()? });
                    t += 1;
                }
                2 => {
                    let template = bin.i32()?;
                    let monster = bin.i32()?;
                    buffs.push(BuffRegion { after: items.len(), template, monster, points: bin.points()? });
                }
                other => return Err(format!("Unknown entry type {other} at offset {} (the server stops loading)", bin.at - 4)),
            }
        }
        MapFile::Region { version, timestamp, items, buffs }
    };
    if bin.at != data.len() {
        return Err(format!("{} bytes follow the data (the server ignores them)", data.len() - bin.at));
    }
    Ok(file)
}

/// The server file the map editor would export with the client one: precincts with an ID, regions and portals in
/// the client's order, then the buff regions.
pub fn write_sev(file: &MapFile) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let put = |out: &mut Vec<u8>, value: i32| out.extend_from_slice(&value.to_le_bytes());
    let vec3 = |out: &mut Vec<u8>, p: &Vec3| {
        for value in [p.x, p.y, p.z] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    };
    match file {
        MapFile::Precinct { version, timestamp, precincts } => {
            let v = *version;
            let server: Vec<&Precinct> = precincts.iter().filter(|p| p.id != 0).collect();
            put(&mut out, v as i32);
            put(&mut out, server.len() as i32);
            if v >= 5 {
                put(&mut out, *timestamp as i32);
            }
            for p in server {
                put(&mut out, p.points.len() as i32);
                put(&mut out, p.priority);
                put(&mut out, p.city_instance);
                if v >= 4 {
                    put(&mut out, p.instance);
                }
                if v >= 6 {
                    put(&mut out, p.domain);
                }
                vec3(&mut out, &p.city);
                if v >= 7 {
                    for value in [p.rebirths, p.level_max, p.level_min, p.kick_city] {
                        put(&mut out, value);
                    }
                    vec3(&mut out, &p.kick_pos);
                }
                for point in &p.points {
                    vec3(&mut out, point);
                }
            }
        }
        MapFile::Region { version, timestamp, items, buffs } => {
            let v = *version;
            if !buffs.is_empty() && v < 8 {
                return Err(format!("Buff regions need version 8; this file is version {v}"));
            }
            let regions = items.iter().filter(|item| matches!(item, RegionItem::Region { .. })).count();
            let portals = items.iter().filter(|item| matches!(item, RegionItem::Portal { .. })).count();
            put(&mut out, v as i32);
            put(&mut out, regions as i32);
            if v >= 2 {
                put(&mut out, portals as i32);
            }
            if v >= 4 {
                put(&mut out, *timestamp as i32);
            }
            if v >= 8 {
                put(&mut out, buffs.len() as i32);
            }
            let mut placed = 0;
            let write_buff = |out: &mut Vec<u8>, buff: &BuffRegion| {
                out.extend_from_slice(&2i32.to_le_bytes());
                out.extend_from_slice(&buff.template.to_le_bytes());
                out.extend_from_slice(&buff.monster.to_le_bytes());
                out.extend_from_slice(&(buff.points.len() as i32).to_le_bytes());
                for point in &buff.points {
                    for value in [point.x, point.y, point.z] {
                        out.extend_from_slice(&value.to_le_bytes());
                    }
                }
            };
            let mut ordered: Vec<&BuffRegion> = buffs.iter().collect();
            ordered.sort_by_key(|buff| buff.after);
            let mut pending = ordered.into_iter().peekable();
            for item in items {
                if matches!(item, RegionItem::GuildBuilding { .. }) {
                    continue;
                }
                while let Some(buff) = pending.next_if(|buff| buff.after <= placed) {
                    write_buff(&mut out, buff);
                }
                placed += 1;
                match item {
                    RegionItem::Region { points, .. } => {
                        put(&mut out, 0);
                        put(&mut out, points.len() as i32);
                        points.iter().for_each(|point| vec3(&mut out, point));
                    }
                    RegionItem::Portal { target, instance, level, line, controller, pos, extents, target_pos } => {
                        put(&mut out, 1);
                        put(&mut out, *target);
                        if v >= 3 {
                            put(&mut out, *instance);
                        }
                        if v >= 5 {
                            put(&mut out, *level);
                        }
                        if v >= 6 {
                            put(&mut out, *line);
                        }
                        if v >= 7 {
                            put(&mut out, *controller);
                        }
                        for p in [pos, extents, target_pos] {
                            vec3(&mut out, p);
                        }
                    }
                    RegionItem::GuildBuilding { .. } => {}
                }
            }
            for buff in pending {
                write_buff(&mut out, buff);
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- files

fn token(data: &[u8]) -> String {
    format!("{:x}", Md5::digest(data))
}

/// `precinct` or `region`, by file name, else by content.
fn kind_of(path: &Path, text: &str) -> &'static str {
    let name = path.file_name().map(|name| name.to_string_lossy().to_lowercase()).unwrap_or_default();
    if name.starts_with("precinct") || name.starts_with("pricinct") {
        "precinct"
    } else if name.starts_with("region") {
        "region"
    } else if text.contains("[region]") || text.contains("[trans]") || text.to_lowercase().contains("region file") {
        "region"
    } else {
        "precinct"
    }
}

pub fn read_clt(path: &Path) -> Result<(MapFile, String), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let text = decode_text(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    let file = if kind_of(path, &text) == "precinct" { parse_precinct_clt(&text) } else { parse_region_clt(&text) };
    Ok((file.map_err(|error| format!("{}: {error}", path.display()))?, token(&bytes)))
}

pub fn read_sev(path: &Path, kind: &str) -> Result<MapFile, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    parse_sev(&bytes, kind).map_err(|error| format!("{}: {error}", path.display()))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideFile {
    pub path: String,
    pub file: MapFile,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub path: String,
    pub token: String,
    pub file: MapFile,
    /// The other client file of the folder (precinct ↔ region), shown on the map.
    pub sibling: Option<SideFile>,
    pub sibling_error: Option<String>,
    /// The folder name (the map's client path, e.g. `z1`).
    pub folder: String,
}

pub fn open(path: &Path) -> Result<Opened, String> {
    let (file, token) = read_clt(path)?;
    let other = if file.kind() == "precinct" { "region.clt" } else { "precinct.clt" };
    let sibling_path = path.with_file_name(other);
    let (sibling, sibling_error) = if sibling_path.is_file() {
        match read_clt(&sibling_path) {
            Ok((file, _)) => (Some(SideFile { path: sibling_path.display().to_string(), file }), None),
            Err(error) => (None, Some(error)),
        }
    } else {
        (None, None)
    };
    let folder = path.parent().and_then(|parent| parent.file_name()).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Opened { path: path.display().to_string(), token, file, sibling, sibling_error, folder })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub token: String,
    pub timestamp: u32,
    pub server_path: String,
    pub backups: Vec<String>,
}

/// Writes the client file and its server file with one new timestamp (the client compares them), each in the
/// file's version; both read back before anything is replaced.
pub fn save(path: &Path, token_now: Option<&str>, file: &MapFile, server_path: &Path, backup: bool, replace_changed: bool) -> Result<SaveReport, String> {
    let mut file = file.clone();
    let now = chrono::Utc::now().timestamp().clamp(0, i32::MAX as i64) as u32;
    file.set_timestamp(now.max(file.timestamp().wrapping_add(1)).min(i32::MAX as u32));
    let client = write_clt(&file)?;
    let server = write_sev(&file)?;
    let text = decode_text(&client)?;
    let back = if file.kind() == "precinct" { parse_precinct_clt(&text)? } else { parse_region_clt(&text)? };
    let without_buffs = |file: &MapFile| match file {
        MapFile::Region { version, timestamp, items, .. } => MapFile::Region { version: *version, timestamp: *timestamp, items: items.clone(), buffs: Vec::new() },
        other => other.clone(),
    };
    if back != without_buffs(&file) {
        return Err("The client file would not read back as these values; nothing was saved".into());
    }
    let server_back = parse_sev(&server, file.kind())?;
    if write_sev(&server_back)? != server {
        return Err("The server file would not read back as written; nothing was saved".into());
    }
    if !replace_changed {
        if let (Some(expected), Ok(current)) = (token_now, std::fs::read(path)) {
            if token(&current) != expected {
                return Err(format!("CHANGED_ON_DISK: {} was changed by another program since it was opened", path.display()));
            }
        }
    }
    if let Some(parent) = server_path.parent() {
        if !parent.is_dir() {
            return Err(format!("The server folder {} does not exist", parent.display()));
        }
    }
    let mut backups = Vec::new();
    if backup {
        for target in [path, server_path] {
            if target.is_file() {
                backups.push(crate::backup::archive(target, &[target.to_path_buf()])?.display().to_string());
            }
        }
    }
    crate::path_data::write_replacing(path, &client)?;
    crate::path_data::write_replacing(server_path, &server)?;
    Ok(SaveReport { path: path.display().to_string(), token: token(&client), timestamp: file.timestamp(), server_path: server_path.display().to_string(), backups })
}

// ---------------------------------------------------------------- problems

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// `error`: the game misbehaves; `warning`; `note`.
    pub severity: &'static str,
    /// `precinct`, `item` (region file entry), `buff` or `file`.
    pub target: &'static str,
    pub index: Option<usize>,
    pub message: String,
}

fn cross(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    (b.x as f64 - a.x as f64) * (c.z as f64 - a.z as f64) - (b.z as f64 - a.z as f64) * (c.x as f64 - a.x as f64)
}

/// Whether two edges that share no point cross (on the ground plane).
fn crosses(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> bool {
    let (d1, d2, d3, d4) = (cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// The map editor refuses polygons with fewer than 3 points or crossing edges (`IntersectWithSelf`).
fn polygon_problem(points: &[Vec3]) -> Option<String> {
    if points.len() < 3 {
        return Some(format!("{} point{}: an area needs at least 3", points.len(), if points.len() == 1 { "" } else { "s" }));
    }
    let n = points.len();
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if crosses(points[i], points[(i + 1) % n], points[j], points[(j + 1) % n]) {
                return Some(format!("The outline crosses itself (edges {}–{} and {}–{}): the game's inside test gives wrong answers there", i + 1, i + 2, j + 1, (j + 1) % n + 1));
            }
        }
    }
    None
}

/// `instances`: IDs from the client's instance.txt (empty: not checked).
pub fn problems(file: &MapFile, server: Option<&MapFile>, instances: &[i32]) -> Vec<Problem> {
    let mut out = Vec::new();
    let known = |id: i32| instances.is_empty() || instances.contains(&id);
    let mut push = |severity, target, index, message: String| out.push(Problem { severity, target, index, message });
    match file {
        MapFile::Precinct { version, precincts, .. } => {
            let mut ids: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
            for (index, p) in precincts.iter().enumerate() {
                let at = Some(index);
                if let Some(problem) = polygon_problem(&p.points) {
                    push("warning", "precinct", at, problem);
                }
                if p.id != 0 {
                    if let Some(first) = ids.insert(p.id, index) {
                        push("warning", "precinct", at, format!("ID {} is also precinct {}", p.id, first + 1));
                    }
                }
                if !known(p.instance) {
                    push("warning", "precinct", at, format!("Instance {} is not in the client's instance list", p.instance));
                }
                if !known(p.city_instance) {
                    push("warning", "precinct", at, format!("Return city instance {} is not in the client's instance list", p.city_instance));
                }
                if *version >= 7 && (p.level_max > 0 || p.level_min > 0) {
                    if p.kick_city <= 0 {
                        push("warning", "precinct", at, "Level limits need a kick-out city above 0: the server ignores them".into());
                    } else if !known(p.kick_city) {
                        push("warning", "precinct", at, format!("Kick-out city instance {} is not in the client's instance list", p.kick_city));
                    }
                    if p.level_max > 0 && p.level_min > p.level_max {
                        push("warning", "precinct", at, format!("Minimum level {} is above the maximum {}: everyone is moved out", p.level_min, p.level_max));
                    }
                }
                for (what, text) in [("name", &p.name), ("sound", &p.sound), ("night sound", &p.sound_night), ("minimap", &p.minimap)] {
                    if text.contains('"') || text.contains('\n') {
                        push("error", "precinct", at, format!("The {what} has a quote or a line break, which the file cannot hold"));
                    }
                }
            }
            let client_only = precincts.iter().filter(|p| p.id == 0).count();
            if client_only > 0 {
                push("note", "file", None, format!("{client_only} precinct{} with ID 0 {} client only (names, music): the server file leaves {}", if client_only == 1 { "" } else { "s" }, if client_only == 1 { "is" } else { "are" }, if client_only == 1 { "it out" } else { "them out" }));
            }
            if let Some(MapFile::Precinct { timestamp: server_stamp, precincts: server_precincts, version: server_version }) = server {
                compare_header(&mut out, file, *server_stamp, *server_version);
                let expected = precincts.iter().filter(|p| p.id != 0).count();
                if expected != server_precincts.len() {
                    out.push(Problem { severity: "warning", target: "file", index: None, message: format!("The server file has {} precincts; these values give {expected} (saving writes it again)", server_precincts.len()) });
                }
            }
        }
        MapFile::Region { version, items, buffs, .. } => {
            for (index, item) in items.iter().enumerate() {
                let at = Some(index);
                match item {
                    RegionItem::Region { name, region_type, points } => {
                        if let Some(problem) = polygon_problem(points) {
                            push("warning", "item", at, problem);
                        }
                        if !(0..=1).contains(region_type) {
                            push("warning", "item", at, format!("Region type {region_type}: the client knows 0 (PK) and 1 (safe zone); the server treats every region as a safe zone"));
                        }
                        if name.contains('"') {
                            push("error", "item", at, "The name has a quote, which the file cannot hold".into());
                        }
                    }
                    RegionItem::Portal { target, instance, level, extents, .. } => {
                        if extents.x <= 0.0 || extents.y <= 0.0 || extents.z <= 0.0 {
                            push("warning", "item", at, "The portal has no size: nobody can walk into it".into());
                        }
                        if !known(*target) {
                            push("warning", "item", at, format!("Target instance {target} is not in the client's instance list"));
                        }
                        if !known(*instance) {
                            push("warning", "item", at, format!("Source instance {instance} is not in the client's instance list"));
                        }
                        if *level > 1 && *version < 5 {
                            push("warning", "item", at, "The level limit needs version 5".into());
                        }
                    }
                    RegionItem::GuildBuilding { .. } => {}
                }
            }
            for (index, buff) in buffs.iter().enumerate() {
                if let Some(problem) = polygon_problem(&buff.points) {
                    push("warning", "buff", Some(index), problem);
                }
            }
            if !buffs.is_empty() && *version < 8 {
                push("error", "file", None, format!("Buff regions need version 8; this file is version {version} (saving fails)"));
            }
            if let Some(MapFile::Region { timestamp: server_stamp, items: server_items, version: server_version, .. }) = server {
                compare_header(&mut out, file, *server_stamp, *server_version);
                let portals = |items: &[RegionItem]| items.iter().filter_map(|item| match item {
                    RegionItem::Portal { target, instance, .. } => Some((*target, *instance)),
                    _ => None,
                }).collect::<Vec<_>>();
                let (here, there) = (portals(items), portals(server_items));
                if here != there {
                    let first = here.iter().zip(&there).position(|(a, b)| a != b).unwrap_or(here.len().min(there.len()));
                    out.push(Problem { severity: "error", target: "file", index: None, message: format!("The server file's portals differ from these from portal {} on ({} here, {} there): the client sends the server the portal's number, so players land in the wrong place until both are saved together", first + 1, here.len(), there.len()) });
                }
            }
        }
    }
    out
}

fn compare_header(out: &mut Vec<Problem>, file: &MapFile, server_stamp: u32, server_version: u32) {
    if server_stamp != file.timestamp() {
        out.push(Problem { severity: "warning", target: "file", index: None, message: format!("The server file's timestamp ({server_stamp}) differs from the client file's ({}): players see an instance data error on entering the map. Saving writes both with one new timestamp.", file.timestamp()) });
    }
    if server_version != file.version() {
        out.push(Problem { severity: "note", target: "file", index: None, message: format!("The server file is version {server_version}, the client file version {} (saving writes both as {})", file.version(), file.version()) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<(&'static str, &'static str)> {
        vec![
            ("E:/Games/ForsakenJD/element/maps", "clt"),
            ("E:/Games/Elite Jade Dynasty - HDN/element/maps", "clt"),
            ("E:/Game Dev/JD/zxserver/zgame/gs/config", "sev"),
            ("E:/Game Dev/JD/1559/gamed/config", "sev"),
            ("E:/Game Dev/JD/zx_18_compiled/gamed/config", "sev"),
        ]
    }

    /// Every official-looking file reads, and writes back byte for byte (client text in the editor's layout, server
    /// binary exactly); the known hand-edited client files fail with a line number.
    #[test]
    fn samples_round_trip() {
        let (mut read, mut same, mut failed) = (0, 0, Vec::new());
        for (root, ext) in roots() {
            let Ok(dirs) = std::fs::read_dir(root) else { continue };
            for dir in dirs.flatten() {
                for kind in ["precinct", "region"] {
                    let path = dir.path().join(format!("{kind}.{ext}"));
                    let Ok(bytes) = std::fs::read(&path) else { continue };
                    if ext == "sev" {
                        let file = parse_sev(&bytes, kind).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                        assert_eq!(write_sev(&file).unwrap(), bytes, "{}", path.display());
                        read += 1;
                        same += 1;
                        continue;
                    }
                    match read_clt(&path) {
                        Ok((file, _)) => {
                            read += 1;
                            if write_clt(&file).unwrap() == bytes {
                                same += 1;
                            } else {
                                // Hand-edited files differ in spacing; they must still read back the same.
                                let text = decode_text(&write_clt(&file).unwrap()).unwrap();
                                let back = if kind == "precinct" { parse_precinct_clt(&text) } else { parse_region_clt(&text) }.unwrap();
                                assert_eq!(back, file, "{}", path.display());
                            }
                        }
                        Err(error) => failed.push(error),
                    }
                }
            }
        }
        println!("MAPREGIONS read {read}, byte-identical {same}, failed {}:\n{}", failed.len(), failed.join("\n"));
        assert!(failed.len() <= 8, "only the known broken client files fail");
    }

    /// The server file made from a client file equals the shipped server file of the same timestamp (within the
    /// text's six decimals, buff regions aside).
    #[test]
    fn client_files_export_the_server_files() {
        let pairs = [("E:/Games/ForsakenJD/element/maps", "E:/Game Dev/JD/1559/gamed/config"), ("E:/Games/ForsakenJD/element/maps", "E:/Game Dev/JD/zxserver/zgame/gs/config")];
        let (mut compared, mut equal) = (0, 0);
        for (client, server) in pairs {
            let Ok(dirs) = std::fs::read_dir(client) else { continue };
            for dir in dirs.flatten() {
                for kind in ["precinct", "region"] {
                    let Ok((file, _)) = read_clt(&dir.path().join(format!("{kind}.clt"))) else { continue };
                    let Ok(shipped) = read_sev(&Path::new(server).join(dir.file_name()).join(format!("{kind}.sev")), kind) else { continue };
                    if shipped.timestamp() != file.timestamp() || shipped.version() != file.version() {
                        continue;
                    }
                    compared += 1;
                    let mut file = file;
                    if let (MapFile::Region { buffs, .. }, MapFile::Region { buffs: server_buffs, .. }) = (&mut file, &shipped) {
                        *buffs = server_buffs.clone();
                    }
                    let exported = parse_sev(&write_sev(&file).unwrap(), kind).unwrap();
                    let round = |file: &MapFile| serde_json::to_string(file).unwrap().split(|c: char| c == ',' || c == ':' || c == '[' || c == ']' || c == '{' || c == '}').map(|part| part.parse::<f64>().map(|n| format!("{:.3}", n)).unwrap_or_else(|_| part.to_string())).collect::<Vec<_>>().join("|");
                    if round(&exported) == round(&shipped) {
                        equal += 1;
                    } else if kind == "region" {
                        println!("MAPREGIONS region differs: {} {}", server, dir.path().display());
                    }
                }
            }
        }
        println!("MAPREGIONS compared {compared}, equal {equal}");
        if compared > 0 {
            assert!(equal * 10 >= compared * 8, "most exports equal the shipped server files ({equal} of {compared})");
        }
    }

    /// Saving writes the client and server file with one new timestamp, keeps the server's buff regions, and both
    /// read back with the same values (temp copies; the fixtures are never written).
    #[test]
    fn saving_writes_both_files_with_one_timestamp() {
        let (client, server) = (Path::new("E:/Games/ForsakenJD/element/maps/z1/region.clt"), Path::new("E:/Game Dev/JD/1559/gamed/config/z1/region.sev"));
        if !client.is_file() || !server.is_file() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("jdide-map-regions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (clt, sev) = (dir.join("region.clt"), dir.join("region.sev"));
        std::fs::copy(client, &clt).unwrap();
        std::fs::copy(server, &sev).unwrap();
        let opened = open(&clt).unwrap();
        let shipped = read_sev(&sev, "region").unwrap();
        let MapFile::Region { version, timestamp, mut items, .. } = opened.file.clone() else { panic!("a region file") };
        let MapFile::Region { buffs, .. } = shipped.clone() else { panic!() };
        assert!(!buffs.is_empty(), "z1 has buff regions on the server");
        if let Some(RegionItem::Region { name, .. }) = items.first_mut() {
            *name = "Edited".into();
        }
        let edited = MapFile::Region { version, timestamp, items, buffs: buffs.clone() };
        let report = save(&clt, Some(&opened.token), &edited, &sev, false, false).unwrap();
        assert!(report.timestamp > timestamp);
        let (client_back, _) = read_clt(&clt).unwrap();
        let server_back = read_sev(&sev, "region").unwrap();
        assert_eq!(client_back.timestamp(), report.timestamp);
        assert_eq!(server_back.timestamp(), report.timestamp);
        let MapFile::Region { items: back_items, .. } = &client_back else { panic!() };
        assert!(matches!(&back_items[0], RegionItem::Region { name, .. } if name == "Edited"));
        let MapFile::Region { buffs: back_buffs, items: server_items, .. } = &server_back else { panic!() };
        assert_eq!(back_buffs, &buffs);
        let MapFile::Region { items: shipped_items, .. } = &shipped else { panic!() };
        assert_eq!(server_items.len(), shipped_items.len());
        // A stale token is refused.
        assert!(save(&clt, Some(&opened.token), &edited, &sev, false, false).unwrap_err().starts_with("CHANGED_ON_DISK"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reading_reports_lines_and_problems() {
        let text = "//  Element region file (client version)\n\nversion  5\n1331203618\n\n[region]\n\"Safe\"\n1  3\n0 0 0\n10 0 0\n0 0 10\n\n[trans]\n5  3  0\t0\n1, 2, 3\n1, 1, 1\n4, 5, 6\n";
        let error = parse_region_clt(text).unwrap_err();
        assert!(error.contains("Line 15") || error.contains("line"), "{error}");
        let file = MapFile::Region {
            version: 8,
            timestamp: 1,
            items: vec![RegionItem::Region { name: "x".into(), region_type: 1, points: vec![Vec3 { x: 0.0, y: 0.0, z: 0.0 }, Vec3 { x: 10.0, y: 0.0, z: 10.0 }, Vec3 { x: 10.0, y: 0.0, z: 0.0 }, Vec3 { x: 0.0, y: 0.0, z: 10.0 }] }],
            buffs: Vec::new(),
        };
        assert!(problems(&file, None, &[]).iter().any(|problem| problem.message.contains("crosses itself")));
        let precincts = MapFile::Precinct { version: 8, timestamp: 1, precincts: Vec::new() };
        let text = decode_text(&write_clt(&precincts).unwrap()).unwrap();
        assert_eq!(parse_precinct_clt(&text).unwrap(), precincts);
    }
}

