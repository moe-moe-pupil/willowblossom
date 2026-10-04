use serde_json::from_str;

use super::{
    participant_from_character,
    regenerate_unrostered_hidden_roles,
    BattleEncounter,
    BattleRoundStore,
};
use crate::{
    hidden_roles::{
        HiddenRoleKind,
        HiddenRoleState,
    },
    napcat::{
        CharacterBuffBaseStats,
        NapcatMessageManager,
        PlayerCharacter,
    },
};

fn fixture(kind: HiddenRoleKind) -> NapcatMessageManager {
    let mut manager: NapcatMessageManager = from_str(r#"{"messages":{}}"#).unwrap();
    let character = PlayerCharacter {
        hp: 4.0,
        max_hp: 20.0,
        hp_regen: 2.0,
        ..Default::default()
    };
    manager
        .player_characters
        .insert("player".to_owned(), character);
    let mut role = HiddenRoleState::new(kind);
    role.stage = 2;
    role.transformed = true;
    manager.hidden_roles.insert("player".to_owned(), role);
    manager
}

#[test]
fn hidden_regen_recovers_once_per_turn_and_caps_hp_without_changing_base_stats() {
    for kind in [HiddenRoleKind::Alien, HiddenRoleKind::Mutant] {
        let mut manager = fixture(kind);
        let character = manager.player_characters.get_mut("player").unwrap();
        character.buff_base_stats = Some(CharacterBuffBaseStats::from_character(
            character,
        ));
        let ids = vec!["player".to_owned(), "player".to_owned()];
        assert!(regenerate_unrostered_hidden_roles(
            &mut manager,
            None,
            &ids
        ));
        let character = &manager.player_characters["player"];
        assert_eq!(character.hp, 11.0);
        assert_eq!(
            character.buff_base_stats.as_ref().unwrap().hp,
            11.0
        );
        assert_eq!(character.max_hp, 20.0);
        assert_eq!(character.hp_regen, 2.0);
        assert!(regenerate_unrostered_hidden_roles(
            &mut manager,
            None,
            &ids
        ));
        assert!(regenerate_unrostered_hidden_roles(
            &mut manager,
            None,
            &ids
        ));
        assert_eq!(
            manager.player_characters["player"].hp,
            20.0
        );
        assert!(!regenerate_unrostered_hidden_roles(
            &mut manager,
            None,
            &ids
        ));
    }
}

#[test]
fn hidden_regen_excludes_disguise_cocoons_dead_characters_and_androids() {
    let ids = vec!["player".to_owned()];
    for kind in [HiddenRoleKind::Alien, HiddenRoleKind::Mutant] {
        let mut manager = fixture(kind);
        manager.hidden_roles.get_mut("player").unwrap().transformed = false;
        assert!(!regenerate_unrostered_hidden_roles(
            &mut manager,
            None,
            &ids
        ));
        assert_eq!(
            manager.player_characters["player"].hp,
            4.0
        );
        manager.hidden_roles.get_mut("player").unwrap().transformed = true;
        manager.player_characters.get_mut("player").unwrap().hp = 0.0;
        assert!(!regenerate_unrostered_hidden_roles(
            &mut manager,
            None,
            &ids
        ));
    }
    let mut manager = fixture(HiddenRoleKind::Alien);
    manager
        .hidden_roles
        .get_mut("player")
        .unwrap()
        .cocoon_turns_remaining = 1;
    manager.hidden_roles.get_mut("player").unwrap().cocoon_hp = 40.0;
    assert!(!regenerate_unrostered_hidden_roles(
        &mut manager,
        None,
        &ids
    ));
    let mut manager = fixture(HiddenRoleKind::Android);
    assert!(!regenerate_unrostered_hidden_roles(
        &mut manager,
        None,
        &ids
    ));
}

#[test]
fn hidden_regen_leaves_rostered_characters_to_encounter_settlement() {
    for active in [false, true] {
        let mut manager = fixture(HiddenRoleKind::Mutant);
        let participant = participant_from_character(
            "player",
            &manager.player_characters["player"],
            &manager,
        );
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                active,
                participants: vec![participant],
                ..Default::default()
            });
        assert!(!regenerate_unrostered_hidden_roles(
            &mut manager,
            Some(&store),
            &["player".to_owned()]
        ));
        assert_eq!(
            manager.player_characters["player"].hp,
            4.0
        );
        assert!(store.next_round("battle"));
        assert_eq!(
            store.encounters["battle"].participants[0].hp,
            if active { 49.0 } else { 56.0 }
        );
    }
}
