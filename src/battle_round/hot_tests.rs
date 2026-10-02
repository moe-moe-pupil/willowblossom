use serde_json::{
    from_str,
    to_string,
    to_value,
};

use super::{
    over_time::{
        apply_gm_dot,
        apply_gm_hot,
        remove_gm_hot,
        GmOverTimeDraft,
        GmOverTimeKind,
        GM_DOT_SOURCE_ID,
        GM_HOT_SOURCE_ID,
    },
    participant_from_character,
    sync_encounter_from_group_clock,
    sync_encounter_to_manager,
    BattleDelayedHealingTick,
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

fn hot_participant() -> BattleParticipantSnapshot {
    from_str(r#"{"target_id":"target","display_name":"目标","hp":10.0,"max_hp":30.0}"#).unwrap()
}

fn hot_store(target: BattleParticipantSnapshot) -> BattleRoundStore {
    let mut store = BattleRoundStore::default();
    store
        .encounters
        .insert("battle".to_owned(), BattleEncounter {
            participants: vec![target],
            ..Default::default()
        });
    store
}

fn hot_draft(rounds: i32) -> GmOverTimeDraft {
    GmOverTimeDraft {
        target_id: "target".to_owned(),
        amount: 3.0,
        rounds,
        kind: GmOverTimeKind::Healing,
        ..Default::default()
    }
}

#[test]
fn gm_hot_ticks_exactly_once_per_round_for_one_or_three_rounds() {
    for rounds in [1, 3] {
        let mut store = hot_store(hot_participant());
        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(apply_gm_hot(
            encounter,
            &hot_draft(rounds)
        ));
        assert_eq!(encounter.participants[0].hp, 10.0);
        assert_eq!(encounter.participants[0].turn, 0);
        assert!(!encounter.participants[0].action_done);

        for round in 1..=rounds + 1 {
            assert!(store.next_round("battle"));
            let encounter = &store.encounters["battle"];
            let target = &encounter.participants[0];
            let ticks = round.min(rounds);
            assert_eq!(target.hp, 10.0 + 3.0 * ticks as f32);
            assert_eq!(
                encounter
                    .combat_log
                    .iter()
                    .filter(|entry| entry.kind == CombatLogKind::Healing)
                    .count(),
                ticks as usize
            );
            if round < rounds {
                assert_eq!(
                    target.delayed_healing_ticks[0].turns_remaining,
                    rounds - round
                );
            } else {
                assert!(target.delayed_healing_ticks.is_empty());
            }
        }
    }
}

#[test]
fn gm_hot_heals_player_npc_and_summon_equally() {
    let mut player = hot_participant();
    player.player_character = true;
    let mut npc = hot_participant();
    npc.unit_template_id = Some("template".to_owned());
    npc.unit_character = Some(PlayerCharacter {
        hp: 10.0,
        max_hp: 30.0,
        ..Default::default()
    });
    let mut summon = hot_participant();
    summon.is_summon = true;
    summon.summon_owner_id = Some("owner".to_owned());
    summon.unit_character = npc.unit_character.clone();

    for target in [player, npc, summon] {
        let mut store = hot_store(target);
        assert!(apply_gm_hot(
            store.encounters.get_mut("battle").unwrap(),
            &hot_draft(1)
        ));
        assert!(store.next_round("battle"));
        assert_eq!(
            store.encounters["battle"].participants[0].hp,
            13.0
        );
    }
}

#[test]
fn gm_hot_uses_fixed_healing_capped_by_max_hp_without_modifying_shields() {
    let mut target = hot_participant();
    target.hp = 27.0;
    target.healing_taken_modifier = 0.1;
    target.arcane_shield = 4.0;
    target.construct_shield = 2.0;
    target.overhealing_shield_cap_rate = 1.0;
    let mut store = hot_store(target);
    let mut draft = hot_draft(2);
    draft.amount = 8.0;
    assert!(apply_gm_hot(
        store.encounters.get_mut("battle").unwrap(),
        &draft
    ));
    assert!(store.next_round("battle"));

    let encounter = &store.encounters["battle"];
    let target = &encounter.participants[0];
    assert_eq!(target.hp, 30.0);
    assert_eq!(target.healing_taken_this_turn, 3.0);
    assert_eq!(target.arcane_shield, 4.0);
    assert_eq!(target.construct_shield, 2.0);
    assert_eq!(target.overhealing_shield, 0.0);
    let entry = encounter
        .combat_log
        .iter()
        .find(|entry| entry.kind == CombatLogKind::Healing)
        .unwrap();
    assert_eq!(entry.base_amount, 8.0);
    assert_eq!(entry.effective_amount, 3.0);
    assert_eq!(entry.source_id, GM_HOT_SOURCE_ID);
    assert_eq!(entry.target_id, "target");
    assert!(entry.benefits.is_empty());
    assert!(encounter
        .action_log
        .iter()
        .any(|line| line.contains("回复")));

    assert!(store.next_round("battle"));
    let target = &store.encounters["battle"].participants[0];
    assert_eq!(target.hp, 30.0);
    assert_eq!(target.overhealing_shield, 0.0);
}

#[test]
fn gm_hot_does_not_revive_a_target_killed_before_the_tick() {
    let mut store = hot_store(hot_participant());
    let encounter = store.encounters.get_mut("battle").unwrap();
    assert!(apply_gm_hot(encounter, &hot_draft(1)));
    let target = &mut encounter.participants[0];
    target.hp = 0.0;
    target.alive = false;

    assert!(store.next_round("battle"));
    let encounter = &store.encounters["battle"];
    assert_eq!(encounter.participants[0].hp, 0.0);
    assert!(!encounter.participants[0].alive);
    assert!(!encounter
        .combat_log
        .iter()
        .any(|entry| entry.kind == CombatLogKind::Healing));
}

#[test]
fn gm_hot_persists_remaining_rounds_and_expires_after_restore() {
    let mut store = hot_store(hot_participant());
    assert!(apply_gm_hot(
        store.encounters.get_mut("battle").unwrap(),
        &hot_draft(3)
    ));
    assert!(store.next_round("battle"));
    let encoded = to_string(&store).unwrap();
    let mut restored: BattleRoundStore = from_str(&encoded).unwrap();
    let tick = &restored.encounters["battle"].participants[0].delayed_healing_ticks[0];
    assert!(tick.repeat_each_round);
    assert_eq!(tick.turns_remaining, 2);
    assert_eq!(tick.source_id, GM_HOT_SOURCE_ID);
    assert_eq!(tick.overhealing_shield_cap_rate, 0.0);

    for expected_hp in [16.0, 19.0, 19.0] {
        assert!(restored.next_round("battle"));
        assert_eq!(
            restored.encounters["battle"].participants[0].hp,
            expected_hp
        );
    }
    assert!(
        restored.encounters["battle"].participants[0]
            .delayed_healing_ticks
            .is_empty()
    );
}

#[test]
fn cancelling_gm_hot_preserves_legacy_delayed_healing_and_simultaneous_dot() {
    let legacy: BattleDelayedHealingTick = from_str(
        r#"{"name":"延迟治疗","source_id":"caster","source_name":"施法者","amount":4.0,"turns_remaining":2}"#,
    ).unwrap();
    assert!(!legacy.repeat_each_round);
    let mut target = hot_participant();
    target.delayed_healing_ticks.push(legacy.clone());
    let mut store = hot_store(target);
    let encounter = store.encounters.get_mut("battle").unwrap();
    assert!(apply_gm_hot(encounter, &hot_draft(3)));
    let damage = GmOverTimeDraft {
        kind: GmOverTimeKind::Damage,
        damage_type: DamageType::Poisoning,
        ..hot_draft(3)
    };
    assert!(apply_gm_dot(encounter, &damage));
    let damage_ticks = encounter.participants[0].delayed_damage_ticks.clone();
    assert!(!remove_gm_hot(encounter, "target", 0));
    assert!(!remove_gm_hot(encounter, "target", 99));
    assert!(!remove_gm_hot(encounter, "missing", 1));
    assert!(remove_gm_hot(encounter, "target", 1));
    assert_eq!(
        encounter.participants[0].delayed_healing_ticks,
        vec![legacy]
    );
    assert_eq!(
        encounter.participants[0].delayed_damage_ticks,
        damage_ticks
    );

    for expected_hp in [7.0, 8.0, 5.0, 5.0] {
        assert!(store.next_round("battle"));
        assert_eq!(
            store.encounters["battle"].participants[0].hp,
            expected_hp
        );
    }
    let log = &store.encounters["battle"].combat_log;
    assert_eq!(
        log.iter()
            .filter(|entry| entry.kind == CombatLogKind::Healing)
            .count(),
        1
    );
    assert_eq!(
        log.iter()
            .filter(
                |entry| entry.source_id == GM_DOT_SOURCE_ID && entry.kind == CombatLogKind::Damage
            )
            .count(),
        3
    );
}

#[test]
fn gm_hot_rejects_invalid_inputs_dead_targets_and_quarantined_encounters() {
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
        let mut target = hot_participant();
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
        assert!(!apply_gm_hot(
            &mut encounter,
            &GmOverTimeDraft {
                target_id: target_id.to_owned(),
                amount,
                rounds,
                ..hot_draft(3)
            }
        ));
        assert_eq!(to_value(&encounter).unwrap(), before);
    }
}

#[test]
fn gm_hot_healing_syncs_to_normal_character_window() {
    let mut manager: NapcatMessageManager = from_str(r#"{"messages":{}}"#).unwrap();
    manager
        .player_characters
        .insert("target".to_owned(), PlayerCharacter {
            hp: 10.0,
            max_hp: 30.0,
            hp_regen: 8.0,
            ..Default::default()
        });
    let participant = participant_from_character(
        "target",
        &manager.player_characters["target"],
        &manager,
    );
    let mut store = hot_store(participant);
    assert!(apply_gm_hot(
        store.encounters.get_mut("battle").unwrap(),
        &hot_draft(1)
    ));
    assert!(store.next_round("battle"));
    assert!(sync_encounter_to_manager(
        store.encounters.get("battle"),
        &mut manager
    ));
    assert_eq!(
        manager.player_characters["target"].hp,
        13.0
    );
    assert_eq!(
        manager.player_characters["target"].healing_taken_this_turn,
        3.0
    );
}

#[test]
fn gm_hot_group_clock_catch_up_ticks_once_and_does_not_repeat_on_resync() {
    let mut manager: NapcatMessageManager = from_str(r#"{"messages":{}}"#).unwrap();
    manager
        .player_characters
        .insert("target".to_owned(), PlayerCharacter {
            hp: 10.0,
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
    let mut store = hot_store(participant);
    let encounter = store.encounters.get_mut("battle").unwrap();
    encounter.trpg_group = Some("party".to_owned());
    encounter.trpg_campaign_id = Some("campaign".to_owned());
    assert!(apply_gm_hot(encounter, &hot_draft(3)));

    assert!(sync_encounter_from_group_clock(
        &mut store, "battle", &manager
    ));
    assert_eq!(store.encounters["battle"].round, 2);
    assert_eq!(
        store.encounters["battle"].participants[0].hp,
        16.0
    );
    assert_eq!(
        store.encounters["battle"].participants[0].delayed_healing_ticks[0].turns_remaining,
        1
    );
    sync_encounter_from_group_clock(&mut store, "battle", &manager);
    assert_eq!(
        store.encounters["battle"].participants[0].hp,
        16.0
    );
    assert_eq!(
        store.encounters["battle"].participants[0].delayed_healing_ticks[0].turns_remaining,
        1
    );
}
