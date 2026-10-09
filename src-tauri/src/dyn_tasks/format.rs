//! `dyn_tasks.data`: the dynamic task pack the server sends to clients.
//!
//! Layout (`ATaskTemplMan::UnmarshalDynTasks`, `ATaskTempl::UnmarshalDynTask`):
//!
//! ```text
//! header   u32 pack_size (= file size), i32 time_mark, u16 version (13), u16 task_count
//! task     u32 mask, u32 mask2, u8 dyn type, [u32 special award: top-level tasks of type 1],
//!          u32 id, u8 name length + UTF-16 name, 17 flag bytes, u8 level min, u8 level max,
//!          optional sections in mask order (bits 0–12), u8 method + its goal data, u8 finish type,
//!          award (i32 mask, gold, experience, SP, reputation, item choices),
//!          3 texts (i32 length + UTF-16), 5 talks, i32 subtask count, subtasks
//! ```
//!
//! Item, monster and time records are copied as the client's packed structures (31, 22 and 24
//! bytes). Newer clients (HDN, Reborn) moved the award's item choices from bit 4 to bit 5; the
//! layout is detected per file. The client caches the pack as `userdata\dyn_tasks.data` and
//! replaces it with the server's whenever the time marks differ.

use serde::{Deserialize, Serialize};

pub const VERSION: u16 = 13;
pub const HEADER_SIZE: usize = 12;
const ITEM_SIZE: usize = 31;
const MONSTER_SIZE: usize = 22;

/// Buffer limits of the client's task template (`TaskTempl.h`, `ExpTypes.h`).
pub const MAX_NAME: usize = 29;
pub const MAX_SHORT_TEXT: usize = 63;
pub const MAX_PREMISE_TASKS: usize = 5;
pub const MAX_MUTEX_TASKS: usize = 5;
pub const MAX_OCCUPATIONS: usize = 45;
pub const MAX_TIMETABLE: usize = 12;
pub const MAX_MONSTERS: usize = 3;
pub const MAX_ITEMS_WANTED: usize = 8;
pub const MAX_CANDIDATES: usize = 16;
pub const MAX_AWARD_ITEMS: usize = 32;
pub const FLAG_COUNT: usize = 17;
pub const TALK_COUNT: usize = 5;

/// Dynamic task types (`enumDTT*`).
pub const TYPE_SPECIAL_AWARD: u8 = 1;
/// Methods with data in the pack (`enumTM*`).
pub const METHOD_KILL: u8 = 1;
pub const METHOD_COLLECT: u8 = 2;
pub const METHOD_REACH_SITE: u8 = 4;
pub const METHOD_WAIT: u8 = 5;
pub const METHOD_LEAVE_SITE: u8 = 13;

/// Where the award stores its item choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwardLayout {
    /// Bit 4 (XtremeJade, 1559, ForsakenJD).
    Classic,
    /// Bit 5; bit 4 holds a value of unknown size that no known pack uses (HDN, Reborn).
    Shifted,
    /// No task has item choices, so the file does not tell.
    Unknown,
}

impl AwardLayout {
    fn candidate_bit(self) -> Option<u32> {
        match self {
            Self::Classic => Some(1 << 4),
            Self::Shifted => Some(1 << 5),
            Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vert {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaskTime {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    pub weekday: i32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemWanted {
    pub item_id: u32,
    pub common_item: u8,
    pub amount: u32,
    pub probability: f32,
    pub bound: u8,
    pub period: i32,
    pub timetable: u8,
    pub day_of_week: u8,
    pub hour: u8,
    pub minute: u8,
    pub refine_condition: u8,
    pub refine_level: u32,
    pub replacement_item_id: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonsterWanted {
    pub monster_id: u32,
    pub amount: u32,
    pub drop_item_id: u32,
    pub drop_item_amount: u32,
    pub drop_common_item: u8,
    pub drop_probability: f32,
    pub killer_level: u8,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Zone {
    pub flag: u8,
    pub world: u32,
    pub min: Vert,
    pub max: Vert,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Transfer {
    pub flag: u8,
    pub world: u32,
    pub point: Vert,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GivenItems {
    /// Recounted from the items when a task is written.
    pub common_count: u8,
    pub task_count: u8,
    pub items: Vec<ItemWanted>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TimetableEntry {
    pub kind: u8,
    pub start: TaskTime,
    pub end: TaskTime,
}

/// What the task asks for: `method` decides which of the other fields are stored.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub method: u8,
    /// Kill monsters.
    pub monsters: Vec<MonsterWanted>,
    /// Collect items (and gold).
    pub items: Vec<ItemWanted>,
    pub gold: i32,
    /// Reach or leave a site.
    pub site_id: u32,
    pub site_min: Vert,
    pub site_max: Vert,
    /// Wait (seconds).
    pub wait: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Candidate {
    pub random: u8,
    pub items: Vec<ItemWanted>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Award {
    pub gold: Option<u32>,
    pub experience: Option<u64>,
    pub sp: Option<u32>,
    pub reputation: Option<i32>,
    pub candidates: Option<Vec<Candidate>>,
    /// Mask bits the reader skips (kept as they were).
    pub extra_mask: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DynOption {
    /// A window ID, or `0x80000000 | function`.
    pub id: u32,
    pub param: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DynWindow {
    /// Stored as one signed byte.
    pub id: i8,
    pub parent: i8,
    /// As stored: official packs end it with a NUL.
    pub text: String,
    pub options: Vec<DynOption>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DynTalk {
    pub prompt: String,
    pub windows: Vec<DynWindow>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynTask {
    pub dyn_type: u8,
    /// Stored for top-level tasks of the special award type: the number a player must have been given.
    pub special_award: u32,
    pub id: u32,
    pub name: String,
    pub flags: Vec<u8>,
    pub level_min: u8,
    pub level_max: u8,
    pub time_limit: Option<u32>,
    pub reputation: Option<i32>,
    pub period: Option<u16>,
    pub premise_items: Option<Vec<ItemWanted>>,
    pub zone: Option<Zone>,
    pub transfer: Option<Transfer>,
    pub given_items: Option<GivenItems>,
    pub deposit: Option<u32>,
    pub premise_tasks: Option<Vec<u32>>,
    pub gender: Option<u8>,
    pub occupations: Option<Vec<u32>>,
    pub mutex_tasks: Option<Vec<u32>>,
    pub timetable: Option<Vec<TimetableEntry>>,
    pub goal: Goal,
    pub finish_type: u8,
    pub award: Award,
    pub description: String,
    pub ok_text: String,
    pub no_text: String,
    pub talks: Vec<DynTalk>,
    pub subtasks: Vec<DynTask>,
    /// Mask bits the reader skips, and the unused second mask (kept as they were).
    pub extra_mask: u32,
    pub mask2: u32,
}

impl DynTask {
    /// This task and every subtask, depth first.
    pub fn walk(&self) -> Vec<&DynTask> {
        let mut out = vec![self];
        for child in &self.subtasks {
            out.extend(child.walk());
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub pack_size: u32,
    pub time_mark: i32,
    pub version: u16,
    pub task_count: u16,
}

pub fn read_header(data: &[u8]) -> Result<Header, String> {
    if data.len() < HEADER_SIZE {
        return Err("The file is too short for a dyn_tasks.data header".into());
    }
    let header = Header {
        pack_size: u32::from_le_bytes(data[0..4].try_into().unwrap()),
        time_mark: i32::from_le_bytes(data[4..8].try_into().unwrap()),
        version: u16::from_le_bytes(data[8..10].try_into().unwrap()),
        task_count: u16::from_le_bytes(data[10..12].try_into().unwrap()),
    };
    if header.version != VERSION {
        return Err(format!("dyn_tasks.data version {} is not supported (the client reads version {VERSION})", header.version));
    }
    if header.pack_size as usize != data.len() {
        return Err(format!("The header says {} bytes but the file has {} (the client refuses it)", header.pack_size, data.len()));
    }
    Ok(header)
}

pub fn write_header(header: &Header) -> [u8; HEADER_SIZE] {
    let mut out = [0u8; HEADER_SIZE];
    out[0..4].copy_from_slice(&header.pack_size.to_le_bytes());
    out[4..8].copy_from_slice(&header.time_mark.to_le_bytes());
    out[8..10].copy_from_slice(&header.version.to_le_bytes());
    out[10..12].copy_from_slice(&header.task_count.to_le_bytes());
    out
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize, what: &str) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(len).filter(|end| *end <= self.data.len()).ok_or_else(|| format!("{what} needs {len} bytes at offset {} but the data ends at {}", self.at, self.data.len()))?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }
    fn u8(&mut self, what: &str) -> Result<u8, String> { Ok(self.take(1, what)?[0]) }
    fn i8(&mut self, what: &str) -> Result<i8, String> { Ok(self.u8(what)? as i8) }
    fn u16(&mut self, what: &str) -> Result<u16, String> { Ok(u16::from_le_bytes(self.take(2, what)?.try_into().unwrap())) }
    fn u32(&mut self, what: &str) -> Result<u32, String> { Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap())) }
    fn i32(&mut self, what: &str) -> Result<i32, String> { Ok(i32::from_le_bytes(self.take(4, what)?.try_into().unwrap())) }
    fn u64(&mut self, what: &str) -> Result<u64, String> { Ok(u64::from_le_bytes(self.take(8, what)?.try_into().unwrap())) }
    fn f32(&mut self, what: &str) -> Result<f32, String> { Ok(f32::from_le_bytes(self.take(4, what)?.try_into().unwrap())) }
    fn units(&mut self, count: usize, what: &str) -> Result<String, String> {
        let bytes = self.take(count * 2, what)?;
        let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        String::from_utf16(&units).map_err(|_| format!("{what} is not valid UTF-16"))
    }
    /// A text stored by byte size (talk prompts and option texts).
    fn sized_text(&mut self, what: &str) -> Result<String, String> {
        let size = self.i32(what)?;
        if size < 0 || size % 2 != 0 {
            return Err(format!("{what} has an invalid size {size}"));
        }
        self.units(size as usize / 2, what)
    }
    fn vert(&mut self, what: &str) -> Result<Vert, String> {
        Ok(Vert { x: self.f32(what)?, y: self.f32(what)?, z: self.f32(what)? })
    }
    fn time(&mut self, what: &str) -> Result<TaskTime, String> {
        Ok(TaskTime { year: self.i32(what)?, month: self.i32(what)?, day: self.i32(what)?, hour: self.i32(what)?, minute: self.i32(what)?, weekday: self.i32(what)? })
    }
    fn item(&mut self) -> Result<ItemWanted, String> {
        let what = "an item";
        let start = self.at;
        let item = ItemWanted {
            item_id: self.u32(what)?,
            common_item: self.u8(what)?,
            amount: self.u32(what)?,
            probability: self.f32(what)?,
            bound: self.u8(what)?,
            period: self.i32(what)?,
            timetable: self.u8(what)?,
            day_of_week: self.u8(what)?,
            hour: self.u8(what)?,
            minute: self.u8(what)?,
            refine_condition: self.u8(what)?,
            refine_level: self.u32(what)?,
            replacement_item_id: self.u32(what)?,
        };
        debug_assert_eq!(self.at - start, ITEM_SIZE);
        Ok(item)
    }
    fn items(&mut self, count: usize) -> Result<Vec<ItemWanted>, String> {
        (0..count).map(|_| self.item()).collect()
    }
    fn monster(&mut self) -> Result<MonsterWanted, String> {
        let what = "a monster";
        let start = self.at;
        let monster = MonsterWanted {
            monster_id: self.u32(what)?,
            amount: self.u32(what)?,
            drop_item_id: self.u32(what)?,
            drop_item_amount: self.u32(what)?,
            drop_common_item: self.u8(what)?,
            drop_probability: self.f32(what)?,
            killer_level: self.u8(what)?,
        };
        debug_assert_eq!(self.at - start, MONSTER_SIZE);
        Ok(monster)
    }
    fn ids(&mut self, what: &str) -> Result<Vec<u32>, String> {
        let count = self.u8(what)? as usize;
        (0..count).map(|_| self.u32(what)).collect()
    }
}

/// Reads one task (with its subtasks) at the reader's position.
fn read_task(reader: &mut Reader, top: bool, layout: AwardLayout, depth: usize) -> Result<DynTask, String> {
    if depth > 16 {
        return Err("Subtasks are nested too deeply".into());
    }
    let mut task = DynTask::default();
    let mask = reader.u32("the task mask")?;
    task.mask2 = reader.u32("the second task mask")?;
    task.extra_mask = mask & !0x1fff;
    task.dyn_type = reader.u8("the dynamic task type")?;
    if top && task.dyn_type == TYPE_SPECIAL_AWARD {
        task.special_award = reader.u32("the special award")?;
    }
    task.id = reader.u32("the task ID")?;
    let name_length = reader.u8("the name length")? as i8;
    if name_length < 0 {
        return Err(format!("Task {}: invalid name length", task.id));
    }
    task.name = reader.units(name_length as usize, "the name")?;
    task.flags = reader.take(FLAG_COUNT, "the flags")?.to_vec();
    task.level_min = reader.u8("the minimum level")?;
    task.level_max = reader.u8("the maximum level")?;
    let bit = |index: u32| mask & (1 << index) != 0;
    if bit(0) { task.time_limit = Some(reader.u32("the time limit")?); }
    if bit(1) { task.reputation = Some(reader.i32("the reputation")?); }
    if bit(2) { task.period = Some(reader.u16("the period")?); }
    if bit(3) {
        let count = reader.u8("the premise items")? as usize;
        task.premise_items = Some(reader.items(count)?);
    }
    if bit(4) {
        task.zone = Some(Zone { flag: reader.u8("the zone")?, world: reader.u32("the zone")?, min: reader.vert("the zone")?, max: reader.vert("the zone")? });
    }
    if bit(5) {
        task.transfer = Some(Transfer { flag: reader.u8("the transfer")?, world: reader.u32("the transfer")?, point: reader.vert("the transfer")? });
    }
    if bit(6) {
        let count = reader.u8("the given items")? as usize;
        let common_count = reader.u8("the given items")?;
        let task_count = reader.u8("the given items")?;
        task.given_items = Some(GivenItems { common_count, task_count, items: reader.items(count)? });
    }
    if bit(7) { task.deposit = Some(reader.u32("the deposit")?); }
    if bit(8) { task.premise_tasks = Some(reader.ids("the premise tasks")?); }
    if bit(9) { task.gender = Some(reader.u8("the gender")?); }
    if bit(10) { task.occupations = Some(reader.ids("the classes")?); }
    if bit(11) { task.mutex_tasks = Some(reader.ids("the mutex tasks")?); }
    if bit(12) {
        let count = reader.u8("the timetable")? as usize;
        let kinds = reader.take(count, "the timetable")?.to_vec();
        let starts: Vec<TaskTime> = (0..count).map(|_| reader.time("the timetable")).collect::<Result<_, _>>()?;
        let ends: Vec<TaskTime> = (0..count).map(|_| reader.time("the timetable")).collect::<Result<_, _>>()?;
        task.timetable = Some(kinds.into_iter().zip(starts).zip(ends).map(|((kind, start), end)| TimetableEntry { kind, start, end }).collect());
    }
    task.goal.method = reader.u8("the method")?;
    match task.goal.method {
        METHOD_KILL => {
            let count = reader.u8("the monsters")? as usize;
            task.goal.monsters = (0..count).map(|_| reader.monster()).collect::<Result<_, _>>()?;
        }
        METHOD_COLLECT => {
            let count = reader.u8("the items wanted")? as usize;
            task.goal.items = reader.items(count)?;
            task.goal.gold = reader.i32("the gold wanted")?;
        }
        METHOD_REACH_SITE | METHOD_LEAVE_SITE => {
            task.goal.site_id = reader.u32("the site")?;
            task.goal.site_min = reader.vert("the site")?;
            task.goal.site_max = reader.vert("the site")?;
        }
        METHOD_WAIT => task.goal.wait = reader.u32("the wait time")?,
        _ => {}
    }
    task.finish_type = reader.u8("the finish type")?;
    let award_mask = reader.i32("the award mask")? as u32;
    let candidate_bit = layout.candidate_bit();
    let known = 0b1111 | candidate_bit.unwrap_or(0);
    if layout == AwardLayout::Shifted && award_mask & (1 << 4) != 0 {
        return Err(format!("Task {}: the award uses a field of unknown size (bit 4)", task.id));
    }
    if layout == AwardLayout::Unknown && award_mask & 0b11_0000 != 0 {
        return Err(format!("Task {}: the award has item choices, but the layout is not known", task.id));
    }
    task.award.extra_mask = award_mask & !known;
    if award_mask & 1 != 0 { task.award.gold = Some(reader.u32("the award gold")?); }
    if award_mask & 2 != 0 { task.award.experience = Some(reader.u64("the award experience")?); }
    if award_mask & 4 != 0 { task.award.sp = Some(reader.u32("the award SP")?); }
    if award_mask & 8 != 0 { task.award.reputation = Some(reader.i32("the award reputation")?); }
    if candidate_bit.is_some_and(|bit| award_mask & bit != 0) {
        let count = reader.u8("the award choices")? as usize;
        let mut candidates = Vec::with_capacity(count);
        for _ in 0..count {
            let random = reader.u8("an award choice")?;
            let items = reader.u8("an award choice")? as usize;
            candidates.push(Candidate { random, items: reader.items(items)? });
        }
        task.award.candidates = Some(candidates);
    }
    let text = |reader: &mut Reader, what: &str| -> Result<String, String> {
        let length = reader.i32(what)?;
        if length < 0 { return Err(format!("{what} has an invalid length")); }
        reader.units(length as usize, what)
    };
    task.description = text(reader, "the description")?;
    task.ok_text = text(reader, "the success text")?;
    task.no_text = text(reader, "the failure text")?;
    for _ in 0..TALK_COUNT {
        let prompt = reader.sized_text("a talk prompt")?;
        let window_count = reader.u8("a talk")? as usize;
        let mut windows = Vec::with_capacity(window_count);
        for _ in 0..window_count {
            let id = reader.i8("a window")?;
            let parent = reader.i8("a window")?;
            let length = reader.i32("a window text")?;
            if length < 0 { return Err("A window text has an invalid length".into()); }
            let text = reader.units(length as usize, "a window text")?;
            let option_count = reader.u8("a window")? as usize;
            let mut options = Vec::with_capacity(option_count);
            for _ in 0..option_count {
                let id = reader.u32("an option")?;
                let param = reader.u32("an option")?;
                options.push(DynOption { id, param, text: reader.sized_text("an option text")? });
            }
            windows.push(DynWindow { id, parent, text, options });
        }
        task.talks.push(DynTalk { prompt, windows });
    }
    let subtasks = reader.i32("the subtask count")?;
    if !(0..=10_000).contains(&subtasks) {
        return Err(format!("Task {}: invalid subtask count {subtasks}", task.id));
    }
    for _ in 0..subtasks {
        task.subtasks.push(read_task(reader, false, layout, depth + 1)?);
    }
    Ok(task)
}

/// A pack: its header, award layout, and every top-level task with its own bytes.
pub struct Pack {
    pub header: Header,
    pub layout: AwardLayout,
    pub tasks: Vec<(DynTask, Vec<u8>)>,
}

fn read_with(data: &[u8], header: Header, layout: AwardLayout) -> Result<Vec<(DynTask, Vec<u8>)>, String> {
    let mut reader = Reader { data, at: HEADER_SIZE };
    let mut tasks = Vec::with_capacity(header.task_count as usize);
    for index in 0..header.task_count {
        let start = reader.at;
        let task = read_task(&mut reader, true, layout, 0).map_err(|error| format!("Task {} of {}: {error}", index + 1, header.task_count))?;
        tasks.push((task, data[start..reader.at].to_vec()));
    }
    if reader.at != data.len() {
        return Err(format!("{} bytes are left after the last task", data.len() - reader.at));
    }
    Ok(tasks)
}

pub fn read(data: &[u8]) -> Result<Pack, String> {
    let header = read_header(data)?;
    // Try both award layouts; a file whose tasks have no item choices reads the same with either.
    let classic = read_with(data, header, AwardLayout::Classic);
    let shifted = read_with(data, header, AwardLayout::Shifted);
    let (layout, tasks) = match (classic, shifted) {
        // Both read only when no task sets bit 4 or 5.
        (Ok(_), Ok(_)) => (AwardLayout::Unknown, read_with(data, header, AwardLayout::Unknown)?),
        (Ok(classic), Err(_)) => (AwardLayout::Classic, classic),
        (Err(_), Ok(shifted)) => (AwardLayout::Shifted, shifted),
        (Err(error), Err(_)) => return Err(error),
    };
    Ok(Pack { header, layout, tasks })
}

struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn u8(&mut self, value: u8) { self.out.push(value); }
    fn u16(&mut self, value: u16) { self.out.extend_from_slice(&value.to_le_bytes()); }
    fn u32(&mut self, value: u32) { self.out.extend_from_slice(&value.to_le_bytes()); }
    fn i32(&mut self, value: i32) { self.out.extend_from_slice(&value.to_le_bytes()); }
    fn f32(&mut self, value: f32) { self.out.extend_from_slice(&value.to_le_bytes()); }
    fn units(&mut self, text: &str) { for unit in text.encode_utf16() { self.u16(unit); } }
    fn vert(&mut self, vert: &Vert) { self.f32(vert.x); self.f32(vert.y); self.f32(vert.z); }
    fn time(&mut self, time: &TaskTime) {
        for value in [time.year, time.month, time.day, time.hour, time.minute, time.weekday] { self.i32(value); }
    }
    fn item(&mut self, item: &ItemWanted) {
        self.u32(item.item_id);
        self.u8(item.common_item);
        self.u32(item.amount);
        self.f32(item.probability);
        self.u8(item.bound);
        self.i32(item.period);
        self.u8(item.timetable);
        self.u8(item.day_of_week);
        self.u8(item.hour);
        self.u8(item.minute);
        self.u8(item.refine_condition);
        self.u32(item.refine_level);
        self.u32(item.replacement_item_id);
    }
    fn count(&mut self, count: usize, max: usize, what: &str) -> Result<(), String> {
        if count > max {
            return Err(format!("{what}: {count} entries, at most {max}"));
        }
        self.u8(count as u8);
        Ok(())
    }
}

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

fn write_task(writer: &mut Writer, task: &DynTask, top: bool, layout: AwardLayout, depth: usize) -> Result<(), String> {
    let at = |message: String| format!("Task {} ({}): {message}", task.id, task.name);
    if depth > 16 {
        return Err(at("subtasks are nested too deeply".into()));
    }
    if task.id == 0 {
        return Err(at("the ID must not be 0".into()));
    }
    if units(&task.name) > MAX_NAME || task.name.contains('\0') {
        return Err(at(format!("the name must be at most {MAX_NAME} characters")));
    }
    if task.flags.len() != FLAG_COUNT {
        return Err(at(format!("expected {FLAG_COUNT} flags")));
    }
    if top && task.dyn_type == TYPE_SPECIAL_AWARD && task.special_award == 0 {
        return Err(at("a special award task needs a special award number (the official tools drop it otherwise)".into()));
    }
    if task.talks.len() != TALK_COUNT {
        return Err(at(format!("expected {TALK_COUNT} talks")));
    }
    let mut mask = task.extra_mask & !0x1fff;
    let sections = [task.time_limit.is_some(), task.reputation.is_some(), task.period.is_some(), task.premise_items.is_some(), task.zone.is_some(), task.transfer.is_some(), task.given_items.is_some(), task.deposit.is_some(), task.premise_tasks.is_some(), task.gender.is_some(), task.occupations.is_some(), task.mutex_tasks.is_some(), task.timetable.is_some()];
    for (bit, present) in sections.iter().enumerate() {
        if *present { mask |= 1 << bit; }
    }
    writer.u32(mask);
    writer.u32(task.mask2);
    writer.u8(task.dyn_type);
    if top && task.dyn_type == TYPE_SPECIAL_AWARD {
        writer.u32(task.special_award);
    }
    writer.u32(task.id);
    writer.u8(units(&task.name) as u8);
    writer.units(&task.name);
    writer.out.extend_from_slice(&task.flags);
    writer.u8(task.level_min);
    writer.u8(task.level_max);
    if let Some(value) = task.time_limit { writer.u32(value); }
    if let Some(value) = task.reputation { writer.i32(value); }
    if let Some(value) = task.period { writer.u16(value); }
    if let Some(items) = &task.premise_items {
        writer.count(items.len(), 255, &at("premise items".into()))?;
        for item in items { writer.item(item); }
    }
    if let Some(zone) = &task.zone {
        writer.u8(zone.flag);
        writer.u32(zone.world);
        writer.vert(&zone.min);
        writer.vert(&zone.max);
    }
    if let Some(transfer) = &task.transfer {
        writer.u8(transfer.flag);
        writer.u32(transfer.world);
        writer.vert(&transfer.point);
    }
    if let Some(given) = &task.given_items {
        writer.count(given.items.len(), 255, &at("given items".into()))?;
        let common = given.items.iter().filter(|item| item.common_item != 0).count();
        writer.u8(common as u8);
        writer.u8((given.items.len() - common) as u8);
        for item in &given.items { writer.item(item); }
    }
    if let Some(value) = task.deposit { writer.u32(value); }
    let ids = |writer: &mut Writer, list: &Option<Vec<u32>>, max: usize, what: &str| -> Result<(), String> {
        if let Some(ids) = list {
            writer.count(ids.len(), max, &at(what.into()))?;
            for id in ids { writer.u32(*id); }
        }
        Ok(())
    };
    ids(writer, &task.premise_tasks, MAX_PREMISE_TASKS, "premise tasks")?;
    if let Some(value) = task.gender { writer.u8(value); }
    ids(writer, &task.occupations, MAX_OCCUPATIONS, "classes")?;
    ids(writer, &task.mutex_tasks, MAX_MUTEX_TASKS, "mutex tasks")?;
    if let Some(entries) = &task.timetable {
        writer.count(entries.len(), MAX_TIMETABLE, &at("timetable".into()))?;
        for entry in entries { writer.u8(entry.kind); }
        for entry in entries { writer.time(&entry.start); }
        for entry in entries { writer.time(&entry.end); }
    }
    writer.u8(task.goal.method);
    match task.goal.method {
        METHOD_KILL => {
            writer.count(task.goal.monsters.len(), MAX_MONSTERS, &at("monsters to kill".into()))?;
            for monster in &task.goal.monsters {
                writer.u32(monster.monster_id);
                writer.u32(monster.amount);
                writer.u32(monster.drop_item_id);
                writer.u32(monster.drop_item_amount);
                writer.u8(monster.drop_common_item);
                writer.f32(monster.drop_probability);
                writer.u8(monster.killer_level);
            }
        }
        METHOD_COLLECT => {
            writer.count(task.goal.items.len(), MAX_ITEMS_WANTED, &at("items to collect".into()))?;
            for item in &task.goal.items { writer.item(item); }
            writer.i32(task.goal.gold);
        }
        METHOD_REACH_SITE | METHOD_LEAVE_SITE => {
            writer.u32(task.goal.site_id);
            writer.vert(&task.goal.site_min);
            writer.vert(&task.goal.site_max);
        }
        METHOD_WAIT => writer.u32(task.goal.wait),
        _ => {}
    }
    writer.u8(task.finish_type);
    let award = &task.award;
    let mut award_mask = award.extra_mask;
    if award.gold.is_some() { award_mask |= 1; }
    if award.experience.is_some() { award_mask |= 2; }
    if award.sp.is_some() { award_mask |= 4; }
    if award.reputation.is_some() { award_mask |= 8; }
    if award.candidates.is_some() {
        award_mask |= layout.candidate_bit().ok_or_else(|| at("this file does not show where item rewards are stored (no task has any); add them to a pack that already has item rewards".into()))?;
    }
    writer.i32(award_mask as i32);
    if let Some(value) = award.gold { writer.u32(value); }
    if let Some(value) = award.experience { writer.out.extend_from_slice(&value.to_le_bytes()); }
    if let Some(value) = award.sp { writer.u32(value); }
    if let Some(value) = award.reputation { writer.i32(value); }
    if let Some(candidates) = &award.candidates {
        writer.count(candidates.len(), MAX_CANDIDATES, &at("reward choices".into()))?;
        for candidate in candidates {
            writer.u8(candidate.random);
            writer.count(candidate.items.len(), MAX_AWARD_ITEMS, &at("reward items".into()))?;
            for item in &candidate.items { writer.item(item); }
        }
    }
    for text in [&task.description, &task.ok_text, &task.no_text] {
        writer.i32(units(text) as i32);
        writer.units(text);
    }
    for talk in &task.talks {
        if units(&talk.prompt) > MAX_SHORT_TEXT {
            return Err(at(format!("a talk prompt is longer than {MAX_SHORT_TEXT} characters")));
        }
        writer.i32(units(&talk.prompt) as i32 * 2);
        writer.units(&talk.prompt);
        writer.count(talk.windows.len(), 127, &at("talk windows".into()))?;
        for window in &talk.windows {
            writer.u8(window.id as u8);
            writer.u8(window.parent as u8);
            writer.i32(units(&window.text) as i32);
            writer.units(&window.text);
            writer.count(window.options.len(), 255, &at("window options".into()))?;
            for option in &window.options {
                if units(&option.text) > MAX_SHORT_TEXT {
                    return Err(at(format!("option \"{}\" is longer than {MAX_SHORT_TEXT} characters", option.text)));
                }
                writer.u32(option.id);
                writer.u32(option.param);
                writer.i32(units(&option.text) as i32 * 2);
                writer.units(&option.text);
            }
        }
    }
    writer.i32(task.subtasks.len() as i32);
    for child in &task.subtasks {
        write_task(writer, child, false, layout, depth + 1)?;
    }
    Ok(())
}

/// The bytes of one top-level task.
pub fn write_task_bytes(task: &DynTask, layout: AwardLayout) -> Result<Vec<u8>, String> {
    let mut writer = Writer { out: Vec::new() };
    write_task(&mut writer, task, true, layout, 0)?;
    // What the client will read back must be exactly this task.
    let mut reader = Reader { data: &writer.out, at: 0 };
    let back = read_task(&mut reader, true, layout, 0)?;
    if reader.at != writer.out.len() || !same_task(&back, task) {
        return Err(format!("Task {}: the written bytes do not read back as the same task", task.id));
    }
    Ok(writer.out)
}

/// Equal apart from the given-item counts, which writing recounts.
fn same_task(a: &DynTask, b: &DynTask) -> bool {
    let normalize = |task: &DynTask| {
        let mut task = task.clone();
        fn clear(task: &mut DynTask) {
            if let Some(given) = &mut task.given_items { given.common_count = 0; given.task_count = 0; }
            task.subtasks.iter_mut().for_each(clear);
        }
        clear(&mut task);
        task
    };
    normalize(a) == normalize(b)
}

/// The whole file.
pub fn write(time_mark: i32, tasks: &[&[u8]]) -> Result<Vec<u8>, String> {
    if tasks.len() > u16::MAX as usize {
        return Err(format!("A pack holds at most {} tasks", u16::MAX));
    }
    let size = HEADER_SIZE + tasks.iter().map(|task| task.len()).sum::<usize>();
    let header = Header { pack_size: u32::try_from(size).map_err(|_| "The pack is too large")?, time_mark, version: VERSION, task_count: tasks.len() as u16 };
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&write_header(&header));
    for task in tasks {
        out.extend_from_slice(task);
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SAMPLES: [&str; 5] = [
        "E:/Games/XtremeJade/element/data/dyn_tasks.data",
        "E:/Game Dev/JD/1559/gamed/config/dyn_tasks.data",
        "E:/Games/ForsakenJD/element/data/dyn_tasks.data",
        "E:/Games/Elite Jade Dynasty - HDN/element/data/dyn_tasks.data",
        "E:/Games/Jade Dynasty Reborn/element/data/dyn_tasks.data",
    ];

    #[test]
    fn real_packs_read_and_write_byte_for_byte() {
        for path in SAMPLES {
            let Ok(data) = std::fs::read(path) else { continue };
            let pack = read(&data).unwrap_or_else(|error| panic!("{path}: {error}"));
            assert_eq!(pack.tasks.len(), pack.header.task_count as usize);
            let expected = if path.contains("HDN") || path.contains("Reborn") { AwardLayout::Shifted } else { AwardLayout::Classic };
            assert_eq!(pack.layout, expected, "{path}");
            for (task, bytes) in &pack.tasks {
                assert_eq!(&write_task_bytes(task, pack.layout).unwrap(), bytes, "{path}: task {}", task.id);
            }
            let parts: Vec<&[u8]> = pack.tasks.iter().map(|(_, bytes)| bytes.as_slice()).collect();
            assert_eq!(write(pack.header.time_mark, &parts).unwrap(), data, "{path}");
        }
    }

    #[test]
    fn writing_checks_the_client_limits() {
        let Ok(data) = std::fs::read(SAMPLES[2]) else { return };
        let pack = read(&data).unwrap();
        let mut task = pack.tasks[0].0.clone();
        task.name = "x".repeat(MAX_NAME + 1);
        assert!(write_task_bytes(&task, pack.layout).is_err());
        let mut task = pack.tasks[0].0.clone();
        task.premise_tasks = Some(vec![1; MAX_PREMISE_TASKS + 1]);
        assert!(write_task_bytes(&task, pack.layout).is_err());
        let mut task = pack.tasks[0].0.clone();
        task.special_award = 0;
        assert!(write_task_bytes(&task, pack.layout).is_err());
        let mut task = pack.tasks[0].0.clone();
        task.time_limit = Some(3600);
        task.gender = Some(1);
        let bytes = write_task_bytes(&task, pack.layout).unwrap();
        assert_eq!(bytes.len(), pack.tasks[0].1.len() + 5);
    }
}
