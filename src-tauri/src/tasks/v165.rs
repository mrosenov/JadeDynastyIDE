//! Static task record layout used by task format v165.
//!
//! The matching client writes a packed 2,490-byte `ATaskTemplFixedData`
//! block followed by sections whose lengths are stored in that block. Fields
//! needed to find those sections are named below. Header ranges whose meaning
//! is not needed yet remain fixed raw bytes; this preserves them exactly and
//! avoids inventing labels while the read-only browser is being built.

use std::collections::BTreeMap;

use super::{
    schema::{Condition, FieldDef, FieldType, Predicate, Schema, StructDef, VersionedSchema},
    structures::{
        self, AWARD_COUNT_SCALE, AWARD_DATA, AWARD_ITEM_SCALE, AWARD_RATIO_SCALE,
        COMPARE_EXPRESSION, INTERACTION_OBJECT_WANTED, ITEM_WANTED, MONSTER_SUMMONED,
        MONSTER_WANTED, TASK_DIALOGS, TASK_TEXTS, TASK_TIME, TEAM_MEMBER_WANTED,
    },
};

pub const VERSION: u32 = 165;
pub const TASK: &str = "TASK_V165";
const FIXED: &str = "TASK_FIXED_V165";
const TIMETABLE: &str = "TASK_TIMETABLE_V165";

fn field(name: &str, ty: FieldType) -> FieldDef {
    FieldDef::new(name, ty)
}

fn named(name: &str) -> FieldType {
    FieldType::Named { name: name.into() }
}

fn raw(len: usize) -> FieldType {
    FieldType::Raw { len }
}

fn array(count_field: &str, item: FieldType) -> FieldType {
    FieldType::CountedArray {
        count_field: count_field.into(),
        item: Box::new(item),
    }
}

fn when_nonzero(field_name: &str) -> Vec<Condition> {
    vec![Condition::Field {
        field: field_name.into(),
        predicate: Predicate::NonZero,
    }]
}

fn conditional(name: &str, ty: FieldType, condition: &str) -> FieldDef {
    FieldDef {
        name: name.into(),
        ty,
        when: when_nonzero(condition),
    }
}

pub(crate) fn fixed_definition() -> StructDef {
    StructDef {
        fields: vec![
            field("id", FieldType::U32),                            // 0000
            field("name", FieldType::FixedUtf16 { units: 30 }),     // 0004
            field("has_signature", FieldType::Bool8),               // 0064
            field("signature_pointer", raw(4)),                     // 0065
            field("task_type", FieldType::U32),                     // 0069
            field("time_limit", FieldType::U32),                    // 0073
            field("absolute_time", FieldType::Bool8),               // 0077
            field("timetable_count", FieldType::U32),               // 0078
            field("timetable_types", FieldType::Bytes { len: 12 }), // 0082
            field("timetable_pointers", raw(8)),                    // 0094
            field("unknown_0102_0484", raw(383)),                   // 0102
            field("change_key_pointers", raw(8)),                   // 0485
            field("change_key_capacity", FieldType::U32),           // 0493
            field("change_key_count", FieldType::U32),              // 0497
            field("change_value_pointers", raw(8)),                 // 0501
            field("change_value_capacity", FieldType::U32),         // 0509
            field("change_value_count", FieldType::U32),            // 0513
            field("change_type_pointers", raw(8)),                  // 0517
            field("change_type_capacity", FieldType::U32),          // 0525
            field("change_type_count", FieldType::U32),             // 0529
            field("unknown_0533_0758", raw(226)),                   // 0533
            field("premise_item_count", FieldType::U32),            // 0759
            field("premise_item_pointer", raw(4)),                  // 0763
            field("show_by_items", FieldType::Bool8),               // 0767
            field("premise_items_not_taken", FieldType::Bool8),     // 0768
            field("summoned_monster_count", FieldType::U32),        // 0769
            field("summon_mode", FieldType::U32),                   // 0773
            field("random_summoned_monster", FieldType::Bool8),     // 0777
            field("summoned_monster_pointer", raw(4)),              // 0778
            field("given_item_count", FieldType::U32),              // 0782
            field("given_common_item_count", FieldType::U32),       // 0786
            field("given_task_item_count", FieldType::U32),         // 0790
            field("given_item_pointer", raw(4)),                    // 0794
            field("premise_title_pointer", raw(4)),                 // 0798
            field("premise_title_count", FieldType::U32),           // 0802
            field("unknown_0806_1317", raw(512)),                   // 0806
            field("teamwork", FieldType::Bool8),                    // 1318
            field("receive_by_team", FieldType::Bool8),             // 1319
            field("shared_task", FieldType::Bool8),                 // 1320
            field("shared_achieved", FieldType::Bool8),             // 1321
            field("check_teammate", FieldType::Bool8),              // 1322
            field("teammate_distance", FieldType::F32),             // 1323
            field("all_fail", FieldType::Bool8),                    // 1327
            field("captain_change_all_fail", FieldType::Bool8),     // 1328
            field("captain_fail", FieldType::Bool8),                // 1329
            field("captain_success", FieldType::Bool8),             // 1330
            field("success_distance", FieldType::F32),              // 1331
            field("all_success", FieldType::Bool8),                 // 1335
            field("dismiss_as_self_fail", FieldType::Bool8),        // 1336
            field("receive_check_members", FieldType::Bool8),       // 1337
            field("receive_member_distance", FieldType::F32),       // 1338
            field("count_by_member_position", FieldType::Bool8),    // 1342
            field("count_member_distance", FieldType::F32),         // 1343
            field("team_member_count", FieldType::U32),             // 1347
            field("team_member_pointer", raw(4)),                   // 1351
            field("show_by_team", FieldType::Bool8),                // 1355
            field("unknown_1356_1644", raw(289)),                   // 1356
            field("premise_compare", FieldType::Bool8),             // 1645
            field("premise_compare_join", FieldType::I32),          // 1646
            field("premise_compare_1", named(COMPARE_EXPRESSION)),  // 1650
            field("premise_compare_2", named(COMPARE_EXPRESSION)),  // 1718
            field("unknown_1786_1944", raw(159)),                   // 1786
            field("monster_wanted_count", FieldType::U32),          // 1945
            field("summon_monster_mode", FieldType::Bool8),         // 1949
            field("monster_wanted_pointer", raw(4)),                // 1950
            field("item_wanted_count", FieldType::U32),             // 1954
            field("item_wanted_pointer", raw(4)),                   // 1958
            field("unknown_1962_1990", raw(29)),                    // 1962
            field("interaction_object_count", FieldType::U32),      // 1991
            field("interaction_object_pointer", raw(4)),            // 1995
            field("unknown_1999_2276", raw(278)),                   // 1999
            field("finish_compare", FieldType::Bool8),              // 2277
            field("finish_compare_join", FieldType::I32),           // 2278
            field("finish_compare_1", named(COMPARE_EXPRESSION)),   // 2282
            field("finish_compare_2", named(COMPARE_EXPRESSION)),   // 2350
            field("unknown_2418_2489", raw(72)),                    // 2418
        ],
    }
}

fn dynamic_bytes(name: &str, count: &str, condition: &str) -> FieldDef {
    conditional(
        name,
        FieldType::CountedBytes {
            count_field: count.into(),
        },
        condition,
    )
}

pub(crate) fn task_definition(
    fixed_name: &str,
    timetable_name: &str,
    task_name: &str,
) -> StructDef {
    let fixed = |name: &str| format!("fixed.{name}");
    let fields = vec![
        field("fixed", named(fixed_name)),
        conditional(
            "signature",
            FieldType::FixedUtf16 { units: 30 },
            &fixed("has_signature"),
        ),
        field(
            "timetables",
            array(&fixed("timetable_count"), named(timetable_name)),
        ),
        conditional(
            "change_keys",
            array(&fixed("change_key_count"), FieldType::I32),
            &fixed("change_key_count"),
        ),
        conditional(
            "change_values",
            array(&fixed("change_key_count"), FieldType::I32),
            &fixed("change_key_count"),
        ),
        conditional(
            "change_types",
            array(&fixed("change_key_count"), FieldType::Bool8),
            &fixed("change_key_count"),
        ),
        field(
            "premise_items",
            array(&fixed("premise_item_count"), named(ITEM_WANTED)),
        ),
        field(
            "summoned_monsters",
            array(&fixed("summoned_monster_count"), named(MONSTER_SUMMONED)),
        ),
        field(
            "premise_titles",
            array(&fixed("premise_title_count"), FieldType::I16),
        ),
        field(
            "given_items",
            array(&fixed("given_item_count"), named(ITEM_WANTED)),
        ),
        FieldDef {
            name: "team_members".into(),
            ty: array(&fixed("team_member_count"), named(TEAM_MEMBER_WANTED)),
            when: when_nonzero(&fixed("teamwork")),
        },
        dynamic_bytes(
            "premise_compare_1_left",
            &fixed("premise_compare_1.left_text_count"),
            &fixed("premise_compare"),
        ),
        dynamic_bytes(
            "premise_compare_1_right",
            &fixed("premise_compare_1.right_text_count"),
            &fixed("premise_compare"),
        ),
        dynamic_bytes(
            "premise_compare_2_left",
            &fixed("premise_compare_2.left_text_count"),
            &fixed("premise_compare"),
        ),
        dynamic_bytes(
            "premise_compare_2_right",
            &fixed("premise_compare_2.right_text_count"),
            &fixed("premise_compare"),
        ),
        field(
            "monsters_wanted",
            array(&fixed("monster_wanted_count"), named(MONSTER_WANTED)),
        ),
        field(
            "items_wanted",
            array(&fixed("item_wanted_count"), named(ITEM_WANTED)),
        ),
        field(
            "interaction_objects_wanted",
            array(
                &fixed("interaction_object_count"),
                named(INTERACTION_OBJECT_WANTED),
            ),
        ),
        dynamic_bytes(
            "finish_compare_1_left",
            &fixed("finish_compare_1.left_text_count"),
            &fixed("finish_compare"),
        ),
        dynamic_bytes(
            "finish_compare_1_right",
            &fixed("finish_compare_1.right_text_count"),
            &fixed("finish_compare"),
        ),
        dynamic_bytes(
            "finish_compare_2_left",
            &fixed("finish_compare_2.left_text_count"),
            &fixed("finish_compare"),
        ),
        dynamic_bytes(
            "finish_compare_2_right",
            &fixed("finish_compare_2.right_text_count"),
            &fixed("finish_compare"),
        ),
        field("success_award", named(AWARD_DATA)),
        field("failure_award", named(AWARD_DATA)),
        field("success_ratio_awards", named(AWARD_RATIO_SCALE)),
        field("failure_ratio_awards", named(AWARD_RATIO_SCALE)),
        field("success_item_awards", named(AWARD_ITEM_SCALE)),
        field("failure_item_awards", named(AWARD_ITEM_SCALE)),
        field("success_count_awards", named(AWARD_COUNT_SCALE)),
        field("failure_count_awards", named(AWARD_COUNT_SCALE)),
        field("texts", named(TASK_TEXTS)),
        field("dialogs", named(TASK_DIALOGS)),
        field("subtask_count", FieldType::I32),
        field(
            "subtasks",
            FieldType::RecursiveArray {
                count_field: "subtask_count".into(),
                target: task_name.into(),
            },
        ),
    ];
    StructDef { fields }
}

pub fn schema() -> Schema {
    schema_with_fixed(FIXED, TIMETABLE, TASK, fixed_definition())
}

pub(crate) fn schema_with_fixed(
    fixed_name: &str,
    timetable_name: &str,
    task_name: &str,
    fixed: StructDef,
) -> Schema {
    let mut definitions: BTreeMap<String, StructDef> = structures::definitions();
    definitions.insert(fixed_name.into(), fixed);
    definitions.insert(
        timetable_name.into(),
        StructDef {
            fields: vec![
                field("start", named(TASK_TIME)),
                field("end", named(TASK_TIME)),
            ],
        },
    );
    definitions.insert(
        task_name.into(),
        task_definition(fixed_name, timetable_name, task_name),
    );
    Schema {
        root: task_name.into(),
        structs: definitions,
    }
}

pub fn versioned_schema() -> VersionedSchema {
    VersionedSchema {
        base_version: VERSION,
        schema: schema(),
        patches: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::tasks::{container::TaskContainer, schema::decode_exact};

    #[test]
    fn fixed_header_is_2490_bytes_at_known_offsets() {
        let schema = Schema {
            root: FIXED.into(),
            structs: schema().structs,
        };
        let bytes = vec![0; 2490];
        let node = decode_exact(&schema, &bytes, VERSION).unwrap();
        assert_eq!(node.byte_len, 2490);
        assert_eq!(node.child("has_signature").unwrap().offset, 64);
        assert_eq!(node.child("change_key_count").unwrap().offset, 497);
        assert_eq!(node.child("premise_item_count").unwrap().offset, 759);
        assert_eq!(node.child("teamwork").unwrap().offset, 1318);
        assert_eq!(node.child("monster_wanted_count").unwrap().offset, 1945);
        assert_eq!(node.child("finish_compare").unwrap().offset, 2277);
    }

    #[test]
    fn first_real_v165_root_round_trips_exactly() {
        let path = Path::new(r"E:\Games\XtremeJade\element\data\tasks.data");
        if !path.exists() {
            eprintln!("skipping missing v165 task fixture: {}", path.display());
            return;
        }
        let tasks = TaskContainer::open(path).unwrap();
        assert_eq!(tasks.header.version, VERSION);
        let bytes = tasks.root(0, 0).unwrap();
        let task = decode_exact(&schema(), &bytes, VERSION).unwrap();
        assert_eq!(task.encode().unwrap(), bytes);
    }

    #[test]
    fn every_real_v165_root_parses_and_round_trips() {
        let path = Path::new(r"E:\Games\XtremeJade\element\data\tasks.data");
        if !path.exists() {
            eprintln!("skipping missing v165 task fixture: {}", path.display());
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
