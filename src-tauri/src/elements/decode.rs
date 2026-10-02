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
    /// A display group of consecutive fields (see `Field::g`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub group: bool,
    /// For fields with conditional types: why this type was chosen,
    /// e.g. "type = 7 → float".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cond: Option<String>,
    /// Path ID of the item icon this value points at (client icons).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
    /// Key of the enum or mask naming this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
    /// The NPC dialog this value refers to (index into the dialogs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub talk: Option<usize>,
}

fn group_node(name: String, members: Vec<Node>) -> Node {
    let off = members.iter().map(|n| n.off).min().unwrap_or(0);
    let end = members.iter().map(|n| n.off + n.size).max().unwrap_or(off);
    let fields = members.iter().filter(|n| !n.unknown).count();
    Node {
        name,
        off,
        size: end - off,
        ty: "group".into(),
        value: Some(format!("{fields} field{}", if fields == 1 { "" } else { "s" })),
        hint: None,
        link: None,
        display: None,
        comment: None,
        children: Some(members),
        unknown: false,
        group: true,
        cond: None,
        icon: None,
        set: None,
        talk: None,
    }
}

/// Wraps runs of consecutive nodes tagged with the same group into one node.
fn grouped(items: Vec<(Option<String>, Node)>) -> Vec<Node> {
    let mut out = Vec::new();
    let mut run: Option<(String, Vec<Node>)> = None;
    for (g, node) in items {
        if let (Some((name, members)), Some(g)) = (run.as_mut(), g.as_deref()) {
            if name == g {
                members.push(node);
                continue;
            }
        }
        if let Some((name, members)) = run.take() {
            out.push(group_node(name, members));
        }
        match g {
            Some(g) => run = Some((g, vec![node])),
            None => out.push(node),
        }
    }
    if let Some((name, members)) = run {
        out.push(group_node(name, members));
    }
    out
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
    pub icon: Option<u32>,
    pub set: Option<String>,
    pub talk: Option<usize>,
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
            group: false,
            cond: None,
            icon: None,
            set: None,
            talk: None,
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
                    node.icon = a.icon;
                    node.set = a.set;
                    node.talk = a.talk;
                }
                // Dates, durations and times of day are shown by the UI, not as floats.
                let timed = matches!(node.display.as_deref(), Some("time" | "duration" | "duration_ms" | "daytime"));
                if node.hint.is_none() && matches!(ty, Ty::I32 | Ty::U32) && !timed {
                    node.hint = float_hint(u32::from_le_bytes(fixed(self.bytes, off)));
                }
                node.value = Some(text);
            }
        }
        node
    }

    fn fields(&self, fields: &[Field], base: usize) -> Vec<Node> {
        grouped(fields.iter().map(|f| (f.g.clone(), self.typed(f, fields, base))).collect())
    }

    /// Decodes a field, applying its conditional type rules: the first rule
    /// whose sibling field holds a listed value picks the type.
    fn typed(&self, f: &Field, siblings: &[Field], base: usize) -> Node {
        if f.when.is_empty() {
            return self.node(f.name.clone(), &f.t, base + f.off, Some(f));
        }
        let value_of = |name: &str| {
            let s = siblings.iter().find(|s| s.name == name)?;
            let at = base + s.off;
            if at + s.t.size() > self.bytes.len() {
                return None;
            }
            scalar(&s.t, self.bytes, at)?.1
        };
        let mut chosen = &f.t;
        let mut why = None;
        for rule in &f.when {
            let Some(v) = value_of(&rule.field) else { continue };
            if rule.matches(v) {
                chosen = &rule.t;
                why = Some(format!("{} = {v} → {}", rule.field, rule.t.label()));
                break;
            }
            why.get_or_insert_with(|| format!("{} = {v} → {}", rule.field, f.t.label()));
        }
        let mut node = self.node(f.name.clone(), chosen, base + f.off, Some(f));
        node.cond = why;
        node
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
                    group: false,
                    cond: None,
                    icon: None,
                    set: None,
                    talk: None,
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
        group: false,
        cond: None,
        icon: None,
        set: None,
        talk: None,
    }
}

/// Decodes a record with `fields`, filling any uncovered byte ranges (gaps
/// between fields or a tail beyond the layout) with unknown nodes.
pub fn decode_record(bytes: &[u8], fields: &[Field], annotate: &Annotator) -> Vec<Node> {
    let ctx = Ctx { bytes, annotate };
    let mut sorted: Vec<&Field> = fields.iter().filter(|f| f.off < bytes.len()).collect();
    sorted.sort_by_key(|f| f.off);

    let mut nodes: Vec<(Option<String>, Node)> = Vec::new();
    let mut cursor = 0;
    let mut previous_group: Option<&str> = None;
    for f in sorted {
        if f.off > cursor {
            // A gap between two fields of one group stays inside the group.
            let g = previous_group.filter(|&p| f.g.as_deref() == Some(p)).map(str::to_string);
            nodes.push((g, gap_node(bytes, cursor, f.off - cursor)));
        }
        nodes.push((f.g.clone(), ctx.typed(f, fields, 0)));
        cursor = cursor.max(f.off + f.t.size());
        previous_group = f.g.as_deref();
    }
    if cursor < bytes.len() {
        nodes.push((None, gap_node(bytes, cursor, bytes.len() - cursor)));
    }
    grouped(nodes)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, off: usize, g: Option<&str>) -> Field {
        Field { name: name.into(), off, t: Ty::I32, c: None, e: None, display: None, refs: vec![], g: g.map(Into::into), when: vec![] }
    }

    #[test]
    fn consecutive_fields_of_a_group_share_one_node() {
        let fields = [
            field("id", 0, None),
            field("addon1", 4, Some("Addons")),
            field("addon2", 8, Some("Addons")),
            // a gap at 12..16 between two members stays in the group
            field("addon3", 16, Some("Addons")),
            field("price", 20, None),
        ];
        let bytes = [1u8; 28];
        let nodes = decode_record(&bytes, &fields, &|_, _| Annotation::default());
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["id", "Addons", "price", "unknown @0x18"]);
        let group = &nodes[1];
        assert!(group.group);
        assert_eq!((group.off, group.size), (4, 16));
        let members: Vec<&str> = group.children.as_ref().unwrap().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(members, ["addon1", "addon2", "unknown @0xc", "addon3"]);
        assert_eq!(group.value.as_deref(), Some("3 fields"));
    }

    #[test]
    fn type_rules_pick_the_type_per_record() {
        use crate::elements::format::TypeRule;
        let mut param = field("param", 4, None);
        param.when = vec![TypeRule { field: "type".into(), values: vec![7, 8], not: false, t: Ty::F32 }];
        let fields = [field("type", 0, None), param];
        let record = |ty: i32, bits: u32| [ty.to_le_bytes(), bits.to_le_bytes()].concat();
        let none = |_: &Field, _: i64| Annotation::default();

        let as_float = decode_record(&record(7, 1.5f32.to_bits()), &fields, &none);
        assert_eq!(as_float[1].ty, "float");
        assert_eq!(as_float[1].value.as_deref(), Some("1.5"));
        assert_eq!(as_float[1].cond.as_deref(), Some("type = 7 → float"));

        let as_int = decode_record(&record(3, 42), &fields, &none);
        assert_eq!(as_int[1].ty, "int32");
        assert_eq!(as_int[1].value.as_deref(), Some("42"));
        assert_eq!(as_int[1].cond.as_deref(), Some("type = 3 → int32"));
    }
}
