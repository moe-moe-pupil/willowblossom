use serde_json::from_str;

use super::*;
use crate::napcat::{
    EquipmentSlot,
    InventoryItem,
};

fn character_edit_manager() -> NapcatMessageManager {
    let mut manager: NapcatMessageManager = from_str(r#"{"messages":{}}"#).unwrap();
    manager
        .player_characters
        .insert("player".to_owned(), PlayerCharacter {
            name: "Original".to_owned(),
            hp: 5.0,
            max_hp: 10.0,
            mp: 4.0,
            max_mp: 8.0,
            speed: 4.0,
            status: CharacterStatus {
                agi: 3,
                int_: 2,
                wis: 1,
                ..Default::default()
            },
            extra_status: CharacterStatus {
                agi: 2,
                ..Default::default()
            },
            ..Default::default()
        });
    manager
}

fn equip_stat_bonus_item(character: &mut PlayerCharacter) {
    character
        .inventory
        .equipment
        .insert(EquipmentSlot::Feet, InventoryItem {
            name: "疾风靴".to_owned(),
            equipment_slot: EquipmentSlot::Feet,
            stat_effects: vec![
                BuffEffect {
                    field: BuffField::Speed,
                    value: BuffValue::AddPercent(25.0),
                },
                BuffEffect {
                    field: BuffField::MaxHp,
                    value: BuffValue::AddPercent(50.0),
                },
            ],
            ..Default::default()
        });
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.0001,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn normal_character_edits_refresh_battle_stats_without_resetting_combat_progress() {
    let mut manager = character_edit_manager();
    let mut participant = participant_from_character(
        "player",
        &manager.player_characters["player"],
        &manager,
    );
    participant.turn = 7;
    participant.combat_turns_completed = 6;
    participant.action_done = true;
    participant.arcane_shield = 3.5;
    let mut store = BattleRoundStore::default();
    store
        .encounters
        .insert("battle".to_owned(), BattleEncounter {
            round: 6,
            participants: vec![participant],
            ..Default::default()
        });
    let mut stamps = HashMap::new();
    remember_encounter_characters(
        &store.encounters["battle"],
        "battle",
        &manager,
        &mut stamps,
    );

    let character = manager.player_characters.get_mut("player").unwrap();
    character.nickname = "Updated".to_owned();
    character.hp = 12.0;
    character.max_hp = 25.0;
    character.mp = 7.0;
    character.max_mp = 14.0;
    character.status.agi = 9;
    character.speed = 8.5;
    character.level = 4;
    character.exp = 35;
    character.damage_taken_this_turn = 6.0;
    character.healing_taken_this_turn = 2.0;
    character.skill_last_cast_turns = HashMap::from([("0".to_owned(), 6)]);
    character.skill_cooldown_ready_turns = HashMap::from([("0".to_owned(), 9)]);

    assert!(sync_encounter_characters(
        &mut store,
        "battle",
        &manager,
        &mut stamps,
    ));

    let participant = &store.encounters["battle"].participants[0];
    let character = &manager.player_characters["player"];
    assert_eq!(participant.display_name, "Updated");
    assert_close(participant.hp, 12.0);
    assert_close(participant.max_hp, 25.0);
    assert_close(participant.mp, 7.0);
    assert_close(participant.max_mp, 14.0);
    assert_eq!(participant.agi, 11);
    assert_close(participant.speed, 8.5);
    assert_eq!(participant.level, 4);
    assert_eq!(participant.exp, 35);
    assert_close(participant.damage_taken_this_turn, 6.0);
    assert_close(participant.healing_taken_this_turn, 2.0);
    assert_eq!(
        participant.skill_last_used_turns,
        character.skill_last_cast_turns
    );
    assert_eq!(
        participant.skill_cooldown_ready_turns,
        character.skill_cooldown_ready_turns
    );
    assert_eq!(participant.turn, 7);
    assert_eq!(participant.combat_turns_completed, 6);
    assert!(participant.action_done);
    assert_close(participant.arcane_shield, 3.5);
    assert_eq!(store.encounters["battle"].round, 6);
    assert!(!sync_encounter_characters(
        &mut store,
        "battle",
        &manager,
        &mut stamps,
    ));
}

#[test]
fn battle_roster_edits_update_character_and_survive_equipment_buff_resync() {
    for nickname in ["", "Old alias"] {
        let mut manager = character_edit_manager();
        let config = manager.character_stat_config_for_target("player");
        let mut rules = RuleEngineState::default();
        let character = manager.player_characters.get_mut("player").unwrap();
        character.nickname = nickname.to_owned();
        equip_stat_bonus_item(character);
        sync_character_buffs(
            "player",
            character,
            &config,
            &mut rules,
            &[],
        );
        let mut participant = participant_from_character(
            "player",
            &manager.player_characters["player"],
            &manager,
        );
        assert_close(participant.speed, 5.0);
        assert_close(participant.max_hp, 15.0);
        let previous = BattleCharacterEdit::from_participant(&participant);
        participant.display_name = "Renamed".to_owned();
        participant.speed = 8.0;
        participant.max_hp = 12.0;
        participant.max_mp = 9.0;
        participant.hp = 3.0;
        participant.mp = 5.0;

        assert!(apply_battle_character_edit(
            &mut participant,
            &previous,
            &mut manager,
            &mut rules,
        ));

        let character = manager.player_characters.get_mut("player").unwrap();
        assert_eq!(
            character.name,
            if nickname.is_empty() { "Renamed" } else { "Original" }
        );
        assert_eq!(
            character.nickname,
            if nickname.is_empty() { "" } else { "Renamed" }
        );
        for _ in 0..3 {
            assert_close(character.hp, 3.0);
            assert_close(character.mp, 5.0);
            assert_close(character.max_hp, 12.0);
            assert_close(character.max_mp, 9.0);
            assert_close(character.speed, 8.0);
            let base = character.buff_base_stats.as_ref().unwrap();
            assert_close(base.max_hp, 8.0);
            assert_close(base.speed, 6.4);
            sync_character_buffs(
                "player",
                character,
                &config,
                &mut rules,
                &[],
            );
        }
        sync_participant_from_manager_with_vitals(&mut participant, &manager);
        assert_eq!(participant.display_name, "Renamed");
        assert_close(participant.hp, 3.0);
        assert_close(participant.mp, 5.0);
        assert_close(participant.max_hp, 12.0);
        assert_close(participant.max_mp, 9.0);
        assert_close(participant.speed, 8.0);
    }
}

#[test]
fn battle_agi_edit_matches_normal_editor_derived_stats_with_equipment() {
    let mut manager = character_edit_manager();
    let config = manager.character_stat_config_for_target("player");
    let mut rules = RuleEngineState::default();
    let character = manager.player_characters.get_mut("player").unwrap();
    equip_stat_bonus_item(character);
    sync_character_buffs(
        "player",
        character,
        &config,
        &mut rules,
        &[],
    );
    let mut expected = character.clone();
    expected.status.agi += 4;
    refresh_character_derived_stats(
        "player",
        &mut expected,
        &config,
        &mut RuleEngineState::default(),
        &[],
    );
    let mut participant = participant_from_character(
        "player",
        &manager.player_characters["player"],
        &manager,
    );
    let previous = BattleCharacterEdit::from_participant(&participant);
    participant.agi += 4;

    assert!(apply_battle_character_edit(
        &mut participant,
        &previous,
        &mut manager,
        &mut rules,
    ));

    let character = &manager.player_characters["player"];
    assert_eq!(
        character.status.agi,
        expected.status.agi
    );
    assert_eq!(
        character.extra_status.agi,
        expected.extra_status.agi
    );
    assert_close(character.hp, expected.hp);
    assert_close(character.mp, expected.mp);
    assert_close(character.max_hp, expected.max_hp);
    assert_close(character.max_mp, expected.max_mp);
    assert_close(character.hp_regen, expected.hp_regen);
    assert_close(character.mp_regen, expected.mp_regen);
    assert_close(character.speed, expected.speed);
    assert_eq!(
        participant.agi,
        expected.status.agi + expected.extra_status.agi
    );
    assert_close(participant.hp, expected.hp);
    assert_close(participant.mp, expected.mp);
    assert_close(participant.max_hp, expected.max_hp);
    assert_close(participant.max_mp, expected.max_mp);
    assert_close(participant.speed, expected.speed);
}
