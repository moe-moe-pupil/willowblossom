use serde_json::{
    from_str,
    to_string,
    to_value,
};

use super::{
    over_time::{
        apply_gm_dot,
        remove_gm_dot,
        GmOverTimeDraft,
        GmOverTimeKind,
        GM_DOT_SOURCE_ID,
    },
    participant_from_character,
    sync_encounter_from_group_clock,
    sync_encounter_to_manager,
    BattleDelayedDamageTick,
    BattleEncounter,
    BattleParticipantSnapshot,
    BattleRoundStore,
    CombatLogKind,
};
use crate::{
    napcat::{
        NapcatMessageManager,
        PlayerCharacter,
        TrpgGroup,
    },
    rule_engine::DamageType,
};

fn dot_participant() -> BattleParticipantSnapshot {
    from_str(r#"{"target_id":"target","display_name":"目标","hp":30.0,"max_hp":30.0}"#).unwrap()
}

fn dot_store(participants: Vec<BattleParticipantSnapshot>) -> BattleRoundStore {
    let mut store = BattleRoundStore::default();
    store
        .encounters
        .insert("battle".to_owned(), BattleEncounter {
            participants,
            ..Default::default()
        });
    store
}

fn dot_draft(rounds: i32) -> GmOverTimeDraft {
    GmOverTimeDraft {
        kind: GmOverTimeKind::Damage,
        target_id: "target".to_owned(),
        amount: 3.0,
        rounds,
        damage_type: DamageType::Poisoning,
    }
}

#[test]
fn gm_dot_ticks_exactly_once_per_round_for_one_or_three_rounds() {
    for rounds in [1, 3] {
        let mut store = dot_store(vec![dot_participant()]);
        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(apply_gm_dot(
            encounter,
            &dot_draft(rounds)
        ));
        assert_eq!(encounter.participants[0].hp, 30.0);

        for round in 1..=rounds + 1 {
            assert!(store.next_round("battle"));
            let encounter = &store.encounters["battle"];
            let target = &encounter.participants[0];
            let ticks = round.min(rounds);
            assert_eq!(target.hp, 30.0 - 3.0 * ticks as f32);
            assert_eq!(
                encounter
                    .combat_log
                    .iter()
                    .filter(|entry| entry.kind == CombatLogKind::Damage)
                    .count(),
                ticks as usize
            );
            if round < rounds {
                assert_eq!(
                    target.delayed_damage_ticks[0].turns_remaining,
                    rounds - round
                );
            } else {
                assert!(target.delayed_damage_ticks.is_empty());
            }
        }
    }
}

#[test]
fn gm_dot_applies_equally_to_player_npc_and_summon_without_spending_actions() {
    let mut player = dot_participant();
    player.player_character = true;
    let mut npc = dot_participant();
    npc.unit_template_id = Some("template".to_owned());
    npc.unit_character = Some(PlayerCharacter {
        hp: 30.0,
        max_hp: 30.0,
        ..Default::default()
    });
    let mut summon = dot_participant();
    summon.is_summon = true;
    summon.summon_owner_id = Some("owner".to_owned());
    summon.unit_character = npc.unit_character.clone();

    for target in [player, npc, summon] {
        let mut store = dot_store(vec![target]);
        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(apply_gm_dot(encounter, &dot_draft(1)));
        assert_eq!(encounter.round, 0);
        assert_eq!(encounter.participants[0].turn, 0);
        assert!(!encounter.participants[0].action_done);
        assert!(store.next_round("battle"));
        assert_eq!(
            store.encounters["battle"].participants[0].hp,
            27.0
        );
    }
}

#[test]
fn gm_dot_persists_remaining_rounds_and_expires_after_restore() {
    let mut store = dot_store(vec![dot_participant()]);
    assert!(apply_gm_dot(
        store.encounters.get_mut("battle").unwrap(),
        &dot_draft(3),
    ));
    assert!(store.next_round("battle"));

    let encoded = to_string(&store).unwrap();
    let mut restored: BattleRoundStore = from_str(&encoded).unwrap();
    let tick = &restored.encounters["battle"].participants[0].delayed_damage_ticks[0];
    assert!(tick.repeat_each_round);
    assert_eq!(tick.turns_remaining, 2);
    assert_eq!(tick.damage_type, DamageType::Poisoning);
    assert_eq!(tick.source_id, GM_DOT_SOURCE_ID);

    for expected_hp in [24.0, 21.0, 21.0] {
        assert!(restored.next_round("battle"));
        assert_eq!(
            restored.encounters["battle"].participants[0].hp,
            expected_hp
        );
    }
    assert!(
        restored.encounters["battle"].participants[0]
            .delayed_damage_ticks
            .is_empty()
    );
}

#[test]
fn cancelling_gm_dot_preserves_legacy_one_shot_damage() {
    let mut target = dot_participant();
    let legacy: BattleDelayedDamageTick = from_str(
        r#"{"name":"延迟伤害","source_id":"caster","source_name":"施法者","amount":4.0,"damage_type":"physical","turns_remaining":2}"#,
    )
    .unwrap();
    assert!(!legacy.repeat_each_round);
    target.delayed_damage_ticks.push(legacy.clone());
    let mut store = dot_store(vec![target]);
    let encounter = store.encounters.get_mut("battle").unwrap();
    assert!(apply_gm_dot(encounter, &dot_draft(3)));
    assert!(!remove_gm_dot(encounter, "target", 0));
    assert!(!remove_gm_dot(encounter, "target", 99));
    assert!(!remove_gm_dot(encounter, "missing", 1));
    assert!(remove_gm_dot(encounter, "target", 1));
    assert_eq!(
        encounter.participants[0].delayed_damage_ticks,
        vec![legacy]
    );

    assert!(store.next_round("battle"));
    assert_eq!(
        store.encounters["battle"].participants[0].hp,
        26.0
    );
    assert!(store.next_round("battle"));
    assert_eq!(
        store.encounters["battle"].participants[0].hp,
        26.0
    );
}

#[test]
fn gm_dot_rejects_invalid_inputs_dead_targets_and_quarantined_encounters() {
    for (amount, rounds, target_id, alive, quarantined) in [
        (0.0, 3, "target", true, false),
        (-1.0, 3, "target", true, false),
        (f32::NAN, 3, "target", true, false),
        (f32::INFINITY, 3, "target", true, false),
        (3.0, 0, "target", true, false),
        (3.0, -1, "target", true, false),
        (3.0, 3, "missing", true, false),
        (3.0, 3, "target", false, false),
        (3.0, 3, "target", true, true),
    ] {
        let mut target = dot_participant();
        target.alive = alive;
        if !alive {
            target.hp = 0.0;
        }
        let mut encounter = BattleEncounter {
            participants: vec![target],
            manager_sync_quarantined: quarantined,
            ..Default::default()
        };
        let before = to_value(&encounter).unwrap();
        assert!(!apply_gm_dot(
            &mut encounter,
            &GmOverTimeDraft {
                kind: GmOverTimeKind::Damage,
                target_id: target_id.to_owned(),
                amount,
                rounds,
                damage_type: DamageType::Bleed,
            },
        ));
        assert_eq!(to_value(&encounter).unwrap(), before);
    }
}

#[test]
fn gm_dot_uses_fixed_damage_with_shields_and_records_effective_damage() {
    let mut target = dot_participant();
    target.arcane_shield = 2.0;
    target.damage_taken_modifier = 0.1;
    let mut store = dot_store(vec![target]);
    assert!(apply_gm_dot(
        store.encounters.get_mut("battle").unwrap(),
        &dot_draft(1),
    ));
    assert!(store.next_round("battle"));

    let encounter = &store.encounters["battle"];
    let target = &encounter.participants[0];
    assert_eq!(target.arcane_shield, 0.0);
    assert_eq!(target.hp, 29.0);
    assert_eq!(target.damage_taken_this_turn, 1.0);
    let damage = encounter
        .combat_log
        .iter()
        .find(|entry| entry.kind == CombatLogKind::Damage)
        .unwrap();
    assert_eq!(damage.base_amount, 3.0);
    assert_eq!(damage.effective_amount, 1.0);
    assert_eq!(damage.source_id, GM_DOT_SOURCE_ID);
    assert_eq!(damage.target_id, "target");
    assert!(!damage.benefits.is_empty());
    assert!(encounter
        .action_log
        .iter()
        .any(|line| line.contains("中毒")));
}

#[test]
fn gm_dot_damage_syncs_to_normal_character_window() {
    let mut manager: NapcatMessageManager = from_str(r#"{"messages":{}}"#).unwrap();
    manager
        .player_characters
        .insert("target".to_owned(), PlayerCharacter {
            hp: 30.0,
            max_hp: 30.0,
            hp_regen: 8.0,
            ..Default::default()
        });
    let participant = participant_from_character(
        "target",
        &manager.player_characters["target"],
        &manager,
    );
    let mut store = dot_store(vec![participant]);
    assert!(apply_gm_dot(
        store.encounters.get_mut("battle").unwrap(),
        &dot_draft(1),
    ));
    assert!(store.next_round("battle"));
    assert!(sync_encounter_to_manager(
        store.encounters.get("battle"),
        &mut manager
    ));
    assert_eq!(
        manager.player_characters["target"].hp,
        27.0
    );
    assert_eq!(
        manager.player_characters["target"].damage_taken_this_turn,
        3.0
    );
    assert!(
        store.encounters["battle"].participants[0]
            .delayed_damage_ticks
            .is_empty()
    );
}

#[test]
fn gm_dot_group_clock_catch_up_ticks_once_and_does_not_repeat_on_resync() {
    let mut manager: NapcatMessageManager = from_str(r#"{"messages":{}}"#).unwrap();
    manager
        .player_characters
        .insert("target".to_owned(), PlayerCharacter {
            hp: 30.0,
            max_hp: 30.0,
            ..Default::default()
        });
    manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
        campaign_id: "campaign".to_owned(),
        players: vec!["target".to_owned()],
        world_turn: 2,
        ..Default::default()
    });
    let participant = participant_from_character(
        "target",
        &manager.player_characters["target"],
        &manager,
    );
    let mut store = dot_store(vec![participant]);
    let encounter = store.encounters.get_mut("battle").unwrap();
    encounter.trpg_group = Some("party".to_owned());
    encounter.trpg_campaign_id = Some("campaign".to_owned());
    assert!(apply_gm_dot(encounter, &dot_draft(3)));

    assert!(sync_encounter_from_group_clock(
        &mut store, "battle", &manager
    ));
    assert_eq!(store.encounters["battle"].round, 2);
    assert_eq!(
        store.encounters["battle"].participants[0].hp,
        24.0
    );
    assert_eq!(
        store.encounters["battle"].participants[0].delayed_damage_ticks[0].turns_remaining,
        1
    );
    sync_encounter_from_group_clock(&mut store, "battle", &manager);
    assert_eq!(
        store.encounters["battle"].participants[0].hp,
        24.0
    );
    assert_eq!(
        store.encounters["battle"].participants[0].delayed_damage_ticks[0].turns_remaining,
        1
    );
}
