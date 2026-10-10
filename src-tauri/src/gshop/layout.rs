//! Item record layouts for gshop files: built-in ones (formats/gshop) and the user's own
//! (`<config>/gshop-layouts/*.json`), so a newer client's shop can be read by describing its record.
//!
//! A layout is an ordered list of fields. A field can carry a *meaning* (`price`, `name`, …) the editor
//! knows; fields without one are shown and edited generically ("Other fields"). Decoding reads every
//! field at its offset; encoding writes a field only when its value changed, so stored bytes (text after
//! a terminator, padding) survive.

use encoding_rs::GBK;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{OtherField, ShopItem, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldType {
    U8,
    U16,
    U32,
    I32,
    F32,
    /// One byte; any non-zero value reads as true.
    Bool,
    /// UTF-16 text of `len` characters (2 × len bytes).
    Wstr { len: usize },
    /// GBK text of `len` bytes.
    Str { len: usize },
    /// Raw bytes (not known yet), shown as hex.
    Bytes { len: usize },
    /// `count` repeats of `fields` (paths like `buy[1].price`).
    Group { count: usize, fields: Vec<Field> },
}

impl FieldType {
    pub fn size(&self) -> usize {
        match self {
            FieldType::U8 | FieldType::Bool => 1,
            FieldType::U16 => 2,
            FieldType::U32 | FieldType::I32 | FieldType::F32 => 4,
            FieldType::Wstr { len } => len * 2,
            FieldType::Str { len } | FieldType::Bytes { len } => *len,
            FieldType::Group { count, fields } => count * fields.iter().map(|field| field.ty.size()).sum::<usize>(),
        }
    }

    /// Short name for the UI (`u32`, `wstr:32`, `bytes:25`).
    pub fn label(&self) -> String {
        match self {
            FieldType::U8 => "u8".into(),
            FieldType::U16 => "u16".into(),
            FieldType::U32 => "u32".into(),
            FieldType::I32 => "i32".into(),
            FieldType::F32 => "f32".into(),
            FieldType::Bool => "bool".into(),
            FieldType::Wstr { len } => format!("wstr:{len}"),
            FieldType::Str { len } => format!("str:{len}"),
            FieldType::Bytes { len } => format!("bytes:{len}"),
            FieldType::Group { count, .. } => format!("group×{count}"),
        }
    }

    fn integer(&self) -> bool {
        matches!(self, FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::I32)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub name: String,
    #[serde(flatten)]
    pub ty: FieldType,
    /// What the editor knows this field as (see [`MEANINGS`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meaning: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub fields: Vec<Field>,
    /// Shipped with JD IDE (not saved in layout files).
    #[serde(default, skip_deserializing)]
    pub builtin: bool,
}

/// The meanings the editor knows, with the kind of field each needs.
pub const MEANINGS: &[(&str, MeaningKind)] = &[
    ("id", MeaningKind::Integer),
    ("num", MeaningKind::Integer),
    ("icon", MeaningKind::Gbk),
    ("price", MeaningKind::Integer),
    ("time", MeaningKind::Integer),
    ("discount", MeaningKind::Integer),
    ("bonus", MeaningKind::Integer),
    ("props", MeaningKind::Integer),
    ("main_type", MeaningKind::Integer),
    ("sub_type", MeaningKind::Integer),
    ("local_id", MeaningKind::Integer),
    ("description", MeaningKind::Text),
    ("name", MeaningKind::Text),
    ("has_present", MeaningKind::Flag),
    ("present_name", MeaningKind::Utf16),
    ("present_id", MeaningKind::Integer),
    ("present_count", MeaningKind::Integer),
    ("present_time", MeaningKind::Integer),
    ("present_icon", MeaningKind::Gbk),
    ("present_bind", MeaningKind::Flag),
    ("present_description", MeaningKind::Utf16),
    ("valid_type", MeaningKind::Integer),
    ("valid_start", MeaningKind::Integer),
    ("valid_end", MeaningKind::Integer),
    ("valid_param", MeaningKind::Integer),
    ("search_keys", MeaningKind::Utf16),
    // The Lucky Bag shop (gshop4.data): its position, and the item paid instead of a price.
    ("place", MeaningKind::Integer),
    ("price_item", MeaningKind::Integer),
    ("price_item_count", MeaningKind::Integer),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeaningKind {
    Integer,
    Flag,
    Utf16,
    Gbk,
    /// UTF-16 or GBK (names and descriptions; the Lucky Bag shop stores its names as GBK).
    Text,
}

/// A field (inside groups: one repeat of a group member) at its offset.
#[derive(Debug, Clone)]
pub struct Leaf {
    pub path: String,
    pub ty: FieldType,
    pub offset: usize,
    pub meaning: Option<String>,
}

impl Layout {
    pub fn size(&self) -> usize {
        self.fields.iter().map(|field| field.ty.size()).sum()
    }

    pub fn leaves(&self) -> Vec<Leaf> {
        fn walk(fields: &[Field], prefix: &str, offset: &mut usize, out: &mut Vec<Leaf>, top: bool) {
            for field in fields {
                let path = if prefix.is_empty() { field.name.clone() } else { format!("{prefix}.{}", field.name) };
                match &field.ty {
                    FieldType::Group { count, fields } => {
                        for index in 0..*count {
                            walk(fields, &format!("{path}[{index}]"), offset, out, false);
                        }
                    }
                    ty => {
                        out.push(Leaf { path, ty: ty.clone(), offset: *offset, meaning: if top { field.meaning.clone() } else { None } });
                        *offset += ty.size();
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.fields, "", &mut 0, &mut out, true);
        out
    }

    /// Text meanings with their slot size and whether it is UTF-16 (characters) or GBK (bytes).
    pub fn text_limits(&self) -> HashMap<String, (usize, bool)> {
        self.fields
            .iter()
            .filter_map(|field| {
                let meaning = field.meaning.clone()?;
                match field.ty {
                    FieldType::Wstr { len } => Some((meaning, (len, true))),
                    FieldType::Str { len } => Some((meaning, (len, false))),
                    _ => None,
                }
            })
            .collect()
    }

    /// The meanings this layout provides.
    pub fn meanings(&self) -> Vec<String> {
        self.fields.iter().filter_map(|field| field.meaning.clone()).collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || !self.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err("The layout ID may use letters, digits, - and _ only".into());
        }
        if self.name.trim().is_empty() {
            return Err("The layout needs a name".into());
        }
        if self.fields.is_empty() {
            return Err("The layout has no fields".into());
        }
        fn check(fields: &[Field], top: bool, seen_meanings: &mut Vec<String>) -> Result<(), String> {
            let mut names: Vec<&str> = Vec::new();
            for field in fields {
                let name = field.name.trim();
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Err(format!("Field name {:?}: use letters, digits and _ only", field.name));
                }
                if names.contains(&name) {
                    return Err(format!("Two fields are named {name}"));
                }
                names.push(name);
                match &field.ty {
                    FieldType::Wstr { len } | FieldType::Str { len } | FieldType::Bytes { len } if *len == 0 => return Err(format!("{name}: the length must be at least 1")),
                    FieldType::Group { count, fields } => {
                        if *count == 0 || fields.is_empty() {
                            return Err(format!("{name}: a group needs a count and fields"));
                        }
                        check(fields, false, seen_meanings)?;
                    }
                    _ => {}
                }
                if let Some(meaning) = &field.meaning {
                    if !top {
                        return Err(format!("{name}: fields inside a group cannot have a meaning"));
                    }
                    let kind = MEANINGS.iter().find(|(known, _)| known == meaning).map(|(_, kind)| *kind).ok_or_else(|| format!("{name}: unknown meaning {meaning}"))?;
                    let fits = match kind {
                        MeaningKind::Integer => field.ty.integer(),
                        MeaningKind::Flag => field.ty.integer() || field.ty == FieldType::Bool,
                        MeaningKind::Utf16 => matches!(field.ty, FieldType::Wstr { .. }),
                        MeaningKind::Gbk => matches!(field.ty, FieldType::Str { .. }),
                        MeaningKind::Text => matches!(field.ty, FieldType::Wstr { .. } | FieldType::Str { .. }),
                    };
                    if !fits {
                        return Err(format!("{name}: the meaning {meaning} needs {} field", match kind { MeaningKind::Integer => "an integer", MeaningKind::Flag => "a bool or integer", MeaningKind::Utf16 => "a UTF-16 text (wstr)", MeaningKind::Gbk => "a GBK text (str)", MeaningKind::Text => "a text (wstr or str)" }));
                    }
                    if seen_meanings.contains(meaning) {
                        return Err(format!("Two fields have the meaning {meaning}"));
                    }
                    seen_meanings.push(meaning.clone());
                }
            }
            Ok(())
        }
        check(&self.fields, true, &mut Vec::new())
    }
}

// ── Reading and writing one field ──

pub fn text16(slot: &[u8]) -> String {
    let units: Vec<u16> = slot.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).take_while(|unit| *unit != 0).collect();
    String::from_utf16_lossy(&units)
}

pub fn text8(slot: &[u8]) -> String {
    let end = slot.iter().position(|byte| *byte == 0).unwrap_or(slot.len());
    GBK.decode_without_bom_handling(&slot[..end]).0.into_owned()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" ")
}

pub fn read(ty: &FieldType, slot: &[u8]) -> Value {
    match ty {
        FieldType::U8 => Value::Int(slot[0] as i64),
        FieldType::U16 => Value::Int(u16::from_le_bytes([slot[0], slot[1]]) as i64),
        FieldType::U32 => Value::Int(u32::from_le_bytes(slot[..4].try_into().unwrap()) as i64),
        FieldType::I32 => Value::Int(i32::from_le_bytes(slot[..4].try_into().unwrap()) as i64),
        FieldType::F32 => Value::Float(f32::from_le_bytes(slot[..4].try_into().unwrap()) as f64),
        FieldType::Bool => Value::Bool(slot[0] != 0),
        FieldType::Wstr { .. } => Value::Text(text16(slot)),
        FieldType::Str { .. } => Value::Text(text8(slot)),
        FieldType::Bytes { .. } => Value::Text(hex(slot)),
        FieldType::Group { .. } => Value::Text(String::new()),
    }
}

/// Writes a value into its slot unless the slot already holds it.
pub fn write(ty: &FieldType, slot: &mut [u8], value: &Value, what: &str) -> Result<(), String> {
    if &read(ty, slot) == value {
        return Ok(());
    }
    let int = |value: &Value| match value {
        Value::Int(number) => Ok(*number),
        Value::Bool(flag) => Ok(*flag as i64),
        Value::Float(number) if number.fract() == 0.0 => Ok(*number as i64),
        _ => Err(format!("{what} needs a whole number")),
    };
    let range = |number: i64, min: i64, max: i64| if (min..=max).contains(&number) { Ok(number) } else { Err(format!("{what}: {number} is out of range ({min} to {max})")) };
    match ty {
        FieldType::U8 => slot[0] = range(int(value)?, 0, u8::MAX as i64)? as u8,
        FieldType::U16 => slot.copy_from_slice(&(range(int(value)?, 0, u16::MAX as i64)? as u16).to_le_bytes()),
        FieldType::U32 => slot.copy_from_slice(&(range(int(value)?, 0, u32::MAX as i64)? as u32).to_le_bytes()),
        FieldType::I32 => slot.copy_from_slice(&(range(int(value)?, i32::MIN as i64, i32::MAX as i64)? as i32).to_le_bytes()),
        FieldType::F32 => {
            let number = match value {
                Value::Float(number) => *number,
                Value::Int(number) => *number as f64,
                _ => return Err(format!("{what} needs a number")),
            };
            slot.copy_from_slice(&(number as f32).to_le_bytes());
        }
        FieldType::Bool => slot[0] = (int(value)? != 0) as u8,
        FieldType::Wstr { len } => {
            let Value::Text(text) = value else { return Err(format!("{what} needs text")) };
            let units: Vec<u16> = text.encode_utf16().collect();
            // Official files fill a slot completely (no terminator); the client reads them.
            if units.len() > *len {
                return Err(format!("{what} is too long: {} of at most {len} characters", units.len()));
            }
            slot.fill(0);
            for (index, unit) in units.iter().enumerate() {
                slot[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
            }
        }
        FieldType::Str { len } => {
            let Value::Text(text) = value else { return Err(format!("{what} needs text")) };
            let (encoded, _, unmappable) = GBK.encode(text);
            if unmappable {
                return Err(format!("{what} has characters GBK cannot store"));
            }
            if encoded.len() > *len {
                return Err(format!("{what} is too long: {} of at most {len} bytes", encoded.len()));
            }
            slot.fill(0);
            slot[..encoded.len()].copy_from_slice(&encoded);
        }
        FieldType::Bytes { len } => {
            let Value::Text(text) = value else { return Err(format!("{what} needs hex bytes")) };
            let digits: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            if digits.len() != len * 2 {
                return Err(format!("{what} needs exactly {len} bytes ({} hex digits)", len * 2));
            }
            for (index, byte) in slot.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16).map_err(|_| format!("{what}: not hex"))?;
            }
        }
        FieldType::Group { .. } => {}
    }
    Ok(())
}

// ── Items ──

pub(super) fn get(item: &ShopItem, meaning: &str) -> Value {
    let int = |number: i64| Value::Int(number);
    match meaning {
        "id" => int(item.id as i64),
        "num" => int(item.num as i64),
        "icon" => Value::Text(item.icon.clone()),
        "price" => int(item.price as i64),
        "time" => int(item.time as i64),
        "discount" => int(item.discount as i64),
        "bonus" => int(item.bonus as i64),
        "props" => int(item.props as i64),
        "main_type" => int(item.main_type as i64),
        "sub_type" => int(item.sub_type as i64),
        "local_id" => int(item.local_id as i64),
        "description" => Value::Text(item.description.clone()),
        "name" => Value::Text(item.name.clone()),
        "has_present" => Value::Bool(item.has_present),
        "present_name" => Value::Text(item.present_name.clone()),
        "present_id" => int(item.present_id as i64),
        "present_count" => int(item.present_count as i64),
        "present_time" => int(item.present_time as i64),
        "present_icon" => Value::Text(item.present_icon.clone()),
        "present_bind" => Value::Bool(item.present_bind),
        "present_description" => Value::Text(item.present_description.clone()),
        "valid_type" => int(item.valid_type as i64),
        "valid_start" => int(item.valid_start as i64),
        "valid_end" => int(item.valid_end as i64),
        "valid_param" => int(item.valid_param as i64),
        "search_keys" => Value::Text(item.search_keys.clone()),
        "place" => int(item.place as i64),
        "price_item" => int(item.price_item as i64),
        "price_item_count" => int(item.price_item_count as i64),
        _ => Value::Int(0),
    }
}

pub(super) fn set(item: &mut ShopItem, meaning: &str, value: Value) {
    let number = match &value {
        Value::Int(number) => *number,
        Value::Bool(flag) => *flag as i64,
        Value::Float(number) => *number as i64,
        Value::Text(_) => 0,
    };
    let text = match &value {
        Value::Text(text) => text.clone(),
        _ => String::new(),
    };
    match meaning {
        "id" => item.id = number as u32,
        "num" => item.num = number as u32,
        "icon" => item.icon = text,
        "price" => item.price = number as u32,
        "time" => item.time = number as u32,
        "discount" => item.discount = number as i32,
        "bonus" => item.bonus = number as i32,
        "props" => item.props = number as u32,
        "main_type" => item.main_type = number as i32,
        "sub_type" => item.sub_type = number as i32,
        "local_id" => item.local_id = number as i32,
        "description" => item.description = text,
        "name" => item.name = text,
        "has_present" => item.has_present = number != 0,
        "present_name" => item.present_name = text,
        "present_id" => item.present_id = number as u32,
        "present_count" => item.present_count = number as u32,
        "present_time" => item.present_time = number as u32,
        "present_icon" => item.present_icon = text,
        "present_bind" => item.present_bind = number != 0,
        "present_description" => item.present_description = text,
        "valid_type" => item.valid_type = number as i32,
        "valid_start" => item.valid_start = number as i32,
        "valid_end" => item.valid_end = number as i32,
        "valid_param" => item.valid_param = number as i32,
        "search_keys" => item.search_keys = text,
        "place" => item.place = number as i32,
        "price_item" => item.price_item = number as u32,
        "price_item_count" => item.price_item_count = number as u32,
        _ => {}
    }
}

pub fn decode(layout: &Layout, record: &[u8]) -> ShopItem {
    let mut item = ShopItem::default();
    for leaf in layout.leaves() {
        let Some(slot) = record.get(leaf.offset..leaf.offset + leaf.ty.size()) else { continue };
        let value = read(&leaf.ty, slot);
        match &leaf.meaning {
            Some(meaning) => set(&mut item, meaning, value),
            None => item.other.push(OtherField { path: leaf.path, ty: leaf.ty.label(), value }),
        }
    }
    item
}

/// Writes an item over its stored record (`base`), or over zeros for a layout-sized record.
pub fn encode(layout: &Layout, item: &ShopItem, base: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let size = layout.size();
    let mut record = vec![0u8; size];
    if let Some(base) = base {
        let len = base.len().min(size);
        record[..len].copy_from_slice(&base[..len]);
    }
    for leaf in layout.leaves() {
        let value = match &leaf.meaning {
            Some(meaning) => get(item, meaning),
            None => match item.other.iter().find(|other| other.path == leaf.path) {
                Some(other) => other.value.clone(),
                None => continue,
            },
        };
        let what = leaf.meaning.as_deref().map(|meaning| meaning.replace('_', " ")).unwrap_or_else(|| leaf.path.clone());
        write(&leaf.ty, &mut record[leaf.offset..leaf.offset + leaf.ty.size()], &value, &what)?;
    }
    Ok(record)
}

/// Every field of a record as (path, type, value) for previews.
pub fn describe(layout: &Layout, record: &[u8]) -> Vec<(String, String, String)> {
    layout
        .leaves()
        .into_iter()
        .map(|leaf| {
            let value = match record.get(leaf.offset..leaf.offset + leaf.ty.size()) {
                Some(slot) => match read(&leaf.ty, slot) {
                    Value::Int(number) => number.to_string(),
                    Value::Float(number) => format!("{number}"),
                    Value::Bool(flag) => flag.to_string(),
                    Value::Text(text) => text,
                },
                None => "(past the record)".into(),
            };
            let label = leaf.meaning.clone().map(|meaning| format!("{} → {meaning}", leaf.path)).unwrap_or(leaf.path);
            (label, leaf.ty.label(), value)
        })
        .collect()
}

// ── Built-in and user layouts ──

const BUILTIN: &[&str] = &[include_str!("../../formats/gshop/source.json"), include_str!("../../formats/gshop/forsakenjd.json"), include_str!("../../formats/gshop/hdn.json"), include_str!("../../formats/gshop/luckybag.json")];

pub fn builtin() -> Vec<Layout> {
    BUILTIN
        .iter()
        .map(|text| {
            let mut layout: Layout = serde_json::from_str(text).expect("built-in gshop layout");
            layout.builtin = true;
            layout
        })
        .collect()
}

pub fn user_folder(config: &std::path::Path) -> std::path::PathBuf {
    config.join("gshop-layouts")
}

/// The user's layouts first (they win over built-ins of the same size), then the built-ins.
pub fn all(config: Option<&std::path::Path>) -> Vec<Layout> {
    let mut out: Vec<Layout> = Vec::new();
    if let Some(config) = config {
        if let Ok(entries) = std::fs::read_dir(user_folder(config)) {
            let mut files: Vec<_> = entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")).collect();
            files.sort();
            for path in files {
                if let Some(layout) = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Layout>(&text).ok()).filter(|layout| layout.validate().is_ok()) {
                    out.push(layout);
                }
            }
        }
    }
    let builtin: Vec<Layout> = builtin().into_iter().filter(|layout| !out.iter().any(|user| user.id == layout.id)).collect();
    out.extend(builtin);
    out
}

pub fn save(config: &std::path::Path, layout: &Layout) -> Result<std::path::PathBuf, String> {
    layout.validate()?;
    if builtin().iter().any(|known| known.id == layout.id) {
        return Err(format!("{} is a built-in layout; save it under another ID", layout.id));
    }
    let folder = user_folder(config);
    std::fs::create_dir_all(&folder).map_err(|error| format!("Could not create {}: {error}", folder.display()))?;
    let path = folder.join(format!("{}.json", layout.id));
    let text = serde_json::to_string_pretty(&Layout { builtin: false, ..layout.clone() }).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    Ok(path)
}

pub fn delete(config: &std::path::Path, id: &str) -> Result<(), String> {
    let path = user_folder(config).join(format!("{id}.json"));
    if !path.is_file() {
        return Err(format!("{id} is not a user layout"));
    }
    std::fs::remove_file(&path).map_err(|error| format!("Could not delete {}: {error}", path.display()))
}
