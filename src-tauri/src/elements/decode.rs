//! Turns record bytes into a tree of display nodes using a profile layout.

use encoding_rs::GBK;
use serde::Serialize;

use super::format::{Field, Ty};

/// Arrays of plain values are previewed inline up to this many elements.
const PREVIEW_ITEMS: usize = 8;
/// Unknown byte ranges are broken into int32 rows up to this size.
const MAX_GAP_ROWS: usize = 1024;

#[derive(Debug, Serialize)]
pub struct Node {
    pub name: String,
    /// Offset within the record.
    pub off: usize,
    pub size: usize,
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Enum label, referenced record or float reading of the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// The record this value refers to, as (list, row).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<(usize, usize)>,
    /// Display role of the value ("path", "icon", "skill").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Node>>,
    /// Bytes not described by the layout.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unknown: bool,
}

pub fn read_wstr(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn read_str(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    GBK.decode(&bytes[..end]).0.into_owned()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// An int32 bit pattern that reads as an ordinary float (e.g. 1065353216 = 1.0).
fn float_hint(bits: u32) -> Option<String> {
    let f = f32::from_bits(bits);
    let plausible = f.is_normal() && f.abs() > 1e-4 && f.abs() < 1e7 && (bits & 0x7fff_ffff) > 0x0100_0000;
    plausible.then(|| format!("as float: {f}"))
}

fn fixed<const N: usize>(bytes: &[u8], off: usize) -> [u8; N] {
    bytes[off..off + N].try_into().unwrap()
}

/// Scalar value as display text plus an integer form for enum lookup.
fn scalar(ty: &Ty, b: &[u8], off: usize) -> Option<(String, Option<i64>)> {
    Some(match ty {
        Ty::I8 => {
            let v = b[off] as i8;
            (v.to_string(), Some(v.into()))
        }
        Ty::U8 => (b[off].to_string(), Some(b[off].into())),
        Ty::Bool => ((b[off] != 0).to_string(), Some(b[off].into())),
        Ty::I16 => {
            let v = i16::from_le_bytes(fixed(b, off));
            (v.to_string(), Some(v.into()))
        }
        Ty::U16 => {
            let v = u16::from_le_bytes(fixed(b, off));
            (v.to_string(), Some(v.into()))
        }
        Ty::I32 => {
            let v = i32::from_le_bytes(fixed(b, off));
            (v.to_string(), Some(v.into()))
        }
        Ty::U32 => {
            let v = u32::from_le_bytes(fixed(b, off));
            (v.to_string(), Some(v.into()))
        }
        Ty::F32 => (f32::from_le_bytes(fixed(b, off)).to_string(), None),
        Ty::F64 => (f64::from_le_bytes(fixed(b, off)).to_string(), None),
        Ty::I64 => {
            let v = i64::from_le_bytes(fixed(b, off));
            (v.to_string(), Some(v))
        }
        Ty::U64 => {
            let v = u64::from_le_bytes(fixed(b, off));
            (v.to_string(), Some(v as i64))
        }
        Ty::Wstr { n } => (read_wstr(&b[off..off + n * 2]), None),
        Ty::Str { n } => (read_str(&b[off..off + n]), None),
        Ty::Bytes { n } => (hex(&b[off..off + n]), None),
        Ty::Array { .. } | Ty::Struct { .. } => return None,
    })
}

/// Extra meaning for an integer field value, supplied by the document.
#[derive(Default)]
pub struct Annotation {
    pub hint: Option<String>,
    pub link: Option<(usize, usize)>,
}

pub type Annotator<'a> = dyn Fn(&Field, i64) -> Annotation + 'a;

struct Ctx<'a> {
    bytes: &'a [u8],
    annotate: &'a Annotator<'a>,
}

impl Ctx<'_> {
    fn node(&self, name: String, ty: &Ty, off: usize, field: Option<&Field>) -> Node {
        let size = ty.size();
        let mut node = Node {
            name,
            off,
            size,
            ty: ty.label(),
            value: None,
            hint: None,
            link: None,
            display: field.and_then(|f| f.display.clone()),
            comment: field.and_then(|f| f.c.clone()),
            children: None,
            unknown: false,
        };
        if off + size > self.bytes.len() {
            node.value = Some("(past end of record)".into());
            return node;
        }
        match ty {
            Ty::Struct { fields } => {
                node.children = Some(self.fields(fields, off));
            }
            Ty::Array { n, stride, t } => {
                let children: Vec<Node> = (0..*n)
                    .map(|i| self.node(format!("[{i}]"), t, off + i * stride, field))
                    .collect();
                if !matches!(**t, Ty::Struct { .. } | Ty::Array { .. }) {
                    let mut preview: Vec<&str> =
                        children.iter().take(PREVIEW_ITEMS).filter_map(|c| c.value.as_deref()).collect();
                    if *n > PREVIEW_ITEMS {
                        preview.push("…");
                    }
                    node.value = Some(format!("[{}]", preview.join(", ")));
                }
                node.children = Some(children);
            }
            _ => {
                let (text, int) = scalar(ty, self.bytes, off).expect("scalar type");
                if let (Some(f), Some(v)) = (field, int) {
                    let a = (self.annotate)(f, v);
                    node.hint = a.hint;
                    node.link = a.link;
                }
                if node.hint.is_none() && matches!(ty, Ty::I32 | Ty::U32) {
                    node.hint = float_hint(u32::from_le_bytes(fixed(self.bytes, off)));
                }
                node.value = Some(text);
            }
        }
        node
    }

    fn fields(&self, fields: &[Field], base: usize) -> Vec<Node> {
        fields.iter().map(|f| self.node(f.name.clone(), &f.t, base + f.off, Some(f))).collect()
    }
}

/// A byte range the layout does not describe, shown as int32 rows when aligned.
pub fn gap_node(bytes: &[u8], off: usize, size: usize) -> Node {
    let chunk = &bytes[off..off + size];
    let children = (size % 4 == 0 && off % 4 == 0 && size <= MAX_GAP_ROWS * 4).then(|| {
        chunk
            .chunks_exact(4)
            .enumerate()
            .map(|(i, c)| {
                let v = i32::from_le_bytes(c.try_into().unwrap());
                Node {
                    name: format!("+0x{:x}", off + i * 4),
                    off: off + i * 4,
                    size: 4,
                    ty: "int32?".into(),
                    value: Some(v.to_string()),
                    hint: float_hint(v as u32),
                    link: None,
                    display: None,
                    comment: None,
                    children: None,
                    unknown: true,
                }
            })
            .collect()
    });
    let preview = hex(&chunk[..size.min(16)]) + if size > 16 { " …" } else { "" };
    Node {
        name: format!("unknown @0x{off:x}"),
        off,
        size,
        ty: format!("byte[{size}]"),
        value: Some(preview),
        hint: None,
        link: None,
        display: None,
        comment: None,
        children,
        unknown: true,
    }
}

/// Decodes a record with `fields`, filling any uncovered byte ranges (gaps
/// between fields or a tail beyond the layout) with unknown nodes.
pub fn decode_record(bytes: &[u8], fields: &[Field], annotate: &Annotator) -> Vec<Node> {
    let ctx = Ctx { bytes, annotate };
    let mut sorted: Vec<&Field> = fields.iter().filter(|f| f.off < bytes.len()).collect();
    sorted.sort_by_key(|f| f.off);

    let mut nodes = Vec::new();
    let mut cursor = 0;
    for f in sorted {
        if f.off > cursor {
            nodes.push(gap_node(bytes, cursor, f.off - cursor));
        }
        nodes.push(ctx.node(f.name.clone(), &f.t, f.off, Some(f)));
        cursor = cursor.max(f.off + f.t.size());
    }
    if cursor < bytes.len() {
        nodes.push(gap_node(bytes, cursor, bytes.len() - cursor));
    }
    nodes
}

/// Finds a likely display name in a record without a layout: a UTF-16 string
/// at one of the offsets the game's structs typically use.
pub fn guess_name(bytes: &[u8]) -> Option<String> {
    [4usize, 12, 8].into_iter().find_map(|off| {
        let raw = bytes.get(off..off + 64)?;
        let text = read_wstr(raw);
        let plausible = !text.is_empty()
            && text.chars().all(|c| !c.is_control() && c != '\u{fffd}')
            && raw[text.encode_utf16().count() * 2..].iter().all(|&b| b == 0);
        plausible.then_some(text)
    })
}
