//! Static task record layout used by task format v184.

use super::{
    schema::{FieldDef, FieldType, Schema},
    v165,
};

pub const VERSION: u32 = 184;
pub const TASK: &str = "TASK_V184";
const FIXED: &str = "TASK_FIXED_V184";
const TIMETABLE: &str = "TASK_TIMETABLE_V184";

/// Builds the v184 layout from the proven v165 section order. v184 includes
/// all v172 header growth plus four task-header `u32` values. Five values
/// introduced in v174 are stored after the dialog bundle.
pub fn schema() -> Schema {
    let mut fixed = v165::fixed_definition();
    resize_raw(&mut fixed, "unknown_0102_0484", 384);
    v165::set_friendship_count(&mut fixed, 48);
    resize_raw(&mut fixed, "unknown_1786_1944", 171);
    insert_after(
        &mut fixed,
        "signature_pointer",
        FieldDef::new("unknown_v184_0", FieldType::U32),
    );

    let mut schema = v165::schema_with_fixed(FIXED, TIMETABLE, TASK, fixed);
    let task = schema.structs.get_mut(TASK).unwrap();
    let subtask = task
        .fields
        .iter()
        .position(|field| field.name == "subtask_count")
        .unwrap();
    for index in 0..5 {
        task.fields.insert(
            subtask + index,
            FieldDef::new(format!("unknown_v174_{}", index + 1), FieldType::U32),
        );
    }
    schema
}

fn resize_raw(definition: &mut super::schema::StructDef, name: &str, len: usize) {
    let field = definition
        .fields
        .iter_mut()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("v184 schema cannot find inherited field {name}"));
    field.ty = FieldType::Raw { len };
}

fn insert_after(definition: &mut super::schema::StructDef, after: &str, field: FieldDef) {
    let index = definition
        .fields
        .iter()
        .position(|candidate| candidate.name == after)
        .unwrap_or_else(|| panic!("v184 schema cannot find inherited field {after}"));
    definition.fields.insert(index + 1, field);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::tasks::{container::TaskContainer, schema::decode_exact};

    #[test]
    fn fixed_header_is_2571_bytes() {
        let full = schema();
        let fixed = Schema {
            root: FIXED.into(),
            structs: full.structs,
        };
        let node = decode_exact(&fixed, &vec![0; 2571], VERSION).unwrap();
        assert_eq!(node.byte_len, 2571);
        assert_eq!(node.child("unknown_v184_0").unwrap().offset, 69);
        assert_eq!(node.child("teamwork").unwrap().offset, 1387);
        assert_eq!(node.child("monster_wanted_count").unwrap().offset, 2026);
        assert_eq!(node.child("finish_compare").unwrap().offset, 2358);
    }

    #[test]
    fn first_real_v184_root_round_trips_exactly() {
        let path = Path::new(r"E:\Games\Elite Jade Dynasty - HDN\element\data\tasks.data");
        if !path.exists() {
            eprintln!("skipping missing v184 task fixture: {}", path.display());
            return;
        }
        let tasks = TaskContainer::open(path).unwrap();
        assert_eq!(tasks.header.version, VERSION);
        let bytes = tasks.root(0, 0).unwrap();
        let task = decode_exact(&schema(), &bytes, VERSION).unwrap();
        assert_eq!(task.encode().unwrap(), bytes);
    }

    #[test]
    fn rare_v184_dialog_and_reward_variants_round_trip() {
        let path = Path::new(r"E:\Games\Elite Jade Dynasty - HDN\element\data\tasks.data");
        if !path.exists() {
            return;
        }
        let tasks = TaskContainer::open(path).unwrap();
        let schema = schema();
        for (pack, root) in [(1, 190), (55, 134)] {
            let bytes = tasks.root(pack, root).unwrap();
            if (pack, root) == (55, 134) {
                let mut prefix_schema = schema.clone();
                let fields = &mut prefix_schema.structs.get_mut(TASK).unwrap().fields;
                let end = fields
                    .iter()
                    .position(|field| field.name == "success_award")
                    .unwrap()
                    + 1;
                fields.truncate(end);
                let (_, used) =
                    crate::tasks::schema::decode_prefix(&prefix_schema, &bytes, VERSION).unwrap();
                assert_eq!(
                    used, 3745,
                    "v184 candidate reward ended at the wrong offset"
                );
            }
            let task = decode_exact(&schema, &bytes, VERSION).unwrap();
            assert_eq!(task.encode().unwrap(), bytes);
        }
    }

    fn verify_fixture(path: &Path) {
        if !path.exists() {
            eprintln!("skipping missing v184 task fixture: {}", path.display());
            return;
        }
        let tasks = TaskContainer::open(path).unwrap();
        let schema = schema();
        let mut checked = 0usize;
        for (pack_index, pack) in tasks.packs.iter().enumerate() {
            for root_index in 0..pack.root_count() {
                let bytes = tasks.root(pack_index, root_index).unwrap();
                let task = decode_exact(&schema, &bytes, VERSION).unwrap_or_else(|error| {
                    panic!(
                        "{}{} root {}: {error}",
                        path.display(),
                        pack_index + 1,
                        root_index + 1
                    )
                });
                assert_eq!(
                    task.encode().unwrap(),
                    bytes,
                    "{}{} root {} changed during a no-edit round trip",
                    path.display(),
                    pack_index + 1,
                    root_index + 1,
                );
                checked += 1;
            }
        }
        assert_eq!(checked, tasks.header.root_count as usize);
    }

    #[test]
    fn every_hdn_v184_root_parses_and_round_trips() {
        verify_fixture(Path::new(
            r"E:\Games\Elite Jade Dynasty - HDN\element\data\tasks.data",
        ));
    }

    #[test]
    fn every_server_v184_root_parses_and_round_trips() {
        verify_fixture(Path::new(r"E:\Game Dev\JD\1792\gamed\config\tasks.data"));
    }
}
