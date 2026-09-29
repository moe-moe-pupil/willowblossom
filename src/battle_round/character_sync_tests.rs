use std::collections::HashMap;

use egui::{
    Context,
    Pos2,
    RawInput,
    Rect,
    Vec2,
};
use serde_json::{
    from_str,
    to_value,
};

use super::{
    encounter_ui,
    participant_from_character,
    remember_encounter_characters,
    set_participant_alive_after_manual_edit,
    sync_encounter_characters,
    sync_encounter_to_manager,
    BattleEncounter,
    BattleEncounterEntity,
    BattleRoundStore,
    BattleRoundUiState,
    BattleTargetingLine,
};
use crate::{
    hidden_roles::{
        HiddenRoleKind,
        HiddenRoleState,
    },
    napcat::{
        NapcatMessageManager,
        PlayerCharacter,
        TrpgGroup,
    },
    rule_engine::RuleEngineState,
};

fn character_sync_fixture() -> (NapcatMessageManager, BattleRoundStore) {
    let mut manager: NapcatMessageManager = from_str(r#"{"messages":{}}"#).unwrap();
    manager
        .player_characters
        .insert("player".to_owned(), PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            mp: 10.0,
            max_mp: 10.0,
            ..Default::default()
        });
    manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
        campaign_id: "campaign".to_owned(),
        players: vec!["player".to_owned()],
        ..Default::default()
    });
    let participant = participant_from_character(
        "player",
        &manager.player_characters["player"],
        &manager,
    );
    let mut store = BattleRoundStore::default();
    store
        .encounters
        .insert("battle".to_owned(), BattleEncounter {
            trpg_group: Some("party".to_owned()),
            trpg_campaign_id: Some("campaign".to_owned()),
            participants: vec![participant],
            ..Default::default()
        });
    (manager, store)
}

#[test]
fn encounter_ui_renders_normal_character_edits_without_writing_back_passive_refreshes() {
    let (mut manager, mut store) = character_sync_fixture();
    let context = Context::default();
    let mut ui_state = BattleRoundUiState::default();
    let mut rules = RuleEngineState::default();
    let mut targeting = BattleTargetingLine::default();
    let entity = BattleEncounterEntity {
        id: "battle".to_owned(),
        name: "Battle".to_owned(),
        active: true,
        round: 0,
        negative_enabled: false,
    };

    for (hp, mp, agi) in [(8.0, 4.0, 12), (0.0, 0.0, 15)] {
        let character = manager.player_characters.get_mut("player").unwrap();
        character.hp = hp;
        character.mp = mp;
        character.status.agi = agi;
        let manager_before_render = to_value(&manager).unwrap();
        let mut encounter_changed = false;
        let mut manager_changed = false;
        let _ = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1280.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                encounter_changed |= encounter_ui(
                    ui,
                    &mut ui_state,
                    &mut store,
                    &mut manager,
                    &mut rules,
                    None,
                    &mut targeting,
                    &entity,
                    &mut manager_changed,
                );
            },
        );

        assert!(encounter_changed);
        assert!(!manager_changed);
        assert_eq!(
            to_value(&manager).unwrap(),
            manager_before_render
        );
        let participant = &store.encounters["battle"].participants[0];
        assert_eq!(participant.hp, hp);
        assert_eq!(participant.mp, mp);
        assert_eq!(participant.agi, agi);
        assert_eq!(participant.alive, hp > 0.0);
    }
}

#[test]
fn live_character_sync_does_not_reapply_hidden_role_healing_or_revive_defeated_players() {
    let (mut manager, mut store) = character_sync_fixture();
    let mut role = HiddenRoleState::new(HiddenRoleKind::Alien);
    role.transformed = true;
    manager.hidden_roles.insert("player".to_owned(), role);
    store.encounters.get_mut("battle").unwrap().participants[0] = participant_from_character(
        "player",
        &manager.player_characters["player"],
        &manager,
    );
    assert_eq!(
        store.encounters["battle"].participants[0].max_hp,
        40.0
    );
    let mut stamps = HashMap::new();
    remember_encounter_characters(
        &store.encounters["battle"],
        "battle",
        &manager,
        &mut stamps,
    );

    for (hp, alive) in [(7.0, true), (0.0, false)] {
        let participant = &mut store.encounters.get_mut("battle").unwrap().participants[0];
        participant.hp = hp;
        participant.mp = 3.0;
        set_participant_alive_after_manual_edit(participant, alive);
        assert!(sync_encounter_to_manager(
            store.encounters.get("battle"),
            &mut manager
        ));
        remember_encounter_characters(
            &store.encounters["battle"],
            "battle",
            &manager,
            &mut stamps,
        );

        manager
            .player_characters
            .get_mut("player")
            .unwrap()
            .nickname = format!("Updated while HP is {hp}");
        sync_encounter_characters(
            &mut store,
            "battle",
            &manager,
            &mut stamps,
        );
        for _ in 0..3 {
            let participant = &store.encounters["battle"].participants[0];
            assert_eq!(participant.hp, hp);
            assert_eq!(participant.mp, 3.0);
            assert_eq!(participant.max_hp, 40.0);
            assert_eq!(participant.alive, alive);
            assert!(!sync_encounter_characters(
                &mut store,
                "battle",
                &manager,
                &mut stamps
            ));
        }
    }
}

#[test]
fn first_live_character_sync_preserves_persisted_hidden_role_damage_and_defeat() {
    for (hp, alive) in [(7.0, true), (0.0, false)] {
        let (mut manager, mut store) = character_sync_fixture();
        let mut role = HiddenRoleState::new(HiddenRoleKind::Alien);
        role.transformed = true;
        manager.hidden_roles.insert("player".to_owned(), role);
        let participant = &mut store.encounters.get_mut("battle").unwrap().participants[0];
        *participant = participant_from_character(
            "player",
            &manager.player_characters["player"],
            &manager,
        );
        participant.hp = hp;
        participant.mp = 3.0;
        set_participant_alive_after_manual_edit(participant, alive);
        let mut stamps = HashMap::new();

        sync_encounter_characters(
            &mut store,
            "battle",
            &manager,
            &mut stamps,
        );

        let participant = &store.encounters["battle"].participants[0];
        assert_eq!(participant.hp, hp);
        assert_eq!(participant.mp, 3.0);
        assert_eq!(participant.alive, alive);
        assert_eq!(participant.max_hp, 40.0);
    }
}

#[test]
fn live_character_sync_applies_explicit_hidden_role_vital_edits_including_death() {
    let (mut manager, mut store) = character_sync_fixture();
    let mut role = HiddenRoleState::new(HiddenRoleKind::Alien);
    role.transformed = true;
    manager.hidden_roles.insert("player".to_owned(), role);
    store.encounters.get_mut("battle").unwrap().participants[0] = participant_from_character(
        "player",
        &manager.player_characters["player"],
        &manager,
    );
    let participant = &mut store.encounters.get_mut("battle").unwrap().participants[0];
    participant.hp = 7.0;
    participant.mp = 3.0;
    sync_encounter_to_manager(
        store.encounters.get("battle"),
        &mut manager,
    );
    let mut stamps = HashMap::new();
    remember_encounter_characters(
        &store.encounters["battle"],
        "battle",
        &manager,
        &mut stamps,
    );

    for (hp, mp, alive) in [(8.0, 4.0, true), (0.0, 0.0, false)] {
        let character = manager.player_characters.get_mut("player").unwrap();
        character.hp = hp;
        character.mp = mp;
        assert!(sync_encounter_characters(
            &mut store,
            "battle",
            &manager,
            &mut stamps,
        ));

        let participant = &store.encounters["battle"].participants[0];
        assert_eq!(participant.hp, hp);
        assert_eq!(participant.mp, mp);
        assert_eq!(participant.alive, alive);
        assert_eq!(participant.max_hp, 40.0);
        sync_encounter_to_manager(
            store.encounters.get("battle"),
            &mut manager,
        );
        assert_eq!(
            manager.player_characters["player"].hp,
            hp
        );
        assert_eq!(
            manager.player_characters["player"].mp,
            mp
        );
    }
}

#[test]
fn live_character_sync_respects_quarantine_campaign_and_canonical_encounter_guards() {
    for guard in ["quarantined", "deleted group", "old campaign", "duplicate"] {
        let (mut manager, mut store) = character_sync_fixture();
        let mut stamps = HashMap::new();
        remember_encounter_characters(
            &store.encounters["battle"],
            "battle",
            &manager,
            &mut stamps,
        );
        manager.player_characters.get_mut("player").unwrap().hp = 2.0;
        match guard {
            "quarantined" => {
                store
                    .encounters
                    .get_mut("battle")
                    .unwrap()
                    .manager_sync_quarantined = true;
            },
            "deleted group" => {
                manager.trpg_groups.remove("party");
            },
            "old campaign" => {
                manager.trpg_groups.get_mut("party").unwrap().campaign_id =
                    "replacement".to_owned();
            },
            "duplicate" => {
                let newer = BattleEncounter {
                    round: 1,
                    ..store.encounters["battle"].clone()
                };
                store.encounters.insert("newer-battle".to_owned(), newer);
            },
            _ => unreachable!(),
        }

        assert!(
            !sync_encounter_characters(
                &mut store,
                "battle",
                &manager,
                &mut stamps
            ),
            "guard {guard} must prevent character synchronization",
        );
        assert_eq!(
            store.encounters["battle"].participants[0].hp, 20.0,
            "{guard}"
        );
        assert_eq!(
            manager.player_characters["player"].hp, 2.0,
            "{guard}"
        );
    }
}

#[test]
fn live_character_sync_does_not_refresh_players_outside_the_linked_group() {
    let (mut manager, mut store) = character_sync_fixture();
    let mut stamps = HashMap::new();
    remember_encounter_characters(
        &store.encounters["battle"],
        "battle",
        &manager,
        &mut stamps,
    );
    manager
        .trpg_groups
        .get_mut("party")
        .unwrap()
        .players
        .clear();
    manager.player_characters.get_mut("player").unwrap().hp = 2.0;

    assert!(!sync_encounter_characters(
        &mut store,
        "battle",
        &manager,
        &mut stamps
    ));
    assert_eq!(
        store.encounters["battle"].participants[0].hp,
        20.0
    );
}
