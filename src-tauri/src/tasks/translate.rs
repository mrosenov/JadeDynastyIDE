//! Copies human-facing task text from a translated \`tasks.data\` into the open
//! one: names, descriptions and NPC dialog text, by task ID and field path.
//!
//! Versions may differ. Only UTF-16 text in the three groups is touched; IDs,
//! numbers, counts and raw bytes never change (counted text keeps its stored
//! length in step). A talk is translated only when its windows and options
//! have the same shape on both sides, so text cannot land in another window.
//! Each text keeps the open file's terminator convention: v165 stores a
//! trailing NUL inside dialog text, later versions do not.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use super::browser::{set_task_field, task_at, task_at_mut, task_heading, verify_task_root, TaskDocument, TaskSearchEntry};
use super::compare::{read_packs, slice, unique_positions, ComparedTasks};
use super::edit::{EditState, EntryDetails, RootChange};
use super::json::dotted;
use super::schema::{decode_exact, FieldType, Node, Schema, Value};

/// Samples listed per group, and issues listed in all.
const SAMPLES_PER_GROUP: usize = 100;
const ISSUES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextGroup {
    Names,
    Descriptions,
    Dialogs,
}

/// The group a task text belongs to; other text (such as dialog parameters) is left alone.
fn text_group(path: &str) -> Option<TextGroup> {
    if path == "fixed.name" || path == "signature" {
        Some(TextGroup::Names)
    } else if path.starts_with("texts.") || path.ends_with(".extra_tribute") || path == "extra_tribute" {
        Some(TextGroup::Descriptions)
    } else if path.starts_with("dialogs.") && (path.ends_with(".prompt") || path.ends_with(".text")) {
        Some(TextGroup::Dialogs)
    } else {
        None
    }
}

/// One planned text change inside a root.
#[derive(Debug, Clone)]
pub struct TextEdit {
    task_path: Vec<usize>,
    field: Vec<String>,
    group: TextGroup,
    text: String,
}

#[derive(Debug, Clone)]
pub struct RootEdits {
    pack: usize,
    root: usize,
    before: Vec<u8>,
    edits: Vec<TextEdit>,
}

/// The translated task set and the last previewed plan.
pub struct TranslationSource {
    pub source: ComparedTasks,
    pub plan: Option<(String, Vec<RootEdits>)>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    pub group: Option<TextGroup>,
    pub tasks: usize,
    pub fields: usize,
    pub too_long: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSample {
    pub id: u32,
    pub name: String,
    pub group: TextGroup,
    pub field: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationIssue {
    pub id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationReport {
    pub token: String,
    pub source_path: String,
    pub source_version: u32,
    pub version: u32,
    /// Open tasks with a unique source task of the same ID.
    pub matched: usize,
    /// Open tasks the source does not have.
    pub missing_source: usize,
    /// IDs several tasks share on either side (not translated).
    pub ambiguous: usize,
    pub groups: Vec<GroupSummary>,
    /// Open texts left as they are because the source text is blank.
    pub blank: usize,
    /// Text that already reads the same.
    pub same: usize,
    /// Talks whose windows or options differ in shape.
    pub shape: usize,
    pub too_long: usize,
    pub samples: Vec<TranslationSample>,
    pub issues: Vec<TranslationIssue>,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<EditState>,
}

/// Every translatable text of one task (not its subtasks), by dotted path.
fn texts(task: &Node) -> BTreeMap<String, (TextGroup, String)> {
    fn walk(node: &Node, path: &mut Vec<String>, output: &mut BTreeMap<String, (TextGroup, String)>) {
        if node.children().is_empty() {
            if let (FieldType::FixedUtf16 { .. } | FieldType::PrefixedUtf16 { .. } | FieldType::CountedUtf16 { .. }, Value::Text(text)) = (&node.ty, &node.value) {
                let key = dotted(path);
                if let Some(group) = text_group(&key) {
                    output.insert(key, (group, text.clone()));
                }
            }
            return;
        }
        for child in node.children() {
            path.push(child.name.clone());
            walk(child, path, output);
            path.pop();
        }
    }
    let mut output = BTreeMap::new();
    for field in task.children().iter().filter(|field| field.name != "subtasks") {
        walk(field, &mut vec![field.name.clone()], &mut output);
    }
    output
}

/// Window and option counts of one talk (\`dialogs.award\`).
fn talk_shape(task: &Node, talk: &str) -> Option<Vec<usize>> {
    let windows = task.child("dialogs")?.child(talk)?.child("windows")?;
    Some(windows.children().iter().map(|window| window.child("options").map_or(0, |options| options.children().len())).collect())
}

/// The source text in the target's terminator convention.
fn in_target_convention(source: &str, target: &str) -> String {
    let terminators = target.len() - target.trim_end_matches('\0').len();
    format!("{}{}", source.trim_end_matches('\0'), "\0".repeat(terminators))
}

#[derive(Default)]
struct Tally {
    groups: BTreeMap<TextGroup, GroupSummary>,
    tasks: BTreeMap<TextGroup, std::collections::HashSet<u32>>,
    blank: usize,
    same: usize,
    shape: usize,
    too_long: usize,
    samples: BTreeMap<TextGroup, Vec<TranslationSample>>,
    issues: Vec<TranslationIssue>,
}

impl Tally {
    fn issue(&mut self, issue: TranslationIssue) {
        if self.issues.len() < ISSUES {
            self.issues.push(issue);
        }
    }

    fn merge(&mut self, other: Tally) {
        for (group, summary) in other.groups {
            let entry = self.groups.entry(group).or_default();
            entry.fields += summary.fields;
            entry.too_long += summary.too_long;
        }
        for (group, tasks) in other.tasks {
            self.tasks.entry(group).or_default().extend(tasks);
        }
        self.blank += other.blank;
        self.same += other.same;
        self.shape += other.shape;
        self.too_long += other.too_long;
        for (group, samples) in other.samples {
            let entry = self.samples.entry(group).or_default();
            entry.extend(samples.into_iter().take(SAMPLES_PER_GROUP.saturating_sub(entry.len())));
        }
        for issue in other.issues {
            self.issue(issue);
        }
    }
}

/// Plans the text edits of one root against its source roots.
fn plan_root(schema: &Schema, version: u32, before: &[u8], sources: &HashMap<(usize, usize), Node>, pairs: &[(&TaskSearchEntry, &TaskSearchEntry)], tally: &mut Tally) -> Result<Vec<TextEdit>, String> {
    let mut working = decode_exact(schema, before, version)?;
    let mut edits = Vec::new();
    for (this, theirs) in pairs {
        let source_task = task_at(&sources[&(theirs.pack, theirs.root)], &theirs.path)?;
        let source_texts = texts(source_task);
        let target_texts = texts(task_at(&working, &this.path)?);
        let mut talks = HashMap::<String, bool>::new();
        for (key, (group, current)) in target_texts {
            let Some((_, translated)) = source_texts.get(&key) else { continue };
            if group == TextGroup::Dialogs {
                let talk = key.split('.').nth(1).unwrap_or_default().to_string();
                let same_shape = *talks.entry(talk.clone()).or_insert_with(|| {
                    let target = task_at(&working, &this.path).ok().and_then(|task| talk_shape(task, &talk));
                    let shaped = target.is_some() && target == talk_shape(source_task, &talk);
                    if !shaped {
                        tally.shape += 1;
                    }
                    shaped
                });
                if !same_shape {
                    continue;
                }
            }
            if translated.trim_end_matches('\0').trim().is_empty() {
                // Count only text a blank source would have cleared.
                if !current.trim_end_matches('\0').trim().is_empty() {
                    tally.blank += 1;
                }
                continue;
            }
            let text = in_target_convention(translated, &current);
            if text == current {
                tally.same += 1;
                continue;
            }
            let field = super::json::field_path(&key)?;
            let task = task_at_mut(&mut working, &this.path)?;
            match set_task_field(schema, task, &field, &text, &|_| true) {
                Ok(Some((old, new))) => {
                    let summary = tally.groups.entry(group).or_default();
                    summary.fields += 1;
                    tally.tasks.entry(group).or_default().insert(this.id);
                    let samples = tally.samples.entry(group).or_default();
                    if samples.len() < SAMPLES_PER_GROUP {
                        samples.push(TranslationSample { id: this.id, name: this.name.clone(), group, field: key.clone(), old, new });
                    }
                    edits.push(TextEdit { task_path: this.path.clone(), field, group, text });
                }
                Ok(None) => tally.same += 1,
                Err(error) => {
                    tally.too_long += 1;
                    tally.groups.entry(group).or_default().too_long += 1;
                    tally.issue(TranslationIssue { id: this.id, name: this.name.clone(), field: Some(key.clone()), message: error });
                }
            }
        }
    }
    if !edits.is_empty() {
        let after = working.encode()?;
        if let Err(error) = verify_task_root(schema, &after, version, "The translated task would be invalid") {
            let (id, name) = task_heading(&working)?;
            tally.issue(TranslationIssue { id, name, field: None, message: format!("{error}; its texts are skipped") });
            return Ok(Vec::new());
        }
    }
    Ok(edits)
}

impl TaskDocument {
    fn translation_token(&self, source: &ComparedTasks) -> String {
        let mut hash = Md5::new();
        hash.update(source.path.as_bytes());
        hash.update(source.size.to_le_bytes());
        hash.update(self.summary.path.as_bytes());
        hash.update(format!("{:?}", self.journal.generation()).as_bytes());
        format!("{:x}", hash.finalize())
    }

    /// Plans every translatable text change from \`source\`.
    pub fn translation_plan(&self, source: &ComparedTasks) -> Result<(TranslationReport, Vec<RootEdits>), String> {
        let started = Instant::now();
        let version = self.container.header.version;
        let this_entries = {
            let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
            if !index.indexed {
                return Err("Tasks are still being indexed. Try again in a moment".into());
            }
            index.entries.clone()
        };
        let (this_by_id, this_duplicates) = unique_positions(&this_entries);
        let (source_by_id, source_duplicates) = unique_positions(&source.entries);
        let this_packs = read_packs(&self.container)?;
        let source_packs = read_packs(&source.container)?;

        // Group the pairs by open root; identical roots have nothing to translate.
        let mut by_root = BTreeMap::<(usize, usize), Vec<(&TaskSearchEntry, &TaskSearchEntry)>>::new();
        let mut matched = 0;
        for (id, this) in &this_by_id {
            let Some(theirs) = source_by_id.get(id) else { continue };
            matched += 1;
            by_root.entry((this.pack, this.root)).or_default().push((this, theirs));
        }
        let mut units = Vec::new();
        for ((pack, root), pairs) in by_root {
            let before = if self.modified.contains_key(&(pack, root)) || root >= self.container.packs[pack].root_count() {
                self.current_root(pack, root)?
            } else {
                slice(&self.container, &this_packs, pack, root)?.to_vec()
            };
            let identical = pairs.iter().all(|(this, theirs)| this.path == theirs.path)
                && pairs.iter().map(|(_, theirs)| (theirs.pack, theirs.root)).collect::<std::collections::HashSet<_>>().len() == 1
                && slice(&source.container, &source_packs, pairs[0].1.pack, pairs[0].1.root)? == before.as_slice();
            if !identical {
                units.push((pack, root, before, pairs));
            }
        }

        let next = AtomicUsize::new(0);
        let workers = std::thread::available_parallelism().map(usize::from).unwrap_or(1).min(units.len().max(1));
        let results = std::thread::scope(|scope| -> Result<Vec<(Tally, Vec<RootEdits>)>, String> {
            let mut handles = Vec::new();
            for _ in 0..workers {
                handles.push(scope.spawn(|| -> Result<(Tally, Vec<RootEdits>), String> {
                    let mut tally = Tally::default();
                    let mut roots = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some((pack, root, before, pairs)) = units.get(index) else { break };
                        let mut sources = HashMap::new();
                        for (_, theirs) in pairs {
                            if !sources.contains_key(&(theirs.pack, theirs.root)) {
                                sources.insert((theirs.pack, theirs.root), decode_exact(&source.schema, slice(&source.container, &source_packs, theirs.pack, theirs.root)?, source.version)?);
                            }
                        }
                        let edits = plan_root(&self.schema, version, before, &sources, pairs, &mut tally)?;
                        if !edits.is_empty() {
                            roots.push(RootEdits { pack: *pack, root: *root, before: before.clone(), edits });
                        }
                    }
                    Ok((tally, roots))
                }));
            }
            handles.into_iter().map(|handle| handle.join().map_err(|_| "Translation worker panicked".to_string())?).collect()
        })?;
        let mut tally = Tally::default();
        let mut roots = Vec::new();
        for (part, edits) in results {
            tally.merge(part);
            roots.extend(edits);
        }
        roots.sort_by_key(|edits| (edits.pack, edits.root));
        let groups = [TextGroup::Names, TextGroup::Descriptions, TextGroup::Dialogs].into_iter().map(|group| {
            let summary = tally.groups.remove(&group).unwrap_or_default();
            GroupSummary { group: Some(group), tasks: tally.tasks.get(&group).map_or(0, |tasks| tasks.len()), fields: summary.fields, too_long: summary.too_long }
        }).collect();
        let report = TranslationReport {
            token: self.translation_token(source),
            source_path: source.path.clone(),
            source_version: source.version,
            version,
            matched,
            missing_source: this_by_id.keys().filter(|id| !source_by_id.contains_key(id) && !source_duplicates.contains(id)).count(),
            ambiguous: this_duplicates.union(&source_duplicates).count(),
            groups,
            blank: tally.blank,
            same: tally.same,
            shape: tally.shape,
            too_long: tally.too_long,
            samples: tally.samples.into_values().flatten().collect(),
            issues: tally.issues,
            elapsed_ms: started.elapsed().as_millis() as u64,
            state: None,
        };
        Ok((report, roots))
    }

    /// Applies the previewed edits of the chosen groups as one undo step.
    pub fn apply_translation(&mut self, roots: &[RootEdits], groups: &[TextGroup]) -> Result<(EditState, usize), String> {
        let version = self.container.header.version;
        let mut changes = Vec::new();
        let mut fields = 0;
        for root in roots {
            let current = self.current_root(root.pack, root.root)?;
            if current != root.before {
                return Err("The open task set changed since the preview. Preview the translation again.".into());
            }
            let mut working = decode_exact(&self.schema, &current, version)?;
            let mut changed = 0;
            for edit in root.edits.iter().filter(|edit| groups.contains(&edit.group)) {
                let task = task_at_mut(&mut working, &edit.task_path)?;
                if set_task_field(&self.schema, task, &edit.field, &edit.text, &|_| true)?.is_some() {
                    changed += 1;
                }
            }
            if changed == 0 {
                continue;
            }
            let after = working.encode()?;
            verify_task_root(&self.schema, &after, version, "The translated task would be invalid")?;
            fields += changed;
            changes.push(RootChange { pack: root.pack, root: root.root, before: current, after });
        }
        if !changes.is_empty() {
            self.apply_changes(&changes)?;
            self.journal.record(EntryDetails {
                label: format!("Translate from tasks.data ({fields} texts)"),
                task_id: 0,
                task_name: "Translated tasks".into(),
                field: "Task texts".into(),
                old: format!("{} task root(s)", changes.len()),
                new: format!("{fields} translated text(s)"),
            }, changes);
        }
        Ok((self.edit_state(), fields))
    }

    /// Whether \`token\` still describes the open set and source (no edit since).
    pub fn translation_current(&self, source: &ComparedTasks, token: &str) -> bool {
        self.translation_token(source) == token
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::tasks::browser::FieldEdit;

    #[test]
    fn groups_and_terminators() {
        assert_eq!(text_group("fixed.name"), Some(TextGroup::Names));
        assert_eq!(text_group("texts.description"), Some(TextGroup::Descriptions));
        assert_eq!(text_group("dialogs.award.windows[0].text"), Some(TextGroup::Dialogs));
        assert_eq!(text_group("dialogs.award.windows[0].options[1].text"), Some(TextGroup::Dialogs));
        assert_eq!(text_group("dialogs.award.windows[0].parameter_text"), None);
        assert_eq!(in_target_convention("Hello", "Hallo\0"), "Hello\0");
        assert_eq!(in_target_convention("Hello\0", "Hallo"), "Hello");
    }

    #[test]
    fn translates_edited_texts_back_from_the_original() {
        let path = r"E:/Games/XtremeJade/element/data/tasks.data";
        if !Path::new(path).is_file() {
            return;
        }
        let source = ComparedTasks::open(path, Path::new(".")).unwrap();
        let mut document = TaskDocument::open(path).unwrap();
        while !document.search("", 0).indexed {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let root = document.summary().roots[0].clone();
        let original = document.current_root(root.pack, root.root).unwrap();
        // "Untranslate" the name, then translate it back from the original file.
        document.edit_field(FieldEdit { pack: root.pack, root: root.root, task_path: Vec::new(), field_path: vec!["fixed".into(), "name".into()], value: "JD IDE untranslated".into() }).unwrap();
        let (report, roots) = document.translation_plan(&source).unwrap();
        let names = report.groups.iter().find(|group| group.group == Some(TextGroup::Names)).unwrap();
        assert_eq!((names.tasks, names.fields), (1, 1));
        assert!(report.samples.iter().any(|sample| sample.old == "JD IDE untranslated" && sample.new == root.name));
        assert!(document.translation_current(&source, &report.token));
        // Applying only dialogs changes nothing; names restores the original bytes.
        assert_eq!(document.apply_translation(&roots, &[TextGroup::Dialogs]).unwrap().1, 0);
        assert_eq!(document.apply_translation(&roots, &[TextGroup::Names]).unwrap().1, 1);
        assert_eq!(document.current_root(root.pack, root.root).unwrap(), original);
        assert!(!document.translation_current(&source, &report.token), "an applied translation changes the token");
        document.undo().unwrap();
        assert_ne!(document.current_root(root.pack, root.root).unwrap(), original);
    }
}

