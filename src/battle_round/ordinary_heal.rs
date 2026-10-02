use super::{
    character_for_participant,
    encounter_basic_config,
    healing::resolve_battle_healing,
    participant_can_act,
    BattleRoundStore,
};
use crate::napcat::NapcatMessageManager;

impl BattleRoundStore {
    pub(super) fn apply_common_heal_and_finish(
        &mut self,
        encounter_id: &str,
        actor_id: &str,
        target_id: &str,
        base_amount: f32,
        manager: &NapcatMessageManager,
    ) -> bool {
        if !base_amount.is_finite()
            || base_amount <= 0.0
            || !self.encounter_is_canonical(encounter_id)
            || !self.encounter_group_exists(encounter_id, manager)
        {
            return false;
        }
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        if encounter.manager_sync_quarantined {
            return false;
        }
        let Some(actor) = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == actor_id)
            .cloned()
        else {
            return false;
        };
        if !participant_can_act(&actor) {
            return false;
        }
        if !encounter
            .participants
            .iter()
            .any(|participant| participant.target_id == target_id)
        {
            return false;
        }
        let config = encounter_basic_config(encounter, manager, actor_id);
        let actor_character = character_for_participant(&actor, manager);
        resolve_battle_healing(
            encounter,
            &actor,
            &actor_character,
            &config,
            "普通治疗",
            base_amount,
            vec![target_id.to_owned()],
            Some(target_id.to_owned()),
            manager,
        );
        self.finish_actor_action(encounter_id, actor_id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::{
        from_str,
        to_vec,
    };

    use super::*;
    use crate::{
        battle_round::{
            participant_from_character,
            sync_encounter_to_manager,
            BattleEncounter,
            CombatLogKind,
        },
        napcat::{
            CharacterSkillMetadata,
            PlayerCharacter,
            TrpgGroup,
        },
    };

    fn healing_fixture() -> (BattleRoundStore, NapcatMessageManager) {
        let mut manager: NapcatMessageManager = from_str(r#"{"messages":{}}"#).unwrap();
        for (target_id, hp, mp) in [("actor", 5.0, 0.0), ("target", 2.0, 4.0)] {
            manager
                .player_characters
                .insert(target_id.to_owned(), PlayerCharacter {
                    name: target_id.to_owned(),
                    hp,
                    max_hp: 10.0,
                    mp,
                    max_mp: 10.0,
                    ..Default::default()
                });
        }
        let participants = ["actor", "target"]
            .into_iter()
            .map(|target_id| {
                participant_from_character(
                    target_id,
                    &manager.player_characters[target_id],
                    &manager,
                )
            })
            .collect();
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants,
                ..Default::default()
            });
        (store, manager)
    }

    fn set_healing_talents(
        store: &mut BattleRoundStore,
        manager: &mut NapcatMessageManager,
        target_id: &str,
        talents: &[&str],
    ) {
        let character = manager.player_characters.get_mut(target_id).unwrap();
        character.skill_names = talents.iter().map(|name| (*name).to_owned()).collect();
        character.skill_notes = talents
            .iter()
            .map(|name| {
                if *name == "复仇之魂" {
                    "【被动】自身会获得治疗数值一半的护盾，最多5点。".to_owned()
                } else {
                    String::new()
                }
            })
            .collect();
        character.skill_metadata = talents
            .iter()
            .map(|_| CharacterSkillMetadata::talent("support_talent", "辅助天赋"))
            .collect();
        let participant = participant_from_character(
            target_id,
            &manager.player_characters[target_id],
            manager,
        );
        *store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == target_id)
            .unwrap() = participant;
    }

    #[test]
    fn ordinary_healing_triggers_mutual_aid_feedback_and_revenge_soul_shield() {
        let (mut store, mut manager) = healing_fixture();
        set_healing_talents(&mut store, &mut manager, "actor", &[
            "互帮互助",
            "复仇之魂",
        ]);
        set_healing_talents(&mut store, &mut manager, "target", &[
            "互帮互助",
        ]);

        assert!(store.apply_common_heal_and_finish("battle", "actor", "target", 4.0, &manager));
        let encounter = &store.encounters["battle"];
        let actor = &encounter.participants[0];
        let target = &encounter.participants[1];
        assert_eq!(target.hp, 6.0);
        assert_eq!(actor.hp, 9.0);
        assert_eq!(actor.healing_taken_this_turn, 4.0);
        assert_eq!(actor.revenge_soul_shield, 2.0);
        assert_eq!(actor.mp, 0.0);
        assert!(actor.skill_last_used_turns.is_empty());
        assert!(actor.skill_cooldown_ready_turns.is_empty());
        let feedback = encounter.combat_log.last().unwrap();
        assert_eq!(feedback.action_name, "互帮互助");
        assert_eq!(feedback.target_id, "actor");
        assert_eq!(feedback.effective_amount, 4.0);
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("触发复仇之魂")));
    }

    #[test]
    fn ordinary_healing_advances_one_heart_and_applies_inspiration() {
        let (mut store, mut manager) = healing_fixture();
        set_healing_talents(&mut store, &mut manager, "actor", &[
            "一心", "振奋",
        ]);

        assert!(store.apply_common_heal_and_finish("battle", "actor", "target", 2.0, &manager));
        let encounter = &store.encounters["battle"];
        assert_eq!(encounter.participants[1].hp, 4.0);
        assert_eq!(
            encounter.participants[0].one_heart_stacks,
            1
        );
        assert_eq!(
            encounter.participants[0].one_heart_target_id.as_deref(),
            Some("target")
        );
        assert_eq!(
            encounter.participants[0].inspiration_target_id.as_deref(),
            Some("target")
        );
        assert_eq!(
            encounter.participants[1].inspiration_sources.get("actor"),
            Some(&1)
        );

        assert!(store.next_round("battle"));
        assert!(store.apply_common_heal_and_finish("battle", "actor", "target", 2.0, &manager));
        let encounter = &store.encounters["battle"];
        assert!((encounter.participants[1].hp - 6.1).abs() < 0.0001);
        assert_eq!(
            encounter.participants[0].one_heart_stacks,
            2
        );
        assert!(encounter
            .combat_log
            .last()
            .unwrap()
            .modifiers
            .iter()
            .any(|modifier| {
                modifier.source == "一心" && (modifier.multiplier - 1.05).abs() < 0.0001
            }));

        assert!(store.next_round("battle"));
        assert!(store.apply_common_heal_and_finish("battle", "actor", "actor", 2.0, &manager));
        let encounter = &store.encounters["battle"];
        assert_eq!(encounter.participants[0].hp, 7.0);
        assert_eq!(
            encounter.participants[0].one_heart_stacks,
            1
        );
        assert_eq!(
            encounter.participants[0].one_heart_target_id.as_deref(),
            Some("actor")
        );
        assert_eq!(
            encounter.participants[0].inspiration_sources.get("actor"),
            Some(&1)
        );
        assert!(encounter.participants[1].inspiration_sources.is_empty());
    }

    #[test]
    fn ordinary_healing_schedules_and_resolves_memory_echoes() {
        let (mut store, mut manager) = healing_fixture();
        set_healing_talents(&mut store, &mut manager, "actor", &[
            "千万回忆",
        ]);
        manager.player_characters.get_mut("target").unwrap().max_hp = 100.0;
        store.encounters.get_mut("battle").unwrap().participants[1].max_hp = 100.0;

        assert!(store.apply_common_heal_and_finish("battle", "actor", "target", 20.0, &manager));
        let target = &store.encounters["battle"].participants[1];
        assert_eq!(target.hp, 22.0);
        assert_eq!(target.delayed_healing_ticks.len(), 2);
        for (tick, amount, delay) in [
            (&target.delayed_healing_ticks[0], 3.0, 1),
            (&target.delayed_healing_ticks[1], 1.0, 2),
        ] {
            assert_eq!(tick.name, "千万回忆");
            assert_eq!(tick.source_id, "actor");
            assert_eq!(tick.amount, amount);
            assert_eq!(tick.turns_remaining, delay);
            assert!(!tick.repeat_each_round);
        }

        assert!(store.next_round("battle"));
        assert_eq!(
            store.encounters["battle"].participants[1].hp,
            25.0
        );
        assert_eq!(
            store.encounters["battle"].participants[1]
                .delayed_healing_ticks
                .len(),
            1
        );
        assert!(store.next_round("battle"));
        assert_eq!(
            store.encounters["battle"].participants[1].hp,
            26.0
        );
        assert!(
            store.encounters["battle"].participants[1]
                .delayed_healing_ticks
                .is_empty()
        );
        assert!(store.next_round("battle"));
        assert_eq!(
            store.encounters["battle"].participants[1].hp,
            26.0
        );
    }

    #[test]
    fn ordinary_healing_at_full_hp_does_not_trigger_follow_up_talents() {
        let (mut store, mut manager) = healing_fixture();
        set_healing_talents(&mut store, &mut manager, "actor", &[
            "互帮互助",
            "一心",
            "振奋",
            "千万回忆",
            "复仇之魂",
        ]);
        store.encounters.get_mut("battle").unwrap().participants[1].hp = 10.0;

        assert!(store.apply_common_heal_and_finish("battle", "actor", "target", 20.0, &manager));
        let encounter = &store.encounters["battle"];
        let actor = &encounter.participants[0];
        let target = &encounter.participants[1];
        assert_eq!(actor.hp, 5.0);
        assert_eq!(actor.revenge_soul_shield, 0.0);
        assert!(actor.one_heart_target_id.is_none());
        assert_eq!(actor.one_heart_stacks, 0);
        assert!(actor.inspiration_target_id.is_none());
        assert!(target.inspiration_sources.is_empty());
        assert!(target.delayed_healing_ticks.is_empty());
        assert_eq!(encounter.combat_log.len(), 1);
        assert_eq!(
            encounter.combat_log[0].effective_amount,
            0.0
        );
        assert!(actor.action_done);
    }

    #[test]
    fn ordinary_healing_needs_no_skill_or_mp_and_syncs_hp_logs_and_totals() {
        let (mut store, mut manager) = healing_fixture();
        assert!(manager.player_characters["actor"].skill_names.is_empty());
        assert!(store.apply_common_heal_and_finish("battle", "actor", "target", 3.0, &manager));
        let encounter = &store.encounters["battle"];
        let actor = &encounter.participants[0];
        let target = &encounter.participants[1];
        assert_eq!(target.hp, 5.0);
        assert_eq!(target.healing_taken_this_turn, 3.0);
        assert_eq!(actor.mp, 0.0);
        assert_eq!(target.mp, 4.0);
        assert!(actor.action_done);
        assert_eq!(actor.turn, 1);
        assert_eq!(actor.combat_turns_completed, 1);
        assert!(actor.skill_last_used_turns.is_empty());
        assert!(actor.skill_cooldown_ready_turns.is_empty());
        assert_eq!(encounter.round, 0);
        let log = encounter.combat_log.last().unwrap();
        assert_eq!(log.kind, CombatLogKind::Healing);
        assert_eq!(log.action_name, "普通治疗");
        assert_eq!(log.source_id, "actor");
        assert_eq!(log.target_id, "target");
        assert_eq!(log.base_amount, 3.0);
        assert_eq!(log.effective_amount, 3.0);
        assert!(encounter
            .action_log
            .last()
            .unwrap()
            .contains("回复3点生命值"));

        assert!(sync_encounter_to_manager(
            Some(encounter),
            &mut manager
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            5.0
        );
        assert_eq!(
            manager.player_characters["target"].healing_taken_this_turn,
            3.0
        );
        assert_eq!(
            manager.player_characters["actor"].mp,
            0.0
        );
        assert!(manager.player_characters["actor"]
            .skill_last_cast_turns
            .is_empty());
    }

    #[test]
    fn ordinary_healing_caps_hp_and_preserves_existing_cooldowns() {
        let (mut store, manager) = healing_fixture();
        let actor = &mut store.encounters.get_mut("battle").unwrap().participants[0];
        actor.skill_last_used_turns = HashMap::from([("existing".to_owned(), 2)]);
        actor.skill_cooldown_ready_turns = HashMap::from([("existing".to_owned(), 8)]);
        let previous_last_used = actor.skill_last_used_turns.clone();
        let previous_ready = actor.skill_cooldown_ready_turns.clone();

        assert!(store.apply_common_heal_and_finish("battle", "actor", "target", 100.0, &manager));
        let encounter = &store.encounters["battle"];
        assert_eq!(encounter.participants[1].hp, 10.0);
        assert_eq!(
            encounter.participants[1].healing_taken_this_turn,
            8.0
        );
        assert_eq!(
            encounter.combat_log.last().unwrap().effective_amount,
            8.0
        );
        assert_eq!(
            encounter.participants[0].skill_last_used_turns,
            previous_last_used
        );
        assert_eq!(
            encounter.participants[0].skill_cooldown_ready_turns,
            previous_ready
        );
        assert_eq!(encounter.participants[0].mp, 0.0);
    }

    #[test]
    fn ordinary_healing_revives_a_downed_single_target() {
        let (mut store, mut manager) = healing_fixture();
        let target = &mut store.encounters.get_mut("battle").unwrap().participants[1];
        target.hp = 0.0;
        target.alive = false;
        manager.player_characters.get_mut("target").unwrap().hp = 0.0;

        assert!(store.apply_common_heal_and_finish("battle", "actor", "target", 4.0, &manager));
        let encounter = &store.encounters["battle"];
        assert_eq!(encounter.participants[1].hp, 4.0);
        assert!(encounter.participants[1].alive);
        assert!(!encounter.participants[1].action_done);
        assert_eq!(encounter.round, 0);
        assert!(sync_encounter_to_manager(
            Some(encounter),
            &mut manager
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            4.0
        );
    }

    #[test]
    fn ordinary_healing_uses_source_recipient_group_and_wound_modifiers() {
        let (mut store, manager) = healing_fixture();
        let encounter = store.encounters.get_mut("battle").unwrap();
        encounter.participants[0].healing_dealt_modifier = 2.0;
        encounter.participants[0].group_modifiers.healing_dealt = 1.5;
        let target = &mut encounter.participants[1];
        target.max_hp = 100.0;
        target.healing_taken_modifier = 0.5;
        target.group_modifiers.healing_taken = 0.8;
        target.wound_healing_taken_turns = 2;

        assert!(store.apply_common_heal_and_finish("battle", "actor", "target", 10.0, &manager));
        let encounter = &store.encounters["battle"];
        assert!((encounter.participants[1].hp - 11.0).abs() < 0.0001);
        let log = encounter.combat_log.last().unwrap();
        assert!((log.effective_amount - 9.0).abs() < 0.0001);
        assert!(log
            .modifiers
            .iter()
            .any(|modifier| modifier.source == "溃伤"));
        assert!(log
            .modifiers
            .iter()
            .any(|modifier| modifier.source == "TRPG组全局承受治疗修正"));
    }

    #[test]
    fn ordinary_self_healing_finishes_the_last_action_and_advances_the_round() {
        let (mut store, manager) = healing_fixture();
        store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .truncate(1);
        assert!(store.apply_common_heal_and_finish("battle", "actor", "actor", 3.0, &manager));
        let encounter = &store.encounters["battle"];
        let actor = &encounter.participants[0];
        assert_eq!(actor.hp, 8.0);
        assert_eq!(actor.mp, 0.0);
        assert_eq!(actor.turn, 1);
        assert_eq!(actor.combat_turns_completed, 1);
        assert_eq!(encounter.round, 1);
        assert!(!actor.action_done);
        assert!(actor.skill_last_used_turns.is_empty());
        assert_eq!(
            encounter.combat_log[0].effective_amount,
            3.0
        );
    }

    #[test]
    fn invalid_healing_and_unavailable_actors_leave_the_battle_unchanged() {
        for amount in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let (mut store, manager) = healing_fixture();
            let before = to_vec(&store).unwrap();
            assert!(
                !store.apply_common_heal_and_finish("battle", "actor", "target", amount, &manager)
            );
            assert_eq!(to_vec(&store).unwrap(), before);
        }
        for invalid in [
            "dead",
            "done",
            "paralyzed",
            "cocooning",
            "missing actor",
            "missing target",
        ] {
            let (mut store, manager) = healing_fixture();
            let actor = &mut store.encounters.get_mut("battle").unwrap().participants[0];
            match invalid {
                "dead" => {
                    actor.hp = 0.0;
                    actor.alive = false;
                },
                "done" => actor.action_done = true,
                "paralyzed" => actor.paralyzed_rounds_remaining = 1,
                "cocooning" => actor.hidden_role.cocooning = true,
                _ => {},
            }
            let before = to_vec(&store).unwrap();
            assert!(
                !store.apply_common_heal_and_finish(
                    "battle",
                    if invalid == "missing actor" { "missing" } else { "actor" },
                    if invalid == "missing target" { "missing" } else { "target" },
                    3.0,
                    &manager,
                ),
                "{invalid}"
            );
            assert_eq!(
                to_vec(&store).unwrap(),
                before,
                "{invalid}"
            );
        }
    }

    #[test]
    fn ordinary_healing_respects_quarantine_group_campaign_and_canonical_encounters() {
        for guard in ["quarantined", "deleted group", "old campaign", "duplicate"] {
            let (mut store, mut manager) = healing_fixture();
            manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
                campaign_id: "campaign".to_owned(),
                players: vec!["actor".to_owned(), "target".to_owned()],
                ..Default::default()
            });
            let encounter = store.encounters.get_mut("battle").unwrap();
            encounter.trpg_group = Some("party".to_owned());
            encounter.trpg_campaign_id = Some("campaign".to_owned());
            match guard {
                "quarantined" => encounter.manager_sync_quarantined = true,
                "deleted group" => {
                    manager.trpg_groups.remove("party");
                },
                "old campaign" => encounter.trpg_campaign_id = Some("old".to_owned()),
                "duplicate" => {
                    let mut newer = encounter.clone();
                    newer.round = 1;
                    store.encounters.insert("newer".to_owned(), newer);
                },
                _ => unreachable!(),
            }
            let before = to_vec(&store).unwrap();
            assert!(
                !store.apply_common_heal_and_finish("battle", "actor", "target", 3.0, &manager),
                "{guard}"
            );
            assert_eq!(
                to_vec(&store).unwrap(),
                before,
                "{guard}"
            );
        }
    }
}
