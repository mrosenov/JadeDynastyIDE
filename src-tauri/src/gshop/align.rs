//! Finding where a shop file of unknown layout keeps the fields a known layout reads.
//!
//! A reference file the editor reads (its layout) and the unknown file are split into items. Items pair by
//! the first eight bytes (item ID and count, the same in every known build). Each field of the reference
//! layout is scored at every position of the unknown record: how often paired items hold the same value
//! there, and whether the values look alike (zeros, ranges, distinct values, texts). A dynamic program then
//! lays the fields out end to end, keeping, dropping or inserting unknown bytes, so they fill the record
//! exactly. The result is a layout proposal with the changes and a score per field.

use std::collections::HashMap;

use serde::Serialize;

use super::layout::{Field, FieldType, Layout};
use super::{parse, split};

/// Items compared at most (spread over the file).
const SAMPLE: usize = 400;
/// Cost of dropping a field, of an insertion and per inserted byte; kept fields gain `score - KEEP`.
const DROP: f64 = 0.2;
const INSERT: f64 = 1.0;
const PER_BYTE: f64 = 0.0005;
/// Ties go to later insertions (new client builds append fields).
const LATE: f64 = 0.000001;
const KEEP: f64 = 0.5;
/// Units of a text compared and checked for plausibility.
const TEXT_UNITS: usize = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldMatch {
    pub name: String,
    /// Where the reference layout has it, and where it was found (none: not in the file).
    pub reference_offset: usize,
    pub offset: Option<usize>,
    pub size: usize,
    /// 0–1: how well it fits there.
    pub score: f64,
    /// Share of paired items with the same value there (none: too few items to tell).
    pub same: Option<f64>,
    /// The reference file rarely fills it, so its place follows its neighbours (a guess).
    pub guessed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutChange {
    /// `inserted` (bytes the reference layout does not have) or `removed` (a field the file lacks).
    pub kind: &'static str,
    /// Offset in the file's record (removed: where the field would have been).
    pub offset: usize,
    pub size: usize,
    pub name: String,
    /// The field it follows (none: the record start).
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutProposal {
    /// The proposed fields (the reference layout's, with insertions and without removed ones).
    pub fields: Vec<Field>,
    pub reference_layout: String,
    pub reference_size: usize,
    pub record_size: usize,
    pub items: usize,
    pub pairs: usize,
    pub matches: Vec<FieldMatch>,
    pub changes: Vec<LayoutChange>,
}

enum Kind {
    Number,
    Text16,
    Text8,
    Blob,
}

fn kind_of(ty: &FieldType) -> Kind {
    match ty {
        FieldType::Wstr { .. } => Kind::Text16,
        FieldType::Str { .. } => Kind::Text8,
        FieldType::Bytes { .. } | FieldType::Group { .. } => Kind::Blob,
        _ => Kind::Number,
    }
}

/// Evenly spread indexes, at most `limit`.
fn spread(count: usize, limit: usize) -> Vec<usize> {
    if count <= limit {
        return (0..count).collect();
    }
    (0..limit).map(|n| n * count / limit).collect()
}

/// How a slot reads as text: 0 empty, 1 a plausible text, 2 anything else.
fn text_class(slot: &[u8], wide: bool) -> u8 {
    let unit = |index: usize| if wide { u16::from_le_bytes([slot[index * 2], slot[index * 2 + 1]]) } else { slot[index] as u16 };
    let units = if wide { slot.len() / 2 } else { slot.len() };
    if units == 0 || unit(0) == 0 {
        return 0;
    }
    for index in 0..units.min(TEXT_UNITS) {
        let value = unit(index);
        if value == 0 {
            return 1;
        }
        // Tabs and real line breaks occur in texts (some shops store CR LF in descriptions).
        if value < 0x20 && ![b'\t', b'\r', b'\n'].contains(&(value as u8)) {
            return 2;
        }
    }
    // Longer texts must still end within the slot.
    if (TEXT_UNITS..units).any(|index| unit(index) == 0) { 1 } else { 2 }
}

/// The bytes of a text up to and including its terminator (at most `TEXT_UNITS` units).
fn text_prefix(slot: &[u8], wide: bool) -> &[u8] {
    let step = if wide { 2 } else { 1 };
    let limit = slot.len().min(TEXT_UNITS * step);
    let mut at = 0;
    while at + step <= limit {
        if slot[at..at + step].iter().all(|&byte| byte == 0) {
            return &slot[..at + step];
        }
        at += step;
    }
    &slot[..limit]
}

fn number(slot: &[u8]) -> u64 {
    slot.iter().rev().fold(0u64, |value, &byte| (value << 8) | byte as u64)
}

/// A summary of a field's values over many items, compared between the files.
#[derive(Clone, Copy)]
struct Stats {
    zero: f64,
    distinct: f64,
    magnitude: Option<f64>,
    text: f64,
}

fn stats(slots: &[&[u8]], kind: &Kind) -> Stats {
    let count = slots.len().max(1) as f64;
    match kind {
        Kind::Number => {
            let values: Vec<u64> = slots.iter().map(|slot| number(slot)).collect();
            let zero = values.iter().filter(|&&value| value == 0).count() as f64 / count;
            let mut sorted = values.clone();
            sorted.sort_unstable();
            sorted.dedup();
            let mut logs: Vec<f64> = values.iter().filter(|&&value| value != 0).map(|&value| (value as f64).log2()).collect();
            logs.sort_by(f64::total_cmp);
            Stats { zero, distinct: sorted.len() as f64 / count, magnitude: logs.get(logs.len() / 2).copied(), text: 0.0 }
        }
        Kind::Text16 | Kind::Text8 => {
            let wide = matches!(kind, Kind::Text16);
            let classes: Vec<u8> = slots.iter().map(|slot| text_class(slot, wide)).collect();
            Stats { zero: classes.iter().filter(|&&class| class == 0).count() as f64 / count, distinct: 0.0, magnitude: None, text: classes.iter().filter(|&&class| class == 1).count() as f64 / count }
        }
        Kind::Blob => Stats { zero: slots.iter().filter(|slot| slot.iter().all(|&byte| byte == 0)).count() as f64 / count, distinct: 0.0, magnitude: None, text: 0.0 },
    }
}

fn similarity(a: &Stats, b: &Stats, kind: &Kind) -> f64 {
    match kind {
        Kind::Number => {
            let magnitude = match (a.magnitude, b.magnitude) {
                (Some(x), Some(y)) => ((x - y).abs() / 8.0).min(1.0),
                (None, None) => 0.0,
                _ => 1.0,
            };
            1.0 - ((a.zero - b.zero).abs() + (a.distinct - b.distinct).abs() + magnitude) / 3.0
        }
        // Junk (neither empty nor a text) is what tells a misplaced text; shops fill texts differently.
        Kind::Text16 | Kind::Text8 => {
            let junk = |stats: &Stats| 1.0 - stats.zero - stats.text;
            (1.0 - (junk(a) - junk(b)).abs() - 0.25 * (a.text - b.text).abs()).max(0.0)
        }
        Kind::Blob => 1.0 - (a.zero - b.zero).abs(),
    }
}

/// Proposes a layout for `target` by aligning it with `reference`, a file one of `layouts` reads.
pub fn propose(reference: &[u8], layouts: &[Layout], target: &[u8]) -> Result<LayoutProposal, String> {
    let (known, layout) = parse(reference, layouts, None).map_err(|error| if error.starts_with("NO_LAYOUT:") { "No item layout reads the reference file; choose a shop file the editor opens".to_string() } else { error })?;
    let unknown = (16..=16384).find_map(|size| split(target, size)).ok_or("The category list of the file was not found at any record size; it is not a shop file of this kind")?;
    let size = unknown.record_size.ok_or("The file has no items to compare")?;
    if known.records.is_empty() {
        return Err("The reference file has no items".into());
    }

    // Pairs by item ID and count; by position when the files have the same items in another layout.
    let mut by_key: HashMap<&[u8], Vec<usize>> = HashMap::new();
    for (index, record) in known.records.iter().enumerate() {
        by_key.entry(&record[..8]).or_default().push(index);
    }
    let mut used: HashMap<&[u8], usize> = HashMap::new();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (index, record) in unknown.records.iter().enumerate() {
        let key = &record[..8.min(record.len())];
        if let Some(candidates) = by_key.get(key) {
            let next = used.entry(key).or_insert(0);
            if let Some(&known_index) = candidates.get(*next) {
                pairs.push((known_index, index));
                *next += 1;
            }
        }
    }
    if pairs.len() * 10 < unknown.records.len() && known.records.len() == unknown.records.len() {
        pairs = (0..known.records.len()).map(|index| (index, index)).collect();
    }
    let pairs: Vec<(usize, usize)> = spread(pairs.len(), SAMPLE).into_iter().map(|index| pairs[index]).collect();
    let known_sample: Vec<&[u8]> = spread(known.records.len(), SAMPLE).into_iter().map(|index| known.records[index].as_slice()).collect();
    let unknown_sample: Vec<&[u8]> = spread(unknown.records.len(), SAMPLE).into_iter().map(|index| unknown.records[index].as_slice()).collect();

    // The fields of the reference layout with their offsets.
    let mut fields = Vec::new();
    let mut offset = 0;
    for field in &layout.fields {
        let length = field.ty.size();
        fields.push((field, offset, length, kind_of(&field.ty)));
        offset += length;
    }

    // score[i][p]: field i at position p of the unknown record (NaN where it does not fit).
    let mut scores: Vec<Vec<f64>> = Vec::with_capacity(fields.len());
    let mut sames: Vec<Vec<Option<f64>>> = Vec::with_capacity(fields.len());
    let mut guessed: Vec<bool> = Vec::with_capacity(fields.len());
    // What dropping each field costs: little for fields the reference rarely fills.
    let mut drop_costs: Vec<f64> = Vec::with_capacity(fields.len());
    for (_, start, length, kind) in &fields {
        let (start, length) = (*start, *length);
        let reference_stats = stats(&known_sample.iter().map(|record| &record[start..start + length]).collect::<Vec<_>>(), kind);
        let informative_share = ((1.0 - reference_stats.zero) * 4.0).min(1.0);
        let mut row = vec![f64::NAN; size + 1];
        let mut same_row = vec![None; size + 1];
        for position in 0..=size.saturating_sub(length) {
            if length > size {
                break;
            }
            let slots: Vec<&[u8]> = unknown_sample.iter().map(|record| &record[position..position + length]).collect();
            // A field the reference rarely fills says little about where it is: its pattern counts only as
            // far as it has values (a quarter of the items filled counts fully).
            let distribution = KEEP + (similarity(&reference_stats, &stats(&slots, kind), kind) - KEEP) * informative_share;
            let (mut informative, mut equal) = (0usize, 0usize);
            for &(a, b) in &pairs {
                let va = &known.records[a][start..start + length];
                let vb = &unknown.records[b][position..position + length];
                let (counts, same) = match kind {
                    Kind::Number => (va.iter().any(|&byte| byte != 0), va == vb),
                    Kind::Text16 | Kind::Text8 => {
                        let wide = matches!(kind, Kind::Text16);
                        let prefix = text_prefix(va, wide);
                        (text_class(va, wide) == 1, vb.starts_with(prefix))
                    }
                    Kind::Blob => (va.iter().any(|&byte| byte != 0), va[..length.min(256)] == vb[..length.min(256)]),
                };
                if counts {
                    informative += 1;
                    equal += same as usize;
                }
            }
            let same = (informative >= 5).then(|| equal as f64 / informative as f64);
            row[position] = distribution;
            same_row[position] = same;
        }
        // Equal values pin a field only where the files hold the same values for it somewhere (the same
        // shop); in other shops (other prices, translations) the value pattern alone has to tell.
        let reliable = same_row.iter().flatten().fold(0.0f64, |best, &same| best.max(same)) >= 0.5;
        if reliable {
            for (score, same) in row.iter_mut().zip(&same_row) {
                if let Some(same) = same {
                    *score = 0.7 * same + 0.3 * *score;
                }
            }
        }
        guessed.push(!reliable && informative_share < 0.25);
        drop_costs.push(if reliable { DROP } else { 0.01 + DROP * informative_share });
        scores.push(row);
        sames.push(same_row);
    }

    // dp over fields: best[p] = best total with the next field starting at or after p.
    let gap_cost = |gap: usize| if gap == 0 { 0.0 } else { INSERT + gap as f64 * PER_BYTE - size as f64 * LATE };
    #[derive(Clone, Copy)]
    enum Step {
        Drop,
        Keep { from: usize, at: usize },
    }
    let mut best = vec![f64::NEG_INFINITY; size + 1];
    best[0] = 0.0;
    let mut steps: Vec<Vec<Option<Step>>> = Vec::with_capacity(fields.len());
    for (index, (_, _, length, _)) in fields.iter().enumerate() {
        // Where a field may start: from the best earlier end, paying for the gap.
        let mut start_score = vec![f64::NEG_INFINITY; size + 1];
        let mut start_from = vec![0usize; size + 1];
        let mut running = (f64::NEG_INFINITY, 0usize);
        for position in 0..=size {
            if best[position] > f64::NEG_INFINITY {
                let value = best[position] + position as f64 * PER_BYTE;
                if value > running.0 {
                    running = (value, position);
                }
            }
            let gapped = running.0 - INSERT - position as f64 * PER_BYTE + position as f64 * LATE;
            let (score, from) = if best[position] >= gapped { (best[position], position) } else { (gapped, running.1) };
            start_score[position] = score;
            start_from[position] = from;
        }
        let mut next = vec![f64::NEG_INFINITY; size + 1];
        let mut step = vec![None; size + 1];
        for position in 0..=size {
            if best[position] - drop_costs[index] > next[position] {
                next[position] = best[position] - drop_costs[index];
                step[position] = Some(Step::Drop);
            }
        }
        for at in 0..=size {
            let end = at + length;
            if end > size || start_score[at] == f64::NEG_INFINITY || scores[index][at].is_nan() {
                continue;
            }
            let value = start_score[at] + scores[index][at] - KEEP;
            if value > next[end] {
                next[end] = value;
                step[end] = Some(Step::Keep { from: start_from[at], at });
            }
        }
        steps.push(step);
        best = next;
    }
    let (mut position, _) = (0..=size).filter(|&p| best[p] > f64::NEG_INFINITY).map(|p| (p, best[p] - gap_cost(size - p))).max_by(|a, b| a.1.total_cmp(&b.1)).ok_or("No layout fits")?;
    let trailing = size - position;

    // Back from the end: where each field went.
    let mut placed: Vec<Option<(usize, usize)>> = vec![None; fields.len()];
    for index in (0..fields.len()).rev() {
        match steps[index][position].expect("a step for every reachable state") {
            Step::Drop => {}
            Step::Keep { from, at } => {
                placed[index] = Some((at, from));
                position = from;
            }
        }
    }

    // The proposal: kept fields in order, unknown bytes in the gaps.
    let mut proposed = Vec::new();
    let mut changes = Vec::new();
    let mut matches = Vec::new();
    let mut cursor = 0;
    let mut previous: Option<String> = None;
    let insert = |at: usize, gap: usize, previous: &Option<String>, proposed: &mut Vec<Field>, changes: &mut Vec<LayoutChange>| {
        let name = format!("unknown_{at}");
        let ty = match gap {
            1 => FieldType::U8,
            2 => FieldType::U16,
            4 => FieldType::I32,
            _ => FieldType::Bytes { len: gap },
        };
        proposed.push(Field { name: name.clone(), ty, meaning: None, note: "Not in the reference layout".into() });
        changes.push(LayoutChange { kind: "inserted", offset: at, size: gap, name, after: previous.clone() });
    };
    for (index, (field, reference_offset, length, _)) in fields.iter().enumerate() {
        match placed[index] {
            Some((at, _)) => {
                if at > cursor {
                    insert(cursor, at - cursor, &previous, &mut proposed, &mut changes);
                }
                proposed.push((*field).clone());
                matches.push(FieldMatch { name: field.name.clone(), reference_offset: *reference_offset, offset: Some(at), size: *length, score: scores[index][at], same: sames[index][at], guessed: guessed[index] });
                cursor = at + length;
                previous = Some(field.name.clone());
            }
            None => {
                changes.push(LayoutChange { kind: "removed", offset: cursor, size: *length, name: field.name.clone(), after: previous.clone() });
                matches.push(FieldMatch { name: field.name.clone(), reference_offset: *reference_offset, offset: None, size: *length, score: 0.0, same: None, guessed: guessed[index] });
            }
        }
    }
    if trailing > 0 {
        insert(cursor, trailing, &previous, &mut proposed, &mut changes);
    }
    Ok(LayoutProposal { fields: proposed, reference_layout: layout.name.clone(), reference_size: layout.size(), record_size: size, items: unknown.records.len(), pairs: pairs.len(), matches, changes })
}

#[cfg(test)]
mod tests {
    use super::super::layout;
    use super::*;

    fn layout_of(fields: &[Field]) -> Vec<(String, usize)> {
        fields.iter().map(|field| (field.name.clone(), field.ty.size())).collect()
    }

    fn inserted(proposal: &LayoutProposal) -> Vec<(String, usize, usize)> {
        proposal.changes.iter().map(|change| (change.kind.to_string(), change.offset, change.size)).collect()
    }

    #[test]
    fn finds_a_field_inserted_in_the_middle() {
        let Ok(data) = std::fs::read("E:/Game Dev/JD/1559/gamed/config/gshop.data") else { return };
        let (file, _) = parse(&data, &layout::builtin(), None).unwrap();
        // A u32 after the price and two bytes after the name, in every record.
        let mut newer = data[..8].to_vec();
        for (index, record) in file.records.iter().enumerate() {
            newer.extend_from_slice(&record[..140]);
            newer.extend_from_slice(&(7000 + index as u32).to_le_bytes());
            newer.extend_from_slice(&record[140..1256]);
            newer.extend_from_slice(&[1, 2]);
            newer.extend_from_slice(&record[1256..]);
        }
        newer.extend_from_slice(&data[8 + file.records.len() * 2630..]);
        let proposal = propose(&data, &layout::builtin(), &newer).unwrap();
        assert_eq!(proposal.record_size, 2636);
        assert_eq!(inserted(&proposal), vec![("inserted".into(), 140, 4), ("inserted".into(), 1260, 2)]);
        assert!(proposal.matches.iter().all(|found| found.offset.is_some()));
        // The proposal reads the file.
        let user = Layout { id: "found".into(), name: "Found".into(), description: String::new(), fields: proposal.fields.clone(), builtin: false };
        user.validate().unwrap();
        assert_eq!(user.size(), 2636);
        let (read, _) = parse(&newer, &[user], None).unwrap();
        assert_eq!(read.records.len(), file.records.len());
    }

    #[test]
    fn finds_bytes_a_client_appends() {
        let reference = std::path::Path::new("E:/Games/XtremeJade/element/data/gshop.data");
        let forsaken = std::path::Path::new("E:/Games/ForsakenJD/element/data/gshop.data");
        let hdn = std::path::Path::new("E:/Games/Elite Jade Dynasty - HDN/element/data/gshop.data");
        let Ok(reference) = std::fs::read(reference) else { return };
        let source = vec![layout::builtin().remove(0)];
        for (path, extra) in [(forsaken, 5), (hdn, 30)] {
            let Ok(target) = std::fs::read(path) else { continue };
            let proposal = propose(&reference, &source, &target).unwrap();
            assert_eq!(inserted(&proposal), vec![("inserted".into(), 2630, extra)], "{}", path.display());
            assert_eq!(layout_of(&proposal.fields)[..26], layout_of(&source[0].fields)[..]);
        }
    }

    #[test]
    fn finds_what_an_old_client_lacks() {
        let Ok(reference) = std::fs::read("E:/Game Dev/JD/1559/gamed/config/gshop.data") else { return };
        let Ok(old) = std::fs::read("C:/Users/mitko/Desktop/gshop.data.20261010-195639.bak") else { return };
        let proposal = propose(&reference, &layout::builtin(), &old).unwrap();
        assert_eq!(proposal.record_size, 1252);
        let offsets: HashMap<&str, Option<usize>> = proposal.matches.iter().map(|found| (found.name.as_str(), found.offset)).collect();
        for (name, offset) in [("id", Some(0)), ("price", Some(136)), ("props", Some(148)), ("main_type", Some(152)), ("sub_type", Some(156)), ("local_id", Some(160)), ("description", Some(164)), ("name", Some(1188)), ("present_name", None), ("search_keys", None)] {
            assert_eq!(offsets[name], offset, "{name}");
        }
    }
}
