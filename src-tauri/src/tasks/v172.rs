//! Static task record layout used by task format v172.

use super::{
    schema::{FieldType, Schema},
    v165,
};

pub const VERSION: u32 = 172;
pub const TASK: &str = "TASK_V172";
const FIXED: &str = "TASK_FIXED_V172";
const TIMETABLE: &str = "TASK_TIMETABLE_V172";

/// v172 retains v165's section order. Its packed header adds the one-byte
/// kermis flag and expands the zone-friendship array from 32 to 48 integers.
pub fn schema() -> Schema {
    let mut fixed = v165::fixed_definition();
    resize_raw(&mut fixed, "unknown_0102_0484", 384);
    resize_raw(&mut fixed, "unknown_0806_1317", 576);
    v165::schema_with_fixed(FIXED, TIMETABLE, TASK, fixed)
}

fn resize_raw(definition: &mut super::schema::StructDef, name: &str, len: usize) {
    let field = definition
        .fields
        .iter_mut()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("v172 schema cannot find inherited field {name}"));
    field.ty = FieldType::Raw { len };
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::tasks::{container::TaskContainer, schema::decode_exact};

    #[test]
    fn fixed_header_is_2555_bytes() {
        let full = schema();
        let fixed = Schema {
            root: FIXED.into(),
            structs: full.structs,
        };
        let node = decode_exact(&fixed, &vec![0; 2555], VERSION).unwrap();
        assert_eq!(node.byte_len, 2555);
        assert_eq!(node.child("teamwork").unwrap().offset, 1383);
        assert_eq!(node.child("monster_wanted_count").unwrap().offset, 2010);
        assert_eq!(node.child("finish_compare").unwrap().offset, 2342);
    }

    #[test]
    fn first_real_v172_root_round_trips_exactly() {
        let path = Path::new(r"E:\Games\ForsakenJD\element\data\tasks.data");
        if !path.exists() {
            eprintln!("skipping missing v172 task fixture: {}", path.display());
            return;
        }
        let tasks = TaskContainer::open(path).unwrap();
        assert_eq!(tasks.header.version, VERSION);
        let bytes = tasks.root(0, 0).unwrap();
        let task = decode_exact(&schema(), &bytes, VERSION).unwrap();
        assert_eq!(task.encode().unwrap(), bytes);
    }

    #[test]
    fn every_real_v172_root_parses_and_round_trips() {
        let path = Path::new(r"E:\Games\ForsakenJD\element\data\tasks.data");
        if !path.exists() {
            eprintln!("skipping missing v172 task fixture: {}", path.display());
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
                        "tasks.data{} root {}: {error}",
                        pack_index + 1,
                        root_index + 1
                    )
                });
                assert_eq!(
                    task.encode().unwrap(),
                    bytes,
                    "tasks.data{} root {} changed during a no-edit round trip",
                    pack_index + 1,
                    root_index + 1,
                );
                checked += 1;
            }
        }
        assert_eq!(checked, tasks.header.root_count as usize);
    }
}
