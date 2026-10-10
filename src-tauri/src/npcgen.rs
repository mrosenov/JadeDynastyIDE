//! `npcgen.data`: one server map's spawns (`CNPCGenMan::Load`, zgame/gs/template/npcgendata.cpp).
//!
//! ```text
//! u32 version (4–14 in real files; AIGENFILE_VERSION 14)
//! header   i32 areas, i32 resource areas, [i32 dynamic objects: v6+], [i32 controllers: v7+]
//! areas    NPCGENFILEAREA(7/12/14) then its generators (60 bytes each) and, v12+, attached export IDs
//! resource NPCGENFILERESAREA(6/7/12/14) then its resources (20 bytes each) and attached export IDs
//! dynamic  NPCGENFILEDYNOBJ(9/10/14)
//! control  NPCGENFILECTRL(8/11), v13+: i32 segment count (low 16 bits) + 2 × 24-byte times per segment
//! ```
//!
//! Structures are packed and `size_t` is 4 bytes (32-bit tools and server). Only the server reads
//! these files: one per map folder (gs.conf `NPCGenFile`). The client's copy is not loaded.
//! Writing keeps the file's version; values a version does not store must stay at their defaults.

use std::path::{Path, PathBuf};

use chrono::Local;
use encoding_rs::GBK;
use serde::{Deserialize, Serialize};

pub const MAX_VERSION: u32 = 14;
const NAME_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Generator {
    /// NPC or monster template.
    pub id: u32,
    pub count: u32,
    /// Respawn time (seconds).
    pub refresh: u32,
    pub died_times: u32,
    pub aggressive: u32,
    pub offset_water: f32,
    pub offset_terrain: f32,
    pub faction: u32,
    pub faction_helper: u32,
    pub faction_accept: u32,
    pub need_help: u8,
    pub default_faction: u8,
    pub default_faction_helper: u8,
    pub default_faction_accept: u8,
    pub path_id: i32,
    pub loop_type: i32,
    pub speed_flag: i32,
    pub dead_time: i32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Area {
    /// 0 on the terrain, 1 a box.
    pub kind: i32,
    pub position: Vec3,
    pub direction: Vec3,
    pub extents: Vec3,
    /// 0 monster, 1 NPC, 2 interaction object.
    pub npc_type: i32,
    pub group_type: i32,
    pub init_gen: u8,
    /// v14: revive type (0–2); before: auto revive (0/1).
    pub revive: u8,
    pub valid_once: u8,
    pub gen_id: u32,
    /// v7+.
    pub controller: i32,
    pub life_time: i32,
    pub max_count: i32,
    /// v12+.
    pub export_id: i32,
    /// v12+: −1 when this area is itself attached to another; otherwise the attached areas' export IDs.
    pub attach_num: i32,
    pub attached: Vec<i32>,
    /// v14.
    pub phase: i32,
    pub generators: Vec<Generator>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub kind: i32,
    /// Mine template.
    pub template: i32,
    pub refresh: u32,
    pub count: u32,
    pub height_offset: f32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceArea {
    pub position: Vec3,
    pub extent_x: f32,
    pub extent_z: f32,
    pub init_gen: u8,
    pub auto_revive: u8,
    pub valid_once: u8,
    pub gen_id: u32,
    /// v6+.
    pub direction: [u8; 2],
    pub radius: u8,
    /// v7+.
    pub controller: i32,
    pub max_count: i32,
    /// v12+.
    pub export_id: i32,
    pub attach_num: i32,
    pub attached: Vec<i32>,
    /// v14.
    pub phase: i32,
    pub resources: Vec<Resource>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicObject {
    pub id: u32,
    pub position: Vec3,
    pub direction: [u8; 2],
    pub radius: u8,
    /// v9+ (16 = 1.0).
    pub scale: u8,
    /// v10+.
    pub controller: u32,
    /// v14.
    pub phase: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CtrlTime {
    pub year: i32,
    pub month: i32,
    pub week: i32,
    pub day: i32,
    pub hours: i32,
    pub minutes: i32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Controller {
    pub id: u32,
    pub controller_id: i32,
    /// GBK; `name_raw` keeps the stored 128 bytes for an unchanged name.
    pub name: String,
    #[serde(default)]
    pub name_raw: Vec<u8>,
    pub active: u8,
    pub wait_time: i32,
    pub stop_time: i32,
    pub active_time_invalid: u8,
    pub stop_time_invalid: u8,
    pub active_time: CtrlTime,
    pub stop_time_at: CtrlTime,
    /// v8+.
    pub active_time_range: i32,
    /// v11+.
    pub repeat: u8,
    /// v13+: the high bits of the stored segment count (segment logic).
    pub segment_logic: i32,
    pub segments: Vec<(CtrlTime, CtrlTime)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NpcGen {
    pub version: u32,
    pub areas: Vec<Area>,
    pub resource_areas: Vec<ResourceArea>,
    pub dynamic_objects: Vec<DynamicObject>,
    pub controllers: Vec<Controller>,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, len: usize, what: &str) -> Result<&[u8], String> {
        let end = self.at.checked_add(len).filter(|end| *end <= self.data.len()).ok_or_else(|| format!("{what} needs {len} bytes at offset {} but the file ends at {}", self.at, self.data.len()))?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }
    fn u8(&mut self, what: &str) -> Result<u8, String> { Ok(self.take(1, what)?[0]) }
    fn u32(&mut self, what: &str) -> Result<u32, String> { Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap())) }
    fn i32(&mut self, what: &str) -> Result<i32, String> { Ok(i32::from_le_bytes(self.take(4, what)?.try_into().unwrap())) }
    fn f32(&mut self, what: &str) -> Result<f32, String> { Ok(f32::from_le_bytes(self.take(4, what)?.try_into().unwrap())) }
    fn vec3(&mut self, what: &str) -> Result<Vec3, String> { Ok(Vec3 { x: self.f32(what)?, y: self.f32(what)?, z: self.f32(what)? }) }
    fn count(&mut self, what: &str) -> Result<usize, String> {
        let value = self.i32(what)?;
        usize::try_from(value).ok().filter(|count| *count <= 1_000_000).ok_or_else(|| format!("{what}: invalid count {value}"))
    }
    fn time(&mut self, what: &str) -> Result<CtrlTime, String> {
        Ok(CtrlTime { year: self.i32(what)?, month: self.i32(what)?, week: self.i32(what)?, day: self.i32(what)?, hours: self.i32(what)?, minutes: self.i32(what)? })
    }
    fn attached(&mut self, attach_num: i32, what: &str) -> Result<Vec<i32>, String> {
        if attach_num <= 0 { return Ok(Vec::new()); }
        if attach_num > 100_000 { return Err(format!("{what}: invalid attached area count {attach_num}")); }
        (0..attach_num).map(|_| self.i32(what)).collect()
    }
}

fn decode_name(raw: &[u8]) -> String {
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    GBK.decode_without_bom_handling(&raw[..end]).0.into_owned()
}

pub fn parse(data: &[u8]) -> Result<NpcGen, String> {
    let mut reader = Reader { data, at: 0 };
    let version = reader.u32("the version")?;
    if !(1..=MAX_VERSION).contains(&version) {
        return Err(format!("npcgen.data version {version} is not supported (the server reads up to {MAX_VERSION})"));
    }
    let areas = reader.count("the area count")?;
    let resource_areas = reader.count("the resource area count")?;
    let dynamic_objects = if version >= 6 { reader.count("the dynamic object count")? } else { 0 };
    let controllers = if version >= 7 { reader.count("the controller count")? } else { 0 };
    let mut out = NpcGen { version, ..NpcGen::default() };
    for index in 0..areas {
        let what = format!("area {}", index + 1);
        let what = what.as_str();
        let mut area = Area { kind: reader.i32(what)?, ..Area::default() };
        let generators = reader.count(what)?;
        area.position = reader.vec3(what)?;
        area.direction = reader.vec3(what)?;
        area.extents = reader.vec3(what)?;
        area.npc_type = reader.i32(what)?;
        area.group_type = reader.i32(what)?;
        area.init_gen = reader.u8(what)?;
        area.revive = reader.u8(what)?;
        area.valid_once = reader.u8(what)?;
        area.gen_id = reader.u32(what)?;
        if version >= 7 {
            area.controller = reader.i32(what)?;
            area.life_time = reader.i32(what)?;
            area.max_count = reader.i32(what)?;
        }
        if version >= 12 {
            area.export_id = reader.i32(what)?;
            area.attach_num = reader.i32(what)?;
        }
        if version >= 14 {
            area.phase = reader.i32(what)?;
        }
        for _ in 0..generators {
            area.generators.push(Generator {
                id: reader.u32(what)?,
                count: reader.u32(what)?,
                refresh: reader.u32(what)?,
                died_times: reader.u32(what)?,
                aggressive: reader.u32(what)?,
                offset_water: reader.f32(what)?,
                offset_terrain: reader.f32(what)?,
                faction: reader.u32(what)?,
                faction_helper: reader.u32(what)?,
                faction_accept: reader.u32(what)?,
                need_help: reader.u8(what)?,
                default_faction: reader.u8(what)?,
                default_faction_helper: reader.u8(what)?,
                default_faction_accept: reader.u8(what)?,
                path_id: reader.i32(what)?,
                loop_type: reader.i32(what)?,
                speed_flag: reader.i32(what)?,
                dead_time: reader.i32(what)?,
            });
        }
        area.attached = reader.attached(area.attach_num, what)?;
        out.areas.push(area);
    }
    for index in 0..resource_areas {
        let what = format!("resource area {}", index + 1);
        let what = what.as_str();
        let mut area = ResourceArea { position: reader.vec3(what)?, extent_x: reader.f32(what)?, extent_z: reader.f32(what)?, ..ResourceArea::default() };
        let resources = reader.count(what)?;
        area.init_gen = reader.u8(what)?;
        area.auto_revive = reader.u8(what)?;
        area.valid_once = reader.u8(what)?;
        area.gen_id = reader.u32(what)?;
        if version >= 6 {
            area.direction = [reader.u8(what)?, reader.u8(what)?];
            area.radius = reader.u8(what)?;
        }
        if version >= 7 {
            area.controller = reader.i32(what)?;
            area.max_count = reader.i32(what)?;
        }
        if version >= 12 {
            area.export_id = reader.i32(what)?;
            area.attach_num = reader.i32(what)?;
        }
        if version >= 14 {
            area.phase = reader.i32(what)?;
        }
        for _ in 0..resources {
            area.resources.push(Resource { kind: reader.i32(what)?, template: reader.i32(what)?, refresh: reader.u32(what)?, count: reader.u32(what)?, height_offset: reader.f32(what)? });
        }
        area.attached = reader.attached(area.attach_num, what)?;
        out.resource_areas.push(area);
    }
    for index in 0..dynamic_objects {
        let what = format!("dynamic object {}", index + 1);
        let what = what.as_str();
        let mut object = DynamicObject { id: reader.u32(what)?, position: reader.vec3(what)?, direction: [reader.u8(what)?, reader.u8(what)?], radius: reader.u8(what)?, ..DynamicObject::default() };
        if version >= 9 { object.scale = reader.u8(what)?; }
        if version >= 10 { object.controller = reader.u32(what)?; }
        if version >= 14 { object.phase = reader.i32(what)?; }
        out.dynamic_objects.push(object);
    }
    for index in 0..controllers {
        let what = format!("controller {}", index + 1);
        let what = what.as_str();
        let id = reader.u32(what)?;
        let controller_id = reader.i32(what)?;
        let name_raw = reader.take(NAME_BYTES, what)?.to_vec();
        let mut controller = Controller {
            id,
            controller_id,
            name: decode_name(&name_raw),
            name_raw,
            active: reader.u8(what)?,
            wait_time: reader.i32(what)?,
            stop_time: reader.i32(what)?,
            active_time_invalid: reader.u8(what)?,
            stop_time_invalid: reader.u8(what)?,
            active_time: reader.time(what)?,
            stop_time_at: reader.time(what)?,
            ..Controller::default()
        };
        if version >= 8 { controller.active_time_range = reader.i32(what)?; }
        if version >= 11 { controller.repeat = reader.u8(what)?; }
        if version >= 13 {
            let stored = reader.i32(what)?;
            controller.segment_logic = stored & !0xffff;
            for _ in 0..(stored & 0xffff) {
                controller.segments.push((reader.time(what)?, reader.time(what)?));
            }
        }
        out.controllers.push(controller);
    }
    if reader.at != data.len() {
        return Err(format!("{} bytes are left after the last controller", data.len() - reader.at));
    }
    Ok(out)
}

/// The 128 stored name bytes: the original ones while the name is unchanged.
fn name_bytes(controller: &Controller) -> Result<Vec<u8>, String> {
    if controller.name_raw.len() == NAME_BYTES && decode_name(&controller.name_raw) == controller.name {
        return Ok(controller.name_raw.clone());
    }
    let (bytes, _, unmappable) = GBK.encode(&controller.name);
    if unmappable {
        return Err(format!("Controller {}: the name has characters GBK cannot store", controller.id));
    }
    if bytes.len() >= NAME_BYTES {
        return Err(format!("Controller {}: the name takes {} GBK bytes; at most {} fit", controller.id, bytes.len(), NAME_BYTES - 1));
    }
    let mut out = bytes.into_owned();
    out.resize(NAME_BYTES, 0);
    Ok(out)
}

pub fn encode(file: &NpcGen) -> Result<Vec<u8>, String> {
    let version = file.version;
    let mut out = Vec::new();
    let put_u32 = |out: &mut Vec<u8>, value: u32| out.extend_from_slice(&value.to_le_bytes());
    let put_i32 = |out: &mut Vec<u8>, value: i32| out.extend_from_slice(&value.to_le_bytes());
    let put_f32 = |out: &mut Vec<u8>, value: f32| out.extend_from_slice(&value.to_le_bytes());
    let put_vec3 = |out: &mut Vec<u8>, value: &Vec3| { for part in [value.x, value.y, value.z] { out.extend_from_slice(&part.to_le_bytes()); } };
    let put_time = |out: &mut Vec<u8>, time: &CtrlTime| { for part in [time.year, time.month, time.week, time.day, time.hours, time.minutes] { out.extend_from_slice(&part.to_le_bytes()); } };
    // A value the file's version does not store must be its default.
    let unstored = |present: bool, value_set: bool, what: String| -> Result<(), String> {
        if !present && value_set { Err(format!("{what} is not stored in npcgen.data version {version}")) } else { Ok(()) }
    };
    let attach_count = |attach_num: i32, attached: &[i32], what: &str| -> Result<i32, String> {
        if attach_num < 0 {
            if !attached.is_empty() { return Err(format!("{what} is itself attached (−1), so it cannot list attached areas")); }
            Ok(attach_num)
        } else {
            Ok(attached.len() as i32)
        }
    };
    put_u32(&mut out, version);
    put_i32(&mut out, file.areas.len() as i32);
    put_i32(&mut out, file.resource_areas.len() as i32);
    unstored(version >= 6, !file.dynamic_objects.is_empty(), "Dynamic objects".into())?;
    if version >= 6 { put_i32(&mut out, file.dynamic_objects.len() as i32); }
    unstored(version >= 7, !file.controllers.is_empty(), "Controllers".into())?;
    if version >= 7 { put_i32(&mut out, file.controllers.len() as i32); }
    for (index, area) in file.areas.iter().enumerate() {
        let what = format!("Area {}", index + 1);
        unstored(version >= 7, area.controller != 0 || area.life_time != 0 || area.max_count != 0, format!("{what}: controller, life time and max count"))?;
        unstored(version >= 12, area.export_id != 0 || area.attach_num != 0 || !area.attached.is_empty(), format!("{what}: export ID and attached areas"))?;
        unstored(version >= 14, area.phase != 0, format!("{what}: phase"))?;
        put_i32(&mut out, area.kind);
        put_i32(&mut out, area.generators.len() as i32);
        put_vec3(&mut out, &area.position);
        put_vec3(&mut out, &area.direction);
        put_vec3(&mut out, &area.extents);
        put_i32(&mut out, area.npc_type);
        put_i32(&mut out, area.group_type);
        out.extend_from_slice(&[area.init_gen, area.revive, area.valid_once]);
        put_u32(&mut out, area.gen_id);
        if version >= 7 {
            put_i32(&mut out, area.controller);
            put_i32(&mut out, area.life_time);
            put_i32(&mut out, area.max_count);
        }
        let attach = attach_count(area.attach_num, &area.attached, &what)?;
        if version >= 12 {
            put_i32(&mut out, area.export_id);
            put_i32(&mut out, attach);
        }
        if version >= 14 { put_i32(&mut out, area.phase); }
        for generator in &area.generators {
            for value in [generator.id, generator.count, generator.refresh, generator.died_times, generator.aggressive] { put_u32(&mut out, value); }
            put_f32(&mut out, generator.offset_water);
            put_f32(&mut out, generator.offset_terrain);
            for value in [generator.faction, generator.faction_helper, generator.faction_accept] { put_u32(&mut out, value); }
            out.extend_from_slice(&[generator.need_help, generator.default_faction, generator.default_faction_helper, generator.default_faction_accept]);
            for value in [generator.path_id, generator.loop_type, generator.speed_flag, generator.dead_time] { put_i32(&mut out, value); }
        }
        if version >= 12 { for id in &area.attached { put_i32(&mut out, *id); } }
    }
    for (index, area) in file.resource_areas.iter().enumerate() {
        let what = format!("Resource area {}", index + 1);
        unstored(version >= 6, area.direction != [0, 0] || area.radius != 0, format!("{what}: direction"))?;
        unstored(version >= 7, area.controller != 0 || area.max_count != 0, format!("{what}: controller and max count"))?;
        unstored(version >= 12, area.export_id != 0 || area.attach_num != 0 || !area.attached.is_empty(), format!("{what}: export ID and attached areas"))?;
        unstored(version >= 14, area.phase != 0, format!("{what}: phase"))?;
        put_vec3(&mut out, &area.position);
        put_f32(&mut out, area.extent_x);
        put_f32(&mut out, area.extent_z);
        put_i32(&mut out, area.resources.len() as i32);
        out.extend_from_slice(&[area.init_gen, area.auto_revive, area.valid_once]);
        put_u32(&mut out, area.gen_id);
        if version >= 6 { out.extend_from_slice(&[area.direction[0], area.direction[1], area.radius]); }
        if version >= 7 {
            put_i32(&mut out, area.controller);
            put_i32(&mut out, area.max_count);
        }
        let attach = attach_count(area.attach_num, &area.attached, &what)?;
        if version >= 12 {
            put_i32(&mut out, area.export_id);
            put_i32(&mut out, attach);
        }
        if version >= 14 { put_i32(&mut out, area.phase); }
        for resource in &area.resources {
            put_i32(&mut out, resource.kind);
            put_i32(&mut out, resource.template);
            put_u32(&mut out, resource.refresh);
            put_u32(&mut out, resource.count);
            put_f32(&mut out, resource.height_offset);
        }
        if version >= 12 { for id in &area.attached { put_i32(&mut out, *id); } }
    }
    for (index, object) in file.dynamic_objects.iter().enumerate() {
        let what = format!("Dynamic object {}", index + 1);
        unstored(version >= 9, object.scale != 0, format!("{what}: scale"))?;
        unstored(version >= 10, object.controller != 0, format!("{what}: controller"))?;
        unstored(version >= 14, object.phase != 0, format!("{what}: phase"))?;
        put_u32(&mut out, object.id);
        put_vec3(&mut out, &object.position);
        out.extend_from_slice(&[object.direction[0], object.direction[1], object.radius]);
        if version >= 9 { out.push(object.scale); }
        if version >= 10 { put_u32(&mut out, object.controller); }
        if version >= 14 { put_i32(&mut out, object.phase); }
    }
    for (index, controller) in file.controllers.iter().enumerate() {
        let what = format!("Controller {}", index + 1);
        unstored(version >= 8, controller.active_time_range != 0, format!("{what}: active time range"))?;
        unstored(version >= 11, controller.repeat != 0, format!("{what}: repeat"))?;
        unstored(version >= 13, !controller.segments.is_empty() || controller.segment_logic != 0, format!("{what}: time segments"))?;
        if controller.segments.len() > 0xffff {
            return Err(format!("{what}: too many time segments"));
        }
        put_u32(&mut out, controller.id);
        put_i32(&mut out, controller.controller_id);
        out.extend_from_slice(&name_bytes(controller)?);
        out.push(controller.active);
        put_i32(&mut out, controller.wait_time);
        put_i32(&mut out, controller.stop_time);
        out.extend_from_slice(&[controller.active_time_invalid, controller.stop_time_invalid]);
        put_time(&mut out, &controller.active_time);
        put_time(&mut out, &controller.stop_time_at);
        if version >= 8 { put_i32(&mut out, controller.active_time_range); }
        if version >= 11 { out.push(controller.repeat); }
        if version >= 13 {
            put_i32(&mut out, (controller.segment_logic & !0xffff) | controller.segments.len() as i32);
            for (start, end) in &controller.segments {
                put_time(&mut out, start);
                put_time(&mut out, end);
            }
        }
    }
    Ok(out)
}

// ── The open file: items by section, an undo journal and saving ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Areas,
    Resources,
    Objects,
    Controllers,
}

/// One item of a section, as the UI edits it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "section", content = "item", rename_all = "snake_case")]
pub enum Item {
    Areas(Area),
    Resources(ResourceArea),
    Objects(DynamicObject),
    Controllers(Controller),
}

impl Item {
    fn section(&self) -> Section {
        match self {
            Item::Areas(_) => Section::Areas,
            Item::Resources(_) => Section::Resources,
            Item::Objects(_) => Section::Objects,
            Item::Controllers(_) => Section::Controllers,
        }
    }
}

#[derive(Clone)]
struct Change {
    index: usize,
    before: Option<Item>,
    after: Option<Item>,
}

struct JournalEntry {
    id: u64,
    label: String,
    time: i64,
    changes: Vec<Change>,
}

/// What the list and the map plot need of every item.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub index: usize,
    pub x: f32,
    pub z: f32,
    /// Half sizes on the map (0 for points).
    pub ext_x: f32,
    pub ext_z: f32,
    /// Areas: NPC type; resources: 0; objects: 0; controllers: active.
    pub kind: i32,
    /// Spawned templates (areas: NPC/monster IDs; resources: mines; objects: the object ID).
    pub ids: Vec<u32>,
    /// Total spawned count.
    pub count: u32,
    pub controller: i32,
    pub label: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    pub id: u64,
    pub label: String,
    pub time: i64,
    pub undone: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub path: String,
    pub version: u32,
    pub size: usize,
    pub areas: Vec<Summary>,
    pub resources: Vec<Summary>,
    pub objects: Vec<Summary>,
    pub controllers: Vec<Summary>,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub history: Vec<HistoryRow>,
    pub saved_entries: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub size: usize,
    pub backup: Option<String>,
}

pub struct Document {
    pub path: PathBuf,
    file: NpcGen,
    /// Items as opened or last saved, to mark changed ones (by section and position at that time).
    saved: NpcGen,
    done: Vec<JournalEntry>,
    undone: Vec<JournalEntry>,
    next_entry: u64,
    saved_entries: Option<usize>,
    size: usize,
    disk: String,
    backed_up: bool,
}

fn digest(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(data).iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Document {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let data = std::fs::read(&path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        let file = parse(&data)?;
        if encode(&file)? != data {
            return Err("This npcgen.data does not write back byte for byte; it is not supported".into());
        }
        Ok(Document { path, saved: file.clone(), file, done: Vec::new(), undone: Vec::new(), next_entry: 1, saved_entries: Some(0), size: data.len(), disk: digest(&data), backed_up: false })
    }

    pub fn item(&self, section: Section, index: usize) -> Result<Item, String> {
        let missing = || format!("No item {} in this section", index + 1);
        Ok(match section {
            Section::Areas => Item::Areas(self.file.areas.get(index).ok_or_else(missing)?.clone()),
            Section::Resources => Item::Resources(self.file.resource_areas.get(index).ok_or_else(missing)?.clone()),
            Section::Objects => Item::Objects(self.file.dynamic_objects.get(index).ok_or_else(missing)?.clone()),
            Section::Controllers => Item::Controllers(self.file.controllers.get(index).ok_or_else(missing)?.clone()),
        })
    }

    fn put(&mut self, section: Section, index: usize, item: Option<Item>, insert: bool) {
        macro_rules! apply {
            ($list:expr, $variant:ident) => {{
                match item {
                    Some(Item::$variant(value)) if insert => $list.insert(index, value),
                    Some(Item::$variant(value)) => $list[index] = value,
                    None => { $list.remove(index); }
                    Some(_) => unreachable!("item of another section"),
                }
            }};
        }
        match section {
            Section::Areas => apply!(self.file.areas, Areas),
            Section::Resources => apply!(self.file.resource_areas, Resources),
            Section::Objects => apply!(self.file.dynamic_objects, Objects),
            Section::Controllers => apply!(self.file.controllers, Controllers),
        }
    }

    fn apply(&mut self, change: &Change, forward: bool) {
        let (from, to) = if forward { (&change.before, &change.after) } else { (&change.after, &change.before) };
        let section = from.as_ref().or(to.as_ref()).map(Item::section).expect("a change has an item");
        match (from, to) {
            (Some(_), Some(item)) => self.put(section, change.index, Some(item.clone()), false),
            (None, Some(item)) => self.put(section, change.index, Some(item.clone()), true),
            (Some(_), None) => self.put(section, change.index, None, false),
            (None, None) => {}
        }
    }

    fn record(&mut self, label: String, change: Change) -> Result<View, String> {
        self.apply(&change, true);
        // The whole file must still write (version limits, names, attached areas).
        if let Err(error) = encode(&self.file) {
            self.apply(&change, false);
            return Err(error);
        }
        if self.saved_entries.is_some_and(|saved| saved > self.done.len()) {
            self.saved_entries = None;
        }
        self.undone.clear();
        self.done.push(JournalEntry { id: self.next_entry, label, time: Local::now().timestamp(), changes: vec![change] });
        self.next_entry += 1;
        Ok(self.view())
    }

    pub fn set_item(&mut self, index: usize, item: Item, label: &str) -> Result<View, String> {
        let before = self.item(item.section(), index)?;
        if before == item {
            return Ok(self.view());
        }
        self.record(label.to_string(), Change { index, before: Some(before), after: Some(item) })
    }

    /// Copies an item below itself.
    pub fn clone_item(&mut self, section: Section, index: usize) -> Result<(View, usize), String> {
        let item = self.item(section, index)?;
        let label = format!("Clone {} {}", section_name(section), index + 1);
        let view = self.record(label, Change { index: index + 1, before: None, after: Some(item) })?;
        Ok((view, index + 1))
    }

    pub fn delete_item(&mut self, section: Section, index: usize) -> Result<View, String> {
        let item = self.item(section, index)?;
        let label = format!("Delete {} {}", section_name(section), index + 1);
        self.record(label, Change { index, before: Some(item), after: None })
    }

    pub fn undo(&mut self) -> Result<View, String> {
        let entry = self.done.pop().ok_or("Nothing to undo")?;
        for change in entry.changes.iter().rev() { self.apply(change, false); }
        self.undone.push(entry);
        Ok(self.view())
    }

    pub fn redo(&mut self) -> Result<View, String> {
        let entry = self.undone.pop().ok_or("Nothing to redo")?;
        for change in &entry.changes { self.apply(change, true); }
        self.done.push(entry);
        Ok(self.view())
    }

    pub fn view(&self) -> View {
        let file = &self.file;
        let saved = &self.saved;
        let areas = file.areas.iter().enumerate().map(|(index, area)| Summary {
            index,
            x: area.position.x,
            z: area.position.z,
            ext_x: area.extents.x,
            ext_z: area.extents.z,
            kind: area.npc_type,
            ids: area.generators.iter().map(|generator| generator.id).collect(),
            count: area.generators.iter().map(|generator| generator.count).sum(),
            controller: area.controller,
            label: String::new(),
            changed: saved.areas.get(index) != Some(area),
        }).collect();
        let resources = file.resource_areas.iter().enumerate().map(|(index, area)| Summary {
            index,
            x: area.position.x,
            z: area.position.z,
            ext_x: area.extent_x,
            ext_z: area.extent_z,
            kind: 0,
            ids: area.resources.iter().map(|resource| resource.template as u32).collect(),
            count: area.resources.iter().map(|resource| resource.count).sum(),
            controller: area.controller,
            label: String::new(),
            changed: saved.resource_areas.get(index) != Some(area),
        }).collect();
        let objects = file.dynamic_objects.iter().enumerate().map(|(index, object)| Summary {
            index,
            x: object.position.x,
            z: object.position.z,
            ext_x: 0.0,
            ext_z: 0.0,
            kind: 0,
            ids: vec![object.id],
            count: 1,
            controller: object.controller as i32,
            label: String::new(),
            changed: saved.dynamic_objects.get(index) != Some(object),
        }).collect();
        let controllers = file.controllers.iter().enumerate().map(|(index, controller)| Summary {
            index,
            x: 0.0,
            z: 0.0,
            ext_x: 0.0,
            ext_z: 0.0,
            kind: i32::from(controller.active),
            ids: vec![controller.id],
            count: 0,
            controller: controller.controller_id,
            label: controller.name.clone(),
            changed: saved.controllers.get(index) != Some(controller),
        }).collect();
        let row = |entry: &JournalEntry, undone: bool| HistoryRow { id: entry.id, label: entry.label.clone(), time: entry.time, undone };
        let mut history: Vec<HistoryRow> = self.done.iter().map(|entry| row(entry, false)).collect();
        history.extend(self.undone.iter().rev().map(|entry| row(entry, true)));
        View {
            path: self.path.display().to_string(),
            version: file.version,
            size: self.size,
            areas,
            resources,
            objects,
            controllers,
            dirty: self.saved_entries != Some(self.done.len()),
            can_undo: !self.done.is_empty(),
            can_redo: !self.undone.is_empty(),
            history,
            saved_entries: self.saved_entries,
        }
    }

    pub fn save(&mut self, target: Option<&str>, backup: bool, replace_changed: bool) -> Result<SaveReport, String> {
        let target = target.map(PathBuf::from).unwrap_or_else(|| self.path.clone());
        let same = crate::path_data::same_path(&target, &self.path);
        if same && !replace_changed {
            if let Ok(current) = std::fs::read(&target) {
                if digest(&current) != self.disk {
                    return Err(format!("CHANGED_ON_DISK: {} was changed by another program since it was opened", target.display()));
                }
            }
        }
        let data = encode(&self.file)?;
        if parse(&data)? != self.file {
            return Err("The saved file would not read back as this map; nothing was written".into());
        }
        let backup_path = if backup && target.is_file() && (!same || !self.backed_up) {
            let stamp = Local::now().format("%Y%m%d-%H%M%S");
            let path = target.with_file_name(format!("npcgen.data.{stamp}.bak"));
            std::fs::copy(&target, &path).map_err(|error| format!("Could not back up {} to {}: {error}", target.display(), path.display()))?;
            Some(path)
        } else {
            None
        };
        crate::path_data::write_replacing(&target, &data)?;
        if same && backup_path.is_some() { self.backed_up = true; }
        self.path = target.clone();
        self.size = data.len();
        self.disk = digest(&data);
        self.saved = self.file.clone();
        self.saved_entries = Some(self.done.len());
        Ok(SaveReport { path: target.display().to_string(), size: data.len(), backup: backup_path.map(|path| path.display().to_string()) })
    }
}

fn section_name(section: Section) -> &'static str {
    match section {
        Section::Areas => "spawn area",
        Section::Resources => "resource area",
        Section::Objects => "dynamic object",
        Section::Controllers => "controller",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every npcgen.data under the server folders (all versions in use: 4–14).
    fn samples() -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() { walk(&path, out); } else if path.file_name().is_some_and(|name| name.eq_ignore_ascii_case("npcgen.data")) { out.push(path); }
            }
        }
        let mut out = Vec::new();
        for root in ["E:/Game Dev/JD/zxserver/zgame/gs/config", "E:/Game Dev/JD/1559/gamed/config", "E:/Games/ForsakenJD/element/data"] {
            walk(Path::new(root), &mut out);
        }
        out
    }

    #[test]
    fn real_maps_read_and_write_byte_for_byte() {
        let mut versions = std::collections::BTreeSet::new();
        for path in samples() {
            let data = std::fs::read(&path).unwrap();
            let file = parse(&data).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            versions.insert(file.version);
            assert_eq!(encode(&file).unwrap(), data, "{}", path.display());
        }
        if !versions.is_empty() {
            assert!(versions.len() >= 4, "versions seen: {versions:?}");
        }
    }

    #[test]
    fn edits_respect_the_version_and_save_round_trip() {
        let Some(path) = samples().into_iter().find(|path| std::fs::read(path).is_ok_and(|data| data.len() > 4 && u32::from_le_bytes(data[0..4].try_into().unwrap()) == 12)) else { return };
        let copy = std::env::temp_dir().join(format!("jdide-npcgen-{}", std::process::id()));
        std::fs::create_dir_all(&copy).unwrap();
        let target = copy.join("npcgen.data");
        std::fs::copy(&path, &target).unwrap();
        let mut document = Document::open(&target).unwrap();
        let Item::Areas(mut area) = document.item(Section::Areas, 0).unwrap() else { panic!() };
        // Version 12 does not store phases.
        area.phase = 3;
        assert!(document.set_item(0, Item::Areas(area.clone()), "Edit area").is_err());
        area.phase = 0;
        area.max_count += 1;
        if let Some(generator) = area.generators.first_mut() { generator.count += 1; }
        let view = document.set_item(0, Item::Areas(area.clone()), "Edit area").unwrap();
        assert!(view.dirty && view.areas[0].changed);
        let areas = view.areas.len();
        let (view, index) = document.clone_item(Section::Areas, 0).unwrap();
        assert_eq!((view.areas.len(), index), (areas + 1, 1));
        document.delete_item(Section::Areas, 1).unwrap();
        document.undo().unwrap();
        document.redo().unwrap();
        document.save(None, true, false).unwrap();
        let reopened = Document::open(&target).unwrap();
        assert_eq!(reopened.item(Section::Areas, 0).unwrap(), Item::Areas(area));
        assert_eq!(reopened.view().areas.len(), areas);
        let _ = std::fs::remove_dir_all(&copy);
    }
}
