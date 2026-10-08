//! The task Problems panel: things in a task set the game may trip over, or
//! that look wrong. Every check reads the background task index, so a scan
//! does not decode the packs again.
//!
//! Errors: duplicate task IDs and references to tasks that do not exist.
//! Warnings: tasks whose prerequisite or exclusion lists name themselves,
//! stale hierarchy links, and item/monster IDs the open elements.data lacks.
//! Info: packs at the 300-task limit.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use serde::Serialize;

use super::browser::{label_path_part, TaskDeleteReference, TaskDocument, TaskSearchEntry};
use super::container::ROOTS_PER_PACK;
use crate::elements::problems::Severity;

/// Problems kept per kind (the rest are only counted).
const PER_KIND: usize = 1000;
/// "Referenced by" entries returned for one task.
const REFERRERS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    DuplicateRoot,
    DuplicateId,
    BrokenReference,
    SelfReference,
    HierarchyLinks,
    MissingElement,
    FullPack,
}

impl Kind {
    const ALL: [Kind; 7] = [Kind::DuplicateRoot, Kind::DuplicateId, Kind::BrokenReference, Kind::SelfReference, Kind::HierarchyLinks, Kind::MissingElement, Kind::FullPack];

    fn severity(self) -> Severity {
        match self {
            Kind::DuplicateRoot | Kind::DuplicateId | Kind::BrokenReference => Severity::Error,
            Kind::SelfReference | Kind::HierarchyLinks | Kind::MissingElement => Severity::Warning,
            Kind::FullPack => Severity::Info,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Kind::DuplicateRoot => "Top-level tasks the game skips",
            Kind::DuplicateId => "Duplicate task IDs",
            Kind::BrokenReference => "Broken task references",
            Kind::SelfReference => "Tasks that name themselves",
            Kind::HierarchyLinks => "Stale hierarchy links",
            Kind::MissingElement => "Items and monsters missing from elements.data",
            Kind::FullPack => "Full task packs",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Kind::DuplicateRoot => "A top-level task whose ID an earlier top-level task already uses. The game loads packs in order, keeps the first and skips this one entirely (\"Dup Task Found\").",
            Kind::DuplicateId => "Tasks (top-level or subquests) sharing an ID. The game's map of all tasks keeps only the one loaded last, so lookups by ID find only that task.",
            Kind::BrokenReference => "A prerequisite, exclusion, award or finish-count field names a task ID that no task in the set has.",
            Kind::SelfReference => "A prerequisite or exclusion list names the task itself.",
            Kind::HierarchyLinks => "The parent, sibling and first-child IDs stored in a task differ from its place in the tree. The game recomputes them when loading, but the official editor keeps them exact and other tools read them. Clone, move or delete in the same root rewrites them.",
            Kind::MissingElement => "A field names an item, monster or object ID the open elements.data does not have.",
            Kind::FullPack => "Numbered packs holding 300 top-level tasks, the most a pack can hold. Clone task places new tasks in another pack.",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub kind: Kind,
    pub pack: usize,
    /// The task's root in the pack; none for pack-level problems.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<usize>,
    pub path: Vec<usize>,
    pub id: u32,
    pub name: String,
    /// The field, as "fixed › premise tasks[0]".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// The ID the field names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<u32>,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindSummary {
    pub kind: Kind,
    pub severity: Severity,
    pub title: &'static str,
    pub description: &'static str,
    pub count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub kinds: Vec<KindSummary>,
    pub problems: Vec<Problem>,
    /// Whether some kinds have more problems than listed.
    pub truncated: bool,
    /// Item and monster IDs are checked only while an elements.data is open.
    pub elements_checked: bool,
    pub tasks: usize,
    pub elapsed_ms: u64,
}

#[derive(Default)]
struct Collector {
    counts: HashMap<Kind, usize>,
    problems: Vec<Problem>,
}

impl Collector {
    fn add(&mut self, problem: Problem) {
        let count = self.counts.entry(problem.kind).or_default();
        *count += 1;
        if *count <= PER_KIND {
            self.problems.push(problem);
        }
    }
}

type Key = (usize, usize, Vec<usize>);

fn field_label(field: &str) -> String {
    field.split('.').map(label_path_part).collect::<Vec<_>>().join(" › ")
}

/// "pack 3 · root 12 › subquest 1.2" for messages.
fn location(task: &TaskSearchEntry) -> String {
    let subquest = if task.path.is_empty() { String::new() } else { format!(" › subquest {}", task.path.iter().map(|index| (index + 1).to_string()).collect::<Vec<_>>().join(".")) };
    format!("pack {} · root {}{subquest}", task.pack + 1, task.root + 1)
}

fn problem(kind: Kind, task: &TaskSearchEntry, field: Option<&str>, target: Option<u32>, message: String) -> Problem {
    Problem {
        kind,
        pack: task.pack,
        root: Some(task.root),
        path: task.path.clone(),
        id: task.id,
        name: task.name.clone(),
        field: field.map(field_label),
        target,
        message,
    }
}

/// Parent, previous sibling, next sibling and first child IDs the tree gives a task.
fn expected_links(task: &TaskSearchEntry, ids: &HashMap<Key, u32>) -> [u32; 4] {
    let at = |path: Vec<usize>| ids.get(&(task.pack, task.root, path)).copied().unwrap_or(0);
    let mut first_child = task.path.clone();
    first_child.push(0);
    let Some((&last, parent)) = task.path.split_last() else {
        return [0, 0, 0, at(first_child)];
    };
    let sibling = |index: usize| {
        let mut path = parent.to_vec();
        path.push(index);
        at(path)
    };
    [at(parent.to_vec()), if last > 0 { sibling(last - 1) } else { 0 }, sibling(last + 1), at(first_child)]
}

const LINK_LABELS: [&str; 4] = ["parent", "previous sibling", "next sibling", "first child"];

impl TaskDocument {
    /// The distinct item, monster and object IDs task fields name, for
    /// checking against elements.data without holding both locks.
    pub fn element_reference_ids(&self) -> Result<HashSet<u32>, String> {
        let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
        Ok(index.entries.iter().flat_map(|task| task.references.iter()).filter(|reference| reference.element && reference.target_id != 0).map(|reference| reference.target_id).collect())
    }

    /// Scans the complete task index. \`missing_elements\` holds the element IDs
    /// the open elements.data lacks, or is none when no elements.data is open.
    pub fn problems(&self, missing_elements: Option<&HashSet<u32>>) -> Result<Report, String> {
        let started = Instant::now();
        let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
        if let Some(error) = &index.error {
            return Err(format!("The task index could not be built: {error}"));
        }
        if !index.indexed {
            return Err("Tasks are still being indexed. Scan again in a moment".into());
        }
        let mut entries = index.entries.iter().collect::<Vec<_>>();
        // Load order: packs, then roots, then depth-first through subtasks.
        entries.sort_by(|left, right| (left.pack, left.root, &left.path).cmp(&(right.pack, right.root, &right.path)));
        let ids = entries.iter().map(|task| ((task.pack, task.root, task.path.clone()), task.id)).collect::<HashMap<Key, u32>>();
        let mut by_id = HashMap::<u32, Vec<&TaskSearchEntry>>::new();
        for task in &entries {
            by_id.entry(task.id).or_default().push(task);
        }
        let mut found = Collector::default();

        for task in &entries {
            let same_id = &by_id[&task.id];
            if same_id.len() > 1 {
                let first_top = same_id.iter().find(|other| other.path.is_empty());
                if let Some(first) = first_top.filter(|first| task.path.is_empty() && !std::ptr::eq(**first, *task)) {
                    found.add(problem(Kind::DuplicateRoot, task, None, None, format!("Top-level task {} at {} already uses this ID; the game skips this task when loading.", first.id, location(first))));
                } else {
                    let others = same_id.iter().filter(|other| !std::ptr::eq(**other, *task)).collect::<Vec<_>>();
                    let shown = others.iter().take(3).map(|other| format!("{} ({})", if other.name.is_empty() { "unnamed" } else { &other.name }, location(other))).collect::<Vec<_>>().join(", ");
                    let more = if others.len() > 3 { format!(" and {} more", others.len() - 3) } else { String::new() };
                    found.add(problem(Kind::DuplicateId, task, None, None, format!("ID {} is also used by {shown}{more}.", task.id)));
                }
            }

            for reference in task.references.iter().filter(|reference| reference.target_id != 0) {
                let label = field_label(&reference.field);
                if reference.element {
                    if missing_elements.is_some_and(|missing| missing.contains(&reference.target_id)) {
                        found.add(problem(Kind::MissingElement, task, Some(&reference.field), Some(reference.target_id), format!("{label} → {}: not in the open elements.data.", reference.target_id)));
                    }
                } else if reference.target_id == task.id && (reference.field.starts_with("fixed.premise") || reference.field.starts_with("fixed.mutex")) {
                    found.add(problem(Kind::SelfReference, task, Some(&reference.field), Some(reference.target_id), format!("{label} names this task itself.")));
                } else if !by_id.contains_key(&reference.target_id) {
                    found.add(problem(Kind::BrokenReference, task, Some(&reference.field), Some(reference.target_id), format!("{label} → {}: no task has this ID.", reference.target_id)));
                }
            }

            if let Some(stored) = task.links {
                let expected = expected_links(task, &ids);
                if stored != expected {
                    let differences = (0..4).filter(|slot| stored[*slot] != expected[*slot])
                        .map(|slot| format!("{} {} (stored {})", LINK_LABELS[slot], expected[slot], stored[slot]))
                        .collect::<Vec<_>>().join(", ");
                    found.add(problem(Kind::HierarchyLinks, task, None, None, format!("The tree gives {differences}.")));
                }
            }
        }

        for pack in 0..self.container.packs.len() {
            let roots = self.root_count(pack)?;
            if roots >= ROOTS_PER_PACK {
                found.add(Problem {
                    kind: Kind::FullPack,
                    pack,
                    root: None,
                    path: Vec::new(),
                    id: 0,
                    name: format!("tasks.data{}", pack + 1),
                    field: None,
                    target: None,
                    message: format!("Holds {roots} top-level tasks, the most a pack can hold."),
                });
            }
        }

        let kinds = Kind::ALL.iter().map(|&kind| KindSummary {
            kind,
            severity: kind.severity(),
            title: kind.title(),
            description: kind.description(),
            count: found.counts.get(&kind).copied().unwrap_or(0),
        }).collect::<Vec<_>>();
        Ok(Report {
            truncated: kinds.iter().any(|kind| kind.count > PER_KIND),
            kinds,
            problems: found.problems,
            elements_checked: missing_elements.is_some(),
            tasks: entries.len(),
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// Tasks whose prerequisite, exclusion, award or finish-count fields name \`id\`.
    pub fn referenced_by(&self, id: u32) -> Result<Vec<TaskDeleteReference>, String> {
        let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
        if let Some(error) = &index.error {
            return Err(format!("The task index could not be built: {error}"));
        }
        let mut result = Vec::new();
        'tasks: for task in &index.entries {
            for reference in task.references.iter().filter(|reference| !reference.element && reference.target_id == id) {
                if result.len() == REFERRERS {
                    break 'tasks;
                }
                result.push(TaskDeleteReference {
                    source_id: task.id,
                    source_name: task.name.clone(),
                    pack: task.pack,
                    root: task.root,
                    path: task.path.clone(),
                    field: field_label(&reference.field),
                    target_id: id,
                });
            }
        }
        result.sort_by(|left, right| (left.pack, left.root, &left.path).cmp(&(right.pack, right.root, &right.path)));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn entry(path: &[usize], id: u32) -> TaskSearchEntry {
        TaskSearchEntry { pack: 0, root: 0, path: path.to_vec(), id, name: String::new(), child_count: 0, references: Vec::new(), links: None }
    }

    #[test]
    fn expected_links_follow_the_tree() {
        let tasks = [entry(&[], 10), entry(&[0], 11), entry(&[1], 12), entry(&[1, 0], 13), entry(&[2], 14)];
        let ids = tasks.iter().map(|task| ((task.pack, task.root, task.path.clone()), task.id)).collect::<HashMap<Key, u32>>();
        assert_eq!(expected_links(&tasks[0], &ids), [0, 0, 0, 11]);
        assert_eq!(expected_links(&tasks[1], &ids), [10, 0, 12, 0]);
        assert_eq!(expected_links(&tasks[2], &ids), [10, 11, 14, 13]);
        assert_eq!(expected_links(&tasks[3], &ids), [12, 0, 0, 0]);
        assert_eq!(expected_links(&tasks[4], &ids), [10, 12, 0, 0]);
    }

    #[test]
    fn scans_a_real_task_set_from_the_index() {
        let path = r"E:/Games/XtremeJade/element/data/tasks.data";
        if !Path::new(path).is_file() {
            return;
        }
        let document = TaskDocument::open(path).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(120);
        let report = loop {
            match document.problems(None) {
                Ok(report) => break report,
                Err(error) if error.contains("still being indexed") && Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(error) => panic!("{error}"),
            }
        };
        let count = |kind: Kind| report.kinds.iter().find(|summary| summary.kind == kind).unwrap().count;
        for summary in &report.kinds {
            println!("{:?}: {}", summary.kind, summary.count);
        }
        for problem in report.problems.iter().filter(|problem| problem.kind != Kind::FullPack).take(12) {
            println!("  {:?} {} {}: {}", problem.kind, problem.id, problem.name, problem.message);
        }
        assert!(report.tasks > 27_000);
        // Every task of the official file stores exact hierarchy links.
        assert_eq!(count(Kind::HierarchyLinks), 0);
        // One premise names a task the set does not have (verified by hand).
        assert!(report.problems.iter().any(|problem| problem.kind == Kind::BrokenReference && problem.field.as_deref().is_some_and(|field| field.contains("premise tasks"))));
        assert!(!report.elements_checked && count(Kind::MissingElement) == 0);

        // Referenced by: a premise target lists the task that requires it.
        let index = document.search.read().unwrap();
        let (source, target) = index.entries.iter().find_map(|task| task.references.iter()
            .find(|reference| !reference.element && reference.field.starts_with("fixed.premise_tasks") && index.by_id.contains_key(&reference.target_id))
            .map(|reference| (task.id, reference.target_id))).unwrap();
        drop(index);
        assert!(document.referenced_by(target).unwrap().iter().any(|referrer| referrer.source_id == source && referrer.field.contains("premise tasks")));
    }
}
