//! NPC talks of a task as editable trees.
//!
//! A talk (`TASK_TALK`, the game's `talk_proc`) is a list of windows. Each window
//! has text and options; an option either opens a child window (its `id` is the
//! window's ID) or runs an NPC function (`id` has the high bit set; `parameter`
//! is a quest ID for the quest functions). The client opens `windows[0]` first,
//! finds the others by ID and goes Back through `parent_id`.
//!
//! Every talk of the sample sets is a tree in the official editor's order: the
//! root (parent -1) first, then each option's child window followed by its own
//! children. `set_dialog` always writes that order and sets the parent IDs, so
//! the UI only sends windows and options. Window text keeps the file's
//! terminator convention (v165 stores a trailing NUL inside the text) and v174+
//! window parameters stay with their window.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::browser::{empty_node, node_at_mut, set_count, task_at, task_at_mut, task_heading, verify_task_root, TaskDocument};
use super::edit::{EditState, EntryDetails, RootChange};
use super::schema::{decode_exact, is_task_option_function, Node, Value};

/// The talks a task holds, in form order.
pub const TALKS: [&str; 5] = ["delivery", "unqualified", "item_delivery", "execution", "award"];
const ROOT_PARENT: u32 = u32::MAX;
const FUNCTION: u32 = 0x8000_0000;
/// Prompt and option texts are `wchar_t[64]`: 63 characters and a terminator.
const SHORT_TEXT: usize = 63;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogOption {
    /// A child window's ID, or `0x80000000 | function`.
    pub target: u32,
    pub text: String,
    pub parameter: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogWindow {
    pub id: u32,
    /// Filled in on reading; ignored on writing (the tree decides it).
    #[serde(default)]
    pub parent_id: u32,
    pub text: String,
    pub options: Vec<DialogOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialog {
    /// `delivery`, `unqualified`, …
    pub talk: String,
    /// The text of the NPC menu entry that starts the talk.
    pub prompt: String,
    /// The root window first.
    pub windows: Vec<DialogWindow>,
}

fn text(node: Option<&Node>) -> String {
    match node.map(|node| &node.value) {
        Some(Value::Text(text)) => text.trim_end_matches('\0').replace("\r\n", "\n"),
        _ => String::new(),
    }
}

fn number(node: Option<&Node>) -> u32 {
    match node.map(|node| &node.value) {
        Some(Value::U64(value)) => *value as u32,
        Some(Value::I64(value)) => *value as u32,
        _ => 0,
    }
}

fn read_dialog(talk: &Node) -> Dialog {
    let windows = talk.child("windows").map(Node::children).unwrap_or_default();
    Dialog {
        talk: talk.name.clone(),
        prompt: text(talk.child("prompt")),
        windows: windows.iter().map(|window| DialogWindow {
            id: number(window.child("id")),
            parent_id: number(window.child("parent_id")),
            text: text(window.child("text")),
            options: window.child("options").map(Node::children).unwrap_or_default().iter().map(|option| DialogOption {
                target: number(option.child("id")),
                text: text(option.child("text")),
                parameter: number(option.child("parameter")),
            }).collect(),
        }).collect(),
    }
}

/// The talks of a task.
pub fn read_dialogs(task: &Node) -> Vec<Dialog> {
    let Some(dialogs) = task.child("dialogs") else { return Vec::new() };
    TALKS.iter().filter_map(|name| dialogs.child(name)).map(read_dialog).collect()
}

/// Checks the tree and returns the windows in the order the official editor writes them, with parents.
fn ordered(dialog: &Dialog) -> Result<Vec<(&DialogWindow, u32)>, String> {
    if dialog.prompt.encode_utf16().count() > SHORT_TEXT {
        return Err(format!("The prompt is longer than {SHORT_TEXT} characters"));
    }
    let Some(root) = dialog.windows.first() else { return Ok(Vec::new()) };
    let mut by_id = HashMap::new();
    for window in &dialog.windows {
        if window.id & FUNCTION != 0 {
            return Err(format!("Window ID {} has the high bit set, which marks NPC functions", window.id));
        }
        if by_id.insert(window.id, window).is_some() {
            return Err(format!("Window ID {} is used twice", window.id));
        }
        for option in &window.options {
            if option.text.encode_utf16().count() > SHORT_TEXT {
                return Err(format!("Window {}: option \"{}\" is longer than {SHORT_TEXT} characters", window.id, option.text));
            }
        }
    }
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    fn visit<'d>(window: &'d DialogWindow, parent: u32, by_id: &HashMap<u32, &'d DialogWindow>, seen: &mut HashSet<u32>, order: &mut Vec<(&'d DialogWindow, u32)>) -> Result<(), String> {
        if !seen.insert(window.id) {
            return Err(format!("Window {} is opened by more than one option", window.id));
        }
        order.push((window, parent));
        for option in &window.options {
            if option.target & FUNCTION != 0 {
                continue;
            }
            let child = by_id.get(&option.target).ok_or_else(|| format!("Window {}: option \"{}\" opens window {}, which does not exist", window.id, option.text, option.target))?;
            visit(child, window.id, by_id, seen, order)?;
        }
        Ok(())
    }
    visit(root, ROOT_PARENT, &by_id, &mut seen, &mut order)?;
    if let Some(lost) = dialog.windows.iter().find(|window| !seen.contains(&window.id)) {
        return Err(format!("Window {} is not opened by any option", lost.id));
    }
    Ok(order)
}

fn set_text(node: &mut Node, field: &str, value: &str) -> Result<(), String> {
    node_at_mut(node, &[field.to_string()])?.set_value(Value::Text(value.to_string()))
}

fn set_number(node: &mut Node, field: &str, value: u32) -> Result<(), String> {
    let target = node_at_mut(node, &[field.to_string()])?;
    let value = match target.value {
        Value::I64(_) => Value::I64(i64::from(value as i32)),
        _ => Value::U64(u64::from(value)),
    };
    if target.value != value {
        target.set_value(value)?;
    }
    Ok(())
}

impl TaskDocument {
    pub fn dialogs(&self, pack: usize, root: usize, task_path: &[usize]) -> Result<Vec<Dialog>, String> {
        let decoded = decode_exact(&self.schema, &self.current_root(pack, root)?, self.container.header.version)?;
        Ok(read_dialogs(task_at(&decoded, task_path)?))
    }

    /// Replaces one talk of a task with `dialog`, as one undo step.
    pub fn set_dialog(&mut self, pack: usize, root: usize, task_path: &[usize], dialog: &Dialog, label: &str) -> Result<EditState, String> {
        if !TALKS.contains(&dialog.talk.as_str()) {
            return Err(format!("{:?} is not a talk of a task", dialog.talk));
        }
        let order = ordered(dialog)?;
        let version = self.container.header.version;
        let before = self.current_root(pack, root)?;
        let mut decoded = decode_exact(&self.schema, &before, version)?;
        {
            let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
            for (window, _) in &order {
                for option in &window.options {
                    let quest = option.target & FUNCTION != 0 && is_task_option_function(i128::from(option.target));
                    if quest && option.parameter != 0 && index.indexed && !index.by_id.contains_key(&option.parameter) {
                        return Err(format!("Window {}: option \"{}\" names quest {}, which does not exist", window.id, option.text, option.parameter));
                    }
                }
            }
        }
        let task = task_at_mut(&mut decoded, task_path)?;
        let dialogs = task.child_mut("dialogs").ok_or("This task layout has no dialogs")?;
        // v165 keeps a NUL inside dialog text; later versions do not. Follow the task's own talks.
        let nul = {
            let mut texts = TALKS.iter().filter_map(|name| dialogs.child(name)).flat_map(|talk| talk.child("windows").map(Node::children).unwrap_or_default().iter().filter_map(|window| match window.child("text").map(|text| &text.value) {
                Some(Value::Text(text)) if !text.is_empty() => Some(text.ends_with('\0')),
                _ => None,
            }).collect::<Vec<_>>());
            texts.next().unwrap_or(version <= 165)
        };
        let talk = dialogs.child_mut(&dialog.talk).ok_or_else(|| format!("This task has no {} talk", dialog.talk))?;
        let window_type = match &talk.child("windows").ok_or("The talk has no windows list")?.ty {
            super::schema::FieldType::CountedArray { item, .. } => (**item).clone(),
            _ => return Err("The talk's windows are not a list".into()),
        };
        let empty_window = empty_node(&self.schema, &window_type, version)?;
        let option_type = match &empty_window.child("options").ok_or("A window has no options list")?.ty {
            super::schema::FieldType::CountedArray { item, .. } => (**item).clone(),
            _ => return Err("A window's options are not a list".into()),
        };
        let empty_option = empty_node(&self.schema, &option_type, version)?;

        if text(talk.child("prompt")) != dialog.prompt {
            set_text(talk, "prompt", &dialog.prompt.replace('\n', " "))?;
        }
        let old: HashMap<u32, Node> = talk.child("windows").map(Node::children).unwrap_or_default().iter().map(|window| (number(window.child("id")), window.clone())).collect();
        let mut windows = Vec::with_capacity(order.len());
        for (window, parent) in &order {
            // An existing window keeps everything not edited here (v174+ parameters, unchanged bytes).
            let mut node = old.get(&window.id).cloned().unwrap_or_else(|| empty_window.clone());
            set_number(&mut node, "id", window.id)?;
            set_number(&mut node, "parent_id", *parent)?;
            if text(node.child("text")) != window.text || node.child("text").is_none() {
                let stored = format!("{}{}", window.text.replace("\r\n", "\n").replace('\n', "\r\n"), if nul { "\0" } else { "" });
                set_text(&mut node, "text", &stored)?;
                set_count(&mut node, "text_length", stored.encode_utf16().count())?;
            }
            let old_options: Vec<Node> = node.child("options").map(Node::children).unwrap_or_default().to_vec();
            let mut options = Vec::with_capacity(window.options.len());
            for (position, option) in window.options.iter().enumerate() {
                let same = old_options.get(position).filter(|old| number(old.child("id")) == option.target && text(old.child("text")) == option.text && number(old.child("parameter")) == option.parameter);
                let mut node = match same {
                    Some(old) => old.clone(),
                    None => {
                        let mut fresh = empty_option.clone();
                        set_number(&mut fresh, "id", option.target)?;
                        set_text(&mut fresh, "text", &option.text)?;
                        set_number(&mut fresh, "parameter", option.parameter)?;
                        fresh
                    }
                };
                node.name = format!("[{position}]");
                options.push(node);
            }
            *node.child_mut("options").and_then(Node::array_mut).ok_or("A window has no options list")? = options;
            set_count(&mut node, "option_count", window.options.len())?;
            node.name = format!("[{}]", windows.len());
            windows.push(node);
        }
        *talk.child_mut("windows").and_then(Node::array_mut).ok_or("The talk has no windows list")? = windows;
        set_count(talk, "window_count", order.len())?;

        let after = decoded.encode()?;
        verify_task_root(&self.schema, &after, version, "The dialog change would make this task invalid")?;
        if after == before {
            return Ok(self.edit_state());
        }
        let (task_id, task_name) = task_heading(task_at(&decode_exact(&self.schema, &after, version)?, task_path)?)?;
        let change = RootChange { pack, root, before, after };
        self.apply_changes(std::slice::from_ref(&change))?;
        let talk_label = dialog.talk.replace('_', " ");
        self.journal.record(EntryDetails { label: label.into(), task_id, task_name, field: format!("dialogs › {talk_label}"), old: String::new(), new: format!("{} window{}", order.len(), if order.len() == 1 { "" } else { "s" }) }, vec![change]);
        Ok(self.edit_state())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn rewrites_talks_as_trees_and_round_trips_unchanged_ones() {
        for path in [r"E:/Games/XtremeJade/element/data/tasks.data", r"E:/Games/ForsakenJD/element/data/tasks.data", r"E:/Games/Elite Jade Dynasty - HDN/element/data/tasks.data"] {
            if Path::new(path).is_file() {
                check(path);
            }
        }
    }

    fn check(path: &str) {
        let mut document = TaskDocument::open(path).unwrap();
        while !document.search("", 0).indexed {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let version = document.container.header.version;
        // A top-level quest whose delivery talk has a child window.
        let root = document.summary().roots.into_iter().find(|root| {
            document.dialogs(root.pack, root.root, &[]).unwrap().iter().any(|dialog| dialog.talk == "delivery" && dialog.windows.len() >= 2)
        }).unwrap();
        let original = document.current_root(root.pack, root.root).unwrap();
        let dialogs = document.dialogs(root.pack, root.root, &[]).unwrap();
        let delivery = dialogs.iter().find(|dialog| dialog.talk == "delivery").unwrap().clone();
        assert_eq!(delivery.windows[0].parent_id, ROOT_PARENT);

        // Writing a talk back unchanged changes nothing.
        document.set_dialog(root.pack, root.root, &[], &delivery, "Edit dialog").unwrap();
        assert_eq!(document.current_root(root.pack, root.root).unwrap(), original);

        // A new window opened from a new option on the root, written in tree order.
        let mut edited = delivery.clone();
        let fresh = edited.windows.iter().map(|window| window.id).max().unwrap() + 1;
        edited.windows[0].options.insert(0, DialogOption { target: fresh, text: "Tell me more".into(), parameter: 0 });
        edited.windows.push(DialogWindow { id: fresh, parent_id: 0, text: "Line one\nLine two".into(), options: vec![DialogOption { target: FUNCTION | 17, text: "Back".into(), parameter: 0 }] });
        edited.windows[0].text = "Changed greeting".into();
        document.set_dialog(root.pack, root.root, &[], &edited, "Edit dialog").unwrap();
        let written = document.dialogs(root.pack, root.root, &[]).unwrap().into_iter().find(|dialog| dialog.talk == "delivery").unwrap();
        assert_eq!(written.windows[1].id, fresh, "the new window follows its parent's first option");
        assert_eq!(written.windows[1].parent_id, written.windows[0].id);
        assert_eq!(written.windows[1].text, "Line one\nLine two");
        assert_eq!(written.windows[0].text, "Changed greeting");
        assert_eq!(written.windows.len(), delivery.windows.len() + 1);
        let decoded = decode_exact(&document.schema, &document.current_root(root.pack, root.root).unwrap(), version).unwrap();
        let stored = decoded.child("dialogs").unwrap().child("delivery").unwrap().child("windows").unwrap().children()[1].child("text").unwrap().clone();
        let Value::Text(raw) = &stored.value else { panic!("text") };
        assert!(raw.contains("\r\n"), "line breaks are stored as CR LF");

        // Broken trees are refused.
        let mut lost = edited.clone();
        lost.windows[0].options.remove(0);
        assert!(document.set_dialog(root.pack, root.root, &[], &lost, "Edit dialog").unwrap_err().contains("not opened"));
        let mut long = edited.clone();
        long.windows[0].options[0].text = "x".repeat(64);
        assert!(document.set_dialog(root.pack, root.root, &[], &long, "Edit dialog").is_err());
        let mut missing = edited.clone();
        missing.windows[0].options.push(DialogOption { target: FUNCTION | 6, text: "Accept".into(), parameter: 4_000_000_000 });
        assert!(document.set_dialog(root.pack, root.root, &[], &missing, "Edit dialog").unwrap_err().contains("does not exist"));

        // Clearing a talk, then undoing everything.
        document.set_dialog(root.pack, root.root, &[], &Dialog { talk: "delivery".into(), prompt: String::new(), windows: Vec::new() }, "Clear dialog").unwrap();
        assert!(document.dialogs(root.pack, root.root, &[]).unwrap().iter().find(|dialog| dialog.talk == "delivery").unwrap().windows.is_empty());
        document.undo().unwrap();
        document.undo().unwrap();
        assert_eq!(document.current_root(root.pack, root.root).unwrap(), original);
    }
}
