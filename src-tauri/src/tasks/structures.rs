//! Shared binary structures used by static task records.
//!
//! Their order and widths come from the v165 client source and are checked
//! against Jade Editor's symmetric reader/writer. Task structs are packed, so
//! one-byte booleans have no alignment padding between fields.

use std::collections::BTreeMap;

use super::schema::{
    Condition, CountWidth, FieldDef, FieldType, Predicate, StructDef, TextLengthUnit,
};

pub const ZONE_VERT: &str = "ZONE_VERT";
pub const TASK_REGION: &str = "TASK_REGION";
pub const TASK_TIME: &str = "TASK_TIME";
pub const TASK_EXPRESSION: &str = "TASK_EXPRESSION";
pub const COMPARE_EXPRESSION: &str = "COMPARE_EXPRESSION";
pub const ITEM_WANTED: &str = "ITEM_WANTED";
pub const MONSTER_WANTED: &str = "MONSTER_WANTED";
pub const MONSTER_SUMMONED: &str = "MONSTER_SUMMONED";
pub const TEAM_MEMBER_WANTED: &str = "TEAM_MEMBER_WANTED";
pub const INTERACTION_OBJECT_WANTED: &str = "INTERACTION_OBJECT_WANTED";
pub const MASTER_APPRENTICE_TASK: &str = "MASTER_APPRENTICE_TASK";
pub const MONSTER_CONTROL: &str = "MONSTER_CONTROL";
pub const PHASE_AWARD: &str = "PHASE_AWARD";
pub const PHASE_PROPERTY: &str = "PHASE_PROPERTY";
pub const AUCTION_AWARD: &str = "AUCTION_AWARD";
pub const FINISH_TASK_COUNT: &str = "FINISH_TASK_COUNT";
pub const AWARD_ITEM_CANDIDATE: &str = "AWARD_ITEM_CANDIDATE";
pub const AWARD_DATA: &str = "AWARD_DATA";
pub const AWARD_RATIO_SCALE: &str = "AWARD_RATIO_SCALE";
pub const AWARD_ITEM_SCALE: &str = "AWARD_ITEM_SCALE";
pub const AWARD_COUNT_SCALE: &str = "AWARD_COUNT_SCALE";
pub const TASK_TEXTS: &str = "TASK_TEXTS";
pub const TALK_OPTION: &str = "TASK_TALK_OPTION";
pub const TALK_WINDOW: &str = "TASK_TALK_WINDOW";
pub const TALK: &str = "TASK_TALK";
pub const TASK_DIALOGS: &str = "TASK_DIALOGS";

fn field(name: &str, ty: FieldType) -> FieldDef {
    FieldDef::new(name, ty)
}

fn named(name: &str) -> FieldType {
    FieldType::Named { name: name.into() }
}

fn since(version: u32) -> Vec<Condition> {
    vec![Condition::Version {
        min: Some(version),
        max: None,
    }]
}

fn definition(fields: Vec<FieldDef>) -> StructDef {
    StructDef { fields }
}

fn fixed_array(len: usize, item: FieldType) -> FieldType {
    FieldType::FixedArray {
        len,
        item: Box::new(item),
    }
}

fn counted_array(count_field: &str, item: FieldType) -> FieldType {
    FieldType::CountedArray {
        count_field: count_field.into(),
        item: Box::new(item),
    }
}

/// Archosaur's 32-bit `abase::vector` stores two pointers, capacity and size.
/// Pointer bytes are preserved but have no meaning after the file is closed.
fn vector_fields(prefix: &str) -> Vec<FieldDef> {
    vec![
        field(&format!("{prefix}_data_pointer"), FieldType::Raw { len: 4 }),
        field(
            &format!("{prefix}_finish_pointer"),
            FieldType::Raw { len: 4 },
        ),
        field(&format!("{prefix}_capacity"), FieldType::U32),
        field(&format!("{prefix}_count"), FieldType::U32),
    ]
}

fn compare_expression() -> StructDef {
    let mut fields = vector_fields("left_text");
    fields.extend(vector_fields("left_tokens"));
    fields.push(field("operator", FieldType::I32));
    fields.extend(vector_fields("right_text"));
    fields.extend(vector_fields("right_tokens"));
    definition(fields)
}

fn award_data() -> StructDef {
    let mut fields = vec![
        field("gold", FieldType::U32),
        field("experience", FieldType::U64),
        FieldDef {
            name: "unknown_v175_1".into(),
            ty: FieldType::I32,
            when: since(175),
        },
        field("experience_coefficient", FieldType::F32),
        field("experience_coefficient_2", FieldType::F32),
        field("experience_coefficient_3", FieldType::F32),
        field("new_task_id", FieldType::U32),
        field("terminate_task_count", FieldType::U32),
        field("terminate_task_ids", fixed_array(8, FieldType::U32)),
        field("camera_move_id", FieldType::U32),
        field("animation_id", FieldType::U32),
        field("circle_group_points", FieldType::U32),
        field("spirit", FieldType::U32),
        field("reputation", FieldType::U32),
        field("contribution", FieldType::I32),
        field("faction_gold_note", FieldType::I32),
        field("faction_grass", FieldType::I32),
        field("faction_mine", FieldType::I32),
        field("faction_monster_core", FieldType::I32),
        field("faction_monster_food", FieldType::I32),
        field("faction_money", FieldType::I32),
        field("building_progress", FieldType::Bool8),
        field("extra_faction_gold_note", FieldType::I32),
        field("extra_faction_grass", FieldType::I32),
        field("extra_faction_mine", FieldType::I32),
        field("extra_faction_monster_core", FieldType::I32),
        field("extra_faction_monster_food", FieldType::I32),
        field("extra_faction_money", FieldType::I32),
        field("travel_item_id", FieldType::I32),
        field("travel_time", FieldType::I32),
        field("travel_speed", FieldType::F32),
        field("travel_path", FieldType::I32),
        field("family_contribution", FieldType::I32),
        field("prosperity", FieldType::U32),
        field("title_id", FieldType::I32),
        field("pk_value", FieldType::I32),
        field("reset_pk_value", FieldType::Bool8),
        field("divorce", FieldType::Bool8),
        FieldDef {
            name: "friendships_v165".into(),
            ty: fixed_array(32, FieldType::I32),
            when: vec![Condition::Version {
                min: None,
                max: Some(171),
            }],
        },
        FieldDef {
            name: "friendships".into(),
            ty: fixed_array(48, FieldType::I32),
            when: since(172),
        },
        field("friendship_reset_selection", FieldType::U32),
        field("new_period", FieldType::U32),
        field("new_relay_station", FieldType::U32),
        field("storehouse_size", FieldType::U32),
        field("faction_storehouse_size", FieldType::U32),
        field("inventory_size", FieldType::I32),
        field("pocket_size", FieldType::I32),
        field("pet_inventory_size", FieldType::U32),
        field("mount_inventory_size", FieldType::U32),
        field("fury_limit", FieldType::U32),
        field("set_produce_skill", FieldType::Bool8),
        field("produce_skill_experience", FieldType::U32),
        field("new_profession", FieldType::U32),
        field("rebirth_count", FieldType::U32),
        field("rebirth_faction", FieldType::U32),
        field("teleport_world_id", FieldType::U32),
        field("teleport_position", named(ZONE_VERT)),
        field("monster_controller", FieldType::I32),
        field("trigger_controller", FieldType::Bool8),
        field("buff_id", FieldType::I32),
        field("buff_level", FieldType::I32),
        field("family_skill_proficiency", FieldType::I32),
        field("family_skill_level", FieldType::I32),
        field("family_skill_index", FieldType::I32),
        field("family_monster_record_index", FieldType::I32),
        field("family_value_index", FieldType::I32),
        field("family_value", FieldType::I32),
        field("send_message", FieldType::Bool8),
        field("message_channel", FieldType::I32),
        field("clear_count_task", FieldType::U32),
        field("double_experience_time", FieldType::U32),
        FieldDef {
            name: "unknown_v184_1".into(),
            ty: FieldType::U32,
            when: since(184),
        },
        FieldDef {
            name: "unknown_v184_2".into(),
            ty: FieldType::U32,
            when: since(184),
        },
        field("special_award_type", FieldType::U32),
        field("special_award_id", FieldType::U32),
        field("candidate_count", FieldType::U32),
        field("candidates_pointer", FieldType::Raw { len: 4 }),
        field("master_moral", FieldType::I32),
        field("leave_master", FieldType::Bool8),
        field("deviate_master", FieldType::Bool8),
        field("apprentice_gets_master_experience", FieldType::Bool8),
        field("master_gets_moral", FieldType::Bool8),
        field("award_selected_role", FieldType::Bool8),
        field("selected_role", FieldType::U32),
        field("selected_role_award_pointer", FieldType::Raw { len: 4 }),
        field("set_cultivation", FieldType::Bool8),
        field("cultivation", FieldType::U32),
        field("clear_cultivation_skill", FieldType::U32),
        field("clear_skill_points", FieldType::Bool8),
        field("clear_book_points", FieldType::Bool8),
        field("parameter_expression_selection", FieldType::I32),
        field("parameter_expression_size", FieldType::U32),
        field("parameter_expression_pointer", FieldType::Raw { len: 4 }),
        field("parameter_token_count_fixed", FieldType::U32),
        field("parameter_tokens_pointer", FieldType::Raw { len: 4 }),
    ];
    fields.extend(vector_fields("change_key"));
    fields.extend(vector_fields("change_value"));
    fields.extend(vector_fields("change_type"));
    fields.extend([
        field("multi_global_key", FieldType::I32),
        field("battle_score", FieldType::I32),
        field("reset_battle_score", FieldType::Bool8),
        field("clear_no_key_active_task", FieldType::Bool8),
        field("monster_control_count", FieldType::U32),
        field("monster_controls", fixed_array(8, named(MONSTER_CONTROL))),
        field("random_monster_control", FieldType::Bool8),
        field("transform_id", FieldType::I32),
        field("transform_duration", FieldType::I32),
        field("transform_level", FieldType::I32),
        field("transform_experience_level", FieldType::I32),
        field("transform_cover", FieldType::Bool8),
        field("fengshen_experience", FieldType::U32),
        field("fengshen_trial", FieldType::Bool8),
        field("open_soul_equipment", FieldType::Bool8),
        field("bonus", FieldType::U32),
        field("battle_score_award", FieldType::U32),
        field("faction_candidate_count", FieldType::U32),
        field("faction_candidates_pointer", FieldType::Raw { len: 4 }),
        field("extra_candidate_count", FieldType::U32),
        field("extra_candidates_pointer", FieldType::Raw { len: 4 }),
        field("extra_monster_control_count", FieldType::U32),
        field(
            "extra_monster_controls",
            fixed_array(8, named(MONSTER_CONTROL)),
        ),
        field("random_extra_monster_control", FieldType::Bool8),
        field("send_extra_message", FieldType::Bool8),
        field("extra_message_channel", FieldType::I32),
        field("extra_tribute_pointer", FieldType::Raw { len: 4 }),
        field("check_global_finish_count", FieldType::Bool8),
        field("global_finish_count_precondition", FieldType::U32),
        field("check_global_expression", FieldType::Bool8),
        field("global_expression", named(COMPARE_EXPRESSION)),
        field("variables", fixed_array(3, FieldType::I32)),
        field("king_score", FieldType::I32),
        field("clear_experience_cooldown", FieldType::Bool8),
        field("phase_count", FieldType::I32),
        field("phases", fixed_array(8, named(PHASE_AWARD))),
        field("auction_count", FieldType::I32),
        field("auctions", fixed_array(8, named(AUCTION_AWARD))),
        FieldDef {
            name: "open_star_soul".into(),
            ty: FieldType::Bool8,
            when: since(172),
        },
        FieldDef {
            name: "star_soul_value".into(),
            ty: FieldType::I32,
            when: since(172),
        },
        FieldDef {
            name: "unknown_v184_block".into(),
            ty: FieldType::Raw { len: 52 },
            when: since(184),
        },
        FieldDef {
            name: "unknown_v184_3".into(),
            ty: FieldType::I32,
            when: since(184),
        },
        FieldDef {
            name: "unknown_v184_4".into(),
            ty: FieldType::I32,
            when: since(184),
        },
        field(
            "candidates",
            counted_array("candidate_count", named(AWARD_ITEM_CANDIDATE)),
        ),
        FieldDef {
            name: "selected_role_award".into(),
            ty: named(AWARD_DATA),
            when: vec![
                Condition::Field {
                    field: "award_selected_role".into(),
                    predicate: Predicate::NonZero,
                },
                Condition::Field {
                    field: "selected_role".into(),
                    predicate: Predicate::NonZero,
                },
            ],
        },
        FieldDef {
            name: "parameter_expression".into(),
            ty: FieldType::CountedBytes {
                count_field: "parameter_expression_size".into(),
            },
            when: when_nonzero("parameter_expression_size"),
        },
        FieldDef {
            name: "parameter_token_count".into(),
            ty: FieldType::U32,
            when: when_nonzero("parameter_expression_size"),
        },
        FieldDef {
            name: "parameter_tokens".into(),
            ty: counted_array("parameter_token_count", named(TASK_EXPRESSION)),
            when: when_nonzero("parameter_expression_size"),
        },
        FieldDef {
            name: "change_keys".into(),
            ty: counted_array("change_key_count", FieldType::I32),
            when: when_nonzero("change_key_count"),
        },
        FieldDef {
            name: "change_values".into(),
            ty: counted_array("change_key_count", FieldType::I32),
            when: when_nonzero("change_key_count"),
        },
        FieldDef {
            name: "change_types".into(),
            ty: counted_array("change_key_count", FieldType::Bool8),
            when: when_nonzero("change_key_count"),
        },
        FieldDef {
            name: "global_expression_left".into(),
            ty: FieldType::CountedBytes {
                count_field: "global_expression.left_text_count".into(),
            },
            when: when_nonzero("check_global_expression"),
        },
        FieldDef {
            name: "global_expression_right".into(),
            ty: FieldType::CountedBytes {
                count_field: "global_expression.right_text_count".into(),
            },
            when: when_nonzero("check_global_expression"),
        },
        field(
            "faction_candidates",
            counted_array("faction_candidate_count", named(AWARD_ITEM_CANDIDATE)),
        ),
        field(
            "extra_candidates",
            counted_array("extra_candidate_count", named(AWARD_ITEM_CANDIDATE)),
        ),
        field(
            "extra_tribute",
            FieldType::PrefixedUtf16 {
                prefix: CountWidth::U32,
                unit: TextLengthUnit::Utf16Units,
                terminated: false,
            },
        ),
        FieldDef {
            name: "unknown_v184_candidates".into(),
            ty: counted_array("unknown_v184_3", named(AWARD_ITEM_CANDIDATE)),
            when: since(184),
        },
    ]);
    definition(fields)
}

fn award_ratio_scale() -> StructDef {
    definition(vec![
        field("scale_count", FieldType::U32),
        field("ratios", fixed_array(5, FieldType::F32)),
        field("awards", counted_array("scale_count", named(AWARD_DATA))),
    ])
}

fn award_item_scale() -> StructDef {
    definition(vec![
        field("scale_count", FieldType::U32),
        field("item_id", FieldType::U32),
        field("counts", fixed_array(5, FieldType::U32)),
        field("awards", counted_array("scale_count", named(AWARD_DATA))),
    ])
}

fn award_count_scale() -> StructDef {
    definition(vec![
        field("scale_count", FieldType::U32),
        field("counts", fixed_array(5, FieldType::U32)),
        field("awards", counted_array("scale_count", named(AWARD_DATA))),
    ])
}

/// Definitions that can be merged into each concrete task-version schema.
pub fn definitions() -> BTreeMap<String, StructDef> {
    BTreeMap::from([
        (
            ZONE_VERT.into(),
            definition(vec![
                field("x", FieldType::F32),
                field("y", FieldType::F32),
                field("z", FieldType::F32),
            ]),
        ),
        (
            TASK_REGION.into(),
            definition(vec![
                field("min_x", FieldType::F32),
                field("min_y", FieldType::F32),
                field("min_z", FieldType::F32),
                field("max_x", FieldType::F32),
                field("max_y", FieldType::F32),
                field("max_z", FieldType::F32),
            ]),
        ),
        (
            TASK_TIME.into(),
            definition(vec![
                field("year", FieldType::I32),
                field("month", FieldType::I32),
                field("day", FieldType::I32),
                field("hour", FieldType::I32),
                field("minute", FieldType::I32),
                field("weekday", FieldType::I32),
            ]),
        ),
        (
            TASK_EXPRESSION.into(),
            definition(vec![
                field("type", FieldType::I32),
                field("value", FieldType::F32),
            ]),
        ),
        (COMPARE_EXPRESSION.into(), compare_expression()),
        (
            ITEM_WANTED.into(),
            definition(vec![
                field("item_id", FieldType::U32),
                field("common_item", FieldType::Bool8),
                field("amount", FieldType::U32),
                field("probability", FieldType::F32),
                field("bound", FieldType::Bool8),
                field("period", FieldType::I32),
                field("timetable", FieldType::Bool8),
                field("day_of_week", FieldType::U8),
                field("hour", FieldType::U8),
                field("minute", FieldType::U8),
                FieldDef {
                    name: "refine_condition".into(),
                    ty: FieldType::U8,
                    when: since(139),
                },
                FieldDef {
                    name: "refine_level".into(),
                    ty: FieldType::U32,
                    when: since(139),
                },
                FieldDef {
                    name: "replacement_item_id".into(),
                    ty: FieldType::U32,
                    when: since(159),
                },
            ]),
        ),
        (
            MONSTER_WANTED.into(),
            definition(vec![
                field("monster_id", FieldType::U32),
                field("amount", FieldType::U32),
                field("drop_item_id", FieldType::U32),
                field("drop_item_amount", FieldType::U32),
                field("drop_common_item", FieldType::Bool8),
                field("drop_probability", FieldType::F32),
                field("killer_level", FieldType::Bool8),
            ]),
        ),
        (
            MONSTER_SUMMONED.into(),
            definition(vec![
                field("monster_id", FieldType::U32),
                field("is_monster", FieldType::Bool8),
                field("amount", FieldType::U32),
                field("map_id", FieldType::U32),
                field("position", named(ZONE_VERT)),
                field("period", FieldType::I32),
            ]),
        ),
        (
            TEAM_MEMBER_WANTED.into(),
            definition(vec![
                field("minimum_level", FieldType::U32),
                field("maximum_level", FieldType::U32),
                field("race", FieldType::U32),
                field("occupation", FieldType::U32),
                field("gender", FieldType::U32),
                field("rebirth_count", FieldType::U32),
                field("same_family", FieldType::Bool8),
                field("minimum_count", FieldType::U32),
                field("maximum_count", FieldType::U32),
                field("task_id", FieldType::U32),
            ]),
        ),
        (
            INTERACTION_OBJECT_WANTED.into(),
            definition(vec![
                field("object_id", FieldType::U32),
                field("amount", FieldType::U32),
            ]),
        ),
        (
            MASTER_APPRENTICE_TASK.into(),
            definition(vec![
                field("level_limit", FieldType::U32),
                field("task_id", FieldType::U32),
            ]),
        ),
        (
            MONSTER_CONTROL.into(),
            definition(vec![
                field("controller_id", FieldType::I32),
                field("probability", FieldType::F32),
                field("open", FieldType::Bool8),
            ]),
        ),
        (
            PHASE_AWARD.into(),
            definition(vec![
                field("phase_id", FieldType::I32),
                field("open", FieldType::Bool8),
            ]),
        ),
        (
            PHASE_PROPERTY.into(),
            definition(vec![
                field("phase_id", FieldType::I32),
                field("open", FieldType::Bool8),
                field("trigger", FieldType::Bool8),
                field("visual", FieldType::Bool8),
            ]),
        ),
        (
            AUCTION_AWARD.into(),
            definition(vec![
                field("item_id", FieldType::U32),
                field("probability", FieldType::F32),
            ]),
        ),
        (
            FINISH_TASK_COUNT.into(),
            definition(vec![
                field("task_id", FieldType::U32),
                field("count", FieldType::U16),
            ]),
        ),
        (
            AWARD_ITEM_CANDIDATE.into(),
            definition(vec![
                field("random_choice", FieldType::Bool8),
                field("item_count", FieldType::U32),
                field(
                    "items",
                    FieldType::CountedArray {
                        count_field: "item_count".into(),
                        item: Box::new(named(ITEM_WANTED)),
                    },
                ),
            ]),
        ),
        (AWARD_DATA.into(), award_data()),
        (AWARD_RATIO_SCALE.into(), award_ratio_scale()),
        (AWARD_ITEM_SCALE.into(), award_item_scale()),
        (AWARD_COUNT_SCALE.into(), award_count_scale()),
        (
            TASK_TEXTS.into(),
            definition(
                [
                    "description",
                    "success_text",
                    "failure_text",
                    "tribute",
                    "hint",
                    "can_deliver_text",
                ]
                .into_iter()
                .map(|name| {
                    field(
                        name,
                        FieldType::PrefixedUtf16 {
                            prefix: CountWidth::U32,
                            unit: TextLengthUnit::Utf16Units,
                            terminated: false,
                        },
                    )
                })
                .collect(),
            ),
        ),
        (
            TALK_OPTION.into(),
            definition(vec![
                field("id", FieldType::U32),
                field("text", FieldType::FixedUtf16 { units: 64 }),
                field("parameter", FieldType::U32),
            ]),
        ),
        (
            TALK_WINDOW.into(),
            definition(vec![
                field("id", FieldType::U32),
                field("parent_id", FieldType::U32),
                field("text_length", FieldType::I32),
                field(
                    "text",
                    FieldType::CountedUtf16 {
                        count_field: "text_length".into(),
                        unit: TextLengthUnit::Utf16Units,
                    },
                ),
                FieldDef {
                    name: "parameter_1".into(),
                    ty: FieldType::I32,
                    when: since(174),
                },
                FieldDef {
                    name: "parameter_text".into(),
                    ty: FieldType::CountedUtf16 {
                        count_field: "parameter_1".into(),
                        unit: TextLengthUnit::Utf16Units,
                    },
                    when: since(174),
                },
                FieldDef {
                    name: "parameter_2".into(),
                    ty: FieldType::I32,
                    when: since(174),
                },
                field("option_count", FieldType::I32),
                field(
                    "options",
                    FieldType::CountedArray {
                        count_field: "option_count".into(),
                        item: Box::new(named(TALK_OPTION)),
                    },
                ),
            ]),
        ),
        (
            TALK.into(),
            definition(vec![
                field("id", FieldType::U32),
                field("prompt", FieldType::FixedUtf16 { units: 64 }),
                field("window_count", FieldType::I32),
                field(
                    "windows",
                    FieldType::CountedArray {
                        count_field: "window_count".into(),
                        item: Box::new(named(TALK_WINDOW)),
                    },
                ),
            ]),
        ),
        (
            TASK_DIALOGS.into(),
            definition(
                [
                    "delivery",
                    "unqualified",
                    "item_delivery",
                    "execution",
                    "award",
                ]
                .into_iter()
                .map(|name| field(name, named(TALK)))
                .collect(),
            ),
        ),
    ])
}

/// Convenience condition for sections controlled by a preceding boolean or
/// count. Concrete task schemas use this for optional structures.
pub fn when_nonzero(field: &str) -> Vec<Condition> {
    vec![Condition::Field {
        field: field.into(),
        predicate: Predicate::NonZero,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{decode_exact, Schema, Value};

    fn schema(root: &str) -> Schema {
        Schema {
            root: root.into(),
            structs: definitions(),
        }
    }

    #[test]
    fn packed_shared_structs_have_the_verified_v165_widths() {
        let cases = [
            (ZONE_VERT, 12),
            (TASK_REGION, 24),
            (TASK_TIME, 24),
            (TASK_EXPRESSION, 8),
            (COMPARE_EXPRESSION, 68),
            (ITEM_WANTED, 31),
            (MONSTER_WANTED, 22),
            (MONSTER_SUMMONED, 29),
            (TEAM_MEMBER_WANTED, 37),
            (INTERACTION_OBJECT_WANTED, 8),
            (MASTER_APPRENTICE_TASK, 8),
            (MONSTER_CONTROL, 9),
            (PHASE_AWARD, 5),
            (PHASE_PROPERTY, 7),
            (AUCTION_AWARD, 8),
            (FINISH_TASK_COUNT, 6),
            (AWARD_DATA, 961),
            (AWARD_RATIO_SCALE, 24),
            (AWARD_ITEM_SCALE, 28),
            (AWARD_COUNT_SCALE, 24),
        ];
        for (name, width) in cases {
            let bytes = vec![0; width];
            let decoded = decode_exact(&schema(name), &bytes, 165)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(decoded.byte_len, width, "{name}");
            assert_eq!(decoded.encode().unwrap(), bytes, "{name}");
        }
    }

    #[test]
    fn item_fields_follow_the_task_version() {
        let bytes_138 = vec![0; 22];
        let bytes_139 = vec![0; 27];
        let bytes_159 = vec![0; 31];
        assert_eq!(
            decode_exact(&schema(ITEM_WANTED), &bytes_138, 138)
                .unwrap()
                .byte_len,
            22
        );
        assert_eq!(
            decode_exact(&schema(ITEM_WANTED), &bytes_139, 139)
                .unwrap()
                .byte_len,
            27
        );
        assert_eq!(
            decode_exact(&schema(ITEM_WANTED), &bytes_159, 159)
                .unwrap()
                .byte_len,
            31
        );
    }

    #[test]
    fn task_dialogs_round_trip_variable_windows_and_options() {
        let mut talk = Vec::new();
        talk.extend_from_slice(&70u32.to_le_bytes());
        talk.extend("Prompt".encode_utf16().flat_map(|unit| unit.to_le_bytes()));
        talk.resize(4 + 128, 0);
        talk.extend_from_slice(&1i32.to_le_bytes());
        talk.extend_from_slice(&1u32.to_le_bytes());
        talk.extend_from_slice(&u32::MAX.to_le_bytes());
        talk.extend_from_slice(&5i32.to_le_bytes());
        talk.extend("Hello".encode_utf16().flat_map(|unit| unit.to_le_bytes()));
        talk.extend_from_slice(&1i32.to_le_bytes());
        talk.extend_from_slice(&0x8000_0001u32.to_le_bytes());
        talk.extend("Accept".encode_utf16().flat_map(|unit| unit.to_le_bytes()));
        talk.resize(talk.len() + (64 - 6) * 2, 0);
        talk.extend_from_slice(&99u32.to_le_bytes());

        let decoded = decode_exact(&schema(TALK), &talk, 165).unwrap();
        assert_eq!(
            decoded.child("prompt").unwrap().value,
            Value::Text("Prompt".into())
        );
        let window = &decoded.child("windows").unwrap().children()[0];
        assert_eq!(
            window.child("text").unwrap().value,
            Value::Text("Hello".into())
        );
        assert_eq!(window.child("options").unwrap().children().len(), 1);
        assert_eq!(decoded.encode().unwrap(), talk);
    }

    #[test]
    fn award_candidates_use_versioned_items() {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&vec![0; 31 * 2]);
        let decoded = decode_exact(&schema(AWARD_ITEM_CANDIDATE), &bytes, 165).unwrap();
        assert_eq!(decoded.child("items").unwrap().children().len(), 2);
        assert_eq!(decoded.encode().unwrap(), bytes);
    }

    #[test]
    fn award_dynamic_sections_follow_the_fixed_v165_header() {
        let mut bytes = vec![0; 957];
        bytes[430..434].copy_from_slice(&1u32.to_le_bytes());
        bytes[470..474].copy_from_slice(&2u32.to_le_bytes());
        bytes[498..502].copy_from_slice(&1u32.to_le_bytes());
        bytes[652..656].copy_from_slice(&1u32.to_le_bytes());
        bytes[660..664].copy_from_slice(&1u32.to_le_bytes());
        bytes[759] = 1;
        bytes[772..776].copy_from_slice(&1u32.to_le_bytes());
        bytes[808..812].copy_from_slice(&2u32.to_le_bytes());

        bytes.push(1); // regular candidate: random choice
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&[b'x', b'+']);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&7i32.to_le_bytes());
        bytes.extend_from_slice(&2.5f32.to_le_bytes());
        bytes.extend_from_slice(&11i32.to_le_bytes());
        bytes.extend_from_slice(&22i32.to_le_bytes());
        bytes.push(1);
        bytes.push(b'L');
        bytes.extend_from_slice(b"RR");
        bytes.push(0); // faction candidate
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(0); // extra candidate
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend("OK".encode_utf16().flat_map(|unit| unit.to_le_bytes()));

        let decoded = decode_exact(&schema(AWARD_DATA), &bytes, 165).unwrap();
        assert_eq!(decoded.child("candidates").unwrap().children().len(), 1);
        assert_eq!(
            decoded.child("parameter_tokens").unwrap().children().len(),
            1
        );
        assert_eq!(decoded.child("change_keys").unwrap().children().len(), 1);
        assert_eq!(
            decoded.child("global_expression_left").unwrap().value,
            Value::Bytes(vec![b'L'])
        );
        assert_eq!(
            decoded.child("extra_tribute").unwrap().value,
            Value::Text("OK".into())
        );
        assert_eq!(decoded.encode().unwrap(), bytes);
    }

    #[test]
    fn scaled_awards_can_contain_complete_award_records() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 20]);
        bytes.extend_from_slice(&[0; 961]);
        let decoded = decode_exact(&schema(AWARD_RATIO_SCALE), &bytes, 165).unwrap();
        assert_eq!(decoded.child("awards").unwrap().children().len(), 1);
        assert_eq!(decoded.encode().unwrap(), bytes);
    }
}
