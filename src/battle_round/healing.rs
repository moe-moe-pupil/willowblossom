use super::{
    apply_encounter_inspiration,
    apply_participant_healing_for_battle,
    character_combat_source_notes,
    character_for_participant,
    format_number,
    grant_participant_revenge_soul_shield,
    participant_group_modifiers,
    participant_healing_modifiers,
    participant_healing_taken_multiplier,
    participant_one_heart_healing_multiplier,
    participant_wound_healing_multiplier,
    record_participant_one_heart_heal,
    schedule_participant_delayed_healing,
    BattleEncounter,
    BattleParticipantSnapshot,
    CombatLogEntry,
    CombatLogKind,
};
use crate::{
    napcat::{
        character_dying_target_healing_modifier,
        character_echoing_memory_healing_rates,
        character_mutual_aid_healing_rate,
        dying_target_healing_multiplier,
        NapcatMessageManager,
        PlayerCharacter,
        TrpgBasicConfig,
    },
    rule_engine::RuleAmountResolution,
};

/// Resolve selected healing targets using the same modifiers and talents for skills and ordinary
/// healing.
pub(super) fn resolve_battle_healing(
    encounter: &mut BattleEncounter,
    actor_snapshot: &BattleParticipantSnapshot,
    actor_character: &Option<PlayerCharacter>,
    basic_config: &TrpgBasicConfig,
    action_name: &str,
    amount: f32,
    target_ids: Vec<String>,
    single_heal_target_id: Option<String>,
    manager: &NapcatMessageManager,
) {
    let actor_id = actor_snapshot.target_id.as_str();
    let actor_name = actor_snapshot.display_name.clone();
    let actor_healing_modifiers = participant_healing_modifiers(
        actor_snapshot,
        actor_character.as_ref(),
        basic_config,
    );
    let actor_healing_multiplier = actor_healing_modifiers.iter().fold(1.0, |value, modifier| {
        value * modifier.multiplier
    });
    let actor_mutual_aid_healing_rate = actor_character
        .as_ref()
        .map(character_mutual_aid_healing_rate)
        .unwrap_or(0.0);
    let actor_source_notes = character_combat_source_notes(actor_character.as_ref(), "施法者");
    let actor_echoing_memory_healing_rates = actor_character
        .as_ref()
        .and_then(|character| character_echoing_memory_healing_rates(character));
    let actor_dying_target_healing_modifier = actor_character
        .as_ref()
        .map(character_dying_target_healing_modifier)
        .unwrap_or(1.0);
    if target_ids.is_empty() {
        encounter.action_log.push(format!(
            "{}使用{}，但范围内没有目标",
            actor_name, action_name
        ));
    }
    let mut pending_actor_mutual_aid_healing = 0.0;
    let mut pending_actor_revenge_soul_healing = 0.0;
    let mut healed_one_heart_target_id = None::<String>;
    let mut healed_inspiration_target_id = None::<String>;
    for resolved_target_id in target_ids {
        let Some(target) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == resolved_target_id)
        else {
            continue;
        };
        let target_character = character_for_participant(target, manager);
        let target_source_notes = character_combat_source_notes(target_character.as_ref(), "目标");
        let target_mutual_aid_healing_rate = target_character
            .as_ref()
            .map(character_mutual_aid_healing_rate)
            .unwrap_or(0.0);
        let wound_multiplier = participant_wound_healing_multiplier(target);
        let dying_multiplier = dying_target_healing_multiplier(
            target.hp,
            target.max_hp,
            actor_dying_target_healing_modifier,
        );
        let target_healing_multiplier =
            participant_healing_taken_multiplier(target) * wound_multiplier * dying_multiplier;
        let one_heart_multiplier = if encounter.active
            && single_heal_target_id.as_deref() == Some(resolved_target_id.as_str())
        {
            participant_one_heart_healing_multiplier(actor_snapshot, &resolved_target_id)
        } else {
            1.0
        };
        let final_amount =
            (amount * actor_healing_multiplier * one_heart_multiplier * target_healing_multiplier)
                .max(0.0);
        let healing_resolution = apply_participant_healing_for_battle(
            target,
            final_amount,
            actor_snapshot.overhealing_shield_cap_rate,
        );
        let effective_amount = healing_resolution.effective_amount();
        pending_actor_revenge_soul_healing += healing_resolution.hp_restored;
        if resolved_target_id != actor_id && effective_amount > f32::EPSILON {
            pending_actor_mutual_aid_healing +=
                effective_amount * (actor_mutual_aid_healing_rate + target_mutual_aid_healing_rate);
        }
        if effective_amount > f32::EPSILON
            && single_heal_target_id.as_deref() == Some(resolved_target_id.as_str())
        {
            healed_one_heart_target_id = Some(resolved_target_id.clone());
            healed_inspiration_target_id = Some(resolved_target_id.clone());
        }
        if effective_amount > f32::EPSILON
            && single_heal_target_id.as_deref() == Some(resolved_target_id.as_str())
        {
            if let Some((first_echo_rate, second_echo_rate)) = actor_echoing_memory_healing_rates {
                schedule_participant_delayed_healing(
                    target,
                    actor_id,
                    &actor_name,
                    "千万回忆",
                    effective_amount * first_echo_rate,
                    actor_snapshot.overhealing_shield_cap_rate,
                    1,
                );
                schedule_participant_delayed_healing(
                    target,
                    actor_id,
                    &actor_name,
                    "千万回忆",
                    effective_amount * second_echo_rate,
                    actor_snapshot.overhealing_shield_cap_rate,
                    2,
                );
            }
        }
        encounter.action_log.push(format!(
            "{}对{}使用{}，回复{}点生命值",
            actor_name,
            target.display_name,
            action_name,
            format_number(effective_amount)
        ));
        let mut modifiers = actor_healing_modifiers.clone();
        if (target.healing_taken_modifier - 1.0).abs() > f32::EPSILON {
            modifiers.push(RuleAmountResolution::factor(
                "目标受到治疗修正（角色/装备/BUFF）",
                target.healing_taken_modifier,
            ));
        }
        let group_healing_taken = participant_group_modifiers(target).healing_taken;
        if (group_healing_taken - 1.0).abs() > f32::EPSILON {
            modifiers.push(RuleAmountResolution::factor(
                "TRPG组全局承受治疗修正",
                group_healing_taken,
            ));
        }
        if (wound_multiplier - 1.0).abs() > f32::EPSILON {
            modifiers.push(RuleAmountResolution::factor(
                "溃伤",
                wound_multiplier,
            ));
        }
        if (dying_multiplier - 1.0).abs() > f32::EPSILON {
            modifiers.push(RuleAmountResolution::factor(
                "濒死治疗",
                dying_multiplier,
            ));
        }
        if (one_heart_multiplier - 1.0).abs() > f32::EPSILON {
            modifiers.push(RuleAmountResolution::factor(
                "一心",
                one_heart_multiplier,
            ));
        }
        let mut benefits = actor_source_notes.clone();
        benefits.extend(target_source_notes);
        if healing_resolution.shield_gained > f32::EPSILON {
            benefits.push(format!(
                "过量治疗转化{}点护盾",
                format_number(healing_resolution.shield_gained)
            ));
        }
        encounter.combat_log.push(CombatLogEntry {
            round: encounter.round,
            kind: CombatLogKind::Healing,
            source_id: actor_id.to_owned(),
            source_name: actor_name.clone(),
            target_id: resolved_target_id.clone(),
            target_name: target.display_name.clone(),
            action_name: action_name.to_owned(),
            base_amount: amount,
            effective_amount,
            modifiers,
            benefits,
        });
        if one_heart_multiplier > 1.0 + f32::EPSILON {
            encounter.action_log.push(format!(
                "{}触发一心，治疗效果提高{}%",
                actor_name,
                format_number((one_heart_multiplier - 1.0) * 100.0)
            ));
        }
    }
    if encounter.active && pending_actor_revenge_soul_healing > f32::EPSILON {
        if let Some(actor) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == actor_id)
        {
            let shield_gained = grant_participant_revenge_soul_shield(
                actor,
                pending_actor_revenge_soul_healing,
            );
            if shield_gained > f32::EPSILON {
                encounter.action_log.push(format!(
                    "{}触发复仇之魂，获得{}点护盾（当前{}点）",
                    actor_name,
                    format_number(shield_gained),
                    format_number(actor.revenge_soul_shield)
                ));
            }
        }
    }
    if encounter.active {
        if let Some(target_id) = healed_one_heart_target_id {
            if let Some(actor) = encounter
                .participants
                .iter_mut()
                .find(|participant| participant.target_id == actor_id)
            {
                record_participant_one_heart_heal(actor, &target_id);
            }
        }
    }
    if encounter.active {
        if let Some(target_id) = healed_inspiration_target_id {
            let target_name = encounter
                .participants
                .iter()
                .find(|participant| participant.target_id == target_id)
                .map(|participant| participant.display_name.clone())
                .unwrap_or_else(|| target_id.clone());
            if apply_encounter_inspiration(encounter, actor_id, &target_id) {
                encounter.action_log.push(format!(
                    "{}触发振奋，使{}获得10%速度与伤害加成，持续1回合",
                    actor_name, target_name
                ));
            }
        }
    }
    if pending_actor_mutual_aid_healing > f32::EPSILON {
        if let Some(actor) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == actor_id)
        {
            let shield_cap_rate = actor.overhealing_shield_cap_rate;
            let resolution = apply_participant_healing_for_battle(
                actor,
                pending_actor_mutual_aid_healing,
                shield_cap_rate,
            );
            let effective_amount = resolution.effective_amount();
            let mut benefits = Vec::new();
            if resolution.shield_gained > f32::EPSILON {
                benefits.push(format!(
                    "过量治疗转化{}点护盾",
                    format_number(resolution.shield_gained)
                ));
            }
            encounter.action_log.push(format!(
                "{}触发互帮互助，回复{}点生命值",
                actor_name,
                format_number(effective_amount)
            ));
            encounter.combat_log.push(CombatLogEntry {
                round: encounter.round,
                kind: CombatLogKind::Healing,
                source_id: actor_id.to_owned(),
                source_name: actor_name.clone(),
                target_id: actor_id.to_owned(),
                target_name: actor_name.clone(),
                action_name: "互帮互助".to_owned(),
                base_amount: pending_actor_mutual_aid_healing,
                effective_amount,
                modifiers: Vec::new(),
                benefits,
            });
        }
    }
}
