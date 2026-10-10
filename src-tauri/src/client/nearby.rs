//! NPCs, monsters, mines and dynamic objects the running game client currently knows about (read-only).
//!
//! `CECGameRun::m_pWorld → CECWorld::m_aManagers[MAN_PLAYER, MAN_NPC, MAN_MATTER, MAN_ORNAMENT,
//! MAN_SKILLGFX, MAN_DECAL]`. The NPC and matter managers keep `abase::hashtab<Object*, int>` tables:
//! `{hash functor, size_t count, vector<Node*> buckets {data, finish, max, size}}`, nodes `{next, value,
//! key}`. Keys are runtime IDs: NPCs `0x8…` without `0x4…`, matters `0xC…` (EC_NetDef.h). Each object
//! keeps an info record starting with its own runtime ID and template ID (`CECNPC::INFO {nid, tid}`,
//! `CECMatter::INFO {mid, tid, dropper_id, dir0, dir1, rad}`), and, as a `CECObject : A3DCoordinate`,
//! its facing at +0x2C and position at +0x3C. Dynamic objects are matters whose template ID has the top bit.
//! Both classes keep `bool m_bPhase; short m_iPhaseId;` (set together from the server's phase).
//!
//! The member offsets differ per client build, so [`find_layout`] searches them and accepts a table only
//! when walking it yields exactly its stored count of correctly typed keys, and an info offset only when
//! every sampled object stores its own key there. The phase offset is accepted only when exactly one
//! offset holds the flag/ID pattern and at least two objects in view are phased.

use super::game::{Vec3, DIRECTION, POSITION};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

/// Reads `buffer.len()` bytes at an address of the game process.
pub type Reader<'a> = &'a dyn Fn(u32, &mut [u8]) -> bool;

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn block(read: Reader, address: u32, length: usize) -> Option<Vec<u8>> {
    if address < 0x1_0000 {
        return None;
    }
    let mut bytes = vec![0u8; length];
    read(address, &mut bytes).then_some(bytes)
}

pub fn is_npc_id(id: u32) -> bool {
    id & 0x8000_0000 != 0 && id & 0x4000_0000 == 0
}

pub fn is_matter_id(id: u32) -> bool {
    id & 0xF000_0000 == 0xC000_0000
}

/// One manager's table and the offsets inside its objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableLayout {
    /// The hash table inside the manager.
    pub table: u32,
    /// The info record (runtime ID, template ID, …) inside an object.
    pub info: u32,
    /// `m_bPhase` inside an object (`m_iPhaseId` follows at +2); unknown until phased objects were seen.
    pub phase: Option<u32>,
}

/// Where a client build keeps the world's tables (offsets in bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Layout {
    /// `CECGameRun::m_pWorld`.
    pub world: u32,
    /// `CECWorld::m_aManagers`.
    pub managers: u32,
    pub npcs: Option<TableLayout>,
    pub matters: Option<TableLayout>,
}

/// The (key, value) pairs of a hash table, if `address` holds one whose walk matches its count.
fn table_entries(read: Reader, address: u32, key_ok: fn(u32) -> bool) -> Option<Vec<(u32, u32)>> {
    let header = block(read, address, 24)?;
    let (count, data, finish, max, size) = (u32_at(&header, 4) as usize, u32_at(&header, 8), u32_at(&header, 12), u32_at(&header, 16) as usize, u32_at(&header, 20) as usize);
    if size == 0 || size > 1 << 18 || max < size || count > 200_000 || finish < data || (finish - data) as usize != size * 4 {
        return None;
    }
    let buckets = block(read, data, size * 4)?;
    let mut entries = Vec::with_capacity(count);
    for index in 0..size {
        let mut node = u32_at(&buckets, index * 4);
        let mut chain = 0;
        while node != 0 {
            chain += 1;
            if chain > 256 || entries.len() >= count {
                return None;
            }
            let bytes = block(read, node, 12)?;
            let (next, value, key) = (u32_at(&bytes, 0), u32_at(&bytes, 4), u32_at(&bytes, 8));
            if !key_ok(key) || value < 0x1_0000 {
                return None;
            }
            entries.push((key, value));
            node = next;
        }
    }
    (entries.len() == count).then_some(entries)
}

/// The first offset in a manager object holding a non-empty table of these keys.
fn find_table(read: Reader, manager: u32, key_ok: fn(u32) -> bool) -> Option<(u32, Vec<(u32, u32)>)> {
    let bytes = block(read, manager, 0x400)?;
    (0..0x400 - 24).step_by(4).find_map(|offset| {
        let (count, data, finish, size) = (u32_at(&bytes, offset + 4), u32_at(&bytes, offset + 8), u32_at(&bytes, offset + 12), u32_at(&bytes, offset + 20));
        if count == 0 || size == 0 || data < 0x1_0000 || finish.wrapping_sub(data) != size.wrapping_mul(4) {
            return None;
        }
        let entries = table_entries(read, manager.wrapping_add(offset as u32), key_ok)?;
        Some((offset as u32, entries))
    })
}

/// The offset at which every sampled object stores its own key (the start of its info record).
fn find_info(read: Reader, entries: &[(u32, u32)]) -> Option<u32> {
    const SPAN: usize = 0x1000;
    let mut common: Option<Vec<u32>> = None;
    for &(key, object) in entries.iter().take(12) {
        let bytes = block(read, object, SPAN)?;
        let here: Vec<u32> = (POSITION as usize + 12..SPAN - 8).step_by(4).filter(|&at| u32_at(&bytes, at) == key).map(|at| at as u32).collect();
        common = Some(match common {
            None => here,
            Some(previous) => previous.into_iter().filter(|at| here.contains(at)).collect(),
        });
    }
    common?.first().copied()
}

/// The `m_bPhase` offset: the only one after the info record where every object holds 0 with phase 0
/// or 1 with a phase of 1–30000, and at least two objects are phased.
fn find_phase(read: Reader, entries: &[(u32, u32)], info: u32) -> Option<u32> {
    const WINDOW: usize = 0x800;
    let objects: Vec<Vec<u8>> = entries.iter().take(400).filter_map(|&(_, object)| block(read, object.wrapping_add(info), WINDOW)).collect();
    if objects.len() < 2 {
        return None;
    }
    let mut found = (16..WINDOW - 4).step_by(4).filter(|&at| {
        let mut phased = 0;
        let fits = objects.iter().all(|bytes| {
            let phase = i16::from_le_bytes([bytes[at + 2], bytes[at + 3]]);
            match bytes[at] {
                0 => phase == 0,
                1 if (1..=30000).contains(&phase) => {
                    phased += 1;
                    true
                }
                _ => false,
            }
        });
        fits && phased >= 2
    });
    let first = found.next()?;
    found.next().is_none().then_some(info + first as u32)
}

fn table_layout(read: Reader, manager: u32, key_ok: fn(u32) -> bool) -> Option<TableLayout> {
    let (table, entries) = find_table(read, manager, key_ok)?;
    let info = find_info(read, &entries)?;
    Some(TableLayout { table, info, phase: find_phase(read, &entries, info) })
}

/// Searches the world and table offsets from `CECGameRun` (needs at least one NPC or matter in view).
pub fn find_layout(read: Reader, game_run: u32) -> Option<Layout> {
    let run = block(read, game_run, 0x100)?;
    for world_offset in (0..0x100).step_by(4) {
        let world = u32_at(&run, world_offset);
        let Some(bytes) = block(read, world, 0x2000) else { continue };
        for managers in (0..0x2000 - 24).step_by(4) {
            let pointers: Vec<u32> = (0..6).map(|index| u32_at(&bytes, managers + index * 4)).collect();
            if pointers.iter().any(|&pointer| pointer < 0x1_0000) || (1..6).any(|i| pointers[..i].contains(&pointers[i])) {
                continue;
            }
            let npcs = table_layout(read, pointers[1], is_npc_id);
            let matters = table_layout(read, pointers[2], is_matter_id);
            if npcs.is_some() || matters.is_some() {
                return Some(Layout { world: world_offset as u32, managers: managers as u32, npcs, matters });
            }
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntityKind {
    /// An NPC or monster (elements.data tells which).
    Npc,
    /// A mine, herb, dropped item or money (template ID without the top bit).
    Matter,
    /// A dynamic object (`0x80000000 | dynamic object ID`).
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    pub kind: EntityKind,
    pub runtime_id: u32,
    /// Template ID (for dynamic objects without the top bit).
    pub template: u32,
    pub position: Vec3,
    pub direction: Option<Vec3>,
    /// Matters: the rotation bytes the server sent (axis dir0, dir1 and turn), as npcgen.data stores them.
    pub rotation: Option<[u8; 3]>,
    /// Matters: the object that dropped it (dropped items and money; 0 for spawned ones).
    pub dropper: u32,
    /// None while this client build's phase offset is unknown.
    pub phase: Option<i32>,
}

fn entity(read: Reader, kind: EntityKind, key: u32, object: u32, layout: &TableLayout) -> Option<Entity> {
    let coordinate = block(read, object.wrapping_add(DIRECTION), (POSITION - DIRECTION) as usize + 12)?;
    let record = block(read, object.wrapping_add(layout.info), 16)?;
    if u32_at(&record, 0) != key {
        return None;
    }
    let float = |at: u32| f32::from_le_bytes(coordinate[(at - DIRECTION) as usize..][..4].try_into().unwrap());
    let position = Vec3 { x: float(POSITION), y: float(POSITION + 4), z: float(POSITION + 8) };
    if ![position.x, position.y, position.z].iter().all(|value| value.is_finite() && value.abs() < 100_000.0) {
        return None;
    }
    let direction = Vec3 { x: float(DIRECTION), y: float(DIRECTION + 4), z: float(DIRECTION + 8) };
    let length = (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z).sqrt();
    let direction = (length.is_finite() && (0.5..2.0).contains(&length)).then(|| Vec3 { x: direction.x / length, y: direction.y / length, z: direction.z / length });
    let phase = match layout.phase {
        Some(at) => {
            let bytes = block(read, object.wrapping_add(at), 4)?;
            Some(if bytes[0] == 1 { i16::from_le_bytes([bytes[2], bytes[3]]) as i32 } else { 0 })
        }
        None => None,
    };
    let matter = kind != EntityKind::Npc;
    let raw_template = u32_at(&record, 4);
    Some(Entity {
        kind: if matter && raw_template & 0x8000_0000 != 0 { EntityKind::Dynamic } else { kind },
        runtime_id: key,
        template: raw_template & 0x7fff_ffff,
        position,
        direction,
        rotation: matter.then(|| [record[12], record[13], record[14]]),
        dropper: if matter { u32_at(&record, 8) } else { 0 },
        phase,
    })
}

fn manager(read: Reader, game_run: u32, layout: &Layout, index: u32) -> Result<u32, String> {
    let unreadable = "The game client's memory could not be read";
    let world = block(read, game_run.wrapping_add(layout.world), 4).map(|bytes| u32_at(&bytes, 0)).ok_or(unreadable)?;
    block(read, world.wrapping_add(layout.managers).wrapping_add(index * 4), 4).map(|bytes| u32_at(&bytes, 0)).ok_or_else(|| unreadable.into())
}

/// Everything the client has loaded, for the tables a layout knows.
pub fn collect(read: Reader, game_run: u32, layout: &Layout) -> Result<Vec<Entity>, String> {
    let mut out = Vec::new();
    for (index, table, key_ok, kind) in [(1, layout.npcs, is_npc_id as fn(u32) -> bool, EntityKind::Npc), (2, layout.matters, is_matter_id, EntityKind::Matter)] {
        let Some(table) = table else { continue };
        let manager = manager(read, game_run, layout, index)?;
        let entries = table_entries(read, manager.wrapping_add(table.table), key_ok).unwrap_or_default();
        out.extend(entries.into_iter().filter_map(|(key, object)| entity(read, kind, key, object, &table)));
    }
    Ok(out)
}

/// Layouts found per client exe (lowercase path) during this session.
static LAYOUTS: Mutex<Option<HashMap<String, Layout>>> = Mutex::new(None);

/// Collects with the layout of this exe, searching (again) for whatever part is still unknown.
pub fn fetch(exe: &str, read: Reader, game_run: u32) -> Result<Vec<Entity>, String> {
    let key = exe.to_lowercase();
    let cached = LAYOUTS.lock().map_err(|_| "layout cache poisoned")?.get_or_insert_with(HashMap::new).get(&key).copied();
    let mut layout = match cached {
        Some(layout) if layout.npcs.is_some() && layout.matters.is_some() => layout,
        cached => {
            let found = find_layout(read, game_run);
            match (cached, found) {
                (Some(old), Some(new)) if (old.world, old.managers) == (new.world, new.managers) => Layout { npcs: old.npcs.or(new.npcs), matters: old.matters.or(new.matters), ..old },
                (_, Some(new)) => new,
                (Some(old), None) => old,
                (None, None) => return Err("Nothing to collect yet: no NPC, monster or mine is in view, or this client build is not recognised".into()),
            }
        }
    };
    // Phases show up only where phased objects are in view: keep looking until found.
    for (index, slot, key_ok) in [(1, &mut layout.npcs, is_npc_id as fn(u32) -> bool), (2, &mut layout.matters, is_matter_id)] {
        let Some(table) = slot.as_mut().filter(|table| table.phase.is_none()) else { continue };
        let manager = manager(read, game_run, &Layout { npcs: None, matters: None, ..layout }, index)?;
        if let Some(entries) = table_entries(read, manager.wrapping_add(table.table), key_ok) {
            table.phase = find_phase(read, &entries, table.info);
        }
    }
    LAYOUTS.lock().map_err(|_| "layout cache poisoned")?.get_or_insert_with(HashMap::new).insert(key, layout);
    collect(read, game_run, &layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A tiny fake address space.
    struct Memory(BTreeMap<u32, Vec<u8>>);
    impl Memory {
        fn put(&mut self, address: u32, bytes: &[u8]) {
            self.0.insert(address, bytes.to_vec());
        }
        fn words(&mut self, address: u32, words: &[u32]) {
            self.put(address, &words.iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>());
        }
        fn read(&self, address: u32, buffer: &mut [u8]) -> bool {
            buffer.fill(0);
            // Zero-filled pages around every region, like committed memory.
            let mut any = false;
            for (&start, bytes) in self.0.range(..address.saturating_add(buffer.len() as u32)) {
                let end = start + bytes.len() as u32;
                if end <= address.saturating_sub(0x4000) {
                    continue;
                }
                any = true;
                for (index, &byte) in bytes.iter().enumerate() {
                    let at = start + index as u32;
                    if at >= address && at < address + buffer.len() as u32 {
                        buffer[(at - address) as usize] = byte;
                    }
                }
            }
            any
        }
    }

    fn object(memory: &mut Memory, at: u32, info: u32, record: &[u32], x: f32, phase: Option<u16>) {
        let floats = [0.6f32, 0.0, 0.8, 0.0, x, 50.0, -20.0];
        memory.put(at + DIRECTION, &floats.iter().flat_map(|value| value.to_le_bytes()).collect::<Vec<_>>());
        memory.words(at + info, record);
        if let Some(phase) = phase {
            memory.words(at + info + 0x40, &[1 | (phase as u32) << 16]);
        }
    }

    #[test]
    fn finds_tables_and_reads_entities() {
        let mut memory = Memory(BTreeMap::new());
        let (run, world, npc_man, matter_man) = (0x0100_0000, 0x0200_0000, 0x0300_0000, 0x0400_0000);
        memory.words(run, &[0xdead_0000, 0x0111_0000, world]); // m_pWorld at +8
        let managers = [0x0500_0000, npc_man, matter_man, 0x0600_0000, 0x0700_0000, 0x0800_0000];
        memory.words(world + 0x150, &managers);
        // NPC table at +0x20: two buckets, one with a chain of three nodes.
        memory.words(npc_man + 0x20, &[0, 3, 0x0900_0000, 0x0900_0008, 2, 2]);
        memory.words(0x0900_0000, &[0x0a00_0000, 0]);
        memory.words(0x0a00_0000, &[0x0a00_0100, 0x0b00_0000, 0x8000_1234]);
        memory.words(0x0a00_0100, &[0x0a00_0200, 0x0b10_0000, 0x8000_1235]);
        memory.words(0x0a00_0200, &[0, 0x0b20_0000, 0x8000_1236]);
        object(&mut memory, 0x0b00_0000, 0x2a0, &[0x8000_1234, 3001], 10.0, Some(1628));
        object(&mut memory, 0x0b10_0000, 0x2a0, &[0x8000_1235, 3002], 11.0, Some(1629));
        object(&mut memory, 0x0b20_0000, 0x2a0, &[0x8000_1236, 3003], 9.0, None);
        // Matter table at +0x10: one mine, one dynamic object.
        memory.words(matter_man + 0x10, &[0, 2, 0x0c00_0000, 0x0c00_0004, 1, 1]);
        memory.words(0x0c00_0000, &[0x0d00_0000]);
        memory.words(0x0d00_0000, &[0x0d00_0100, 0x0e00_0000, 0xc000_0001]);
        memory.words(0x0d00_0100, &[0, 0x0e10_0000, 0xc000_0002]);
        object(&mut memory, 0x0e00_0000, 0x1b0, &[0xc000_0001, 18892, 0, 0x0000_40c0], 12.0, None);
        object(&mut memory, 0x0e10_0000, 0x1b0, &[0xc000_0002, 0x8000_009a, 0, 0x001a_0000], 13.0, None);
        let read = |address: u32, buffer: &mut [u8]| memory.read(address, buffer);
        let layout = find_layout(&read, run).unwrap();
        assert_eq!((layout.world, layout.managers), (8, 0x150));
        assert_eq!(layout.npcs, Some(TableLayout { table: 0x20, info: 0x2a0, phase: Some(0x2e0) }));
        // No phased matter in view: its phase stays unknown rather than guessed.
        assert_eq!(layout.matters, Some(TableLayout { table: 0x10, info: 0x1b0, phase: None }));
        let mut found = collect(&read, run, &layout).unwrap();
        found.sort_by_key(|entity| entity.runtime_id);
        let summary: Vec<_> = found.iter().map(|entity| (entity.kind, entity.template, entity.position.x, entity.rotation, entity.phase)).collect();
        assert_eq!(summary, vec![
            (EntityKind::Npc, 3001, 10.0, None, Some(1628)),
            (EntityKind::Npc, 3002, 11.0, None, Some(1629)),
            (EntityKind::Npc, 3003, 9.0, None, Some(0)),
            (EntityKind::Matter, 18892, 12.0, Some([192, 64, 0]), None),
            (EntityKind::Dynamic, 154, 13.0, Some([0, 0, 26]), None),
        ]);
        assert_eq!(found[0].direction, Some(Vec3 { x: 0.6, y: 0.0, z: 0.8 }));
        // A table whose walk does not match its count is rejected.
        memory.words(npc_man + 0x20, &[0, 4, 0x0900_0000, 0x0900_0008, 2, 2]);
        let read = |address: u32, buffer: &mut [u8]| memory.read(address, buffer);
        assert!(table_entries(&read, npc_man + 0x20, is_npc_id).is_none());
    }
}
