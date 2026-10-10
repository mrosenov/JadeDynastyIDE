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

use std::collections::{HashMap, HashSet};
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
    /// Full sizes on the map (0 for points; the server's area is position ± size / 2).
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

/// Something seen in the running game client, to add as a point spawn, a resource area or an object.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbyImport {
    /// "npc", "monster", "mine" or "dynamic".
    pub kind: String,
    pub template: u32,
    pub position: Vec3,
    /// Spawns: where it faces.
    pub direction: Option<Vec3>,
    /// Mines and objects: axis bytes and turn, as the client received them.
    pub rotation: Option<[u8; 3]>,
    pub phase: Option<i32>,
    /// A group of monsters or mines seen close together: the full size (x, z) of the area they stood
    /// in and how many there were. Groups become areas instead of points.
    #[serde(default)]
    pub size: Option<[f32; 2]>,
    #[serde(default)]
    pub members: Option<u32>,
}

/// Count and respawn time for imported monsters and mines (NPCs: one, no extra delay).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbyOptions {
    pub count: u32,
    pub refresh: u32,
}

/// One thing the server would reject, skip, clamp or crash on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// "error" (the server refuses, skips or crashes), "warning" (works differently than it looks) or
    /// "note" (harmless but probably unintended; common in official files).
    pub severity: &'static str,
    pub section: Section,
    pub index: usize,
    pub message: String,
}

/// What a check knows beyond the file.
#[derive(Debug, Clone, Default)]
pub struct ProblemContext {
    /// Structure names (upper case) of the templates, when an elements.data is open.
    pub templates: Option<HashMap<u32, String>>,
    /// Half the map size (the client's rows × 512), when the map is known.
    pub half_size: Option<f32>,
}

/// What resource rows store as their type (`DT_MINE_ESSENCE`; the server does not read it).
const MINE_DATA_TYPE: i32 = 47;

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
        self.record_all(label, vec![change])
    }

    /// Applies changes in order as one journal entry.
    fn record_all(&mut self, label: String, changes: Vec<Change>) -> Result<View, String> {
        for change in &changes {
            self.apply(change, true);
        }
        // The whole file must still write (version limits, names, attached areas).
        if let Err(error) = encode(&self.file) {
            for change in changes.iter().rev() {
                self.apply(change, false);
            }
            return Err(error);
        }
        if self.saved_entries.is_some_and(|saved| saved > self.done.len()) {
            self.saved_entries = None;
        }
        self.undone.clear();
        self.done.push(JournalEntry { id: self.next_entry, label, time: Local::now().timestamp(), changes });
        self.next_entry += 1;
        Ok(self.view())
    }

    /// Every NPC, monster and mine template the file spawns.
    pub fn template_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.file.areas.iter().flat_map(|area| area.generators.iter().map(|generator| generator.id)).collect();
        ids.extend(self.file.resource_areas.iter().flat_map(|area| area.resources.iter().map(|resource| resource.template as u32)));
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Checks the file the way the server's `npc_generator::LoadGenData` reads it.
    pub fn problems(&self, context: &ProblemContext) -> Vec<Problem> {
        let file = &self.file;
        let mut out = Vec::new();
        let mut add = |severity, section, index, message: String| out.push(Problem { severity, section, index, message });
        let controller_ids: HashSet<u32> = file.controllers.iter().map(|controller| controller.id).collect();
        let outside = |position: &Vec3| context.half_size.filter(|half| position.x.abs() > *half || position.z.abs() > *half);
        let structure = |id: u32| context.templates.as_ref().map(|templates| templates.get(&id).map(String::as_str));

        // Controllers: ID 0 stops the whole map; a repeated ID or trigger ID leaves the later one out.
        let mut seen_ids = HashMap::new();
        let mut seen_triggers = HashMap::new();
        for (index, controller) in file.controllers.iter().enumerate() {
            if controller.id == 0 {
                add("error", Section::Controllers, index, "Controller ID 0: the server refuses to load this map".into());
            } else if let Some(first) = seen_ids.get(&controller.id) {
                add("error", Section::Controllers, index, format!("Controller ID {} is also used by controller {}; the server keeps only the first", controller.id, *first + 1));
            } else {
                seen_ids.insert(controller.id, index);
            }
            if controller.controller_id > 0 {
                if let Some(first) = seen_triggers.get(&controller.controller_id) {
                    add("error", Section::Controllers, index, format!("Trigger ID {} is also used by controller {}; the server does not create this one, so its areas never spawn", controller.controller_id, *first + 1));
                } else {
                    seen_triggers.insert(controller.controller_id, index);
                }
            }
        }
        let missing_controller = |id: i64| id != 0 && (id < 0 || !controller_ids.contains(&(id as u32)));

        // Export IDs and attachments, per section (the server looks attached areas up by export ID).
        fn exports<'a>(list: impl Iterator<Item = (i32, i32, &'a [i32])>) -> (HashMap<i32, Vec<usize>>, HashSet<i32>) {
            let mut by_id: HashMap<i32, Vec<usize>> = HashMap::new();
            let mut attached = HashSet::new();
            for (index, (export, _, list)) in list.enumerate() {
                by_id.entry(export).or_default().push(index);
                attached.extend(list.iter().copied());
            }
            (by_id, attached)
        }
        let linked = file.version >= 12;
        let (area_exports, area_attached) = exports(file.areas.iter().map(|area| (area.export_id, area.attach_num, area.attached.as_slice())));
        let (resource_exports, resource_attached) = exports(file.resource_areas.iter().map(|area| (area.export_id, area.attach_num, area.attached.as_slice())));
        let check_links = |add: &mut dyn FnMut(&'static str, Section, usize, String), section, index, export_id: i32, attach_num: i32, attached: &[i32], exports: &HashMap<i32, Vec<usize>>, all_attached: &HashSet<i32>, flagged: &dyn Fn(usize) -> bool| {
            if !linked {
                return;
            }
            if let Some(others) = exports.get(&export_id).filter(|others| others.len() > 1 && others[0] != index) {
                add("warning", section, index, format!("Export ID {export_id} is also used by item {}; attachments to it are ambiguous", others[0] + 1));
            }
            if attach_num < 0 && !all_attached.contains(&export_id) {
                add("note", section, index, "Marked as attached to another area, but no area attaches it".into());
            }
            for id in attached {
                match exports.get(id) {
                    None => add("error", section, index, format!("Attaches export ID {id}, which no item in this section has (the server reads it without checking)")),
                    Some(targets) if !flagged(targets[0]) => add("warning", section, index, format!("Attaches item {} (export ID {id}), which is not marked as attached, so it also spawns on its own", targets[0] + 1)),
                    _ => {}
                }
            }
        };

        for (index, area) in file.areas.iter().enumerate() {
            let section = Section::Areas;
            if area.generators.is_empty() {
                add("note", section, index, "Spawns nothing (no generators)".into());
            }
            if missing_controller(area.controller as i64) {
                add("error", section, index, format!("Controller {} does not exist; the server never spawns this area", area.controller));
            }
            if !(0..=2).contains(&area.npc_type) {
                add("warning", section, index, format!("Type {} is unknown; the server treats it as monsters", area.npc_type));
            }
            if area.npc_type == 0 && !(0..=2).contains(&area.group_type) {
                add("warning", section, index, format!("Group type {} is unknown; the server treats it as normal", area.group_type));
            }
            if area.npc_type == 1 && area.revive == 2 && file.version >= 14 {
                add("error", section, index, "NPC areas cannot revive when switched on (the official editor refuses this)".into());
            }
            if let Some(half) = outside(&area.position) {
                add("warning", section, index, format!("Lies outside the map (±{half:.0}); the server does not load it"));
            }
            check_links(&mut add, section, index, area.export_id, area.attach_num, &area.attached, &area_exports, &area_attached, &|target| file.areas[target].attach_num < 0);
            for (row, generator) in area.generators.iter().enumerate() {
                let which = if area.generators.len() > 1 { format!("Row {}: ", row + 1) } else { String::new() };
                match structure(generator.id) {
                    Some(None) => add("error", section, index, format!("{which}{} is not in the open elements.data; the server skips it", generator.id)),
                    Some(Some("MONSTER_ESSENCE")) if area.npc_type == 1 => add("warning", section, index, format!("{which}{} is a monster in an NPC area", generator.id)),
                    Some(Some("NPC_ESSENCE")) if area.npc_type == 0 => add("warning", section, index, format!("{which}{} is an NPC in a monster area", generator.id)),
                    Some(Some(name)) if name != "MONSTER_ESSENCE" && name != "NPC_ESSENCE" => add("error", section, index, format!("{which}{} is not an NPC or monster ({name})", generator.id)),
                    _ => {}
                }
                if generator.count == 0 {
                    add("warning", section, index, format!("{which}{} spawns 0", generator.id));
                }
                if generator.aggressive > 2 {
                    add("warning", section, index, format!("{which}aggressive {} is unknown; the server treats it as the template's", generator.aggressive));
                }
                if generator.dead_time != 0 && !(5..=1800).contains(&generator.dead_time) {
                    add("warning", section, index, format!("{which}corpse time {} s; the server keeps corpses 5–1800 s", generator.dead_time));
                }
                if generator.path_id != 0 && !(0..=2).contains(&generator.loop_type) {
                    add("warning", section, index, format!("{which}path type {} is unknown", generator.loop_type));
                }
            }
        }
        for (index, area) in file.resource_areas.iter().enumerate() {
            let section = Section::Resources;
            if area.resources.is_empty() {
                add("note", section, index, "Spawns nothing (no resources)".into());
            }
            if missing_controller(area.controller as i64) {
                add("error", section, index, format!("Controller {} does not exist; the server never spawns this area", area.controller));
            }
            if let Some(half) = outside(&area.position) {
                add("warning", section, index, format!("Lies outside the map (±{half:.0}); the server does not load it"));
            }
            check_links(&mut add, section, index, area.export_id, area.attach_num, &area.attached, &resource_exports, &resource_attached, &|target| file.resource_areas[target].attach_num < 0);
            for resource in &area.resources {
                match structure(resource.template as u32) {
                    Some(None) => add("error", section, index, format!("{} is not in the open elements.data; the server skips it", resource.template)),
                    Some(Some(name)) if name != "MINE_ESSENCE" => add("error", section, index, format!("{} is not a mine ({name})", resource.template)),
                    _ => {}
                }
                if resource.count == 0 {
                    add("warning", section, index, format!("{} spawns 0", resource.template));
                }
            }
        }
        for (index, object) in file.dynamic_objects.iter().enumerate() {
            if missing_controller(object.controller as i64) {
                add("error", Section::Objects, index, format!("Controller {} does not exist; the server never shows this object", object.controller));
            }
            if let Some(half) = outside(&object.position) {
                add("warning", Section::Objects, index, format!("Lies outside the map (±{half:.0}); the server does not load it"));
            }
        }
        out
    }

    /// Adds what the game client showed: NPCs and monsters as point spawns, mines as point resource
    /// areas, dynamic objects as objects, each at the end of its section, as one journal entry. Defaults
    /// follow official point spawns (fixed height, spawn at start, revive, valid once); new export IDs
    /// continue after the section's highest; the phase is kept from version 14.
    pub fn import_nearby(&mut self, rows: Vec<NearbyImport>, options: NearbyOptions) -> Result<View, String> {
        if rows.is_empty() {
            return Err("Nothing to import".into());
        }
        let version = self.file.version;
        let next_export = |ids: &mut dyn Iterator<Item = i32>| ids.max().unwrap_or(0).max(0) + 1;
        let mut area_export = next_export(&mut self.file.areas.iter().map(|area| area.export_id));
        let mut resource_export = next_export(&mut self.file.resource_areas.iter().map(|area| area.export_id));
        let mut lengths = [self.file.areas.len(), self.file.resource_areas.len(), self.file.dynamic_objects.len()];
        let phase = |row: &NearbyImport| if version >= 14 { row.phase.unwrap_or(0) } else { 0 };
        let export = |next: &mut i32| {
            if version < 12 {
                return 0;
            }
            *next += 1;
            *next - 1
        };
        let mut changes = Vec::with_capacity(rows.len());
        for row in &rows {
            let (slot, item) = match row.kind.as_str() {
                "monster" if row.size.is_some() => {
                    // Like official monster areas: on the terrain, 20 high, death count 50.
                    let [width, depth] = row.size.unwrap_or_default();
                    let generator = Generator {
                        id: row.template,
                        count: row.members.unwrap_or(1).max(1),
                        refresh: options.refresh,
                        died_times: 50,
                        default_faction: 1,
                        default_faction_helper: 1,
                        default_faction_accept: 1,
                        ..Default::default()
                    };
                    let area = Area {
                        kind: 0,
                        position: row.position,
                        direction: Vec3 { x: 0.0, y: 0.0, z: 1.0 },
                        extents: Vec3 { x: width, y: 20.0, z: depth },
                        init_gen: 1,
                        revive: 1,
                        export_id: export(&mut area_export),
                        phase: phase(row),
                        generators: vec![generator],
                        ..Default::default()
                    };
                    (0, Item::Areas(area))
                }
                "npc" | "monster" => {
                    let npc = row.kind == "npc";
                    let facing = row.direction.and_then(|d| {
                        let length = (d.x * d.x + d.z * d.z).sqrt();
                        (length > 1e-3).then(|| Vec3 { x: d.x / length, y: 0.0, z: d.z / length })
                    });
                    let generator = Generator {
                        id: row.template,
                        count: if npc { 1 } else { options.count.max(1) },
                        refresh: if npc { 0 } else { options.refresh },
                        default_faction: 1,
                        default_faction_helper: 1,
                        default_faction_accept: 1,
                        ..Default::default()
                    };
                    let area = Area {
                        kind: 1,
                        position: row.position,
                        direction: facing.unwrap_or(Vec3 { x: 0.0, y: 0.0, z: 1.0 }),
                        npc_type: if npc { 1 } else { 0 },
                        init_gen: 1,
                        revive: 1,
                        valid_once: 1,
                        export_id: export(&mut area_export),
                        phase: phase(row),
                        generators: vec![generator],
                        ..Default::default()
                    };
                    (0, Item::Areas(area))
                }
                "mine" => {
                    let [axis0, axis1, turn] = row.rotation.unwrap_or([192, 64, 0]);
                    let count = row.members.unwrap_or(options.count).max(1);
                    let resource = Resource { kind: MINE_DATA_TYPE, template: row.template as i32, refresh: options.refresh, count, height_offset: 0.0 };
                    let [extent_x, extent_z] = row.size.unwrap_or_default();
                    let area = ResourceArea {
                        position: row.position,
                        extent_x,
                        extent_z,
                        init_gen: 1,
                        auto_revive: 1,
                        valid_once: 1,
                        direction: if version >= 6 { [axis0, axis1] } else { [0, 0] },
                        radius: if version >= 6 { turn } else { 0 },
                        export_id: export(&mut resource_export),
                        phase: phase(row),
                        resources: vec![resource],
                        ..Default::default()
                    };
                    (1, Item::Resources(area))
                }
                "dynamic" => {
                    let [axis0, axis1, turn] = row.rotation.unwrap_or([0, 0, 0]);
                    (2, Item::Objects(DynamicObject { id: row.template, position: row.position, direction: [axis0, axis1], radius: turn, scale: 16, controller: 0, phase: phase(row) }))
                }
                other => return Err(format!("Cannot import a {other:?} from the game")),
            };
            changes.push(Change { index: lengths[slot], before: None, after: Some(item) });
            lengths[slot] += 1;
        }
        let label = if rows.len() == 1 { "Import 1 item from the game".to_string() } else { format!("Import {} items from the game", rows.len()) };
        self.record_all(label, changes)
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

// ── Comparing with another npcgen.data and copying from it ──

/// Another npcgen.data opened read-only.
pub struct ComparedGen {
    pub path: PathBuf,
    file: NpcGen,
}

impl ComparedGen {
    /// An npcgen.data, or a JSON export (`.json`).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let data = std::fs::read(&path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        let is_json = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
        Ok(ComparedGen { file: if is_json { from_json(&data)? } else { parse(&data)? }, path })
    }
}

// ── JSON export (`jdide-npcgen` version 1) ──

const JSON_FORMAT: &str = "jdide-npcgen";
const JSON_FORMAT_VERSION: u32 = 1;

/// Items as the editor shows them (controller names as text; their raw bytes are not exported).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonExport {
    format: String,
    format_version: u32,
    /// The npcgen.data version the items come from.
    npcgen_version: u32,
    #[serde(default)]
    source: String,
    #[serde(default)]
    areas: Vec<Area>,
    #[serde(default)]
    resources: Vec<ResourceArea>,
    #[serde(default)]
    objects: Vec<DynamicObject>,
    #[serde(default)]
    controllers: Vec<Controller>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCounts {
    pub areas: usize,
    pub resources: usize,
    pub objects: usize,
    pub controllers: usize,
}

/// Reads a JSON export; it must fit the npcgen.data version it names.
fn from_json(data: &[u8]) -> Result<NpcGen, String> {
    let export: JsonExport = serde_json::from_slice(data).map_err(|error| format!("Not an npcgen JSON export: {error}"))?;
    if export.format != JSON_FORMAT {
        return Err(format!("Not an npcgen JSON export (format {:?})", export.format));
    }
    if export.format_version != JSON_FORMAT_VERSION {
        return Err(format!("This JSON export is format version {}; this editor reads version {JSON_FORMAT_VERSION}", export.format_version));
    }
    if !(1..=MAX_VERSION).contains(&export.npcgen_version) {
        return Err(format!("npcgen.data version {} is not supported", export.npcgen_version));
    }
    let file = NpcGen { version: export.npcgen_version, areas: export.areas, resource_areas: export.resources, dynamic_objects: export.objects, controllers: export.controllers };
    encode(&file).map_err(|error| format!("This JSON export does not fit npcgen.data version {}: {error}", file.version))?;
    Ok(file)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenCompareRow {
    pub section: Section,
    /// The item in the other file (none: only in this one).
    pub there: Option<usize>,
    /// The item in this file (none: only in the other one).
    pub here: Option<usize>,
    /// "missing" (only in the other file), "different", "same" or "only_here".
    pub status: &'static str,
    /// Differing fields (`generators.refresh`, `phase`, …).
    pub fields: Vec<String>,
    /// Areas: templates; resources: mines; objects: the object ID; controllers: the controller ID.
    pub ids: Vec<u32>,
    pub x: f32,
    pub z: f32,
    /// Controllers: the name.
    pub label: String,
    /// Why it cannot be copied, when it cannot.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenComparison {
    pub path: String,
    pub version: u32,
    pub rows: Vec<GenCompareRow>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyReport {
    pub added: usize,
    pub replaced: usize,
    /// Controllers copied because copied items use them.
    pub controllers: usize,
    /// Attached export IDs left out because the attached area is not in this file.
    pub dropped_attachments: usize,
    /// Items whose values this file's version cannot store were cleared.
    pub fitted: usize,
}

/// Pairs items (ours, theirs) by a class key and position: same place first, then the nearest within 10 m.
fn pair_items<T>(ours: &[T], theirs: &[T], key: impl Fn(&T) -> (String, f32, f32)) -> Vec<(Option<usize>, Option<usize>)> {
    let our_keys: Vec<_> = ours.iter().map(&key).collect();
    let their_keys: Vec<_> = theirs.iter().map(&key).collect();
    let mut by_class: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, (class, _, _)) in our_keys.iter().enumerate() {
        by_class.entry(class.as_str()).or_default().push(index);
    }
    let mut taken = vec![false; ours.len()];
    let mut partner: Vec<Option<usize>> = vec![None; theirs.len()];
    for limit in [0.05f32, 10.0] {
        for (there, (class, x, z)) in their_keys.iter().enumerate() {
            if partner[there].is_some() {
                continue;
            }
            let Some(candidates) = by_class.get(class.as_str()) else { continue };
            let best = candidates
                .iter()
                .filter(|&&here| !taken[here])
                .map(|&here| (here, (our_keys[here].1 - x).hypot(our_keys[here].2 - z)))
                .filter(|(_, distance)| *distance <= limit)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((here, _)) = best {
                taken[here] = true;
                partner[there] = Some(here);
            }
        }
    }
    let mut out: Vec<_> = partner.iter().enumerate().map(|(there, here)| (*here, Some(there))).collect();
    out.extend(taken.iter().enumerate().filter(|(_, taken)| !**taken).map(|(here, _)| (Some(here), None)));
    out
}

fn sorted_ids(ids: impl Iterator<Item = u32>) -> Vec<u32> {
    let mut ids: Vec<u32> = ids.collect();
    ids.sort_unstable();
    ids
}

/// Pairs controllers by ID (the first of repeated IDs).
fn pair_controllers(ours: &[Controller], theirs: &[Controller]) -> Vec<(Option<usize>, Option<usize>)> {
    let first = |list: &[Controller]| {
        let mut map = HashMap::new();
        for (index, controller) in list.iter().enumerate() {
            map.entry(controller.id).or_insert(index);
        }
        map
    };
    let (mine, other) = (first(ours), first(theirs));
    let mut out: Vec<_> = (0..theirs.len()).map(|there| (mine.get(&theirs[there].id).copied().filter(|_| other.get(&theirs[there].id) == Some(&there)), Some(there))).collect();
    let paired: HashSet<usize> = out.iter().filter_map(|(here, _)| *here).collect();
    out.extend((0..ours.len()).filter(|here| !paired.contains(here)).map(|here| (Some(here), None)));
    out
}

struct Pairs {
    areas: Vec<(Option<usize>, Option<usize>)>,
    resources: Vec<(Option<usize>, Option<usize>)>,
    objects: Vec<(Option<usize>, Option<usize>)>,
    controllers: Vec<(Option<usize>, Option<usize>)>,
}

impl Pairs {
    fn of(ours: &NpcGen, theirs: &NpcGen) -> Self {
        Pairs {
            areas: pair_items(&ours.areas, &theirs.areas, |area| (format!("{}:{:?}", area.npc_type, sorted_ids(area.generators.iter().map(|generator| generator.id))), area.position.x, area.position.z)),
            resources: pair_items(&ours.resource_areas, &theirs.resource_areas, |area| (format!("{:?}", sorted_ids(area.resources.iter().map(|resource| resource.template as u32))), area.position.x, area.position.z)),
            objects: pair_items(&ours.dynamic_objects, &theirs.dynamic_objects, |object| (object.id.to_string(), object.position.x, object.position.z)),
            controllers: pair_controllers(&ours.controllers, &theirs.controllers),
        }
    }

    fn list(&self, section: Section) -> &[(Option<usize>, Option<usize>)] {
        match section {
            Section::Areas => &self.areas,
            Section::Resources => &self.resources,
            Section::Objects => &self.objects,
            Section::Controllers => &self.controllers,
        }
    }

    /// Their export ID → ours, through paired items of a section.
    fn exports(&self, section: Section, ours: &[i32], theirs: &[i32]) -> HashMap<i32, i32> {
        self.list(section).iter().filter_map(|pair| match *pair {
            (Some(here), Some(there)) => Some((theirs[there], ours[here])),
            _ => None,
        }).collect()
    }
}

/// Differing fields of two serialized items (one level into lists of records).
fn differing(ours: &serde_json::Value, theirs: &serde_json::Value) -> Vec<String> {
    let (Some(a), Some(b)) = (ours.as_object(), theirs.as_object()) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for (key, value) in a {
        if key == "nameRaw" {
            continue;
        }
        let other = b.get(key);
        if other == Some(value) {
            continue;
        }
        match (value.as_array(), other.and_then(|other| other.as_array())) {
            (Some(x), Some(y)) if x.len() == y.len() && x.iter().all(serde_json::Value::is_object) => {
                for (p, q) in x.iter().zip(y) {
                    for sub in differing(p, q) {
                        let name = format!("{key}.{sub}");
                        if !out.contains(&name) {
                            out.push(name);
                        }
                    }
                }
            }
            _ => out.push(key.clone()),
        }
    }
    out
}

/// Clears what a version cannot store; true when something was cleared.
fn fit_to_version(item: &mut Item, version: u32) -> bool {
    let before = item.clone();
    match item {
        Item::Areas(area) => {
            if version < 14 { area.phase = 0; }
            if version < 12 { area.export_id = 0; area.attach_num = 0; area.attached.clear(); }
            if version < 7 { area.controller = 0; area.life_time = 0; area.max_count = 0; }
        }
        Item::Resources(area) => {
            if version < 14 { area.phase = 0; }
            if version < 12 { area.export_id = 0; area.attach_num = 0; area.attached.clear(); }
            if version < 7 { area.controller = 0; area.max_count = 0; }
            if version < 6 { area.direction = [0, 0]; area.radius = 0; }
        }
        Item::Objects(object) => {
            if version < 14 { object.phase = 0; }
            if version < 10 { object.controller = 0; }
            if version < 9 { object.scale = 0; }
        }
        Item::Controllers(controller) => {
            if version < 13 { controller.segments.clear(); controller.segment_logic = 0; }
            if version < 11 { controller.repeat = 0; }
            if version < 8 { controller.active_time_range = 0; }
        }
    }
    *item != before
}

impl Document {
    fn item_list(file: &NpcGen, section: Section, index: usize) -> Item {
        match section {
            Section::Areas => Item::Areas(file.areas[index].clone()),
            Section::Resources => Item::Resources(file.resource_areas[index].clone()),
            Section::Objects => Item::Objects(file.dynamic_objects[index].clone()),
            Section::Controllers => Item::Controllers(file.controllers[index].clone()),
        }
    }

    /// Pairs this file with another and says what differs. Export IDs are file-local: attachments are
    /// compared through the pairing.
    pub fn compare(&self, other: &ComparedGen) -> GenComparison {
        let (ours, theirs) = (&self.file, &other.file);
        let pairs = Pairs::of(ours, theirs);
        let area_exports = pairs.exports(Section::Areas, &ours.areas.iter().map(|area| area.export_id).collect::<Vec<_>>(), &theirs.areas.iter().map(|area| area.export_id).collect::<Vec<_>>());
        let resource_exports = pairs.exports(Section::Resources, &ours.resource_areas.iter().map(|area| area.export_id).collect::<Vec<_>>(), &theirs.resource_areas.iter().map(|area| area.export_id).collect::<Vec<_>>());
        // Comparable form: no export ID, attachments in this file's export IDs.
        let normalize = |item: Item, map: Option<&HashMap<i32, i32>>| -> serde_json::Value {
            let translate = |attached: &mut Vec<i32>| if let Some(map) = map { *attached = attached.iter().map(|id| map.get(id).copied().unwrap_or(i32::MIN)).collect(); };
            let value = match item {
                Item::Areas(mut area) => { area.export_id = 0; translate(&mut area.attached); serde_json::to_value(area) }
                Item::Resources(mut area) => { area.export_id = 0; translate(&mut area.attached); serde_json::to_value(area) }
                Item::Objects(object) => serde_json::to_value(object),
                Item::Controllers(controller) => serde_json::to_value(controller),
            };
            value.unwrap_or_default()
        };
        let our_triggers: HashMap<i32, u32> = ours.controllers.iter().filter(|controller| controller.controller_id > 0).map(|controller| (controller.controller_id, controller.id)).collect();
        let version = ours.version;
        let mut rows = Vec::new();
        for section in [Section::Areas, Section::Resources, Section::Objects, Section::Controllers] {
            let exports = match section {
                Section::Areas => Some(&area_exports),
                Section::Resources => Some(&resource_exports),
                _ => None,
            };
            for &(here, there) in pairs.list(section) {
                let shown = match (there, here) {
                    (Some(there), _) => Self::item_list(theirs, section, there),
                    (None, Some(here)) => Self::item_list(ours, section, here),
                    (None, None) => continue,
                };
                let (status, fields) = match (here, there) {
                    (Some(here), Some(there)) => {
                        let fields = differing(&normalize(Self::item_list(ours, section, here), None), &normalize(Self::item_list(theirs, section, there), exports));
                        (if fields.is_empty() { "same" } else { "different" }, fields)
                    }
                    (None, Some(_)) => ("missing", Vec::new()),
                    _ => ("only_here", Vec::new()),
                };
                let (ids, x, z, label) = match &shown {
                    Item::Areas(area) => (area.generators.iter().map(|generator| generator.id).collect(), area.position.x, area.position.z, String::new()),
                    Item::Resources(area) => (area.resources.iter().map(|resource| resource.template as u32).collect(), area.position.x, area.position.z, String::new()),
                    Item::Objects(object) => (vec![object.id], object.position.x, object.position.z, String::new()),
                    Item::Controllers(controller) => (vec![controller.id], 0.0, 0.0, controller.name.clone()),
                };
                let blocked = match (&shown, status) {
                    (_, "same" | "only_here") => None,
                    (Item::Objects(_), _) if version < 6 => Some(format!("Version {version} files store no dynamic objects")),
                    (Item::Controllers(_), _) if version < 7 => Some(format!("Version {version} files store no controllers")),
                    (Item::Controllers(controller), _) => our_triggers.get(&controller.controller_id).filter(|id| **id != controller.id).map(|id| format!("Trigger ID {} is already used by controller {id} here", controller.controller_id)),
                    _ => None,
                };
                rows.push(GenCompareRow { section, there, here, status, fields, ids, x, z, label, blocked });
            }
        }
        GenComparison { path: other.path.display().to_string(), version: theirs.version, rows }
    }

    /// Copies items of the other file (`(section, index there)`): ones this file lacks are added at the end,
    /// paired ones replace ours (keeping our export ID). Controllers that copied items use and this file
    /// lacks come along; attachments are translated through the pairing; what this version cannot store is
    /// cleared. One journal entry.
    pub fn copy_compared(&mut self, other: &ComparedGen, picks: &[(Section, usize)]) -> Result<(View, CopyReport), String> {
        if picks.is_empty() {
            return Err("Nothing to copy".into());
        }
        let comparison = self.compare(other);
        let (ours, theirs) = (&self.file, &other.file);
        let version = ours.version;
        let pairs = Pairs::of(ours, theirs);
        let partner = |section: Section, there: usize| pairs.list(section).iter().find(|pair| pair.1 == Some(there)).and_then(|pair| pair.0);
        for &(section, there) in picks {
            let row = comparison.rows.iter().find(|row| row.section == section && row.there == Some(there)).ok_or("That item is not in the other file")?;
            if let Some(reason) = &row.blocked {
                return Err(reason.clone());
            }
        }
        let mut report = CopyReport::default();
        let mut changes: Vec<Change> = Vec::new();
        let mut lengths = [ours.areas.len(), ours.resource_areas.len(), ours.dynamic_objects.len(), ours.controllers.len()];
        let slot = |section: Section| match section { Section::Areas => 0, Section::Resources => 1, Section::Objects => 2, Section::Controllers => 3 };
        let push = |changes: &mut Vec<Change>, report: &mut CopyReport, section: Section, here: Option<usize>, mut item: Item, lengths: &mut [usize; 4]| {
            if fit_to_version(&mut item, version) {
                report.fitted += 1;
            }
            match here {
                Some(index) => {
                    let before = Self::item_list(ours, section, index);
                    if before != item {
                        report.replaced += 1;
                        changes.push(Change { index, before: Some(before), after: Some(item) });
                    }
                }
                None => {
                    report.added += 1;
                    changes.push(Change { index: lengths[slot(section)], before: None, after: Some(item) });
                    lengths[slot(section)] += 1;
                }
            }
        };

        // Controllers: the picked ones, then the ones copied items need.
        let mut have: HashSet<u32> = ours.controllers.iter().map(|controller| controller.id).collect();
        for &(section, there) in picks.iter().filter(|(section, _)| *section == Section::Controllers) {
            let controller = theirs.controllers[there].clone();
            have.insert(controller.id);
            push(&mut changes, &mut report, section, partner(section, there), Item::Controllers(controller), &mut lengths);
        }
        let mut needed: Vec<u32> = picks.iter().filter_map(|&(section, there)| match section {
            Section::Areas => Some(theirs.areas[there].controller as u32),
            Section::Resources => Some(theirs.resource_areas[there].controller as u32),
            Section::Objects => Some(theirs.dynamic_objects[there].controller),
            Section::Controllers => None,
        }).filter(|id| *id != 0 && !have.contains(id)).collect();
        needed.sort_unstable();
        needed.dedup();
        for id in needed {
            if let Some(controller) = theirs.controllers.iter().find(|controller| controller.id == id) {
                push(&mut changes, &mut report, Section::Controllers, None, Item::Controllers(controller.clone()), &mut lengths);
                report.added -= 1;
                report.controllers += 1;
            }
        }

        // Spawn and resource areas: new export IDs where theirs are taken here; attachments translated.
        for section in [Section::Areas, Section::Resources] {
            let (our_exports, their_exports): (Vec<i32>, Vec<i32>) = match section {
                Section::Areas => (ours.areas.iter().map(|area| area.export_id).collect(), theirs.areas.iter().map(|area| area.export_id).collect()),
                _ => (ours.resource_areas.iter().map(|area| area.export_id).collect(), theirs.resource_areas.iter().map(|area| area.export_id).collect()),
            };
            let paired = pairs.exports(section, &our_exports, &their_exports);
            let mut used: HashSet<i32> = our_exports.iter().copied().collect();
            let mut next = our_exports.iter().copied().max().unwrap_or(0).max(0) + 1;
            let mut added: HashMap<i32, i32> = HashMap::new();
            let picked: Vec<usize> = picks.iter().filter(|(picked, _)| *picked == section).map(|(_, there)| *there).collect();
            for &there in &picked {
                if partner(section, there).is_none() && version >= 12 {
                    let export = their_exports[there];
                    let id = if export > 0 && !used.contains(&export) { export } else { next };
                    used.insert(id);
                    next = next.max(id + 1);
                    added.insert(export, id);
                }
            }
            for there in picked {
                let here = partner(section, there);
                let export = match here {
                    Some(index) => our_exports[index],
                    None => added.get(&their_exports[there]).copied().unwrap_or(0),
                };
                let translate = |attached: &[i32], report: &mut CopyReport| -> Vec<i32> {
                    attached.iter().filter_map(|id| {
                        let found = added.get(id).or_else(|| paired.get(id)).copied();
                        if found.is_none() {
                            report.dropped_attachments += 1;
                        }
                        found
                    }).collect()
                };
                let item = match section {
                    Section::Areas => {
                        let mut area = theirs.areas[there].clone();
                        area.export_id = export;
                        area.attached = translate(&area.attached, &mut report);
                        if area.attach_num >= 0 { area.attach_num = area.attached.len() as i32; }
                        Item::Areas(area)
                    }
                    _ => {
                        let mut area = theirs.resource_areas[there].clone();
                        area.export_id = export;
                        area.attached = translate(&area.attached, &mut report);
                        if area.attach_num >= 0 { area.attach_num = area.attached.len() as i32; }
                        Item::Resources(area)
                    }
                };
                push(&mut changes, &mut report, section, here, item, &mut lengths);
            }
        }
        for &(section, there) in picks.iter().filter(|(section, _)| *section == Section::Objects) {
            push(&mut changes, &mut report, section, partner(section, there), Item::Objects(theirs.dynamic_objects[there].clone()), &mut lengths);
        }
        if changes.is_empty() {
            return Err("The picked items are already the same here".into());
        }
        let label = match (report.added, report.replaced) {
            (added, 0) => format!("Copy {added} item{} from another npcgen.data", if added == 1 { "" } else { "s" }),
            (0, replaced) => format!("Replace {replaced} item{} from another npcgen.data", if replaced == 1 { "" } else { "s" }),
            (added, replaced) => format!("Copy {added} and replace {replaced} items from another npcgen.data"),
        };
        let view = self.record_all(label, changes)?;
        Ok((view, report))
    }
}

impl Document {
    /// A JSON export of the whole file (`picks` none) or of picked items. With `related`, the controllers
    /// they use and the areas they attach come along, so an import elsewhere is complete.
    pub fn export_json(&self, picks: Option<&[(Section, usize)]>, related: bool) -> Result<(String, ExportCounts), String> {
        let file = &self.file;
        let all = |len: usize| (0..len).collect::<std::collections::BTreeSet<usize>>();
        let (mut areas, mut resources, objects, mut controllers) = match picks {
            None => (all(file.areas.len()), all(file.resource_areas.len()), all(file.dynamic_objects.len()), all(file.controllers.len())),
            Some(picks) => {
                let mut sets: [std::collections::BTreeSet<usize>; 4] = Default::default();
                for &(section, index) in picks {
                    let (slot, len) = match section {
                        Section::Areas => (0, file.areas.len()),
                        Section::Resources => (1, file.resource_areas.len()),
                        Section::Objects => (2, file.dynamic_objects.len()),
                        Section::Controllers => (3, file.controllers.len()),
                    };
                    if index >= len {
                        return Err(format!("No item {} in this section", index + 1));
                    }
                    sets[slot].insert(index);
                }
                let [a, r, o, c] = sets;
                (a, r, o, c)
            }
        };
        if related && picks.is_some() {
            // Attached areas (by export ID, until nothing new comes in), then the controllers of everything.
            loop {
                let before = areas.len() + resources.len();
                let attached: Vec<i32> = areas.iter().flat_map(|&index| file.areas[index].attached.clone()).collect();
                areas.extend(file.areas.iter().enumerate().filter(|(_, area)| attached.contains(&area.export_id)).map(|(index, _)| index));
                let attached: Vec<i32> = resources.iter().flat_map(|&index| file.resource_areas[index].attached.clone()).collect();
                resources.extend(file.resource_areas.iter().enumerate().filter(|(_, area)| attached.contains(&area.export_id)).map(|(index, _)| index));
                if areas.len() + resources.len() == before {
                    break;
                }
            }
            let used: HashSet<u32> = areas.iter().map(|&index| file.areas[index].controller as u32)
                .chain(resources.iter().map(|&index| file.resource_areas[index].controller as u32))
                .chain(objects.iter().map(|&index| file.dynamic_objects[index].controller))
                .filter(|id| *id != 0)
                .collect();
            for id in used {
                if let Some(index) = file.controllers.iter().position(|controller| controller.id == id) {
                    controllers.insert(index);
                }
            }
        }
        let counts = ExportCounts { areas: areas.len(), resources: resources.len(), objects: objects.len(), controllers: controllers.len() };
        if counts == ExportCounts::default() {
            return Err("Nothing to export".into());
        }
        let export = JsonExport {
            format: JSON_FORMAT.into(),
            format_version: JSON_FORMAT_VERSION,
            npcgen_version: file.version,
            source: self.path.display().to_string(),
            areas: areas.iter().map(|&index| file.areas[index].clone()).collect(),
            resources: resources.iter().map(|&index| file.resource_areas[index].clone()).collect(),
            objects: objects.iter().map(|&index| file.dynamic_objects[index].clone()).collect(),
            controllers: controllers.iter().map(|&index| Controller { name_raw: Vec::new(), ..file.controllers[index].clone() }).collect(),
        };
        let mut value = serde_json::to_value(&export).map_err(|error| error.to_string())?;
        for controller in value.get_mut("controllers").and_then(serde_json::Value::as_array_mut).into_iter().flatten() {
            if let Some(object) = controller.as_object_mut() {
                object.remove("nameRaw");
            }
        }
        Ok((serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?, counts))
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

    #[test]
    fn imports_what_the_game_showed_as_one_step() {
        // z1 (version 13, no phases) and x1 (version 14) of the zxserver sample.
        for map in ["z1", "x1"] {
            let path = std::path::PathBuf::from(format!("E:/Game Dev/JD/zxserver/zgame/gs/config/{map}/npcgen.data"));
            if !path.is_file() {
                continue;
            }
            let mut document = Document::open(&path).unwrap();
            let version = document.file.version;
            let before = document.view();
            let max_export = document.file.areas.iter().map(|area| area.export_id).max().unwrap_or(0);
            let at = Vec3 { x: 185.4, y: 164.9, z: 104.5 };
            let row = |kind: &str, template, rotation, phase| NearbyImport { kind: kind.into(), template, position: at, direction: Some(Vec3 { x: 3.0, y: 0.5, z: 4.0 }), rotation, phase, size: None, members: None };
            let rows = vec![row("npc", 1173, None, Some(26)), row("monster", 3001, None, None), row("mine", 18892, Some([192, 63, 0]), None), row("dynamic", 154, Some([0, 0, 26]), Some(7))];
            let view = document.import_nearby(rows, NearbyOptions { count: 2, refresh: 60 }).unwrap();
            assert_eq!((view.areas.len(), view.resources.len(), view.objects.len()), (before.areas.len() + 2, before.resources.len() + 1, before.objects.len() + 1), "{map}");
            assert_eq!(view.history.len(), 1);
            let Item::Areas(npc) = document.item(Section::Areas, before.areas.len()).unwrap() else { panic!() };
            assert_eq!((npc.kind, npc.npc_type, npc.init_gen, npc.revive, npc.valid_once, npc.position), (1, 1, 1, 1, 1, at));
            assert_eq!((npc.direction.x, npc.direction.y, npc.direction.z), (0.6, 0.0, 0.8));
            assert_eq!((npc.generators[0].id, npc.generators[0].count, npc.generators[0].refresh, npc.generators[0].default_faction), (1173, 1, 0, 1));
            assert_eq!(npc.export_id, if version >= 12 { max_export + 1 } else { 0 });
            assert_eq!(npc.phase, if version >= 14 { 26 } else { 0 }, "{map}");
            let Item::Areas(monster) = document.item(Section::Areas, before.areas.len() + 1).unwrap() else { panic!() };
            assert_eq!((monster.npc_type, monster.generators[0].count, monster.generators[0].refresh), (0, 2, 60));
            assert_eq!(monster.export_id, if version >= 12 { max_export + 2 } else { 0 });
            let Item::Resources(mine) = document.item(Section::Resources, before.resources.len()).unwrap() else { panic!() };
            assert_eq!((mine.direction, mine.radius, mine.resources[0].template, mine.resources[0].kind, mine.resources[0].refresh), ([192, 63], 0, 18892, 47, 60));
            let Item::Objects(object) = document.item(Section::Objects, before.objects.len()).unwrap() else { panic!() };
            assert_eq!((object.id, object.direction, object.radius, object.scale), (154, [0, 0], 26, 16));
            assert!(encode(&document.file).is_ok());
            // One undo removes all four.
            let view = document.undo().unwrap();
            assert_eq!((view.areas.len(), view.resources.len(), view.objects.len()), (before.areas.len(), before.resources.len(), before.objects.len()));
            assert!(document.import_nearby(vec![row("tree", 1, None, None)], NearbyOptions { count: 1, refresh: 0 }).is_err());
        }
    }

    #[test]
    fn groups_import_as_areas() {
        let path = std::path::PathBuf::from("E:/Game Dev/JD/zxserver/zgame/gs/config/x1/npcgen.data");
        let Ok(mut document) = Document::open(&path) else { return };
        let (areas, resources) = (document.file.areas.len(), document.file.resource_areas.len());
        let group = |kind: &str, template| NearbyImport { kind: kind.into(), template, position: Vec3 { x: 10.0, y: 5.0, z: 20.0 }, direction: None, rotation: Some([192, 63, 0]), phase: None, size: Some([24.0, 16.0]), members: Some(6) };
        document.import_nearby(vec![group("monster", 3001), group("mine", 18892)], NearbyOptions { count: 1, refresh: 30 }).unwrap();
        let Item::Areas(area) = document.item(Section::Areas, areas).unwrap() else { panic!() };
        assert_eq!((area.kind, area.npc_type, area.extents, area.valid_once), (0, 0, Vec3 { x: 24.0, y: 20.0, z: 16.0 }, 0));
        assert_eq!((area.generators[0].count, area.generators[0].refresh, area.generators[0].died_times), (6, 30, 50));
        let Item::Resources(mine) = document.item(Section::Resources, resources).unwrap() else { panic!() };
        assert_eq!((mine.extent_x, mine.extent_z, mine.resources[0].count), (24.0, 16.0, 6));
    }

    #[test]
    fn problems_follow_the_server() {
        let path = std::path::PathBuf::from("E:/Game Dev/JD/zxserver/zgame/gs/config/x1/npcgen.data");
        let Ok(mut document) = Document::open(&path) else { return };
        let context = ProblemContext::default();
        let errors = |document: &Document, context: &ProblemContext| document.problems(context).into_iter().filter(|problem| problem.severity == "error").collect::<Vec<_>>();
        // The official file has no errors.
        assert!(errors(&document, &context).is_empty());

        let file = &mut document.file;
        let mut copy = file.controllers[0].clone();
        copy.name = "copy".into();
        file.controllers.push(copy.clone());
        copy.id = 0;
        copy.controller_id = 0;
        file.controllers.push(copy);
        let area = file.areas.iter().position(|area| area.npc_type == 0 && !area.generators.is_empty()).unwrap();
        file.areas[area].controller = 999_999;
        file.areas[area].attached = vec![-12_345];
        file.areas[area].attach_num = 1;
        file.areas[area].generators[0].aggressive = 7;
        let monster = file.areas[area].generators[0].id;
        let found = errors(&document, &context);
        let messages: Vec<&str> = found.iter().map(|problem| problem.message.as_str()).collect();
        let count = document.file.controllers.len();
        assert!(found.iter().any(|problem| problem.section == Section::Controllers && problem.index == count - 2 && problem.message.contains("keeps only the first")), "{messages:?}");
        assert!(found.iter().any(|problem| problem.section == Section::Controllers && problem.index == count - 2 && problem.message.contains("does not create this one")), "{messages:?}");
        assert!(found.iter().any(|problem| problem.index == count - 1 && problem.message.contains("refuses to load this map")), "{messages:?}");
        assert!(found.iter().any(|problem| problem.section == Section::Areas && problem.index == area && problem.message.contains("Controller 999999 does not exist")), "{messages:?}");
        assert!(found.iter().any(|problem| problem.index == area && problem.message.contains("Attaches export ID -12345")), "{messages:?}");
        assert!(document.problems(&context).iter().any(|problem| problem.index == area && problem.severity == "warning" && problem.message.contains("aggressive 7")));

        // With elements.data: a missing template and an NPC in a monster area; with the map size: outside.
        let mut templates = HashMap::new();
        templates.insert(monster, "NPC_ESSENCE".to_string());
        let context = ProblemContext { templates: Some(templates), half_size: Some(1.0) };
        let all = document.problems(&context);
        assert!(all.iter().any(|problem| problem.index == area && problem.message.contains("is an NPC in a monster area")));
        assert!(all.iter().any(|problem| problem.severity == "error" && problem.message.contains("is not in the open elements.data")));
        assert!(all.iter().any(|problem| problem.message.contains("outside the map")));
    }

    #[test]
    fn copies_what_another_server_has_and_the_result_compares_equal() {
        let ours = std::path::PathBuf::from("E:/Game Dev/JD/zxserver/zgame/gs/config/x1/npcgen.data");
        let theirs = std::path::PathBuf::from("E:/Game Dev/JD/1559/gamed/config/x1/npcgen.data");
        let (Ok(mut document), Ok(other)) = (Document::open(&ours), ComparedGen::open(&theirs)) else { return };
        let before = document.view();
        let errors = |document: &Document| document.problems(&ProblemContext::default()).into_iter().filter(|problem| problem.severity == "error").count();
        let errors_before = errors(&document);
        let comparison = document.compare(&other);
        let picks: Vec<(Section, usize)> = comparison.rows.iter().filter(|row| matches!(row.status, "missing" | "different") && row.blocked.is_none()).map(|row| (row.section, row.there.unwrap())).collect();
        let missing = comparison.rows.iter().filter(|row| row.status == "missing").count();
        assert!(missing > 100 && picks.len() > missing);
        let (view, report) = document.copy_compared(&other, &picks).unwrap();
        assert_eq!(report.added, missing);
        assert_eq!(view.history.len(), 1);
        assert!(encode(&document.file).is_ok());
        assert_eq!(errors(&document), errors_before);
        // Everything picked now compares equal; only what is only here stays.
        let again = document.compare(&other);
        assert!(again.rows.iter().all(|row| matches!(row.status, "same" | "only_here")), "{:?}", again.rows.iter().filter(|row| !matches!(row.status, "same" | "only_here")).take(3).collect::<Vec<_>>());
        // New export IDs do not clash, and the attachments still point at areas.
        let mut exports: Vec<i32> = document.file.areas.iter().map(|area| area.export_id).filter(|id| *id != 0).collect();
        let total = exports.len();
        exports.sort_unstable();
        exports.dedup();
        assert_eq!(exports.len(), total);
        let undone = document.undo().unwrap();
        assert_eq!((undone.areas.len(), undone.resources.len(), undone.objects.len(), undone.controllers.len()), (before.areas.len(), before.resources.len(), before.objects.len(), before.controllers.len()));
        // A newer file into an older version: values it cannot store are cleared, not refused.
        document.file.version = 13;
        document.file.areas.iter_mut().for_each(|area| area.phase = 0);
        document.file.resource_areas.iter_mut().for_each(|area| area.phase = 0);
        document.file.dynamic_objects.iter_mut().for_each(|object| object.phase = 0);
        let comparison = document.compare(&other);
        let row = comparison.rows.iter().find(|row| row.section == Section::Areas && row.status == "missing" && other.file.areas[row.there.unwrap()].phase != 0).unwrap();
        let phased = row.there.unwrap();
        let (_, report) = document.copy_compared(&other, &[(Section::Areas, phased)]).unwrap();
        assert_eq!(report.fitted, 1, "{row:?}");
    }

    #[test]
    fn json_exports_import_through_the_comparison() {
        let path = std::path::PathBuf::from("E:/Game Dev/JD/zxserver/zgame/gs/config/x1/npcgen.data");
        let Ok(document) = Document::open(&path) else { return };
        let folder = std::env::temp_dir().join(format!("jdide-npcgen-json-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        // The whole file comes back identical.
        let (json, counts) = document.export_json(None, true).unwrap();
        assert_eq!((counts.areas, counts.controllers), (document.file.areas.len(), document.file.controllers.len()));
        let whole = folder.join("whole.json");
        std::fs::write(&whole, &json).unwrap();
        let comparison = document.compare(&ComparedGen::open(&whole).unwrap());
        assert!(comparison.rows.iter().all(|row| row.status == "same"));
        assert!(!json.contains("nameRaw"));

        // An area with attachments and one with a controller bring them along (z10 has attachments).
        let Ok(document) = Document::open("E:/Game Dev/JD/zxserver/zgame/gs/config/z10/npcgen.data") else { return };
        let attaching = document.file.areas.iter().position(|area| area.attach_num > 0).unwrap();
        let controlled = document.file.areas.iter().position(|area| area.controller != 0 && area.attach_num == 0).unwrap();
        let picks = [(Section::Areas, attaching), (Section::Areas, controlled)];
        let (json, counts) = document.export_json(Some(&picks), true).unwrap();
        let attached = document.file.areas[attaching].attached.len();
        let controllers = [attaching, controlled].iter().map(|&index| document.file.areas[index].controller).filter(|id| *id != 0).collect::<HashSet<_>>().len();
        assert_eq!((counts.areas, counts.controllers), (2 + attached, controllers));
        let (_, alone) = document.export_json(Some(&picks), false).unwrap();
        assert_eq!((alone.areas, alone.controllers), (2, 0));
        let part = folder.join("part.json");
        std::fs::write(&part, &json).unwrap();
        // Into another map: everything is new, copies cleanly, and the attachments still resolve.
        let Ok(mut other) = Document::open(&path) else { return };
        let imported = ComparedGen::open(&part).unwrap();
        let comparison = other.compare(&imported);
        let picks: Vec<(Section, usize)> = comparison.rows.iter().filter(|row| row.status == "missing" && row.section == Section::Areas).map(|row| (row.section, row.there.unwrap())).collect();
        assert_eq!(picks.len(), counts.areas);
        let errors = |document: &Document| document.problems(&ProblemContext::default()).into_iter().filter(|problem| problem.severity == "error").count();
        let before = errors(&other);
        let (_, report) = other.copy_compared(&imported, &picks).unwrap();
        assert_eq!((report.added, report.dropped_attachments), (counts.areas, 0));
        assert_eq!(errors(&other), before);

        // Broken or mismatched exports are refused.
        let bad = folder.join("bad.json");
        std::fs::write(&bad, r#"{"format":"something","formatVersion":1,"npcgenVersion":14}"#).unwrap();
        assert!(ComparedGen::open(&bad).is_err());
        std::fs::write(&bad, json.replace("\"npcgenVersion\": 14", "\"npcgenVersion\": 11")).unwrap();
        assert!(ComparedGen::open(&bad).err().unwrap_or_default().contains("does not fit npcgen.data version 11"));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
