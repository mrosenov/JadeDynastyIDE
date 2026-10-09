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
        COMPARE_EXPRESSION, FINISH_TASK_COUNT, INTERACTION_OBJECT_WANTED, ITEM_WANTED, MASTER_APPRENTICE_TASK, MONSTER_CONTROL, MONSTER_SUMMONED, PHASE_PROPERTY, ZONE_VERT,
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

fn fixed_array(len: usize, item: FieldType) -> FieldType {
    FieldType::FixedArray { len, item: Box::new(item) }
}

/// Zone-friendship premises per task: 32 in v165, 48 from v172.
const FRIENDSHIP_V165: usize = 32;

/// The premise block from `m_ulPremise_Deposit` to `m_bPremise_GM` (TaskTempl.h).
/// The task-ID lists are named so references can be followed, checked and
/// remapped; the remaining ranges stay raw. Offsets in comments are v165's.
/// Verified on every task of the v165, v172 and v184 fixtures: counts are at
/// most 5, unused slots are zero, and the IDs name existing tasks.
fn premise_fields() -> Vec<FieldDef> {
    vec![
        field("premise_deposit", FieldType::U32),                // 0806 m_ulPremise_Deposit
        field("show_by_deposit", FieldType::Bool8),              // 0810 m_bShowByDeposit
        field("premise_reputation", FieldType::I32),             // 0811 m_lPremise_Reputation
        field("reputation_deposit", FieldType::Bool8),           // 0815 m_bRepuDeposit
        field("show_by_reputation", FieldType::Bool8),           // 0816 m_bShowByRepu
        field("premise_contribution", FieldType::I32),           // 0817 m_lPremise_Contribution
        field("deposit_contribution", FieldType::Bool8),         // 0821 m_bDepositContribution
        field("premise_family_contrib", FieldType::I32),         // 0822 m_nPremise_FamContrib
        field("premise_family_contrib_max", FieldType::I32),     // 0826 m_nPremFamContribMax
        field("deposit_family_contrib", FieldType::Bool8),       // 0830 m_bDepositFamContrib
        field("premise_battle_score_min", FieldType::I32),       // 0831 m_nPremBattleScoreMin
        field("premise_battle_score_max", FieldType::I32),       // 0835 m_nPremBattleScoreMax
        field("deposit_battle_score", FieldType::Bool8),         // 0839 m_bDepositBattleScore
        field("premise_sj_battle_score", FieldType::I32),        // 0840 m_nPremSJBattleScore
        field("sj_deposit_battle_score", FieldType::Bool8),      // 0844 m_bSJDepostiBattleScore
        field("premise_friendship", fixed_array(FRIENDSHIP_V165, FieldType::I32)),  // 0845
        field("friendship_deposit", FieldType::Bool8),                              // 0973
        field("premise_task_count", FieldType::U32),                                // 0974
        field("premise_tasks", fixed_array(5, FieldType::U32)),                     // 0978 tasks finished first
        field("show_by_premise_task", FieldType::Bool8),                            // 0998
        field("premise_finish_task_count", FieldType::U32),                         // 0999
        field("premise_finish_tasks", fixed_array(5, named(FINISH_TASK_COUNT))),    // 1003 tasks finished N times
        field("premise_global_count", FieldType::U32),                              // 1033
        field("premise_global_task", FieldType::U32),                               // 1037 global finish count of this task
        field("premise_period", FieldType::U32),                 // 1041 m_ulPremise_Period
        field("show_by_period", FieldType::Bool8),               // 1045 m_bShowByPeriod
        field("premise_faction", FieldType::U32),                // 1046 m_ulPremise_Faction
        field("show_by_faction", FieldType::Bool8),              // 1050 m_bShowByFaction
        field("premise_faction_master", FieldType::Bool8),       // 1051 m_bPremise_FactionMaster
        field("gender", FieldType::U32),                         // 1052 m_ulGender
        field("show_by_gender", FieldType::Bool8),               // 1056 m_bShowByGender
        field("occupation_count", FieldType::U32),                    // 1057 m_ulOccupations
        field("occupations", fixed_array(45, FieldType::U32)), // 1061 m_Occupations
        field("show_by_occupation", FieldType::Bool8),           // 1241 m_bShowByOccup
        field("premise_spouse", FieldType::Bool8),               // 1242 m_bPremise_Spouse
        field("show_by_spouse", FieldType::Bool8),               // 1243 m_bShowBySpouse
        field("premise_cotask", FieldType::U32),                                    // 1244
        field("cotask_condition", FieldType::U32),                                  // 1248
        field("mutex_task_count", FieldType::U32),                                  // 1252
        field("mutex_tasks", fixed_array(5, FieldType::U32)),                       // 1256 tasks that exclude this one
        field("living_skill_count", FieldType::I32),                    // 1276 m_nSkillLev
        field("living_skill_levels", fixed_array(4, FieldType::I32)), // 1280 m_lSkillLev
        field("pet_con", FieldType::I32),                        // 1296 m_nPetCon
        field("pet_civ", FieldType::I32),                        // 1300 m_nPetCiv
        field("dynamic_task_type", FieldType::I8),               // 1304 m_DynTaskType
        field("special_award", FieldType::U32),                  // 1305 m_ulSpecialAward
        field("pk_value_min", FieldType::I32),                   // 1309 m_lPKValueMin
        field("pk_value_max", FieldType::I32),                   // 1313 m_lPKValueMax
        field("premise_gm", FieldType::Bool8),                   // 1317 m_bPremise_GM
    ]
}

/// Inserts a field after an inherited one (later versions add header fields).
pub(crate) fn insert_after(definition: &mut StructDef, after: &str, field: FieldDef) {
    let index = definition.fields.iter().position(|candidate| candidate.name == after).unwrap_or_else(|| panic!("fixed definition has no field {after}"));
    definition.fields.insert(index + 1, field);
}

/// v172 and later store 48 zone-friendship premises instead of 32.
pub(crate) fn set_friendship_count(definition: &mut StructDef, count: usize) {
    let field = definition.fields.iter_mut().find(|field| field.name == "premise_friendship").expect("fixed definition has premise_friendship");
    field.ty = fixed_array(count, FieldType::I32);
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
    let mut definition = StructDef {
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
            field("avail_frequency", FieldType::I32),            // 0102 m_lAvailFrequency
            field("time_interval", FieldType::I32),              // 0106 m_lTimeInterval
            field("choose_one", FieldType::Bool8),               // 0110 m_bChooseOne
            field("rand_one", FieldType::Bool8),                 // 0111 m_bRandOne
            field("execute_child_in_order", FieldType::Bool8),   // 0112 m_bExeChildInOrder
            field("parent_also_fail", FieldType::Bool8),         // 0113 m_bParentAlsoFail
            field("parent_also_succ", FieldType::Bool8),         // 0114 m_bParentAlsoSucc
            field("can_give_up", FieldType::Bool8),              // 0115 m_bCanGiveUp
            field("can_redo", FieldType::Bool8),                 // 0116 m_bCanRedo
            field("can_redo_after_failure", FieldType::Bool8),   // 0117 m_bCanRedoAfterFailure
            field("clear_as_give_up", FieldType::Bool8),         // 0118 m_bClearAsGiveUp
            field("need_record", FieldType::Bool8),              // 0119 m_bNeedRecord
            field("fail_as_player_die", FieldType::Bool8),       // 0120 m_bFailAsPlayerDie
            field("max_receiver", FieldType::U32),               // 0121 m_ulMaxReceiver
            field("clear_receiver_type", FieldType::I32),        // 0125 m_nClearReceiverType
            field("clear_receiver_time_interval", FieldType::I32), // 0129 m_lClearReceiverTimeInterval
            field("deliver_in_zone", FieldType::Bool8),          // 0133 m_bDelvInZone
            field("deliver_world", FieldType::U32),              // 0134 m_ulDelvWorld
            field("deliver_min_vertex", named(ZONE_VERT)),       // 0138 m_DelvMinVert
            field("deliver_max_vertex", named(ZONE_VERT)),       // 0150 m_DelvMaxVert
            field("trans_to", FieldType::Bool8),                 // 0162 m_bTransTo
            field("trans_world_id", FieldType::U32),             // 0163 m_ulTransWldId
            field("trans_point", named(ZONE_VERT)),              // 0167 m_TransPt
            field("phase_count", FieldType::I32),                // 0179 m_iPhaseCnt
            field("phase", fixed_array(8, named(PHASE_PROPERTY))), // 0183 m_Phase
            field("monster_control", FieldType::I32),            // 0239 m_lMonsCtrl
            field("trig_control", FieldType::Bool8),             // 0243 m_bTrigCtrl
            field("monster_control_count", FieldType::U32),      // 0244 m_ulMonCtrlCnt
            field("monster_control_0248", fixed_array(8, named(MONSTER_CONTROL))), // 0248 m_MonCtrl
            field("random_monster_control", FieldType::Bool8),   // 0320 m_bRanMonCtrl
            field("auto_deliver", FieldType::Bool8),             // 0321 m_bAutoDeliver
            field("deliver_window_mode", FieldType::Bool8),      // 0322 m_bDeliverWindowMode
            field("death_trig", FieldType::Bool8),               // 0323 m_bDeathTrig
            field("manual_trig", FieldType::Bool8),              // 0324 m_bManualTrig
            field("must_shown", FieldType::Bool8),               // 0325 m_bMustShown
            field("clear_acquired", FieldType::Bool8),           // 0326 m_bClearAcquired
            field("suitable_level", FieldType::U32),             // 0327 m_ulSuitableLevel
            field("show_prompt", FieldType::Bool8),              // 0331 m_bShowPrompt
            field("key_task", FieldType::Bool8),                 // 0332 m_bKeyTask
            field("deliver_npc", FieldType::U32),                // 0333 m_ulDelvNPC
            field("award_npc", FieldType::U32),                  // 0337 m_ulAwardNPC
            field("skill_task", FieldType::Bool8),               // 0341 m_bSkillTask
            field("can_seek_out", FieldType::Bool8),             // 0342 m_bCanSeekOut
            field("show_direction", FieldType::Bool8),           // 0343 m_bShowDirection
            field("storage_weight", FieldType::F32),             // 0344 m_fStorageWeight
            field("rank", FieldType::U32),                       // 0348 m_ulRank
            field("marriage", FieldType::Bool8),                 // 0352 m_bMarriage
            field("faction", FieldType::Bool8),                  // 0353 m_bFaction
            field("shared_by_family", FieldType::Bool8),         // 0354 m_bSharedByFamily
            field("rec_finish_count", FieldType::Bool8),         // 0355 m_bRecFinishCount
            field("rec_finish_count_global", FieldType::Bool8),  // 0356 m_bRecFinishCountGlobal
            field("max_finish_count", FieldType::U32),           // 0357 m_ulMaxFinishCount
            field("finish_clear_time", named(TASK_TIME)),        // 0361 m_FinishClearTime
            field("dynamic_finish_clear_time", FieldType::I32),  // 0385 m_lDynFinishClearTime
            field("finish_time_type", FieldType::I32),           // 0389 m_nFinishTimeType
            field("life_again_reset", FieldType::Bool8),         // 0393 m_bLifeAgainReset
            field("fail_after_logout", FieldType::Bool8),        // 0394 m_bFailAfterLogout
            field("logout_fail_time", FieldType::U32),           // 0395 m_ulLogoutFailTime
            field("absolute_fail", FieldType::Bool8),            // 0399 m_bAbsFail
            field("absolute_fail_time", named(TASK_TIME)),       // 0400 m_tmAbsFailTime
            field("prentice_task", FieldType::Bool8),            // 0424 m_bPrenticeTask
            field("hidden", FieldType::Bool8),                   // 0425 m_bHidden
            field("out_zone_fail", FieldType::Bool8),            // 0426 m_bOutZoneFail
            field("out_zone_world_id", FieldType::U32),          // 0427 m_ulOutZoneWorldID
            field("out_zone_min_vertex", named(ZONE_VERT)),      // 0431 m_OutZoneMinVert
            field("out_zone_max_vertex", named(ZONE_VERT)),      // 0443 m_OutZoneMaxVert
            field("enter_zone_fail", FieldType::Bool8),          // 0455 m_bEnterZoneFail
            field("enter_zone_world_id", FieldType::U32),        // 0456 m_ulEnterZoneWorldID
            field("enter_zone_min_vertex", named(ZONE_VERT)),    // 0460 m_EnterZoneMinVert
            field("enter_zone_max_vertex", named(ZONE_VERT)),    // 0472 m_EnterZoneMaxVert
            field("clear_some_illegal_states", FieldType::Bool8), // 0484 m_bClearSomeIllegalStates
            field("change_key_pointers", raw(8)),                   // 0485
            field("change_key_capacity", FieldType::U32),           // 0493
            field("change_key_count", FieldType::U32),              // 0497
            field("change_value_pointers", raw(8)),                 // 0501
            field("change_value_capacity", FieldType::U32),         // 0509
            field("change_value_count", FieldType::U32),            // 0513
            field("change_type_pointers", raw(8)),                  // 0517
            field("change_type_capacity", FieldType::U32),          // 0525
            field("change_type_count", FieldType::U32),             // 0529
            field("kill_monster_fail", FieldType::Bool8),        // 0533 m_bKillMonsterFail
            field("kill_fail_monster_count", FieldType::U32),          // 0534 m_ulKillFailMonster
            field("kill_fail_monsters", fixed_array(8, FieldType::U32)), // 0538 m_KillFailMonsters
            field("have_item_fail", FieldType::Bool8),           // 0570 m_bHaveItemFail
            field("have_fail_item_count", FieldType::U32),        // 0571 m_ulHaveItemFail
            field("have_fail_items", fixed_array(16, FieldType::U32)), // 0575 m_HaveFailItems
            field("have_item_fail_not_take_off", FieldType::Bool8), // 0639 m_bHaveItemFailNotTakeOff
            field("not_have_item_fail", FieldType::Bool8),       // 0640 m_bNotHaveItemFail
            field("not_have_fail_item_count", FieldType::U32),    // 0641 m_ulNotHaveItemFail
            field("not_have_fail_items", fixed_array(16, FieldType::U32)), // 0645 m_NotHaveFailItems
            field("camera_move", FieldType::U32),                // 0709 m_ulCameraMove
            field("animation", FieldType::U32),                  // 0713 m_ulAnimation
            field("variables", fixed_array(3, FieldType::I32)),  // 0717 m_lVariables
            field("display_type", FieldType::U32),               // 0729 m_ulDisplayType
            field("recommend_type", FieldType::U32),             // 0733 m_ulRecommendType
            field("tiny_game_id", FieldType::U32),               // 0737 m_ulTinyGameID
            field("clear_xp_cd", FieldType::Bool8),              // 0741 m_bClearXpCD
            field("premise_level_min", FieldType::U32),          // 0742 m_ulPremise_Lev_Min
            field("premise_level_max", FieldType::U32),          // 0746 m_ulPremise_Lev_Max
            field("show_by_level", FieldType::Bool8),            // 0750 m_bShowByLev
            field("talisman_value_min", FieldType::I32),         // 0751 m_nTalismanValueMin
            field("talisman_value_max", FieldType::I32),         // 0755 m_nTalismanValueMax
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
            field("premise_block", raw(512)),                       // 0806 replaced by premise_fields()
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
            field("share_work", FieldType::Bool8),               // 1356 m_bShareWork
            field("master", FieldType::Bool8),                   // 1357 m_bMaster
            field("prentice", FieldType::Bool8),                 // 1358 m_bPrentice
            field("master_moral", FieldType::I32),               // 1359 m_lMasterMoral
            field("master_apprentice_task", FieldType::Bool8),   // 1363 m_bMPTask
            field("out_master_task", FieldType::Bool8),          // 1364 m_bOutMasterTask
            field("master_apprentice_task_count", FieldType::U32), // 1365 m_ulMPTaskCnt
            field("master_apprentice_tasks", fixed_array(8, named(MASTER_APPRENTICE_TASK))), // 1369 m_MPTask
            field("in_family", FieldType::Bool8),                // 1433 m_bInFamily
            field("family_header", FieldType::Bool8),            // 1434 m_bFamilyHeader
            field("family_skill_level_min", FieldType::I32),     // 1435 m_nFamilySkillLevelMin
            field("family_skill_level_max", FieldType::I32),     // 1439 m_nFamilySkillLevelMax
            field("family_skill_proficiency_min", FieldType::I32), // 1443 m_nFamilySkillProficiencyMin
            field("family_skill_proficiency_max", FieldType::I32), // 1447 m_nFamilySkillProficiencyMax
            field("family_skill_index", FieldType::I32),         // 1451 m_nFamilySkillIndex
            field("family_monster_record_index", FieldType::I32), // 1455 m_nFamilyMonRecordIndex
            field("family_monster_record_min", FieldType::I32),  // 1459 m_nFamilyMonRecordMin
            field("family_monster_record_max", FieldType::I32),  // 1463 m_nFamilyMonRecordMax
            field("family_value_index", FieldType::I32),         // 1467 m_nFamilyValueIndex
            field("deposit_family_value", FieldType::Bool8),     // 1471 m_bDepositFamilyValue
            field("family_value_min", FieldType::I32),           // 1472 m_nFamilyValueMin
            field("family_value_max", FieldType::I32),           // 1476 m_nFamilyValueMax
            field("check_life_again", FieldType::Bool8),         // 1480 m_bCheckLifeAgain
            field("spouse_again", FieldType::Bool8),             // 1481 m_bSpouseAgain
            field("life_again_count", FieldType::U32),           // 1482 m_ulLifeAgainCnt
            field("life_again_count_compare", FieldType::I32),   // 1486 m_nLifeAgainCntCompare
            field("life_again_one_occupation", fixed_array(45, FieldType::Bool8)), // 1490 m_bLifeAgainOneOccup
            field("life_again_two_occupation", fixed_array(45, FieldType::Bool8)), // 1535 m_bLifeAgainTwoOccup
            field("life_again_thr_occupation", fixed_array(45, FieldType::Bool8)), // 1580 m_bLifeAgainThrOccup
            field("premise_cult", FieldType::U32),               // 1625 m_ulPremCult
            field("consume_treasure_min", FieldType::I32),       // 1629 m_nConsumeTreasureMin
            field("consume_treasure_max", FieldType::I32),       // 1633 m_nConsumeTreasureMax
            field("premise_total_case_add_min", FieldType::I32), // 1637 m_nPremTotalCaseAddMin
            field("premise_total_case_add_max", FieldType::I32), // 1641 m_nPremTotalCaseAddMax
            field("premise_compare", FieldType::Bool8),             // 1645
            field("premise_compare_join", FieldType::I32),          // 1646
            field("premise_compare_1", named(COMPARE_EXPRESSION)),  // 1650
            field("premise_compare_2", named(COMPARE_EXPRESSION)),  // 1718
            field("premise_accompany_count", FieldType::U32),    // 1786 m_ulPremAccompCnt
            field("premise_accompany_id", fixed_array(8, FieldType::U32)), // 1790 m_aPremAccompID
            field("premise_accompany_cond", FieldType::Bool8),   // 1822 m_bPremAccompCond
            field("premise_transform_id", FieldType::I32),       // 1823 m_iPremTransformID
            field("premise_transform_level", FieldType::I32),    // 1827 m_iPremTransformLevel
            field("premise_transform_exp_level", FieldType::I32), // 1831 m_iPremTransformExpLevel
            field("script_open_task", FieldType::Bool8),         // 1835 m_bScriptOpenTask
            field("premise_achievement_min", FieldType::U32),    // 1836 m_ulPremAchievementMin
            field("premise_achievement_max", FieldType::U32),    // 1840 m_ulPremAchievementMax
            field("premise_circle_group_min", FieldType::U32),   // 1844 m_ulPremCircleGroupMin
            field("premise_circle_group_max", FieldType::U32),   // 1848 m_ulPremCircleGroupMax
            field("premise_territory_score_min", FieldType::U32), // 1852 m_ulPremTerritoryScoreMin
            field("premise_territory_score_max", FieldType::U32), // 1856 m_ulPremTerritoryScoreMax
            field("premise_fengshen_type", FieldType::I32),      // 1860 m_nPremFengshenType
            field("premise_fengshen_lvl_min", FieldType::U32),   // 1864 m_ulPremFengshenLvlMin
            field("premise_fengshen_lvl_max", FieldType::U32),   // 1868 m_ulPremFengshenLvlMax
            field("exp_must_full", FieldType::Bool8),            // 1872 m_bExpMustFull
            field("show_by_fengshen_lvl", FieldType::Bool8),     // 1873 m_bShowByFengshenLvl
            field("create_role_time_duration", FieldType::U32),  // 1874 m_ulCreateRoleTimeDuration
            field("build_id", FieldType::I32),                   // 1878 m_nBuildId
            field("build_level", FieldType::I32),                // 1882 m_nBuildLevel
            field("premise_faction_gold_note", FieldType::I32),  // 1886 m_iPremise_FactionGoldNote
            field("show_by_faction_gold_note", FieldType::Bool8), // 1890 m_bShowByFactionGoldNote
            field("premise_faction_grass", FieldType::I32),      // 1891 m_iPremise_FactionGrass
            field("premise_faction_mine", FieldType::I32),       // 1895 m_iPremise_FactionMine
            field("premise_faction_monster_core", FieldType::I32), // 1899 m_iPremise_FactionMonsterCore
            field("premise_faction_monster_food", FieldType::I32), // 1903 m_iPremise_FactionMonsterFood
            field("premise_faction_money", FieldType::I32),      // 1907 m_iPremise_FactionMoney
            field("build_level_in_construct", FieldType::I32),   // 1911 m_nBuildLevelInConstruct
            field("interaction_object_id", FieldType::I32),      // 1915 m_iInterObjId
            field("show_by_interaction_object_id", FieldType::Bool8), // 1919 m_bShowByInterObjId
            field("premise_nation_position_mask", FieldType::U32), // 1920 m_ulPremNationPositionMask
            field("premise_king_score_cost", FieldType::I32),    // 1924 m_nPremKingScoreCost
            field("premise_king_score_max", FieldType::I32),     // 1928 m_nPremKingScoreMax
            field("premise_king_score_min", FieldType::I32),     // 1932 m_nPremKingScoreMin
            field("premise_has_king", FieldType::Bool8),         // 1936 m_bPremHasKing
            field("method", FieldType::U32),                     // 1937 m_enumMethod
            field("finish_type", FieldType::U32),                // 1941 m_enumFinishType
            field("monster_wanted_count", FieldType::U32),          // 1945
            field("summon_monster_mode", FieldType::Bool8),         // 1949
            field("monster_wanted_pointer", raw(4)),                // 1950
            field("item_wanted_count", FieldType::U32),             // 1954
            field("item_wanted_pointer", raw(4)),                   // 1958
            field("gold_wanted", FieldType::U32),                // 1962 m_ulGoldWanted
            field("faction_gold_note_wanted", FieldType::I32),   // 1966 m_iFactionGoldNoteWanted
            field("auto_move_for_collect_num_items", FieldType::Bool8), // 1970 m_bAutoMoveForCollectNumItems
            field("faction_grass_wanted", FieldType::I32),       // 1971 m_iFactionGrassWanted
            field("faction_mine_wanted", FieldType::I32),        // 1975 m_iFactionMineWanted
            field("faction_monster_core_wanted", FieldType::I32), // 1979 m_iFactionMonsterCoreWanted
            field("faction_monster_food_wanted", FieldType::I32), // 1983 m_iFactionMonsterFoodWanted
            field("faction_money_wanted", FieldType::I32),       // 1987 m_iFactionMoneyWanted
            field("interaction_object_count", FieldType::U32),      // 1991
            field("interaction_object_pointer", raw(4)),            // 1995
            field("interaction_reach_site_min", named(ZONE_VERT)), // 1999 m_InterReachSiteMin
            field("interaction_reach_site_max", named(ZONE_VERT)), // 2011 m_InterReachSiteMax
            field("interaction_reach_site_id", FieldType::U32),  // 2023 m_ulInterReachSiteId
            field("interaction_reach_item_id", FieldType::U32),  // 2027 m_iInterReachItemId
            field("interaction_leave_site_min", named(ZONE_VERT)), // 2031 m_InterLeaveSiteMin
            field("interaction_leave_site_max", named(ZONE_VERT)), // 2043 m_InterLeaveSiteMax
            field("interaction_leave_site_id", FieldType::U32),  // 2055 m_ulInterLeaveSiteId
            field("interaction_leave_item_id", FieldType::U32),  // 2059 m_iInterLeaveItemId
            field("building_id_wanted", FieldType::I32),         // 2063 m_iBuildingIdWanted
            field("building_level_wanted", FieldType::I32),      // 2067 m_iBuildingLevelWanted
            field("npc_to_protect", FieldType::U32),             // 2071 m_ulNPCToProtect
            field("protect_time_len", FieldType::U32),           // 2075 m_ulProtectTimeLen
            field("npc_moving", FieldType::U32),                 // 2079 m_ulNPCMoving
            field("npc_dest_site", FieldType::U32),              // 2083 m_ulNPCDestSite
            field("reach_site_min", named(ZONE_VERT)),           // 2087 m_ReachSiteMin
            field("reach_site_max", named(ZONE_VERT)),           // 2099 m_ReachSiteMax
            field("auto_move_dest_pos", named(ZONE_VERT)),       // 2111 m_AutoMoveDestPos
            field("auto_move_for_reach_fixed_site", FieldType::Bool8), // 2123 m_bAutoMoveForReachFixedSite
            field("auto_move_dest_pos_name", FieldType::FixedUtf16 { units: 30 }), // 2124 m_szAutoMoveDestPosName
            field("reach_site_id", FieldType::U32),              // 2184 m_ulReachSiteId
            field("wait_time", FieldType::U32),                  // 2188 m_ulWaitTime
            field("show_wait_time", FieldType::Bool8),           // 2192 m_bShowWaitTime
            field("leave_site_id", FieldType::U32),              // 2193 m_ulLeaveSiteId
            field("leave_site_min", named(ZONE_VERT)),           // 2197 m_LeaveSiteMin
            field("leave_site_max", named(ZONE_VERT)),           // 2209 m_LeaveSiteMax
            field("title_wanted_num", FieldType::U32),           // 2221 m_ulTitleWantedNum
            field("title_wanted", fixed_array(5, FieldType::I16)), // 2225 m_TitleWanted
            field("finish_achievement", FieldType::U32),         // 2235 m_ulFinishAchievement
            field("friend_num", FieldType::U32),                 // 2239 m_ulFriendNum
            field("finish_level", FieldType::U32),               // 2243 m_ulFinishLev
            field("disable_finish_dialog", FieldType::Bool8),    // 2247 m_bDisFinDlg
            field("script_finish_task", FieldType::Bool8),       // 2248 m_bScriptFinishTask
            field("fixed_type", FieldType::I32),                 // 2249 m_iFixedType
            field("fixed_time", named(TASK_TIME)),               // 2253 m_tmFixedTime
            field("finish_compare", FieldType::Bool8),              // 2277
            field("finish_compare_join", FieldType::I32),           // 2278
            field("finish_compare_1", named(COMPARE_EXPRESSION)),   // 2282
            field("finish_compare_2", named(COMPARE_EXPRESSION)),   // 2350
            field("action_npc", FieldType::U32),                 // 2418 m_ulActionNPC
            field("action_id", FieldType::I32),                  // 2422 m_nActionID
            field("total_case_add_min", FieldType::I32),         // 2426 m_nTotalCaseAddMin
            field("total_case_add_max", FieldType::I32),         // 2430 m_nTotalCaseAddMax
            field("award_type_s", FieldType::U32),               // 2434 m_ulAwardType_S
            field("award_type_f", FieldType::U32),               // 2438 m_ulAwardType_F
            field("award_s_pointer", raw(4)),                    // 2442 m_Award_S
            field("award_f_pointer", raw(4)),                    // 2446 m_Award_F
            field("award_by_ratio_s_pointer", raw(4)),           // 2450 m_AwByRatio_S
            field("award_by_ratio_f_pointer", raw(4)),           // 2454 m_AwByRatio_F
            field("award_by_items_s_pointer", raw(4)),           // 2458 m_AwByItems_S
            field("award_by_items_f_pointer", raw(4)),           // 2462 m_AwByItems_F
            field("award_by_count_s_pointer", raw(4)),           // 2466 m_AwByCount_S
            field("award_by_count_f_pointer", raw(4)),           // 2470 m_AwByCount_F
            // Hierarchy links (m_ulParent … m_ulFirstChild), refreshed by
            // ATaskTempl::SynchID before official saves; see LINK_FIELDS.
            field("hierarchy_parent", FieldType::U32),              // 2474
            field("hierarchy_previous_sibling", FieldType::U32),    // 2478
            field("hierarchy_next_sibling", FieldType::U32),        // 2482
            field("hierarchy_first_child", FieldType::U32),         // 2486
        ],
    };
    let at = definition.fields.iter().position(|field| field.name == "premise_block").unwrap();
    definition.fields.splice(at..=at, premise_fields());
    definition
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
