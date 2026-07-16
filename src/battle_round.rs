use std::{
    collections::{
        hash_map::DefaultHasher,
        HashMap,
        HashSet,
    },
    hash::{
        Hash,
        Hasher,
    },
    path::Path,
};

use bevy::prelude::*;
use bevy_egui::{
    egui,
    EguiContexts,
    EguiPrimaryContextPass,
};
use bevy_persistent::{
    Persistent,
    StorageFormat,
};
use serde::{
    Deserialize,
    Serialize,
};

#[cfg(test)]
use crate::rule_engine::{
    BuffEffect,
    BuffField,
    BuffKind,
    BuffSpec,
    BuffValue,
};
use crate::{
    napcat::{
        arrogance_damage_dealt_multiplier,
        champion_damage_dealt_multiplier,
        champion_damage_taken_multiplier,
        character_arcane_shield_amount,
        character_arrogance_damage_bonus_per_source,
        character_calm_heart_healing_rate,
        character_champion_damage_bonus_per_stack,
        character_champion_damage_reduction_per_stack,
        character_chaos_output_variance,
        character_damage_attribute_multiplier,
        character_damage_dealt_talent_buffs,
        character_damage_taken_attribute_multiplier,
        character_dominion_max_hp_bonus_cap,
        character_dominion_max_hp_gain_rate,
        character_dying_healing_taken_modifier,
        character_echoing_memory_healing_rates,
        character_endless_pain_bonus_damage_per_stack,
        character_fighting_spirit_damage_taken_multiplier,
        character_gale_force_battle_speeds,
        character_healing_attribute_multiplier,
        character_hope_avatar_available,
        character_infinite_focus_damage_bonus_per_stack,
        character_inspiration_available,
        character_keen_evasion_available,
        character_large_hit_damage_taken_modifier,
        character_liquid_body_damage_delay_rate,
        character_liquid_body_self_healing_rate,
        character_minimum_damage_floor,
        character_minimum_range_meters,
        character_moonberry_talent_damage_attribute_bonus,
        character_mutual_aid_healing_rate,
        character_one_heart_healing_bonus_per_stack,
        character_overhealing_shield_cap_rate,
        character_penance_healing_bonus_percent,
        character_physical_damage_followup_rate,
        character_physical_damage_lifesteal,
        character_range_magic_converter_damage_bonus,
        character_sin_on_sin_exp_bonus_per_stack,
        character_sin_on_sin_recovery_rate,
        character_spell_range_multiplier,
        character_undying_rage_available,
        character_valorous_battle_damage_multiplier,
        character_wounded_healing_dealt_modifier,
        dying_healing_taken_multiplier,
        endless_pain_bonus_damage,
        infinite_focus_damage_dealt_multiplier,
        large_hit_damage_taken_multiplier,
        low_hp_damage_multiplier,
        moonberry_chaos_output_multiplier,
        moonberry_effective_skill_range_radius_with_multiplier,
        moonberry_skill_type_is_spell,
        one_heart_healing_dealt_multiplier,
        penance_decayed_healing_dealt_modifier,
        sin_on_sin_exp_bonus_percent,
        skill_rule_args,
        status_damage_attribute_multiplier,
        status_healing_attribute_multiplier,
        wounded_healing_dealt_multiplier,
        CharacterStatus,
        NapcatMessageManager,
        PlayerCharacter,
        SkillRuleArgs,
        TrpgBasicConfig,
        TrpgDamageBonusKind,
        TrpgDamageTakenKind,
        TrpgGroup,
        UnitPoolEntry,
    },
    rule_engine::{
        apply_skill_type_damage_default,
        legacy_moonberry_buff_machine_skill_cast_rule,
        parse_rule_with_named_args,
        Action,
        ActorRef,
        BuffTickAction,
        DamageType,
        RuleBuffTemplate,
        RuleEngineState,
        TargetSelector,
        ValueExpr,
    },
    scene::SceneCharacterPositions,
    ui::{
        advance_buffs_for_players,
        sync_character_buffs,
    },
};

pub struct BattleRoundPlugin;

impl Plugin for BattleRoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BattleRoundUiState>()
            .add_systems(Startup, setup_battle_round_store)
            .add_systems(Update, sync_battle_round_entities)
            .add_systems(
                EguiPrimaryContextPass,
                battle_round_panel,
            );
    }
}

#[derive(Resource, Default)]
pub struct BattleRoundUiState {
    panel_open: bool,
    new_encounter_name: String,
    selected_group: String,
    selected_add_player: HashMap<String, String>,
    selected_add_unit: HashMap<String, String>,
    selected_action_target: HashMap<String, String>,
    selected_skill_index: HashMap<String, usize>,
    action_amount: HashMap<String, f32>,
    confirm_next_round: HashSet<String>,
}

impl BattleRoundUiState {
    pub fn open_panel(&mut self) { self.panel_open = true; }
}

#[derive(Resource, Serialize, Deserialize, Default)]
pub struct BattleRoundStore {
    #[serde(default)]
    pub encounters: HashMap<String, BattleEncounter>,
    #[serde(default)]
    pub active_encounter_id: Option<String>,
    #[serde(default = "default_next_encounter_index")]
    next_encounter_index: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BattleEncounter {
    pub name: String,
    #[serde(default)]
    pub trpg_group: Option<String>,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default = "default_true")]
    pub sort_by_turn: bool,
    #[serde(default)]
    pub negative_enabled: bool,
    #[serde(default)]
    pub round: u32,
    #[serde(default)]
    pub participants: Vec<BattleParticipantSnapshot>,
    #[serde(default)]
    pub action_log: Vec<String>,
}

impl Default for BattleEncounter {
    fn default() -> Self {
        Self {
            name: String::new(),
            trpg_group: None,
            active: true,
            sort_by_turn: true,
            negative_enabled: false,
            round: 0,
            participants: Vec::new(),
            action_log: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BattleParticipantSnapshot {
    pub target_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_template_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_character: Option<PlayerCharacter>,
    #[serde(default)]
    pub player_character: bool,
    #[serde(default)]
    pub turn: u32,
    #[serde(default)]
    #[serde(rename = "str")]
    pub str_: i32,
    #[serde(default)]
    pub agi: i32,
    #[serde(default)]
    pub dex: i32,
    #[serde(default, rename = "int")]
    pub int_: i32,
    #[serde(default)]
    pub wis: i32,
    #[serde(default)]
    pub action_done: bool,
    #[serde(default = "default_true")]
    pub alive: bool,
    #[serde(default)]
    pub negative_layers: u32,
    #[serde(default)]
    pub pending_negative: bool,
    #[serde(default)]
    pub hp: f32,
    #[serde(default)]
    pub max_hp: f32,
    #[serde(default)]
    pub mp: f32,
    #[serde(default)]
    pub max_mp: f32,
    #[serde(default)]
    pub hp_regen: f32,
    #[serde(default)]
    pub mp_regen: f32,
    #[serde(default)]
    pub speed: f32,
    #[serde(default)]
    pub low_survivor_speed: f32,
    #[serde(default = "default_combat_modifier")]
    pub damage_dealt_modifier: f32,
    #[serde(default = "default_combat_modifier")]
    pub damage_taken_modifier: f32,
    #[serde(default = "default_combat_modifier")]
    pub healing_dealt_modifier: f32,
    #[serde(default = "default_combat_modifier")]
    pub healing_taken_modifier: f32,
    #[serde(default)]
    pub arrogance_damage_bonus_per_source: f32,
    #[serde(default)]
    pub arrogance_damage_source_ids: Vec<String>,
    #[serde(default)]
    pub endless_pain_bonus_damage_per_stack: f32,
    #[serde(default)]
    pub endless_pain_stacks: u32,
    #[serde(default)]
    pub infinite_focus_damage_bonus_per_stack: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infinite_focus_target_id: Option<String>,
    #[serde(default)]
    pub infinite_focus_stacks: u32,
    #[serde(default)]
    pub one_heart_healing_bonus_per_stack: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub one_heart_target_id: Option<String>,
    #[serde(default)]
    pub one_heart_stacks: u32,
    #[serde(default)]
    pub inspiration_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspiration_target_id: Option<String>,
    #[serde(default)]
    pub inspiration_sources: HashMap<String, u32>,
    #[serde(default)]
    pub keen_evasion_enabled: bool,
    #[serde(default)]
    pub keen_evasion_available: bool,
    #[serde(default)]
    pub arcane_shield: f32,
    #[serde(default)]
    pub overhealing_shield_cap_rate: f32,
    #[serde(default)]
    pub overhealing_shield: f32,
    #[serde(default)]
    pub overhealing_shield_turns_remaining: u32,
    #[serde(default)]
    pub undying_rage_enabled: bool,
    #[serde(default)]
    pub undying_rage_used: bool,
    #[serde(default)]
    pub undying_rage_active: bool,
    #[serde(default)]
    pub hope_avatar_enabled: bool,
    #[serde(default)]
    pub hope_avatar_used: bool,
    #[serde(default)]
    pub hope_avatar_rounds_remaining: u32,
    #[serde(default)]
    pub liquid_body_damage_delay_rate: f32,
    #[serde(default)]
    pub liquid_body_self_healing_rate: f32,
    #[serde(default)]
    pub calm_heart_healing_rate: f32,
    #[serde(default)]
    pub combat_damage_taken_total: f32,
    #[serde(default)]
    pub champion_damage_bonus_per_stack: f32,
    #[serde(default)]
    pub champion_damage_reduction_per_stack: f32,
    #[serde(default)]
    pub champion_stacks: u32,
    #[serde(default)]
    pub dominion_max_hp_gain_rate: f32,
    #[serde(default)]
    pub dominion_max_hp_bonus_cap: f32,
    #[serde(default)]
    pub dominion_max_hp_bonus: f32,
    #[serde(default)]
    pub sin_on_sin_exp_bonus_per_stack: f32,
    #[serde(default)]
    pub sin_on_sin_recovery_rate: f32,
    #[serde(default)]
    pub sin_on_sin_stacks: u32,
    #[serde(default)]
    pub penance_healing_bonus_percent: f32,
    #[serde(default)]
    pub penance_kill_assist_count: u32,
    #[serde(default)]
    pub damage_contributors: Vec<String>,
    #[serde(default)]
    pub wound_healing_taken_turns: i32,
    #[serde(default)]
    pub delayed_damage_ticks: Vec<BattleDelayedDamageTick>,
    #[serde(default)]
    pub delayed_healing_ticks: Vec<BattleDelayedHealingTick>,
    #[serde(default)]
    pub damage_taken_this_turn: f32,
    #[serde(default)]
    pub healing_taken_this_turn: f32,
    #[serde(default)]
    pub skill_last_used_turns: HashMap<String, u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BattleDelayedDamageTick {
    pub name: String,
    pub source_id: String,
    pub source_name: String,
    pub amount: f32,
    pub damage_type: DamageType,
    pub turns_remaining: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BattleDelayedHealingTick {
    pub name: String,
    pub source_id: String,
    pub source_name: String,
    pub amount: f32,
    #[serde(default)]
    pub overhealing_shield_cap_rate: f32,
    pub turns_remaining: i32,
}

#[derive(Debug, Clone)]
struct CharacterSkill {
    index: usize,
    name: String,
    note: String,
    skill_type: Option<String>,
    legacy_buff_machine_json: Option<String>,
    mp_cost: f32,
    cooldown_turns: u32,
    cooldown_left: Option<u32>,
    target_count: Option<u32>,
    target_class: Option<String>,
    range: Option<i32>,
    arg_values: SkillRuleArgs,
}

#[derive(Component, Debug, Clone)]
pub struct BattleEncounterEntity {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub round: u32,
    pub negative_enabled: bool,
}

#[derive(Component, Debug, Clone)]
pub struct BattleParticipantEntity {
    pub encounter_id: String,
    pub target_id: String,
    pub display_name: String,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct TurnCounter {
    pub current: u32,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct BattlePresence {
    pub alive: bool,
    pub negative_layers: u32,
    pub pending_negative: bool,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct BattleVitals {
    pub hp: f32,
    pub max_hp: f32,
    pub mp: f32,
    pub max_mp: f32,
    pub hp_regen: f32,
    pub mp_regen: f32,
}

#[derive(Component)]
struct BattleRoundRuntime;

fn default_next_encounter_index() -> u64 { 1 }

fn default_true() -> bool { true }

fn default_combat_modifier() -> f32 { 1.0 }

fn record_participant_damage_taken(
    participant: &mut BattleParticipantSnapshot,
    amount: f32,
) -> bool {
    let amount = amount.max(0.0);
    if amount <= f32::EPSILON {
        return false;
    }
    participant.damage_taken_this_turn += amount;
    true
}

fn record_participant_healing_taken(
    participant: &mut BattleParticipantSnapshot,
    amount: f32,
) -> bool {
    let amount = amount.max(0.0);
    if amount <= f32::EPSILON {
        return false;
    }
    participant.healing_taken_this_turn += amount;
    true
}

fn apply_participant_healing_for_battle(
    participant: &mut BattleParticipantSnapshot,
    amount: f32,
    overhealing_shield_cap_rate: f32,
) {
    let amount = amount.max(0.0);
    if amount <= f32::EPSILON {
        return;
    }
    record_participant_healing_taken(participant, amount);
    let missing_hp = (participant.max_hp - participant.hp).max(0.0);
    let applied_healing = amount.min(missing_hp);
    participant.hp = (participant.hp + applied_healing).min(participant.max_hp);
    participant.alive = participant.hp > 0.0;

    let overhealing = (amount - applied_healing).max(0.0);
    let shield_cap = participant.max_hp.max(0.0) * overhealing_shield_cap_rate.max(0.0);
    if overhealing > f32::EPSILON && shield_cap > f32::EPSILON {
        participant.overhealing_shield =
            (participant.overhealing_shield.max(0.0) + overhealing).min(shield_cap);
        participant.overhealing_shield_turns_remaining = 2;
    }
}

fn set_encounter_active_state(encounter: &mut BattleEncounter, active: bool) -> bool {
    if encounter.active == active {
        return false;
    }

    if active {
        for participant in &mut encounter.participants {
            participant.combat_damage_taken_total = 0.0;
        }
    } else {
        let mut logs = Vec::new();
        for participant in &mut encounter.participants {
            let healing = participant.combat_damage_taken_total.max(0.0)
                * participant.calm_heart_healing_rate.max(0.0);
            participant.combat_damage_taken_total = 0.0;
            if !participant.alive || healing <= f32::EPSILON {
                continue;
            }
            let shield_cap_rate = participant.overhealing_shield_cap_rate;
            apply_participant_healing_for_battle(participant, healing, shield_cap_rate);
            logs.push(format!(
                "{}触发息心，回复{}点生命值",
                participant.display_name,
                format_number(healing)
            ));
        }
        encounter.action_log.extend(logs);
    }
    encounter.active = active;
    true
}

fn advance_participant_overhealing_shield(participant: &mut BattleParticipantSnapshot) {
    participant.overhealing_shield = participant
        .overhealing_shield
        .max(0.0)
        .min(participant.max_hp.max(0.0) * 0.30);
    if participant.overhealing_shield_turns_remaining > 0 {
        participant.overhealing_shield_turns_remaining -= 1;
    }
    if participant.overhealing_shield_turns_remaining == 0 {
        participant.overhealing_shield = 0.0;
    }
}

fn record_participant_damage_contributor(
    participant: &mut BattleParticipantSnapshot,
    source_id: &str,
) {
    if source_id.trim().is_empty() || participant.target_id == source_id {
        return;
    }
    if !participant
        .damage_contributors
        .iter()
        .any(|contributor| contributor == source_id)
    {
        participant.damage_contributors.push(source_id.to_owned());
    }
}

fn record_participant_arrogance_damage_source(
    participant: &mut BattleParticipantSnapshot,
    source_id: &str,
) {
    if participant.arrogance_damage_bonus_per_source <= f32::EPSILON
        || source_id.trim().is_empty()
        || participant.target_id == source_id
        || participant.arrogance_damage_source_ids.len() >= 3
    {
        return;
    }
    if !participant
        .arrogance_damage_source_ids
        .iter()
        .any(|existing| existing == source_id)
    {
        participant
            .arrogance_damage_source_ids
            .push(source_id.to_owned());
    }
}

fn record_participant_endless_pain_stack(participant: &mut BattleParticipantSnapshot) {
    if participant.endless_pain_bonus_damage_per_stack <= f32::EPSILON {
        return;
    }
    participant.endless_pain_stacks = participant.endless_pain_stacks.saturating_add(1).min(2);
}

fn participant_infinite_focus_damage_multiplier(
    participant: &BattleParticipantSnapshot,
    target_id: &str,
) -> f32 {
    if participant.infinite_focus_damage_bonus_per_stack <= f32::EPSILON {
        return 1.0;
    }
    if participant.infinite_focus_target_id.as_deref() != Some(target_id) {
        return 1.0;
    }
    infinite_focus_damage_dealt_multiplier(
        participant.infinite_focus_damage_bonus_per_stack,
        participant.infinite_focus_stacks,
    )
}

fn record_participant_infinite_focus_hit(
    participant: &mut BattleParticipantSnapshot,
    target_id: &str,
) {
    if participant.infinite_focus_damage_bonus_per_stack <= f32::EPSILON
        || target_id.trim().is_empty()
        || participant.target_id == target_id
    {
        return;
    }
    if participant.infinite_focus_target_id.as_deref() == Some(target_id) {
        participant.infinite_focus_stacks =
            participant.infinite_focus_stacks.saturating_add(1).min(2);
    } else {
        participant.infinite_focus_target_id = Some(target_id.to_owned());
        participant.infinite_focus_stacks = 1;
    }
}

fn participant_one_heart_healing_multiplier(
    participant: &BattleParticipantSnapshot,
    target_id: &str,
) -> f32 {
    if participant.one_heart_healing_bonus_per_stack <= f32::EPSILON {
        return 1.0;
    }
    if participant.one_heart_target_id.as_deref() != Some(target_id) {
        return 1.0;
    }
    one_heart_healing_dealt_multiplier(
        participant.one_heart_healing_bonus_per_stack,
        participant.one_heart_stacks,
    )
}

fn record_participant_one_heart_heal(participant: &mut BattleParticipantSnapshot, target_id: &str) {
    if participant.one_heart_healing_bonus_per_stack <= f32::EPSILON || target_id.trim().is_empty()
    {
        return;
    }
    if participant.one_heart_target_id.as_deref() == Some(target_id) {
        participant.one_heart_stacks = participant.one_heart_stacks.saturating_add(1).min(5);
    } else {
        participant.one_heart_target_id = Some(target_id.to_owned());
        participant.one_heart_stacks = 1;
    }
}

fn participant_inspiration_multiplier(participant: &BattleParticipantSnapshot) -> f32 {
    if participant
        .inspiration_sources
        .values()
        .any(|turns| *turns > 0)
    {
        1.10
    } else {
        1.0
    }
}

fn apply_encounter_inspiration(
    encounter: &mut BattleEncounter,
    source_id: &str,
    target_id: &str,
) -> bool {
    let enabled = encounter
        .participants
        .iter()
        .find(|participant| participant.target_id == source_id)
        .is_some_and(|participant| participant.inspiration_enabled);
    let target_exists = encounter
        .participants
        .iter()
        .any(|participant| participant.target_id == target_id);
    if !enabled || !target_exists {
        return false;
    }
    for participant in &mut encounter.participants {
        participant.inspiration_sources.remove(source_id);
    }
    if let Some(source) = encounter
        .participants
        .iter_mut()
        .find(|participant| participant.target_id == source_id)
    {
        source.inspiration_target_id = Some(target_id.to_owned());
    }
    if let Some(target) = encounter
        .participants
        .iter_mut()
        .find(|participant| participant.target_id == target_id)
    {
        target.inspiration_sources.insert(source_id.to_owned(), 1);
    }
    true
}

fn advance_encounter_inspiration(encounter: &mut BattleEncounter) {
    let mut expired = Vec::new();
    for target in &mut encounter.participants {
        let target_id = target.target_id.clone();
        target.inspiration_sources.retain(|source_id, turns| {
            *turns = turns.saturating_sub(1);
            if *turns == 0 {
                expired.push((source_id.clone(), target_id.clone()));
                false
            } else {
                true
            }
        });
    }
    for (source_id, target_id) in expired {
        if let Some(source) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == source_id)
        {
            if source.inspiration_target_id.as_deref() == Some(target_id.as_str()) {
                source.inspiration_target_id = None;
            }
        }
    }
}

fn sync_participant_keen_evasion(participant: &mut BattleParticipantSnapshot, enabled: bool) {
    if enabled && !participant.keen_evasion_enabled {
        participant.keen_evasion_available = true;
    } else if !enabled {
        participant.keen_evasion_available = false;
    }
    participant.keen_evasion_enabled = enabled;
}

fn sync_participant_undying_rage(participant: &mut BattleParticipantSnapshot, enabled: bool) {
    if !enabled {
        participant.undying_rage_active = false;
    }
    participant.undying_rage_enabled = enabled;
}

fn participant_undying_rage_damage_multiplier(participant: &BattleParticipantSnapshot) -> f32 {
    if participant.undying_rage_active {
        1.10
    } else {
        1.0
    }
}

fn participant_hope_avatar_active(participant: &BattleParticipantSnapshot) -> bool {
    participant.hope_avatar_used && participant.hope_avatar_rounds_remaining > 0
}

fn skill_effects_are_hope_avatar_healing(effects: &[SkillEffect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, SkillEffect::Heal { .. }))
        && !effects
            .iter()
            .any(|effect| matches!(effect, SkillEffect::Damage { .. }))
}

fn skill_damage_triggers_keen_evasion(target: TargetSelector, target_class: Option<&str>) -> bool {
    target.area.is_some()
        || skill_target_class_is_area(target_class)
        || matches!(
            target_class.map(str::trim),
            Some("多目标" | "无目标")
        )
        || matches!(target.actor, ActorRef::Source)
}

fn participant_keen_evasion_evades_damage(
    participant: &mut BattleParticipantSnapshot,
    amount: f32,
    target: TargetSelector,
    target_class: Option<&str>,
) -> bool {
    if amount <= f32::EPSILON
        || !participant.keen_evasion_enabled
        || !participant.keen_evasion_available
        || !skill_damage_triggers_keen_evasion(target, target_class)
    {
        return false;
    }
    participant.keen_evasion_available = false;
    true
}

fn participant_liquid_body_split_damage(
    participant: &BattleParticipantSnapshot,
    amount: f32,
) -> (f32, f32) {
    let amount = amount.max(0.0);
    let delay_rate = participant.liquid_body_damage_delay_rate.clamp(0.0, 1.0);
    if amount <= f32::EPSILON || delay_rate <= f32::EPSILON {
        return (amount, 0.0);
    }
    let delayed = amount * delay_rate;
    (
        (amount - delayed).max(0.0),
        delayed.max(0.0),
    )
}

fn apply_participant_liquid_body_healing(
    participant: &mut BattleParticipantSnapshot,
    previous_damage_taken: f32,
) -> Option<String> {
    if !participant.alive || participant.liquid_body_self_healing_rate <= f32::EPSILON {
        return None;
    }
    let healing = previous_damage_taken.max(0.0) * participant.liquid_body_self_healing_rate;
    if healing <= f32::EPSILON {
        return None;
    }
    let shield_cap_rate = participant.overhealing_shield_cap_rate;
    apply_participant_healing_for_battle(participant, healing, shield_cap_rate);
    Some(format!(
        "{}触发液态躯体，回复{}点生命值",
        participant.display_name,
        format_number(healing)
    ))
}

struct BattleDefeatOutcome {
    contributors: Vec<String>,
    defeated_player_character: bool,
    defeated_max_hp: f32,
}

struct BattleDamageResolution {
    damage_applied: f32,
    damage_absorbed: f32,
    undying_rage_triggered: bool,
    hope_avatar_triggered: bool,
    hope_avatar_immune: bool,
    defeat_outcome: Option<BattleDefeatOutcome>,
}

fn participant_defeat_outcome(
    participant: &mut BattleParticipantSnapshot,
    was_alive: bool,
) -> Option<BattleDefeatOutcome> {
    if !was_alive || participant.alive {
        return None;
    }
    let contributors = std::mem::take(&mut participant.damage_contributors);
    Some(BattleDefeatOutcome {
        contributors,
        defeated_player_character: participant.player_character,
        defeated_max_hp: participant.max_hp,
    })
}

fn apply_participant_damage_for_battle(
    participant: &mut BattleParticipantSnapshot,
    amount: f32,
    source_id: &str,
    encounter_active: bool,
) -> BattleDamageResolution {
    let incoming_amount = amount.max(0.0);
    if participant_hope_avatar_active(participant) {
        return BattleDamageResolution {
            damage_applied: 0.0,
            damage_absorbed: incoming_amount,
            undying_rage_triggered: false,
            hope_avatar_triggered: false,
            hope_avatar_immune: true,
            defeat_outcome: None,
        };
    }
    let available_overhealing_shield = participant.overhealing_shield.max(0.0);
    let overhealing_absorbed = available_overhealing_shield.min(incoming_amount);
    participant.overhealing_shield = available_overhealing_shield - overhealing_absorbed;
    if participant.overhealing_shield <= f32::EPSILON {
        participant.overhealing_shield = 0.0;
        participant.overhealing_shield_turns_remaining = 0;
    }
    let after_overhealing_shield = (incoming_amount - overhealing_absorbed).max(0.0);
    let available_shield = participant.arcane_shield.max(0.0);
    let absorbed = available_shield.min(after_overhealing_shield);
    participant.arcane_shield = available_shield - absorbed;
    let mut final_amount = (after_overhealing_shield - absorbed).max(0.0);
    let mut undying_rage_triggered = false;
    let mut hope_avatar_triggered = false;
    let within_undying_rage_limit =
        participant.max_hp > f32::EPSILON && final_amount <= participant.max_hp + f32::EPSILON;
    if participant.undying_rage_active && within_undying_rage_limit {
        final_amount = 0.0;
    } else if participant.undying_rage_enabled
        && !participant.undying_rage_used
        && participant.hp > f32::EPSILON
        && final_amount + f32::EPSILON >= participant.hp
        && within_undying_rage_limit
    {
        participant.undying_rage_used = true;
        participant.undying_rage_active = true;
        undying_rage_triggered = true;
        final_amount = 0.0;
    }
    if final_amount <= f32::EPSILON {
        return BattleDamageResolution {
            damage_applied: 0.0,
            damage_absorbed: incoming_amount,
            undying_rage_triggered,
            hope_avatar_triggered,
            hope_avatar_immune: false,
            defeat_outcome: None,
        };
    }
    let was_alive = participant.alive;
    record_participant_damage_taken(participant, final_amount);
    if encounter_active {
        participant.combat_damage_taken_total += final_amount;
    }
    if was_alive {
        record_participant_damage_contributor(participant, source_id);
        record_participant_arrogance_damage_source(participant, source_id);
        record_participant_endless_pain_stack(participant);
    }
    participant.hp = (participant.hp - final_amount).max(0.0);
    participant.alive = participant.hp > 0.0;
    if !participant.alive && participant.hope_avatar_enabled && !participant.hope_avatar_used {
        participant.alive = true;
        participant.hope_avatar_used = true;
        participant.hope_avatar_rounds_remaining = 2;
        hope_avatar_triggered = true;
    }
    BattleDamageResolution {
        damage_applied: final_amount,
        damage_absorbed: (incoming_amount - final_amount).max(0.0),
        undying_rage_triggered,
        hope_avatar_triggered,
        hope_avatar_immune: false,
        defeat_outcome: participant_defeat_outcome(participant, was_alive),
    }
}

fn advance_participant_hope_avatar(
    participant: &mut BattleParticipantSnapshot,
) -> (
    Option<String>,
    Option<BattleDefeatOutcome>,
) {
    if !participant_hope_avatar_active(participant) {
        return (None, None);
    }
    participant.hope_avatar_rounds_remaining -= 1;
    if participant.hope_avatar_rounds_remaining > 0 {
        return (None, None);
    }
    let was_alive = participant.alive;
    participant.hp = 0.0;
    participant.alive = false;
    (
        Some(format!(
            "{}的希望化身结束，角色死亡",
            participant.display_name
        )),
        participant_defeat_outcome(participant, was_alive),
    )
}

fn apply_penance_kill_assists(
    encounter: &mut BattleEncounter,
    contributor_ids: impl IntoIterator<Item = String>,
) {
    let contributors = contributor_ids.into_iter().collect::<HashSet<_>>();
    if contributors.is_empty() {
        return;
    }
    for participant in &mut encounter.participants {
        if contributors.contains(&participant.target_id) {
            participant.penance_kill_assist_count =
                participant.penance_kill_assist_count.saturating_add(1);
        }
    }
}

fn apply_champion_player_elimination(encounter: &mut BattleEncounter) {
    for participant in &mut encounter.participants {
        if !participant.alive
            || (participant.champion_damage_bonus_per_stack <= f32::EPSILON
                && participant.champion_damage_reduction_per_stack <= f32::EPSILON)
        {
            continue;
        }
        participant.champion_stacks = participant.champion_stacks.saturating_add(1);
    }
}

fn apply_dominion_target_death(encounter: &mut BattleEncounter, defeated_max_hp: f32) {
    let defeated_max_hp = defeated_max_hp.max(0.0);
    if defeated_max_hp <= f32::EPSILON {
        return;
    }
    let mut logs = Vec::new();
    for participant in &mut encounter.participants {
        if !participant.alive
            || participant.dominion_max_hp_gain_rate <= f32::EPSILON
            || participant.dominion_max_hp_bonus_cap <= f32::EPSILON
        {
            continue;
        }
        let remaining =
            (participant.dominion_max_hp_bonus_cap - participant.dominion_max_hp_bonus).max(0.0);
        if remaining <= f32::EPSILON {
            continue;
        }
        let gained = (defeated_max_hp * participant.dominion_max_hp_gain_rate)
            .min(remaining)
            .max(0.0);
        if gained <= f32::EPSILON {
            continue;
        }
        participant.dominion_max_hp_bonus =
            (participant.dominion_max_hp_bonus + gained).min(participant.dominion_max_hp_bonus_cap);
        participant.max_hp += gained;
        logs.push(format!(
            "{}触发役于我手，生命上限提高{}点（{}/{}）",
            participant.display_name,
            format_number(gained),
            format_number(participant.dominion_max_hp_bonus),
            format_number(participant.dominion_max_hp_bonus_cap)
        ));
    }
    encounter.action_log.extend(logs);
}

fn apply_sin_on_sin_kill_participation(
    encounter: &mut BattleEncounter,
    contributor_ids: &HashSet<String>,
) {
    if contributor_ids.is_empty() {
        return;
    }
    let mut logs = Vec::new();
    for participant in &mut encounter.participants {
        if !participant.alive
            || !contributor_ids.contains(&participant.target_id)
            || (participant.sin_on_sin_exp_bonus_per_stack <= f32::EPSILON
                && participant.sin_on_sin_recovery_rate <= f32::EPSILON)
        {
            continue;
        }
        participant.sin_on_sin_stacks = participant.sin_on_sin_stacks.saturating_add(1);
        let hp_recovered = ((participant.max_hp - participant.hp).max(0.0)
            * participant.sin_on_sin_recovery_rate)
            .max(0.0);
        let mp_recovered = ((participant.max_mp - participant.mp).max(0.0)
            * participant.sin_on_sin_recovery_rate)
            .max(0.0);
        if hp_recovered > f32::EPSILON {
            let shield_cap_rate = participant.overhealing_shield_cap_rate;
            apply_participant_healing_for_battle(
                participant,
                hp_recovered,
                shield_cap_rate,
            );
        }
        if mp_recovered > f32::EPSILON {
            participant.mp = (participant.mp + mp_recovered).min(participant.max_mp);
        }
        logs.push(format!(
            "{}触发罪上加罪，回复{}点生命值、{}点魔法值，经验加成{}%",
            participant.display_name,
            format_number(hp_recovered),
            format_number(mp_recovered),
            format_number(sin_on_sin_exp_bonus_percent(
                participant.sin_on_sin_exp_bonus_per_stack,
                participant.sin_on_sin_stacks,
            ))
        ));
    }
    encounter.action_log.extend(logs);
}

fn apply_battle_defeat_outcome(encounter: &mut BattleEncounter, outcome: BattleDefeatOutcome) {
    apply_dominion_target_death(encounter, outcome.defeated_max_hp);
    let contributors = outcome.contributors.into_iter().collect::<HashSet<_>>();
    if !contributors.is_empty() {
        apply_penance_kill_assists(encounter, contributors.iter().cloned());
        apply_sin_on_sin_kill_participation(encounter, &contributors);
    }
    if outcome.defeated_player_character {
        apply_champion_player_elimination(encounter);
    }
}

fn reset_participant_turn_totals(participant: &mut BattleParticipantSnapshot) -> bool {
    let changed = participant.damage_taken_this_turn.abs() > f32::EPSILON
        || participant.healing_taken_this_turn.abs() > f32::EPSILON;
    participant.damage_taken_this_turn = 0.0;
    participant.healing_taken_this_turn = 0.0;
    changed
}

fn completed_participant_turns(encounter: &BattleEncounter) -> u32 {
    encounter
        .participants
        .iter()
        .map(|participant| participant.turn)
        .fold(0_u32, u32::saturating_add)
}

fn schedule_participant_delayed_damage(
    participant: &mut BattleParticipantSnapshot,
    source_id: &str,
    source_name: &str,
    name: &str,
    amount: f32,
    damage_type: DamageType,
) {
    participant
        .delayed_damage_ticks
        .push(BattleDelayedDamageTick {
            name: name.to_owned(),
            source_id: source_id.to_owned(),
            source_name: source_name.to_owned(),
            amount: amount.max(0.0),
            damage_type,
            turns_remaining: 2,
        });
}

fn schedule_participant_delayed_healing(
    participant: &mut BattleParticipantSnapshot,
    source_id: &str,
    source_name: &str,
    name: &str,
    amount: f32,
    overhealing_shield_cap_rate: f32,
    turns_remaining: i32,
) {
    participant
        .delayed_healing_ticks
        .push(BattleDelayedHealingTick {
            name: name.to_owned(),
            source_id: source_id.to_owned(),
            source_name: source_name.to_owned(),
            amount: amount.max(0.0),
            overhealing_shield_cap_rate: overhealing_shield_cap_rate.max(0.0),
            turns_remaining: turns_remaining.max(1),
        });
}

#[derive(Default)]
struct BattleDelayedDamageAdvance {
    logs: Vec<String>,
    defeat_outcomes: Vec<BattleDefeatOutcome>,
}

fn advance_participant_delayed_damage_ticks(
    participant: &mut BattleParticipantSnapshot,
    encounter_active: bool,
) -> BattleDelayedDamageAdvance {
    if participant.delayed_damage_ticks.is_empty() {
        return BattleDelayedDamageAdvance::default();
    }
    let mut advance = BattleDelayedDamageAdvance::default();
    let display_name = participant.display_name.clone();
    let mut due = Vec::new();
    participant.delayed_damage_ticks.retain_mut(|tick| {
        if tick.turns_remaining <= 0 {
            return false;
        }
        tick.turns_remaining -= 1;
        if tick.turns_remaining > 0 {
            due.push(tick.clone());
            true
        } else {
            false
        }
    });
    if !participant.alive {
        return advance;
    }
    for tick in due {
        let final_amount = tick.amount.max(0.0);
        if final_amount <= f32::EPSILON {
            continue;
        }
        let resolution = apply_participant_damage_for_battle(
            participant,
            final_amount,
            &tick.source_id,
            encounter_active,
        );
        if let Some(outcome) = resolution.defeat_outcome {
            advance.defeat_outcomes.push(outcome);
        }
        advance.logs.push(format!(
            "{}触发{}，对{}造成{}点{}伤害",
            tick.source_name,
            tick.name,
            display_name,
            format_number(resolution.damage_applied),
            battle_damage_type_label(tick.damage_type)
        ));
        if resolution.hope_avatar_triggered {
            advance.logs.push(format!(
                "{}触发希望化身，进入持续2回合的无敌天使形态",
                display_name
            ));
        } else if resolution.hope_avatar_immune {
            advance.logs.push(format!(
                "{}处于希望化身，免疫本次伤害",
                display_name
            ));
        } else if resolution.undying_rage_triggered {
            advance.logs.push(format!(
                "{}触发不死者之怒，免疫本次致命伤害",
                display_name
            ));
        } else if resolution.damage_absorbed > f32::EPSILON {
            advance.logs.push(format!(
                "{}吸收{}点伤害",
                display_name,
                format_number(resolution.damage_absorbed)
            ));
        }
    }
    advance
}

fn advance_participant_delayed_healing_ticks(
    participant: &mut BattleParticipantSnapshot,
) -> Vec<String> {
    if participant.delayed_healing_ticks.is_empty() {
        return Vec::new();
    }
    let display_name = participant.display_name.clone();
    let mut due = Vec::new();
    participant.delayed_healing_ticks.retain_mut(|tick| {
        tick.turns_remaining -= 1;
        if tick.turns_remaining <= 0 {
            due.push(tick.clone());
            false
        } else {
            true
        }
    });
    let mut logs = Vec::new();
    for tick in due {
        let final_amount = tick.amount.max(0.0);
        if final_amount <= f32::EPSILON {
            continue;
        }
        apply_participant_healing_for_battle(
            participant,
            final_amount,
            tick.overhealing_shield_cap_rate,
        );
        logs.push(format!(
            "{}触发{}，为{}回复{}点生命值",
            tick.source_name,
            tick.name,
            display_name,
            format_number(final_amount)
        ));
    }
    logs
}

fn setup_battle_round_store(mut commands: Commands) {
    let config_dir = Path::new(".data").join("willowblossom");
    commands.insert_resource(
        Persistent::<BattleRoundStore>::builder()
            .name("battle_rounds")
            .format(StorageFormat::Toml)
            .path(config_dir.join("battle_rounds.toml"))
            .default(BattleRoundStore::default())
            .revertible(true)
            .revert_to_default_on_deserialization_errors(true)
            .build()
            .expect("failed to init battle round store"),
    );
}

fn sync_battle_round_entities(
    mut commands: Commands,
    store: Option<Res<Persistent<BattleRoundStore>>>,
    existing: Query<Entity, With<BattleRoundRuntime>>,
    mut last_signature: Local<u64>,
) {
    let Some(store) = store else {
        return;
    };
    let signature = battle_store_signature(&store);
    if *last_signature == signature {
        return;
    }

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    for (encounter_id, encounter) in &store.encounters {
        commands.spawn((
            BattleRoundRuntime,
            BattleEncounterEntity {
                id: encounter_id.clone(),
                name: encounter.name.clone(),
                active: encounter.active,
                round: encounter.round,
                negative_enabled: encounter.negative_enabled,
            },
        ));

        for participant in &encounter.participants {
            commands.spawn((
                BattleRoundRuntime,
                BattleParticipantEntity {
                    encounter_id: encounter_id.clone(),
                    target_id: participant.target_id.clone(),
                    display_name: participant.display_name.clone(),
                },
                TurnCounter {
                    current: participant.turn,
                },
                BattlePresence {
                    alive: participant.alive,
                    negative_layers: participant.negative_layers,
                    pending_negative: participant.pending_negative,
                },
                BattleVitals {
                    hp: participant.hp,
                    max_hp: participant.max_hp,
                    mp: participant.mp,
                    max_mp: participant.max_mp,
                    hp_regen: participant.hp_regen,
                    mp_regen: participant.mp_regen,
                },
            ));
        }
    }

    *last_signature = signature;
}

fn battle_store_signature(store: &BattleRoundStore) -> u64 {
    let mut hasher = DefaultHasher::new();
    store.active_encounter_id.hash(&mut hasher);
    store.next_encounter_index.hash(&mut hasher);
    let mut encounter_ids = store.encounters.keys().collect::<Vec<_>>();
    encounter_ids.sort();
    for encounter_id in encounter_ids {
        encounter_id.hash(&mut hasher);
        let encounter = &store.encounters[encounter_id];
        encounter.name.hash(&mut hasher);
        encounter.trpg_group.hash(&mut hasher);
        encounter.active.hash(&mut hasher);
        encounter.sort_by_turn.hash(&mut hasher);
        encounter.negative_enabled.hash(&mut hasher);
        encounter.round.hash(&mut hasher);
        for entry in &encounter.action_log {
            entry.hash(&mut hasher);
        }
        for participant in &encounter.participants {
            participant.target_id.hash(&mut hasher);
            participant.display_name.hash(&mut hasher);
            participant.unit_template_id.hash(&mut hasher);
            participant.player_character.hash(&mut hasher);
            participant.turn.hash(&mut hasher);
            participant.str_.hash(&mut hasher);
            participant.agi.hash(&mut hasher);
            participant.dex.hash(&mut hasher);
            participant.int_.hash(&mut hasher);
            participant.wis.hash(&mut hasher);
            participant.action_done.hash(&mut hasher);
            participant.alive.hash(&mut hasher);
            participant.negative_layers.hash(&mut hasher);
            participant.pending_negative.hash(&mut hasher);
            participant.hp.to_bits().hash(&mut hasher);
            participant.max_hp.to_bits().hash(&mut hasher);
            participant.mp.to_bits().hash(&mut hasher);
            participant.max_mp.to_bits().hash(&mut hasher);
            participant.hp_regen.to_bits().hash(&mut hasher);
            participant.mp_regen.to_bits().hash(&mut hasher);
            participant.speed.to_bits().hash(&mut hasher);
            participant.low_survivor_speed.to_bits().hash(&mut hasher);
            participant
                .damage_dealt_modifier
                .to_bits()
                .hash(&mut hasher);
            participant
                .damage_taken_modifier
                .to_bits()
                .hash(&mut hasher);
            participant
                .healing_dealt_modifier
                .to_bits()
                .hash(&mut hasher);
            participant
                .healing_taken_modifier
                .to_bits()
                .hash(&mut hasher);
            participant
                .arrogance_damage_bonus_per_source
                .to_bits()
                .hash(&mut hasher);
            for source_id in &participant.arrogance_damage_source_ids {
                source_id.hash(&mut hasher);
            }
            participant
                .endless_pain_bonus_damage_per_stack
                .to_bits()
                .hash(&mut hasher);
            participant.endless_pain_stacks.hash(&mut hasher);
            participant
                .infinite_focus_damage_bonus_per_stack
                .to_bits()
                .hash(&mut hasher);
            participant.infinite_focus_target_id.hash(&mut hasher);
            participant.infinite_focus_stacks.hash(&mut hasher);
            participant
                .one_heart_healing_bonus_per_stack
                .to_bits()
                .hash(&mut hasher);
            participant.one_heart_target_id.hash(&mut hasher);
            participant.one_heart_stacks.hash(&mut hasher);
            participant.inspiration_enabled.hash(&mut hasher);
            participant.inspiration_target_id.hash(&mut hasher);
            let mut inspiration_sources =
                participant.inspiration_sources.iter().collect::<Vec<_>>();
            inspiration_sources.sort_by(|left, right| left.0.cmp(right.0));
            for (source_id, turns) in inspiration_sources {
                source_id.hash(&mut hasher);
                turns.hash(&mut hasher);
            }
            participant.keen_evasion_enabled.hash(&mut hasher);
            participant.keen_evasion_available.hash(&mut hasher);
            participant.arcane_shield.to_bits().hash(&mut hasher);
            participant
                .overhealing_shield_cap_rate
                .to_bits()
                .hash(&mut hasher);
            participant.overhealing_shield.to_bits().hash(&mut hasher);
            participant
                .overhealing_shield_turns_remaining
                .hash(&mut hasher);
            participant.undying_rage_enabled.hash(&mut hasher);
            participant.undying_rage_used.hash(&mut hasher);
            participant.undying_rage_active.hash(&mut hasher);
            participant.hope_avatar_enabled.hash(&mut hasher);
            participant.hope_avatar_used.hash(&mut hasher);
            participant.hope_avatar_rounds_remaining.hash(&mut hasher);
            participant
                .liquid_body_damage_delay_rate
                .to_bits()
                .hash(&mut hasher);
            participant
                .liquid_body_self_healing_rate
                .to_bits()
                .hash(&mut hasher);
            participant
                .calm_heart_healing_rate
                .to_bits()
                .hash(&mut hasher);
            participant
                .combat_damage_taken_total
                .to_bits()
                .hash(&mut hasher);
            participant
                .champion_damage_bonus_per_stack
                .to_bits()
                .hash(&mut hasher);
            participant
                .champion_damage_reduction_per_stack
                .to_bits()
                .hash(&mut hasher);
            participant.champion_stacks.hash(&mut hasher);
            participant
                .dominion_max_hp_gain_rate
                .to_bits()
                .hash(&mut hasher);
            participant
                .dominion_max_hp_bonus_cap
                .to_bits()
                .hash(&mut hasher);
            participant
                .dominion_max_hp_bonus
                .to_bits()
                .hash(&mut hasher);
            participant
                .sin_on_sin_exp_bonus_per_stack
                .to_bits()
                .hash(&mut hasher);
            participant
                .sin_on_sin_recovery_rate
                .to_bits()
                .hash(&mut hasher);
            participant.sin_on_sin_stacks.hash(&mut hasher);
            participant
                .penance_healing_bonus_percent
                .to_bits()
                .hash(&mut hasher);
            participant.penance_kill_assist_count.hash(&mut hasher);
            for contributor in &participant.damage_contributors {
                contributor.hash(&mut hasher);
            }
            participant.wound_healing_taken_turns.hash(&mut hasher);
            for tick in &participant.delayed_damage_ticks {
                tick.name.hash(&mut hasher);
                tick.source_id.hash(&mut hasher);
                tick.source_name.hash(&mut hasher);
                tick.amount.to_bits().hash(&mut hasher);
                tick.damage_type.hash(&mut hasher);
                tick.turns_remaining.hash(&mut hasher);
            }
            for tick in &participant.delayed_healing_ticks {
                tick.name.hash(&mut hasher);
                tick.source_id.hash(&mut hasher);
                tick.source_name.hash(&mut hasher);
                tick.amount.to_bits().hash(&mut hasher);
                tick.overhealing_shield_cap_rate.to_bits().hash(&mut hasher);
                tick.turns_remaining.hash(&mut hasher);
            }
            participant
                .damage_taken_this_turn
                .to_bits()
                .hash(&mut hasher);
            participant
                .healing_taken_this_turn
                .to_bits()
                .hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn battle_round_panel(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<BattleRoundUiState>,
    mut store: Option<ResMut<Persistent<BattleRoundStore>>>,
    mut manager: Option<ResMut<Persistent<NapcatMessageManager>>>,
    mut rule_engine_state: ResMut<RuleEngineState>,
    scene_positions: Option<Res<SceneCharacterPositions>>,
    encounters: Query<&BattleEncounterEntity>,
) {
    if !ui_state.panel_open {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Some(store) = store.as_deref_mut() else {
        return;
    };
    let Some(manager) = manager.as_deref_mut() else {
        return;
    };

    let mut panel_open = ui_state.panel_open;
    let mut changed = false;
    let mut manager_changed = false;
    let mut close_requested = false;

    egui::Window::new("战斗轮")
        .default_pos(egui::pos2(390.0, 430.0))
        .default_width(480.0)
        .resizable(true)
        .open(&mut panel_open)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    changed |= create_encounter_ui(ui, &mut ui_state, store, manager);
                    ui.separator();

                    let mut encounter_rows = encounters.iter().collect::<Vec<_>>();
                    encounter_rows
                        .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));

                    if encounter_rows.is_empty() {
                        ui.label("还没有战斗轮。");
                    }

                    for encounter_entity in encounter_rows {
                        let encounter_changed = encounter_ui(
                            ui,
                            &mut ui_state,
                            store,
                            manager,
                            &mut rule_engine_state,
                            scene_positions.as_deref(),
                            encounter_entity,
                        );
                        changed |= encounter_changed;
                        if encounter_changed {
                            manager_changed |= sync_encounter_to_manager(
                                store.encounters.get(encounter_entity.id.as_str()),
                                manager,
                            );
                        }
                        ui.add_space(6.0);
                    }

                    ui.separator();
                    if ui.button("关闭").clicked() {
                        close_requested = true;
                    }
                });
        });

    ui_state.panel_open = panel_open && !close_requested;
    if changed {
        store.persist().ok();
    }
    if manager_changed {
        manager.persist().ok();
    }
}

fn create_encounter_ui(
    ui: &mut egui::Ui,
    ui_state: &mut BattleRoundUiState,
    store: &mut BattleRoundStore,
    manager: &NapcatMessageManager,
) -> bool {
    let mut changed = false;
    let mut group_names = manager.trpg_groups.keys().cloned().collect::<Vec<_>>();
    group_names.sort();
    if ui_state.selected_group.is_empty() {
        if let Some(first_group) = group_names.first() {
            ui_state.selected_group = first_group.clone();
        }
    }

    ui.horizontal_wrapped(|ui| {
        ui.label("TRPG组");
        egui::ComboBox::from_id_salt("battle_round_group_select")
            .selected_text(if ui_state.selected_group.is_empty() {
                "无分组"
            } else {
                ui_state.selected_group.as_str()
            })
            .show_ui(ui, |ui| {
                for group_name in &group_names {
                    ui.selectable_value(
                        &mut ui_state.selected_group,
                        group_name.clone(),
                        group_name,
                    );
                }
            });
        ui.label("名称");
        ui.text_edit_singleline(&mut ui_state.new_encounter_name);
        if ui.button("创建").clicked() {
            let group_name = ui_state.selected_group.trim();
            if let Some(group) = manager.trpg_groups.get(group_name) {
                let name = if ui_state.new_encounter_name.trim().is_empty() {
                    group_name.to_owned()
                } else {
                    ui_state.new_encounter_name.trim().to_owned()
                };
                let encounter_id = store.create_encounter_from_group(
                    name,
                    group_name.to_owned(),
                    group,
                    manager,
                );
                store.active_encounter_id = Some(encounter_id);
                ui_state.new_encounter_name.clear();
                changed = true;
            }
        }
    });

    changed
}

fn encounter_ui(
    ui: &mut egui::Ui,
    ui_state: &mut BattleRoundUiState,
    store: &mut BattleRoundStore,
    manager: &mut NapcatMessageManager,
    rule_engine_state: &mut RuleEngineState,
    scene_positions: Option<&SceneCharacterPositions>,
    encounter_entity: &BattleEncounterEntity,
) -> bool {
    let mut changed = false;
    let encounter_id = encounter_entity.id.as_str();
    let initial_round = store
        .encounters
        .get(encounter_id)
        .map(|encounter| encounter.round)
        .unwrap_or_default();
    let mut remove = false;
    if !store.encounters.contains_key(encounter_id) {
        return false;
    }

    ui.group(|ui| {
        ui.set_width(ui.available_width());
        let mut next_round_requested = false;
        {
            let encounter = store
                .encounters
                .get_mut(encounter_id)
                .expect("encounter existence checked");
            ui.horizontal_wrapped(|ui| {
                ui.heading(&encounter_entity.name);
                ui.small(format!("第{}轮", encounter.round));
                ui.small(if encounter_entity.active { "进行中" } else { "休整" });
                if encounter_entity.negative_enabled {
                    ui.small("消极已开");
                }
                let mut active = encounter.active;
                if ui.checkbox(&mut active, "进行中").changed() {
                    changed |= set_encounter_active_state(encounter, active);
                }
                changed |= ui
                    .checkbox(&mut encounter.negative_enabled, "消极")
                    .changed();
                changed |= ui
                    .checkbox(&mut encounter.sort_by_turn, "排序")
                    .on_hover_text("按速度和AGI排序行动顺序。")
                    .changed();
                if ui.button("刷新玩家").clicked() {
                    changed |= refresh_encounter_players(encounter, manager);
                }
                if ui.button("下一轮").clicked() {
                    next_round_requested = true;
                }
                if ui.button("删除").clicked() {
                    remove = true;
                }
            });
        }
        if next_round_requested {
            if store.encounter_has_pending_actions(encounter_id)
                && !ui_state.confirm_next_round.contains(encounter_id)
            {
                ui_state.confirm_next_round.insert(encounter_id.to_owned());
            } else {
                changed |= store.next_round(encounter_id);
                ui_state.confirm_next_round.remove(encounter_id);
            }
        }

        changed |= store.fill_missing_display_names(encounter_id, manager);
        if ui_state.confirm_next_round.contains(encounter_id) {
            let mut confirm_open = true;
            egui::Window::new("确认进入下一轮")
                .collapsible(false)
                .resizable(false)
                .open(&mut confirm_open)
                .show(ui.ctx(), |ui| {
                    ui.label("还有角色未完成行动。确定要强制进入下一轮吗？");
                    ui.horizontal(|ui| {
                        if ui.button("确认下一轮").clicked() {
                            changed |= store.next_round(encounter_id);
                            ui_state.confirm_next_round.remove(encounter_id);
                        }
                        if ui.button("取消").clicked() {
                            ui_state.confirm_next_round.remove(encounter_id);
                        }
                    });
                });
            if !confirm_open {
                ui_state.confirm_next_round.remove(encounter_id);
            }
        }

        changed |= encounter_roster_ui(
            ui,
            ui_state,
            encounter_id,
            store,
            manager,
            scene_positions,
        );
        ui.separator();
        changed |= encounter_action_ui(
            ui,
            ui_state,
            encounter_id,
            store,
            manager,
            scene_positions,
        );
        ui.separator();
        encounter_log_ui(ui, store, encounter_id);
    });

    changed |= sync_battle_round_buff_advancement(
        store,
        encounter_id,
        initial_round,
        manager,
        rule_engine_state,
    );

    if remove {
        store.encounters.remove(encounter_id);
        if store.active_encounter_id.as_deref() == Some(encounter_id) {
            store.active_encounter_id = None;
        }
        changed = true;
    }

    changed
}

fn encounter_roster_ui(
    ui: &mut egui::Ui,
    ui_state: &mut BattleRoundUiState,
    encounter_id: &str,
    store: &mut BattleRoundStore,
    manager: &NapcatMessageManager,
    _scene_positions: Option<&SceneCharacterPositions>,
) -> bool {
    let mut changed = false;
    let Some(encounter) = store.encounters.get_mut(encounter_id) else {
        return false;
    };

    ui.label("行动顺序");
    let order = ordered_participant_indices(encounter);
    let living_player_count = living_player_participant_count(encounter);
    for (order_index, participant_index) in order.iter().copied().enumerate() {
        let mut remove = false;
        let participant = &mut encounter.participants[participant_index];
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{}.", order_index + 1));
            changed |= ui.checkbox(&mut participant.action_done, "").changed();
            changed |= ui
                .text_edit_singleline(&mut participant.display_name)
                .changed();
            ui.small(&participant.target_id);
            ui.label("速度");
            changed |= ui
                .add(egui::DragValue::new(&mut participant.speed).speed(0.5))
                .changed();
            let effective_speed = participant_order_speed(participant, living_player_count);
            if (effective_speed - participant.speed).abs() > f32::EPSILON {
                ui.small(format!(
                    "实 {}",
                    format_number(effective_speed)
                ));
            }
            if participant.penance_healing_bonus_percent > f32::EPSILON
                && participant.penance_kill_assist_count > 0
            {
                ui.small(format!(
                    "忏悔{}次",
                    participant.penance_kill_assist_count
                ));
            }
            if participant.arrogance_damage_bonus_per_source > f32::EPSILON
                && !participant.arrogance_damage_source_ids.is_empty()
            {
                ui.small(format!(
                    "狂妄{}层",
                    participant.arrogance_damage_source_ids.len()
                ));
            }
            if participant.endless_pain_bonus_damage_per_stack > f32::EPSILON
                && participant.endless_pain_stacks > 0
            {
                ui.small(format!(
                    "无尽痛楚{}层",
                    participant.endless_pain_stacks
                ));
            }
            if participant.infinite_focus_damage_bonus_per_stack > f32::EPSILON
                && participant.infinite_focus_stacks > 0
            {
                ui.small(format!(
                    "无限专注{}层",
                    participant.infinite_focus_stacks
                ));
            }
            if participant.one_heart_healing_bonus_per_stack > f32::EPSILON
                && participant.one_heart_stacks > 0
            {
                ui.small(format!(
                    "一心{}层",
                    participant.one_heart_stacks
                ));
            }
            if participant_inspiration_multiplier(participant) > 1.0 + f32::EPSILON {
                ui.small("振奋：速度与伤害+10%");
            }
            if participant.keen_evasion_enabled && participant.keen_evasion_available {
                ui.small("敏锐待机");
            }
            if participant.arcane_shield > f32::EPSILON {
                ui.small(format!(
                    "奥术护盾{}",
                    format_number(participant.arcane_shield)
                ));
            }
            if participant.overhealing_shield > f32::EPSILON {
                ui.small(format!(
                    "过量治疗护盾{}",
                    format_number(participant.overhealing_shield)
                ));
            }
            if participant.undying_rage_active {
                ui.small("不死者之怒生效");
            } else if participant.undying_rage_enabled && participant.undying_rage_used {
                ui.small("不死者之怒已触发");
            }
            if participant_hope_avatar_active(participant) {
                ui.small(format!(
                    "希望化身：剩余{}回合",
                    participant.hope_avatar_rounds_remaining
                ));
            } else if participant.hope_avatar_enabled && participant.hope_avatar_used {
                ui.small("希望化身已结束");
            }
            if participant.liquid_body_damage_delay_rate > f32::EPSILON
                || participant.liquid_body_self_healing_rate > f32::EPSILON
            {
                ui.small("液态躯体");
            }
            if participant.calm_heart_healing_rate > f32::EPSILON {
                if encounter.active && participant.combat_damage_taken_total > f32::EPSILON {
                    ui.small(format!(
                        "息心累计伤害{}",
                        format_number(participant.combat_damage_taken_total)
                    ));
                } else {
                    ui.small("息心");
                }
            }
            let pending_delayed_damage = participant
                .delayed_damage_ticks
                .iter()
                .map(|tick| tick.amount.max(0.0))
                .sum::<f32>();
            if pending_delayed_damage > f32::EPSILON {
                ui.small(format!(
                    "待伤害+{}",
                    format_number(pending_delayed_damage)
                ));
            }
            let pending_delayed_healing = participant
                .delayed_healing_ticks
                .iter()
                .map(|tick| tick.amount.max(0.0))
                .sum::<f32>();
            if pending_delayed_healing > f32::EPSILON {
                ui.small(format!(
                    "待治疗+{}",
                    format_number(pending_delayed_healing)
                ));
            }
            if (participant.champion_damage_bonus_per_stack > f32::EPSILON
                || participant.champion_damage_reduction_per_stack > f32::EPSILON)
                && participant.champion_stacks > 0
            {
                ui.small(format!(
                    "总冠军{}层",
                    participant.champion_stacks
                ));
            }
            if participant.dominion_max_hp_gain_rate > f32::EPSILON
                && participant.dominion_max_hp_bonus > f32::EPSILON
            {
                ui.small(format!(
                    "役于我手+{}/{}",
                    format_number(participant.dominion_max_hp_bonus),
                    format_number(participant.dominion_max_hp_bonus_cap)
                ));
            }
            if (participant.sin_on_sin_exp_bonus_per_stack > f32::EPSILON
                || participant.sin_on_sin_recovery_rate > f32::EPSILON)
                && participant.sin_on_sin_stacks > 0
            {
                ui.small(format!(
                    "罪上加罪{}层/{}%",
                    participant.sin_on_sin_stacks,
                    format_number(sin_on_sin_exp_bonus_percent(
                        participant.sin_on_sin_exp_bonus_per_stack,
                        participant.sin_on_sin_stacks,
                    ))
                ));
            }
            ui.label("AGI");
            changed |= ui
                .add(egui::DragValue::new(&mut participant.agi).speed(1))
                .changed();
            ui.label("HP");
            changed |= ui
                .add(egui::DragValue::new(&mut participant.hp).speed(1.0))
                .changed();
            ui.label("/");
            changed |= ui
                .add(egui::DragValue::new(&mut participant.max_hp).speed(1.0))
                .changed();
            ui.label("MP");
            changed |= ui
                .add(egui::DragValue::new(&mut participant.mp).speed(1.0))
                .changed();
            ui.label("/");
            changed |= ui
                .add(egui::DragValue::new(&mut participant.max_mp).speed(1.0))
                .changed();
            changed |= ui.checkbox(&mut participant.alive, "存活").changed();
            ui.small(format!(
                "本轮承伤 {} / 受疗 {}",
                format_number(participant.damage_taken_this_turn),
                format_number(participant.healing_taken_this_turn)
            ));
            if participant.action_done {
                ui.small("已完成");
            }
            if ui.button("移除").clicked() {
                remove = true;
            }
        });
        if remove {
            encounter.participants.remove(participant_index);
            changed = true;
            break;
        }
    }

    let candidates = available_group_players(encounter, manager);
    if !candidates.is_empty() {
        let selected = ui_state
            .selected_add_player
            .entry(encounter_id.to_owned())
            .or_insert_with(|| candidates[0].0.clone());
        if !candidates
            .iter()
            .any(|(target_id, _)| target_id == selected)
        {
            *selected = candidates[0].0.clone();
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("添加玩家");
            egui::ComboBox::from_id_salt(format!(
                "battle_add_player_{encounter_id}"
            ))
            .selected_text(
                candidates
                    .iter()
                    .find(|(target_id, _)| target_id == selected)
                    .map(|(_, name)| name.as_str())
                    .unwrap_or(selected.as_str()),
            )
            .show_ui(ui, |ui| {
                for (target_id, name) in &candidates {
                    ui.selectable_value(selected, target_id.clone(), name);
                }
            });
            if ui.button("添加").clicked() {
                let mut participant = participant_from_target(selected, manager);
                initialize_participant_clock(
                    &mut participant,
                    encounter.trpg_group.as_deref(),
                    manager,
                );
                encounter.participants.push(participant);
                changed = true;
            }
        });
    }

    let unit_candidates = available_unit_templates(manager);
    if !unit_candidates.is_empty() {
        let selected = ui_state
            .selected_add_unit
            .entry(encounter_id.to_owned())
            .or_insert_with(|| unit_candidates[0].0.clone());
        if !unit_candidates
            .iter()
            .any(|(unit_id, _)| unit_id == selected)
        {
            *selected = unit_candidates[0].0.clone();
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("添加单位");
            egui::ComboBox::from_id_salt(format!(
                "battle_add_unit_{encounter_id}"
            ))
            .selected_text(
                unit_candidates
                    .iter()
                    .find(|(unit_id, _)| unit_id == selected)
                    .map(|(_, name)| name.as_str())
                    .unwrap_or(selected.as_str()),
            )
            .show_ui(ui, |ui| {
                for (unit_id, name) in &unit_candidates {
                    ui.selectable_value(selected, unit_id.clone(), name);
                }
            });
            if ui.button("添加单位").clicked() {
                let unit_id = selected.as_str();
                if let Some(unit) = manager.unit_pool.get(unit_id) {
                    let target_id = next_unit_participant_id(encounter, unit_id);
                    encounter.participants.push(participant_from_unit_template(
                        &target_id, unit_id, unit,
                    ));
                    changed = true;
                }
            }
        });
    }

    if changed {
        normalize_encounter_after_edit(encounter);
    }
    changed
}

fn encounter_action_ui(
    ui: &mut egui::Ui,
    ui_state: &mut BattleRoundUiState,
    encounter_id: &str,
    store: &mut BattleRoundStore,
    manager: &mut NapcatMessageManager,
    scene_positions: Option<&SceneCharacterPositions>,
) -> bool {
    let mut changed = false;
    let Some(encounter) = store.encounters.get(encounter_id) else {
        return false;
    };
    let Some(actor_index) = current_actor_index(encounter) else {
        ui.label("所有行动已完成。");
        if ui.button("开始下一轮").clicked() {
            changed |= store.next_round(encounter_id);
        }
        return changed;
    };
    let actor = encounter.participants[actor_index].clone();
    let target_options = encounter
        .participants
        .iter()
        .filter(|participant| participant.alive)
        .map(|participant| {
            (
                participant.target_id.clone(),
                participant.display_name.clone(),
            )
        })
        .collect::<Vec<_>>();
    let skills = character_for_participant(&actor, manager)
        .as_ref()
        .map(|character| character_skills(character))
        .unwrap_or_default();

    ui.label(format!(
        "当前行动者：{}",
        actor.display_name
    ));
    let target = ui_state
        .selected_action_target
        .entry(encounter_id.to_owned())
        .or_insert_with(|| {
            target_options
                .iter()
                .find(|(target_id, _)| target_id != &actor.target_id)
                .or_else(|| target_options.first())
                .map(|(target_id, _)| target_id.clone())
                .unwrap_or_default()
        });
    if !target_options
        .iter()
        .any(|(target_id, _)| target_id == target)
    {
        *target = target_options
            .first()
            .map(|(target_id, _)| target_id.clone())
            .unwrap_or_default();
    }
    let amount = ui_state
        .action_amount
        .entry(encounter_id.to_owned())
        .or_insert(1.0);

    ui.horizontal_wrapped(|ui| {
        ui.label("目标");
        egui::ComboBox::from_id_salt(format!(
            "battle_action_target_{encounter_id}"
        ))
        .selected_text(display_name_for_target(
            &target_options,
            target,
        ))
        .show_ui(ui, |ui| {
            for (target_id, name) in &target_options {
                ui.selectable_value(target, target_id.clone(), name);
            }
        });
        ui.label("伤害");
        ui.add(egui::DragValue::new(amount).speed(1.0).range(0.0..=9999.0));
        if ui
            .add_enabled(
                !participant_hope_avatar_active(&actor),
                egui::Button::new("普通攻击"),
            )
            .clicked()
        {
            changed |= store.apply_action(
                encounter_id,
                &actor.target_id,
                target,
                "普通攻击",
                *amount,
            );
            changed |= store.finish_actor_action(encounter_id, &actor.target_id);
        }
        if ui.button("标记完成").clicked() {
            changed |= store.finish_actor_action(encounter_id, &actor.target_id);
        }
        if ui.button("跳过+消极").clicked() {
            changed |= store.skip_negative_participant(encounter_id, &actor.target_id);
        }
    });

    if !skills.is_empty() {
        let selected_skill = ui_state
            .selected_skill_index
            .entry(encounter_id.to_owned())
            .or_insert(0);
        if *selected_skill >= skills.len() {
            *selected_skill = 0;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("技能");
            egui::ComboBox::from_id_salt(format!("battle_skill_{encounter_id}"))
                .selected_text(skills[*selected_skill].name.as_str())
                .show_ui(ui, |ui| {
                    for (index, skill) in skills.iter().enumerate() {
                        let remaining = skill_cooldown_remaining(
                            &actor,
                            skill.index,
                            skill.cooldown_turns,
                            skill.cooldown_left,
                        );
                        let mut label = skill.name.clone();
                        let mut details = Vec::new();
                        if skill.mp_cost > 0.0 {
                            details.push(format!(
                                "MP {}",
                                format_number(skill.mp_cost)
                            ));
                        }
                        if remaining > 0 {
                            details.push(format!("CD {remaining}"));
                        } else if skill.cooldown_turns > 0 {
                            details.push(format!("CD {}", skill.cooldown_turns));
                        }
                        if !details.is_empty() {
                            label = format!("{label} ({})", details.join(", "));
                        }
                        ui.selectable_value(selected_skill, index, label);
                    }
                });
            let skill = &skills[*selected_skill];
            let cooldown_remaining = skill_cooldown_remaining(
                &actor,
                skill.index,
                skill.cooldown_turns,
                skill.cooldown_left,
            );
            let can_pay = actor.mp + f32::EPSILON >= skill.mp_cost.max(0.0);
            let effects = static_skill_effects(
                &skill.note,
                &skill.arg_values,
                skill.skill_type.as_deref(),
                skill.legacy_buff_machine_json.as_deref(),
            );
            let hope_avatar_allows = !participant_hope_avatar_active(&actor)
                || skill_effects_are_hope_avatar_healing(&effects);
            let can_use = cooldown_remaining == 0 && can_pay && hope_avatar_allows;
            let response = ui.add_enabled(can_use, egui::Button::new("使用技能"));
            if response.clicked() {
                changed |= store.record_skill_use_with_buffs(
                    encounter_id,
                    &actor.target_id,
                    target,
                    skill,
                    manager,
                    scene_positions,
                );
                changed |= store.finish_actor_action(encounter_id, &actor.target_id);
            }
            if !hope_avatar_allows {
                ui.small("希望化身期间只能释放治疗技能");
            } else if !can_pay {
                ui.small(format!(
                    "需要{} MP",
                    format_number(skill.mp_cost.max(0.0))
                ));
            } else if cooldown_remaining > 0 {
                ui.small(format!(
                    "冷却还剩{cooldown_remaining}轮"
                ));
            }
        });
    } else {
        ui.small("这个角色没有技能。");
    }

    changed
}

fn encounter_log_ui(ui: &mut egui::Ui, store: &BattleRoundStore, encounter_id: &str) {
    let Some(encounter) = store.encounters.get(encounter_id) else {
        return;
    };
    if encounter.action_log.is_empty() {
        return;
    }
    ui.label("日志");
    for entry in encounter.action_log.iter().rev().take(6) {
        ui.small(entry);
    }
}

impl BattleRoundStore {
    fn create_encounter_from_group(
        &mut self,
        name: String,
        group_name: String,
        group: &TrpgGroup,
        manager: &NapcatMessageManager,
    ) -> String {
        let encounter_id = format!("battle-{}", self.next_encounter_index);
        self.next_encounter_index += 1;
        let participants = group
            .players
            .iter()
            .map(|target_id| {
                let mut participant = participant_from_target(target_id, manager);
                initialize_participant_clock(
                    &mut participant,
                    Some(&group_name),
                    manager,
                );
                participant
            })
            .collect::<Vec<_>>();

        self.encounters
            .insert(encounter_id.clone(), BattleEncounter {
                name,
                trpg_group: Some(group_name),
                active: true,
                sort_by_turn: group.battle_sort_by_turn,
                negative_enabled: group.battle_negative_enabled,
                round: group.world_turn,
                participants,
                action_log: Vec::new(),
            });
        encounter_id
    }

    fn next_round(&mut self, encounter_id: &str) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        encounter.round += 1;
        advance_encounter_inspiration(encounter);
        let mut delayed_logs = Vec::new();
        let mut defeat_outcomes = Vec::new();
        for participant in &mut encounter.participants {
            participant.action_done = false;
            participant.undying_rage_active = false;
            advance_participant_overhealing_shield(participant);
            let previous_damage_taken = participant.damage_taken_this_turn;
            reset_participant_turn_totals(participant);
            let (hope_log, hope_outcome) = advance_participant_hope_avatar(participant);
            if let Some(log) = hope_log {
                delayed_logs.push(log);
                if let Some(outcome) = hope_outcome {
                    defeat_outcomes.push(outcome);
                }
                continue;
            }
            if let Some(log) =
                apply_participant_liquid_body_healing(participant, previous_damage_taken)
            {
                delayed_logs.push(log);
            }
            if participant.wound_healing_taken_turns > 0 {
                participant.wound_healing_taken_turns -= 1;
            }
            if participant.alive {
                if !encounter.active {
                    participant.hp =
                        (participant.hp + participant.hp_regen).min(participant.max_hp);
                }
                participant.mp = (participant.mp + participant.mp_regen).min(participant.max_mp);
            }
            let delayed = advance_participant_delayed_damage_ticks(participant, encounter.active);
            delayed_logs.extend(delayed.logs);
            defeat_outcomes.extend(delayed.defeat_outcomes);
            delayed_logs.extend(advance_participant_delayed_healing_ticks(participant));
        }
        for outcome in defeat_outcomes {
            apply_battle_defeat_outcome(encounter, outcome);
        }
        encounter
            .action_log
            .push(format!("第{}轮开始", encounter.round));
        encounter.action_log.extend(delayed_logs);
        if encounter.negative_enabled {
            mark_negative_candidates(encounter);
        }
        true
    }

    fn encounter_has_pending_actions(&self, encounter_id: &str) -> bool {
        self.encounters
            .get(encounter_id)
            .map(|encounter| {
                encounter
                    .participants
                    .iter()
                    .any(|participant| participant.alive && !participant.action_done)
            })
            .unwrap_or(false)
    }

    fn fill_missing_display_names(
        &mut self,
        encounter_id: &str,
        manager: &NapcatMessageManager,
    ) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        let mut changed = false;
        for participant in &mut encounter.participants {
            if participant.display_name.trim().is_empty()
                || participant.display_name == participant.target_id
            {
                let display_name = participant_snapshot_display_name(participant, manager);
                if display_name != participant.display_name {
                    participant.display_name = display_name;
                    changed = true;
                }
            }
        }
        changed
    }

    fn finish_actor_action(&mut self, encounter_id: &str, target_id: &str) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        let Some(participant) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == target_id)
        else {
            return false;
        };
        participant.action_done = true;
        participant.turn += 1;
        participant.pending_negative = false;
        if encounter
            .participants
            .iter()
            .all(|participant| !participant.alive || participant.action_done)
        {
            let _ = self.next_round(encounter_id);
        } else if encounter.negative_enabled {
            mark_negative_candidates(encounter);
        }
        true
    }

    fn apply_action(
        &mut self,
        encounter_id: &str,
        actor_id: &str,
        target_id: &str,
        action_name: &str,
        damage: f32,
    ) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        let (actor_name, actor_hope_avatar_active) = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == actor_id)
            .map(|participant| {
                (
                    participant.display_name.clone(),
                    participant_hope_avatar_active(participant),
                )
            })
            .unwrap_or_else(|| (actor_id.to_owned(), false));
        if actor_hope_avatar_active {
            encounter.action_log.push(format!(
                "{}处于希望化身，只能释放治疗技能",
                actor_name
            ));
            return false;
        }
        let Some(target) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == target_id)
        else {
            return false;
        };
        let final_damage = damage.max(0.0);
        let resolution = apply_participant_damage_for_battle(
            target,
            final_damage,
            actor_id,
            encounter.active,
        );
        let target_display_name = target.display_name.clone();
        encounter.action_log.push(format!(
            "{}对{}使用{}，造成{}点伤害",
            actor_name,
            target_display_name,
            action_name,
            format_number(resolution.damage_applied)
        ));
        if resolution.hope_avatar_triggered {
            encounter.action_log.push(format!(
                "{}触发希望化身，进入持续2回合的无敌天使形态",
                target_display_name
            ));
        } else if resolution.hope_avatar_immune {
            encounter.action_log.push(format!(
                "{}处于希望化身，免疫本次伤害",
                target_display_name
            ));
        } else if resolution.undying_rage_triggered {
            encounter.action_log.push(format!(
                "{}触发不死者之怒，免疫本次致命伤害",
                target_display_name
            ));
        } else if resolution.damage_absorbed > f32::EPSILON {
            encounter.action_log.push(format!(
                "{}吸收{}点伤害",
                target_display_name,
                format_number(resolution.damage_absorbed)
            ));
        }
        if let Some(outcome) = resolution.defeat_outcome {
            apply_battle_defeat_outcome(encounter, outcome);
        }
        true
    }

    fn record_skill_use(
        &mut self,
        encounter_id: &str,
        actor_id: &str,
        target_id: &str,
        skill: &CharacterSkill,
        manager: &NapcatMessageManager,
        scene_positions: Option<&SceneCharacterPositions>,
    ) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        let basic_config = encounter_basic_config(encounter, manager, actor_id);
        let Some(mut actor_snapshot) = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == actor_id)
            .cloned()
        else {
            return false;
        };
        let actor_character = character_for_participant(&actor_snapshot, manager);
        let effects = static_skill_effects(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            skill.legacy_buff_machine_json.as_deref(),
        );
        let actor_damage_dealt_buffs = actor_character
            .as_ref()
            .map(|character| character_damage_dealt_talent_buffs(character, actor_id))
            .unwrap_or_default();
        let actor_physical_damage_lifesteal = actor_character
            .as_ref()
            .map(character_physical_damage_lifesteal)
            .unwrap_or(0.0);
        let actor_physical_damage_followup_rate = actor_character
            .as_ref()
            .map(character_physical_damage_followup_rate)
            .unwrap_or(0.0);
        let actor_minimum_damage_floor = actor_character
            .as_ref()
            .map(character_minimum_damage_floor)
            .unwrap_or(0.0);
        let actor_name = actor_snapshot.display_name.clone();
        let target_name = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == target_id)
            .map(|participant| participant.display_name.clone())
            .unwrap_or_else(|| target_id.to_owned());
        if participant_hope_avatar_active(&actor_snapshot)
            && !skill_effects_are_hope_avatar_healing(&effects)
        {
            encounter.action_log.push(format!(
                "{}处于希望化身，只能释放治疗技能",
                actor_name
            ));
            return false;
        }
        let Some(actor) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == actor_id)
        else {
            return false;
        };
        let mp_cost = skill.mp_cost.max(0.0);
        let cooldown_remaining = skill_cooldown_remaining(
            actor,
            skill.index,
            skill.cooldown_turns,
            skill.cooldown_left,
        );
        if cooldown_remaining > 0 {
            encounter.action_log.push(format!(
                "{}不能使用{}；冷却还剩{}轮",
                actor_name, skill.name, cooldown_remaining
            ));
            return false;
        }
        if actor.mp + f32::EPSILON < mp_cost {
            encounter.action_log.push(format!(
                "{}不能使用{}；需要{} MP",
                actor_name,
                skill.name,
                format_number(mp_cost)
            ));
            return false;
        }
        actor.mp = (actor.mp - mp_cost).max(0.0);
        actor.skill_last_used_turns.insert(
            skill.index.to_string(),
            actor.turn.saturating_add(1),
        );

        if effects.is_empty() {
            let note = skill.note.trim();
            if note.is_empty() {
                encounter.action_log.push(format!(
                    "{}对{}使用{}",
                    actor_name, target_name, skill.name
                ));
            } else {
                encounter.action_log.push(format!(
                    "{}对{}使用{}（{}）",
                    actor_name, target_name, skill.name, note
                ));
            }
        }
        for effect in effects {
            if let Some(current_actor) = encounter
                .participants
                .iter()
                .find(|participant| participant.target_id == actor_id)
            {
                actor_snapshot = current_actor.clone();
            }
            match effect {
                SkillEffect::Damage {
                    amount,
                    target,
                    damage_type,
                } => {
                    let actor_damage_multiplier = participant_damage_multiplier(
                        &actor_snapshot,
                        actor_character.as_ref(),
                        &basic_config,
                        completed_participant_turns(encounter),
                        damage_type,
                    );
                    let fallback_radius = battle_skill_damage_range_radius(
                        skill.range,
                        actor_character.as_ref(),
                        damage_type,
                        skill.skill_type.as_deref(),
                    );
                    let target_ids = resolve_skill_targets(
                        target,
                        actor_id,
                        target_id,
                        encounter,
                        scene_positions,
                        fallback_radius,
                        skill.target_class.as_deref(),
                    );
                    let target_ids = limit_skill_targets(
                        target_ids,
                        skill_target_limit(
                            skill.target_count,
                            skill.target_class.as_deref(),
                        ),
                    );
                    let infinite_focus_target_id = infinite_focus_eligible_target_id(
                        target,
                        actor_id,
                        &target_ids,
                        skill.target_class.as_deref(),
                    );
                    if target_ids.is_empty() {
                        encounter.action_log.push(format!(
                            "{}使用{}，但范围内没有目标",
                            actor_name, skill.name
                        ));
                    }
                    let mut pending_actor_lifesteal = 0.0;
                    let mut pending_endless_pain_bonus_damage = endless_pain_bonus_damage(
                        actor_snapshot.endless_pain_bonus_damage_per_stack,
                        actor_snapshot.endless_pain_stacks,
                    );
                    let mut consumed_endless_pain_stacks = 0_u32;
                    let mut infinite_focus_hit_target_id = None::<String>;
                    let damage_target_selector = target;
                    let damage_target_class = skill.target_class.as_deref();
                    for resolved_target_id in target_ids {
                        let Some(target) = encounter
                            .participants
                            .iter_mut()
                            .find(|participant| participant.target_id == resolved_target_id)
                        else {
                            continue;
                        };
                        let target_character = character_for_participant(target, manager);
                        let target_damage_multiplier = target.damage_taken_modifier
                            * champion_damage_taken_multiplier(
                                target.champion_damage_reduction_per_stack,
                                target.champion_stacks,
                            )
                            * target_character
                                .as_ref()
                                .map(|character| {
                                    character_damage_taken_attribute_multiplier(
                                        character,
                                        trpg_damage_taken_kind(damage_type),
                                    )
                                })
                                .unwrap_or(1.0)
                            * target_character
                                .as_ref()
                                .map(|character| {
                                    character_fighting_spirit_damage_taken_multiplier(
                                        character,
                                        target.turn,
                                    )
                                })
                                .unwrap_or(1.0);
                        let infinite_focus_multiplier = if infinite_focus_target_id.as_deref()
                            == Some(resolved_target_id.as_str())
                        {
                            participant_infinite_focus_damage_multiplier(
                                &actor_snapshot,
                                &resolved_target_id,
                            )
                        } else {
                            1.0
                        };
                        let incoming_amount = (amount
                            * actor_damage_multiplier
                            * infinite_focus_multiplier
                            * target_damage_multiplier)
                            .max(0.0);
                        let target_large_hit_modifier = target_character
                            .as_ref()
                            .map(character_large_hit_damage_taken_modifier)
                            .unwrap_or(1.0);
                        let typed_final_amount = (incoming_amount
                            * large_hit_damage_taken_multiplier(
                                target.max_hp,
                                incoming_amount,
                                target_large_hit_modifier,
                            ))
                        .max(0.0);
                        let mut final_amount =
                            if amount > f32::EPSILON && actor_minimum_damage_floor > f32::EPSILON {
                                typed_final_amount.max(actor_minimum_damage_floor)
                            } else {
                                typed_final_amount
                            };
                        let evaded_by_keen_evasion = participant_keen_evasion_evades_damage(
                            target,
                            final_amount,
                            damage_target_selector,
                            damage_target_class,
                        );
                        if evaded_by_keen_evasion {
                            final_amount = 0.0;
                        }
                        let endless_pain_bonus = if final_amount > f32::EPSILON
                            && pending_endless_pain_bonus_damage > f32::EPSILON
                        {
                            pending_endless_pain_bonus_damage
                        } else {
                            0.0
                        };
                        let resolved_amount = final_amount + endless_pain_bonus;
                        let physical_damage_share = if resolved_amount > f32::EPSILON {
                            (final_amount / resolved_amount).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let (final_amount, delayed_liquid_body_damage) =
                            if participant_hope_avatar_active(target) {
                                (resolved_amount, 0.0)
                            } else {
                                participant_liquid_body_split_damage(target, resolved_amount)
                            };
                        let target_display_name = target.display_name.clone();
                        if delayed_liquid_body_damage > f32::EPSILON {
                            schedule_participant_delayed_damage(
                                target,
                                actor_id,
                                &target_display_name,
                                "液态躯体",
                                delayed_liquid_body_damage,
                                damage_type,
                            );
                        }
                        let resolution = apply_participant_damage_for_battle(
                            target,
                            final_amount,
                            actor_id,
                            encounter.active,
                        );
                        let applied_physical_damage =
                            resolution.damage_applied * physical_damage_share;
                        let endless_pain_damage_committed = endless_pain_bonus > f32::EPSILON
                            && (resolution.damage_applied > f32::EPSILON
                                || delayed_liquid_body_damage > f32::EPSILON);
                        if endless_pain_damage_committed {
                            pending_endless_pain_bonus_damage = 0.0;
                            consumed_endless_pain_stacks =
                                actor_snapshot.endless_pain_stacks.min(2);
                        }
                        if resolution.damage_applied > f32::EPSILON
                            && actor_damage_dealt_buffs
                                .iter()
                                .any(|buff| buff.name == "溃伤")
                        {
                            target.wound_healing_taken_turns = 1;
                        }
                        if applied_physical_damage > f32::EPSILON
                            && damage_type == DamageType::Physical
                        {
                            pending_actor_lifesteal +=
                                applied_physical_damage * actor_physical_damage_lifesteal;
                            if actor_physical_damage_followup_rate > f32::EPSILON {
                                schedule_participant_delayed_damage(
                                    target,
                                    actor_id,
                                    &actor_name,
                                    "苏萨斯之爪",
                                    applied_physical_damage * actor_physical_damage_followup_rate,
                                    DamageType::Magical,
                                );
                            }
                        }
                        encounter.action_log.push(format!(
                            "{}对{}使用{}，造成{}点伤害",
                            actor_name,
                            target_display_name,
                            skill.name,
                            format_number(resolution.damage_applied)
                        ));
                        if evaded_by_keen_evasion {
                            encounter.action_log.push(format!(
                                "{}触发敏锐，闪避本次伤害",
                                target_display_name
                            ));
                        }
                        if delayed_liquid_body_damage > f32::EPSILON {
                            encounter.action_log.push(format!(
                                "{}触发液态躯体，延后{}点伤害",
                                target_display_name,
                                format_number(delayed_liquid_body_damage)
                            ));
                        }
                        if resolution.hope_avatar_triggered {
                            encounter.action_log.push(format!(
                                "{}触发希望化身，进入持续2回合的无敌天使形态",
                                target_display_name
                            ));
                        } else if resolution.hope_avatar_immune {
                            encounter.action_log.push(format!(
                                "{}处于希望化身，免疫本次伤害",
                                target_display_name
                            ));
                        } else if resolution.undying_rage_triggered {
                            encounter.action_log.push(format!(
                                "{}触发不死者之怒，免疫本次致命伤害",
                                target_display_name
                            ));
                        } else if resolution.damage_absorbed > f32::EPSILON {
                            encounter.action_log.push(format!(
                                "{}吸收{}点伤害",
                                target_display_name,
                                format_number(resolution.damage_absorbed)
                            ));
                        }
                        if endless_pain_damage_committed {
                            encounter.action_log.push(format!(
                                "{}触发无尽痛楚，追加{}点无类型伤害",
                                actor_name,
                                format_number(endless_pain_bonus)
                            ));
                        }
                        if resolution.damage_applied > f32::EPSILON
                            && infinite_focus_target_id.as_deref()
                                == Some(resolved_target_id.as_str())
                        {
                            infinite_focus_hit_target_id = Some(resolved_target_id.clone());
                            if infinite_focus_multiplier > 1.0 + f32::EPSILON {
                                encounter.action_log.push(format!(
                                    "{}触发无限专注，伤害提高{}%",
                                    actor_name,
                                    format_number((infinite_focus_multiplier - 1.0) * 100.0)
                                ));
                            }
                        }
                        if let Some(outcome) = resolution.defeat_outcome {
                            apply_battle_defeat_outcome(encounter, outcome);
                        }
                    }
                    if let Some(hit_target_id) = infinite_focus_hit_target_id {
                        if let Some(actor) = encounter
                            .participants
                            .iter_mut()
                            .find(|participant| participant.target_id == actor_id)
                        {
                            record_participant_infinite_focus_hit(actor, &hit_target_id);
                        }
                    }
                    if consumed_endless_pain_stacks > 0 {
                        if let Some(actor) = encounter
                            .participants
                            .iter_mut()
                            .find(|participant| participant.target_id == actor_id)
                        {
                            actor.endless_pain_stacks = actor
                                .endless_pain_stacks
                                .saturating_sub(consumed_endless_pain_stacks);
                        }
                    }
                    if pending_actor_lifesteal > f32::EPSILON {
                        if let Some(actor) = encounter
                            .participants
                            .iter_mut()
                            .find(|participant| participant.target_id == actor_id)
                        {
                            apply_participant_healing_for_battle(
                                actor,
                                pending_actor_lifesteal,
                                actor_snapshot.overhealing_shield_cap_rate,
                            );
                            encounter.action_log.push(format!(
                                "{}触发禅宗古训，回复{}点生命值",
                                actor_name,
                                format_number(pending_actor_lifesteal)
                            ));
                        }
                    }
                },
                SkillEffect::Heal { amount, target } => {
                    let actor_healing_multiplier = participant_healing_multiplier(
                        &actor_snapshot,
                        actor_character.as_ref(),
                        &basic_config,
                    );
                    let actor_mutual_aid_healing_rate = actor_character
                        .as_ref()
                        .map(character_mutual_aid_healing_rate)
                        .unwrap_or(0.0);
                    let actor_echoing_memory_healing_rates = actor_character
                        .as_ref()
                        .and_then(|character| character_echoing_memory_healing_rates(character));
                    let target_ids = resolve_skill_targets(
                        target,
                        actor_id,
                        target_id,
                        encounter,
                        scene_positions,
                        skill_range_radius(skill.range),
                        skill.target_class.as_deref(),
                    );
                    let target_ids = limit_skill_targets(
                        target_ids,
                        skill_target_limit(
                            skill.target_count,
                            skill.target_class.as_deref(),
                        ),
                    );
                    let single_heal_target_id = one_heart_eligible_target_id(
                        target,
                        &target_ids,
                        skill.target_class.as_deref(),
                    );
                    if target_ids.is_empty() {
                        encounter.action_log.push(format!(
                            "{}使用{}，但范围内没有目标",
                            actor_name, skill.name
                        ));
                    }
                    let mut pending_actor_mutual_aid_healing = 0.0;
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
                        let target_dying_healing_modifier = target_character
                            .as_ref()
                            .map(character_dying_healing_taken_modifier)
                            .unwrap_or(1.0);
                        let target_mutual_aid_healing_rate = target_character
                            .as_ref()
                            .map(character_mutual_aid_healing_rate)
                            .unwrap_or(0.0);
                        let target_healing_multiplier = target.healing_taken_modifier
                            * participant_wound_healing_multiplier(target)
                            * dying_healing_taken_multiplier(
                                target.hp,
                                target.max_hp,
                                target_dying_healing_modifier,
                            );
                        let one_heart_multiplier = if single_heal_target_id.as_deref()
                            == Some(resolved_target_id.as_str())
                        {
                            participant_one_heart_healing_multiplier(
                                &actor_snapshot,
                                &resolved_target_id,
                            )
                        } else {
                            1.0
                        };
                        let final_amount = (amount
                            * actor_healing_multiplier
                            * one_heart_multiplier
                            * target_healing_multiplier)
                            .max(0.0);
                        apply_participant_healing_for_battle(
                            target,
                            final_amount,
                            actor_snapshot.overhealing_shield_cap_rate,
                        );
                        if resolved_target_id != actor_id && final_amount > f32::EPSILON {
                            pending_actor_mutual_aid_healing += final_amount
                                * (actor_mutual_aid_healing_rate + target_mutual_aid_healing_rate);
                        }
                        if final_amount > f32::EPSILON
                            && single_heal_target_id.as_deref() == Some(resolved_target_id.as_str())
                        {
                            healed_one_heart_target_id = Some(resolved_target_id.clone());
                            healed_inspiration_target_id = Some(resolved_target_id.clone());
                        }
                        if final_amount > f32::EPSILON
                            && single_heal_target_id.as_deref() == Some(resolved_target_id.as_str())
                        {
                            if let Some((first_echo_rate, second_echo_rate)) =
                                actor_echoing_memory_healing_rates
                            {
                                schedule_participant_delayed_healing(
                                    target,
                                    actor_id,
                                    &actor_name,
                                    "千万回忆",
                                    final_amount * first_echo_rate,
                                    actor_snapshot.overhealing_shield_cap_rate,
                                    1,
                                );
                                schedule_participant_delayed_healing(
                                    target,
                                    actor_id,
                                    &actor_name,
                                    "千万回忆",
                                    final_amount * second_echo_rate,
                                    actor_snapshot.overhealing_shield_cap_rate,
                                    2,
                                );
                            }
                        }
                        encounter.action_log.push(format!(
                            "{}对{}使用{}，回复{}点生命值",
                            actor_name,
                            target.display_name,
                            skill.name,
                            format_number(final_amount)
                        ));
                        if one_heart_multiplier > 1.0 + f32::EPSILON {
                            encounter.action_log.push(format!(
                                "{}触发一心，治疗效果提高{}%",
                                actor_name,
                                format_number((one_heart_multiplier - 1.0) * 100.0)
                            ));
                        }
                    }
                    if let Some(target_id) = healed_one_heart_target_id {
                        if let Some(actor) = encounter
                            .participants
                            .iter_mut()
                            .find(|participant| participant.target_id == actor_id)
                        {
                            record_participant_one_heart_heal(actor, &target_id);
                        }
                    }
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
                    if pending_actor_mutual_aid_healing > f32::EPSILON {
                        if let Some(actor) = encounter
                            .participants
                            .iter_mut()
                            .find(|participant| participant.target_id == actor_id)
                        {
                            let shield_cap_rate = actor.overhealing_shield_cap_rate;
                            apply_participant_healing_for_battle(
                                actor,
                                pending_actor_mutual_aid_healing,
                                shield_cap_rate,
                            );
                            encounter.action_log.push(format!(
                                "{}触发互帮互助，回复{}点生命值",
                                actor_name,
                                format_number(pending_actor_mutual_aid_healing)
                            ));
                        }
                    }
                },
                SkillEffect::GrantBuff { target, buff } => {
                    let target_ids = resolve_skill_targets(
                        target,
                        actor_id,
                        target_id,
                        encounter,
                        scene_positions,
                        skill_range_radius(skill.range),
                        skill.target_class.as_deref(),
                    );
                    let target_ids = limit_skill_targets(
                        target_ids,
                        skill_target_limit(
                            skill.target_count,
                            skill.target_class.as_deref(),
                        ),
                    );
                    if target_ids.is_empty() {
                        encounter.action_log.push(format!(
                            "{}使用{}，但范围内没有目标",
                            actor_name, skill.name
                        ));
                    }
                    for resolved_target_id in target_ids {
                        let target_name = encounter
                            .participants
                            .iter()
                            .find(|participant| participant.target_id == resolved_target_id)
                            .map(|participant| participant.display_name.clone())
                            .unwrap_or_else(|| resolved_target_id.clone());
                        encounter.action_log.push(format!(
                            "{}对{}使用{}，施加{}状态",
                            actor_name, target_name, skill.name, buff.name
                        ));
                    }
                },
            }
        }
        if mp_cost > 0.0 {
            encounter.action_log.push(format!(
                "{}消耗{} MP",
                actor_name,
                format_number(mp_cost)
            ));
        }
        true
    }

    fn record_skill_use_with_buffs(
        &mut self,
        encounter_id: &str,
        actor_id: &str,
        target_id: &str,
        skill: &CharacterSkill,
        manager: &mut NapcatMessageManager,
        scene_positions: Option<&SceneCharacterPositions>,
    ) -> bool {
        if !self.record_skill_use(
            encounter_id,
            actor_id,
            target_id,
            skill,
            manager,
            scene_positions,
        ) {
            return false;
        }

        let effects = static_skill_effects(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            skill.legacy_buff_machine_json.as_deref(),
        );
        let granted_buffs = {
            let Some(encounter) = self.encounters.get(encounter_id) else {
                return true;
            };
            effects
                .into_iter()
                .filter_map(|effect| {
                    let SkillEffect::GrantBuff { target, buff } = effect else {
                        return None;
                    };
                    let targets = resolve_skill_targets(
                        target,
                        actor_id,
                        target_id,
                        encounter,
                        scene_positions,
                        skill_range_radius(skill.range),
                        skill.target_class.as_deref(),
                    );
                    Some((
                        limit_skill_targets(
                            targets,
                            skill_target_limit(
                                skill.target_count,
                                skill.target_class.as_deref(),
                            ),
                        ),
                        buff,
                    ))
                })
                .collect::<Vec<_>>()
        };
        if granted_buffs.is_empty() {
            return true;
        }

        let _ = sync_encounter_to_manager(
            self.encounters.get(encounter_id),
            manager,
        );
        let skill_pool = manager.skill_pool.clone();
        let mut rule_engine_state = RuleEngineState::default();
        for (target_ids, buff) in granted_buffs {
            for resolved_target_id in target_ids {
                let unit_template_id = self
                    .encounters
                    .get(encounter_id)
                    .and_then(|encounter| {
                        encounter
                            .participants
                            .iter()
                            .find(|participant| participant.target_id == resolved_target_id)
                    })
                    .and_then(|participant| participant.unit_template_id.clone());
                let stat_config = manager.character_stat_config_for_target(&resolved_target_id);
                let buff = buff.to_buff_spec(actor_id);
                if unit_template_id.is_some() {
                    let Some(participant) =
                        self.encounters.get_mut(encounter_id).and_then(|encounter| {
                            encounter
                                .participants
                                .iter_mut()
                                .find(|participant| participant.target_id == resolved_target_id)
                        })
                    else {
                        continue;
                    };
                    let Some(mut character) = character_for_participant(participant, manager)
                    else {
                        continue;
                    };
                    character.active_buffs.push(buff);
                    participant.unit_character = Some(character);
                    sync_participant_from_manager_with_vitals(participant, manager);
                    continue;
                } else {
                    let Some(character) = manager.player_characters.get_mut(&resolved_target_id)
                    else {
                        continue;
                    };
                    character.active_buffs.push(buff);
                    sync_character_buffs(
                        &resolved_target_id,
                        character,
                        &stat_config,
                        &mut rule_engine_state,
                        &skill_pool,
                    );
                }
                if let Some(participant) =
                    self.encounters.get_mut(encounter_id).and_then(|encounter| {
                        encounter
                            .participants
                            .iter_mut()
                            .find(|participant| participant.target_id == resolved_target_id)
                    })
                {
                    sync_participant_from_manager_with_vitals(participant, manager);
                }
            }
        }
        true
    }

    fn advance_participant(&mut self, encounter_id: &str, target_id: &str, resume: bool) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        let Some(participant) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == target_id)
        else {
            return false;
        };

        if resume {
            participant.hp = participant.max_hp;
            participant.mp = participant.max_mp;
            participant.alive = true;
            participant.hope_avatar_rounds_remaining = 0;
        } else if participant.alive {
            if !encounter.active {
                participant.hp = (participant.hp + participant.hp_regen).min(participant.max_hp);
            }
            participant.mp = (participant.mp + participant.mp_regen).min(participant.max_mp);
        }
        let previous_damage_taken = participant.damage_taken_this_turn;
        reset_participant_turn_totals(participant);
        participant.undying_rage_active = false;
        advance_participant_overhealing_shield(participant);
        let mut delayed_logs = Vec::new();
        let (hope_log, hope_outcome) = advance_participant_hope_avatar(participant);
        let mut defeat_outcomes = hope_outcome.into_iter().collect::<Vec<_>>();
        participant.inspiration_sources.retain(|_, turns| {
            *turns = turns.saturating_sub(1);
            *turns > 0
        });
        if let Some(log) = hope_log {
            delayed_logs.push(log);
        } else {
            if let Some(log) =
                apply_participant_liquid_body_healing(participant, previous_damage_taken)
            {
                delayed_logs.push(log);
            }
            if participant.wound_healing_taken_turns > 0 {
                participant.wound_healing_taken_turns -= 1;
            }
            let delayed = advance_participant_delayed_damage_ticks(participant, encounter.active);
            delayed_logs.extend(delayed.logs);
            defeat_outcomes.extend(delayed.defeat_outcomes);
            delayed_logs.extend(advance_participant_delayed_healing_ticks(participant));
        }
        participant.turn += 1;
        participant.pending_negative = false;
        encounter.round = encounter
            .participants
            .iter()
            .filter(|participant| participant.alive)
            .map(|participant| participant.turn)
            .min()
            .unwrap_or_default();

        if encounter.negative_enabled {
            mark_negative_candidates(encounter);
        }
        for outcome in defeat_outcomes {
            apply_battle_defeat_outcome(encounter, outcome);
        }
        encounter.action_log.extend(delayed_logs);
        true
    }

    fn skip_negative_participant(&mut self, encounter_id: &str, target_id: &str) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        let Some(participant) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == target_id)
        else {
            return false;
        };
        participant.negative_layers += 1;
        participant.pending_negative = false;
        let _ = participant;
        self.finish_actor_action(encounter_id, target_id)
    }
}

fn refresh_encounter_players(
    encounter: &mut BattleEncounter,
    manager: &NapcatMessageManager,
) -> bool {
    let Some(group_name) = encounter.trpg_group.as_deref() else {
        return false;
    };
    let Some(group) = manager.trpg_groups.get(group_name) else {
        return false;
    };

    let before_signature = encounter_participants_signature(&encounter.participants);
    encounter.participants.retain(|participant| {
        participant.unit_template_id.is_some() || group.players.contains(&participant.target_id)
    });
    for participant in encounter
        .participants
        .iter_mut()
        .filter(|participant| participant.unit_template_id.is_some())
    {
        refresh_unit_participant_from_template(participant, manager);
    }
    for target_id in &group.players {
        if let Some(participant) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == *target_id)
        {
            sync_participant_from_manager(participant, manager);
        } else {
            let mut participant = participant_from_target(target_id, manager);
            initialize_participant_clock(
                &mut participant,
                Some(group_name),
                manager,
            );
            encounter.participants.push(participant);
        }
    }
    before_signature != encounter_participants_signature(&encounter.participants)
}

fn initialize_participant_clock(
    participant: &mut BattleParticipantSnapshot,
    group_name: Option<&str>,
    manager: &NapcatMessageManager,
) {
    let Some(character) = manager.player_characters.get(&participant.target_id) else {
        return;
    };
    let group = group_name.and_then(|name| manager.trpg_groups.get(name));
    participant.turn = group
        .and_then(|group| group.player_turns.get(&participant.target_id))
        .map(|turn| turn.turns_passed)
        .or_else(|| group.map(|group| group.world_turn))
        .unwrap_or_default();
    participant.skill_last_used_turns = character.skill_last_cast_turns.clone();
}

fn sync_encounter_to_manager(
    encounter: Option<&BattleEncounter>,
    manager: &mut NapcatMessageManager,
) -> bool {
    let Some(encounter) = encounter else {
        return false;
    };
    let mut changed = false;

    for participant in &encounter.participants {
        if participant.unit_template_id.is_some() || !participant.player_character {
            continue;
        }
        let Some(character) = manager.player_characters.get_mut(&participant.target_id) else {
            continue;
        };
        let hp = participant.hp.clamp(0.0, character.max_hp.max(0.0));
        let mp = participant.mp.clamp(0.0, character.max_mp.max(0.0));
        if (character.hp - hp).abs() > f32::EPSILON {
            character.hp = hp;
            changed = true;
        }
        if (character.mp - mp).abs() > f32::EPSILON {
            character.mp = mp;
            changed = true;
        }
        if (character.damage_taken_this_turn - participant.damage_taken_this_turn).abs()
            > f32::EPSILON
        {
            character.damage_taken_this_turn = participant.damage_taken_this_turn.max(0.0);
            changed = true;
        }
        if (character.healing_taken_this_turn - participant.healing_taken_this_turn).abs()
            > f32::EPSILON
        {
            character.healing_taken_this_turn = participant.healing_taken_this_turn.max(0.0);
            changed = true;
        }
        if character.skill_last_cast_turns != participant.skill_last_used_turns {
            character.skill_last_cast_turns = participant.skill_last_used_turns.clone();
            changed = true;
        }
    }

    let Some(group_name) = encounter.trpg_group.as_deref() else {
        return changed;
    };
    let Some(group) = manager.trpg_groups.get_mut(group_name) else {
        return changed;
    };
    changed |= group.sync_turn_players();
    if group.world_turn < encounter.round {
        group.world_turn = encounter.round;
        changed = true;
    }
    for participant in encounter
        .participants
        .iter()
        .filter(|participant| participant.player_character)
    {
        let Some(turn) = group.player_turns.get_mut(&participant.target_id) else {
            continue;
        };
        if turn.turns_passed < participant.turn {
            turn.turns_passed = participant.turn;
            changed = true;
        }
        if turn.acted != participant.action_done || turn.skipped {
            turn.acted = participant.action_done;
            turn.skipped = false;
            changed = true;
        }
    }
    changed |= group.refresh_legacy_negative_timers();
    changed
}

fn sync_battle_round_buff_advancement(
    store: &mut BattleRoundStore,
    encounter_id: &str,
    previous_round: u32,
    manager: &mut NapcatMessageManager,
    rule_engine_state: &mut RuleEngineState,
) -> bool {
    let Some(encounter) = store.encounters.get(encounter_id) else {
        return false;
    };
    if encounter.round <= previous_round {
        return false;
    }
    let canonical_round = encounter
        .trpg_group
        .as_deref()
        .and_then(|group_name| manager.trpg_groups.get(group_name))
        .map(|group| group.world_turn)
        .unwrap_or(previous_round);
    let rounds_to_advance = encounter
        .round
        .saturating_sub(canonical_round.max(previous_round));
    let player_ids = encounter
        .participants
        .iter()
        .filter(|participant| participant.player_character)
        .map(|participant| participant.target_id.clone())
        .collect::<Vec<_>>();

    let mut changed = sync_encounter_to_manager(Some(encounter), manager);
    for _ in 0..rounds_to_advance {
        changed |= advance_buffs_for_players(manager, &player_ids, rule_engine_state);
        changed |= advance_unit_participant_buffs(store, encounter_id, manager);
    }
    if rounds_to_advance == 0 {
        return changed;
    }
    if let Some(encounter) = store.encounters.get_mut(encounter_id) {
        for participant in encounter
            .participants
            .iter_mut()
            .filter(|participant| participant.player_character)
        {
            sync_participant_from_manager_with_vitals(participant, manager);
        }
    }
    true
}

#[derive(Clone)]
struct BattleBuffTick {
    source_id: String,
    target_id: String,
    action: BuffTickAction,
}

fn advance_unit_participant_buffs(
    store: &mut BattleRoundStore,
    encounter_id: &str,
    manager: &NapcatMessageManager,
) -> bool {
    let Some(encounter) = store.encounters.get_mut(encounter_id) else {
        return false;
    };
    let mut changed = false;
    let mut ticks = Vec::new();
    for participant in encounter
        .participants
        .iter_mut()
        .filter(|participant| participant.unit_template_id.is_some())
    {
        let Some(mut character) = character_for_participant(participant, manager) else {
            continue;
        };
        let before = character.active_buffs.clone();
        character.active_buffs.retain_mut(|buff| {
            if buff.turns_remaining == 0 {
                return true;
            }
            if buff.turns_remaining < 0 {
                return false;
            }
            buff.turns_remaining -= 1;
            if buff.turns_remaining <= 0 {
                return false;
            }
            ticks.extend(
                buff.tick_actions
                    .iter()
                    .cloned()
                    .map(|action| BattleBuffTick {
                        source_id: buff.source_id.clone(),
                        target_id: participant.target_id.clone(),
                        action,
                    }),
            );
            true
        });
        if character.active_buffs != before {
            participant.unit_character = Some(character);
            sync_participant_from_manager_with_vitals(participant, manager);
            changed = true;
        }
    }
    if !ticks.is_empty() {
        apply_battle_buff_ticks(encounter, manager, &ticks);
        changed = true;
    }
    changed
}

fn apply_battle_buff_ticks(
    encounter: &mut BattleEncounter,
    manager: &NapcatMessageManager,
    ticks: &[BattleBuffTick],
) {
    for tick in ticks {
        let source_undying_rage_damage_multiplier = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == tick.source_id)
            .map(participant_undying_rage_damage_multiplier)
            .unwrap_or(1.0);
        let source_character = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == tick.source_id)
            .and_then(|participant| character_for_participant(participant, manager))
            .or_else(|| manager.player_characters.get(&tick.source_id).cloned());
        let source_name = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == tick.source_id)
            .map(|participant| participant.display_name.clone())
            .unwrap_or_else(|| tick.source_id.clone());
        let Some(target_index) = encounter
            .participants
            .iter()
            .position(|participant| participant.target_id == tick.target_id)
        else {
            continue;
        };
        let target_character = character_for_participant(
            &encounter.participants[target_index],
            manager,
        );
        let target_name = encounter.participants[target_index].display_name.clone();
        match tick.action {
            BuffTickAction::Damage {
                amount,
                damage_type,
            } => {
                let stat_config = manager.character_stat_config_for_target(&tick.source_id);
                let source_multiplier = source_character
                    .as_ref()
                    .map(|source| {
                        source.damage_dealt_modifier
                            * low_hp_damage_multiplier(source.hp, source.max_hp)
                            * character_damage_attribute_multiplier(
                                source,
                                &stat_config,
                                trpg_damage_bonus_kind(damage_type),
                            )
                    })
                    .unwrap_or(1.0)
                    * source_undying_rage_damage_multiplier;
                let target_multiplier = encounter.participants[target_index].damage_taken_modifier
                    * target_character
                        .as_ref()
                        .map(|target| {
                            character_damage_taken_attribute_multiplier(
                                target,
                                trpg_damage_taken_kind(damage_type),
                            )
                        })
                        .unwrap_or(1.0);
                let final_amount =
                    (amount.max(0.0) * source_multiplier * target_multiplier).max(0.0);
                let resolution = apply_participant_damage_for_battle(
                    &mut encounter.participants[target_index],
                    final_amount,
                    &tick.source_id,
                    encounter.active,
                );
                encounter.action_log.push(format!(
                    "状态触发：{}对{}造成{}点伤害",
                    source_name,
                    target_name,
                    format_number(resolution.damage_applied)
                ));
                if resolution.hope_avatar_triggered {
                    encounter.action_log.push(format!(
                        "{}触发希望化身，进入持续2回合的无敌天使形态",
                        target_name
                    ));
                } else if resolution.hope_avatar_immune {
                    encounter.action_log.push(format!(
                        "{}处于希望化身，免疫本次伤害",
                        target_name
                    ));
                } else if resolution.undying_rage_triggered {
                    encounter.action_log.push(format!(
                        "{}触发不死者之怒，免疫本次致命伤害",
                        target_name
                    ));
                } else if resolution.damage_absorbed > f32::EPSILON {
                    encounter.action_log.push(format!(
                        "{}吸收{}点伤害",
                        target_name,
                        format_number(resolution.damage_absorbed)
                    ));
                }
                if let Some(outcome) = resolution.defeat_outcome {
                    apply_battle_defeat_outcome(encounter, outcome);
                }
            },
            BuffTickAction::FixedDamage { amount, .. } => {
                let final_amount = amount.max(0.0);
                let resolution = apply_participant_damage_for_battle(
                    &mut encounter.participants[target_index],
                    final_amount,
                    &tick.source_id,
                    encounter.active,
                );
                encounter.action_log.push(format!(
                    "状态触发：{}对{}造成{}点固定伤害",
                    source_name,
                    target_name,
                    format_number(resolution.damage_applied)
                ));
                if resolution.hope_avatar_triggered {
                    encounter.action_log.push(format!(
                        "{}触发希望化身，进入持续2回合的无敌天使形态",
                        target_name
                    ));
                } else if resolution.hope_avatar_immune {
                    encounter.action_log.push(format!(
                        "{}处于希望化身，免疫本次伤害",
                        target_name
                    ));
                } else if resolution.undying_rage_triggered {
                    encounter.action_log.push(format!(
                        "{}触发不死者之怒，免疫本次致命伤害",
                        target_name
                    ));
                } else if resolution.damage_absorbed > f32::EPSILON {
                    encounter.action_log.push(format!(
                        "{}吸收{}点伤害",
                        target_name,
                        format_number(resolution.damage_absorbed)
                    ));
                }
                if let Some(outcome) = resolution.defeat_outcome {
                    apply_battle_defeat_outcome(encounter, outcome);
                }
            },
            BuffTickAction::Heal { amount } => {
                let stat_config = manager.character_stat_config_for_target(&tick.source_id);
                let source_multiplier = source_character
                    .as_ref()
                    .map(|source| {
                        source.healing_dealt_modifier
                            * character_healing_attribute_multiplier(source, &stat_config)
                            * wounded_healing_dealt_multiplier(
                                source.hp,
                                source.max_hp,
                                character_wounded_healing_dealt_modifier(source),
                            )
                    })
                    .unwrap_or(1.0);
                let target_multiplier = encounter.participants[target_index].healing_taken_modifier
                    * participant_wound_healing_multiplier(&encounter.participants[target_index])
                    * target_character
                        .as_ref()
                        .map(|target| {
                            dying_healing_taken_multiplier(
                                target.hp,
                                target.max_hp,
                                character_dying_healing_taken_modifier(target),
                            )
                        })
                        .unwrap_or(1.0);
                let final_amount =
                    (amount.max(0.0) * source_multiplier * target_multiplier).max(0.0);
                let source_overhealing_shield_cap_rate = source_character
                    .as_ref()
                    .map(character_overhealing_shield_cap_rate)
                    .unwrap_or(0.0);
                let mutual_aid_healing =
                    if tick.source_id != tick.target_id && final_amount > f32::EPSILON {
                        final_amount
                            * (source_character
                                .as_ref()
                                .map(character_mutual_aid_healing_rate)
                                .unwrap_or(0.0)
                                + target_character
                                    .as_ref()
                                    .map(character_mutual_aid_healing_rate)
                                    .unwrap_or(0.0))
                    } else {
                        0.0
                    };
                let target = &mut encounter.participants[target_index];
                apply_participant_healing_for_battle(
                    target,
                    final_amount,
                    source_overhealing_shield_cap_rate,
                );
                encounter.action_log.push(format!(
                    "状态触发：{}为{}回复{}点生命值",
                    source_name,
                    target_name,
                    format_number(final_amount)
                ));
                if mutual_aid_healing > f32::EPSILON {
                    if let Some(source) = encounter
                        .participants
                        .iter_mut()
                        .find(|participant| participant.target_id == tick.source_id)
                    {
                        let shield_cap_rate = source.overhealing_shield_cap_rate;
                        apply_participant_healing_for_battle(
                            source,
                            mutual_aid_healing,
                            shield_cap_rate,
                        );
                        encounter.action_log.push(format!(
                            "{}触发互帮互助，回复{}点生命值",
                            source_name,
                            format_number(mutual_aid_healing)
                        ));
                    }
                }
            },
        }
    }
}

fn encounter_participants_signature(participants: &[BattleParticipantSnapshot]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for participant in participants {
        participant.target_id.hash(&mut hasher);
        participant.display_name.hash(&mut hasher);
        participant.unit_template_id.hash(&mut hasher);
        if let Some(character) = participant.unit_character.as_ref() {
            if let Ok(active_buffs) = serde_json::to_string(&character.active_buffs) {
                active_buffs.hash(&mut hasher);
            }
        }
        participant.player_character.hash(&mut hasher);
        participant.str_.hash(&mut hasher);
        participant.agi.hash(&mut hasher);
        participant.dex.hash(&mut hasher);
        participant.int_.hash(&mut hasher);
        participant.wis.hash(&mut hasher);
        participant.action_done.hash(&mut hasher);
        participant.alive.hash(&mut hasher);
        participant.hp.to_bits().hash(&mut hasher);
        participant.max_hp.to_bits().hash(&mut hasher);
        participant.mp.to_bits().hash(&mut hasher);
        participant.max_mp.to_bits().hash(&mut hasher);
        participant.hp_regen.to_bits().hash(&mut hasher);
        participant.mp_regen.to_bits().hash(&mut hasher);
        participant.speed.to_bits().hash(&mut hasher);
        participant.low_survivor_speed.to_bits().hash(&mut hasher);
        participant
            .damage_dealt_modifier
            .to_bits()
            .hash(&mut hasher);
        participant
            .damage_taken_modifier
            .to_bits()
            .hash(&mut hasher);
        participant
            .healing_dealt_modifier
            .to_bits()
            .hash(&mut hasher);
        participant
            .healing_taken_modifier
            .to_bits()
            .hash(&mut hasher);
        participant
            .arrogance_damage_bonus_per_source
            .to_bits()
            .hash(&mut hasher);
        for source_id in &participant.arrogance_damage_source_ids {
            source_id.hash(&mut hasher);
        }
        participant
            .endless_pain_bonus_damage_per_stack
            .to_bits()
            .hash(&mut hasher);
        participant.endless_pain_stacks.hash(&mut hasher);
        participant
            .infinite_focus_damage_bonus_per_stack
            .to_bits()
            .hash(&mut hasher);
        participant.infinite_focus_target_id.hash(&mut hasher);
        participant.infinite_focus_stacks.hash(&mut hasher);
        participant
            .one_heart_healing_bonus_per_stack
            .to_bits()
            .hash(&mut hasher);
        participant.one_heart_target_id.hash(&mut hasher);
        participant.one_heart_stacks.hash(&mut hasher);
        participant.inspiration_enabled.hash(&mut hasher);
        participant.inspiration_target_id.hash(&mut hasher);
        let mut inspiration_sources = participant.inspiration_sources.iter().collect::<Vec<_>>();
        inspiration_sources.sort_by(|left, right| left.0.cmp(right.0));
        for (source_id, turns) in inspiration_sources {
            source_id.hash(&mut hasher);
            turns.hash(&mut hasher);
        }
        participant.keen_evasion_enabled.hash(&mut hasher);
        participant.keen_evasion_available.hash(&mut hasher);
        participant.arcane_shield.to_bits().hash(&mut hasher);
        participant
            .overhealing_shield_cap_rate
            .to_bits()
            .hash(&mut hasher);
        participant.overhealing_shield.to_bits().hash(&mut hasher);
        participant
            .overhealing_shield_turns_remaining
            .hash(&mut hasher);
        participant.undying_rage_enabled.hash(&mut hasher);
        participant.undying_rage_used.hash(&mut hasher);
        participant.undying_rage_active.hash(&mut hasher);
        participant.hope_avatar_enabled.hash(&mut hasher);
        participant.hope_avatar_used.hash(&mut hasher);
        participant.hope_avatar_rounds_remaining.hash(&mut hasher);
        participant
            .liquid_body_damage_delay_rate
            .to_bits()
            .hash(&mut hasher);
        participant
            .liquid_body_self_healing_rate
            .to_bits()
            .hash(&mut hasher);
        participant
            .calm_heart_healing_rate
            .to_bits()
            .hash(&mut hasher);
        participant
            .combat_damage_taken_total
            .to_bits()
            .hash(&mut hasher);
        participant
            .champion_damage_bonus_per_stack
            .to_bits()
            .hash(&mut hasher);
        participant
            .champion_damage_reduction_per_stack
            .to_bits()
            .hash(&mut hasher);
        participant.champion_stacks.hash(&mut hasher);
        participant
            .dominion_max_hp_gain_rate
            .to_bits()
            .hash(&mut hasher);
        participant
            .dominion_max_hp_bonus_cap
            .to_bits()
            .hash(&mut hasher);
        participant
            .dominion_max_hp_bonus
            .to_bits()
            .hash(&mut hasher);
        participant
            .sin_on_sin_exp_bonus_per_stack
            .to_bits()
            .hash(&mut hasher);
        participant
            .sin_on_sin_recovery_rate
            .to_bits()
            .hash(&mut hasher);
        participant.sin_on_sin_stacks.hash(&mut hasher);
        participant
            .penance_healing_bonus_percent
            .to_bits()
            .hash(&mut hasher);
        participant.penance_kill_assist_count.hash(&mut hasher);
        for contributor in &participant.damage_contributors {
            contributor.hash(&mut hasher);
        }
        participant.wound_healing_taken_turns.hash(&mut hasher);
        for tick in &participant.delayed_damage_ticks {
            tick.name.hash(&mut hasher);
            tick.source_id.hash(&mut hasher);
            tick.source_name.hash(&mut hasher);
            tick.amount.to_bits().hash(&mut hasher);
            tick.damage_type.hash(&mut hasher);
            tick.turns_remaining.hash(&mut hasher);
        }
        for tick in &participant.delayed_healing_ticks {
            tick.name.hash(&mut hasher);
            tick.source_id.hash(&mut hasher);
            tick.source_name.hash(&mut hasher);
            tick.amount.to_bits().hash(&mut hasher);
            tick.overhealing_shield_cap_rate.to_bits().hash(&mut hasher);
            tick.turns_remaining.hash(&mut hasher);
        }
        participant
            .damage_taken_this_turn
            .to_bits()
            .hash(&mut hasher);
        participant
            .healing_taken_this_turn
            .to_bits()
            .hash(&mut hasher);
    }
    hasher.finish()
}

fn character_battle_speeds(character: &PlayerCharacter) -> (f32, f32) {
    character_gale_force_battle_speeds(character).unwrap_or_else(|| {
        let speed = character.speed.max(0.0);
        (speed, speed)
    })
}

fn participant_from_character(
    target_id: &str,
    character: &PlayerCharacter,
    manager: &NapcatMessageManager,
) -> BattleParticipantSnapshot {
    let status = character.status.combined(&character.extra_status);
    let (speed, low_survivor_speed) = character_battle_speeds(character);
    BattleParticipantSnapshot {
        target_id: target_id.to_owned(),
        display_name: character_display_name(target_id, character, manager),
        unit_template_id: None,
        unit_character: None,
        player_character: true,
        turn: 0,
        str_: status.str_,
        agi: status.agi,
        dex: status.dex,
        int_: status.int_,
        wis: status.wis,
        action_done: false,
        alive: character.hp > 0.0,
        negative_layers: 0,
        pending_negative: false,
        hp: character.hp,
        max_hp: character.max_hp,
        mp: character.mp,
        max_mp: character.max_mp,
        hp_regen: character.hp_regen,
        mp_regen: character.mp_regen,
        speed,
        low_survivor_speed,
        damage_dealt_modifier: character.damage_dealt_modifier,
        damage_taken_modifier: character.damage_taken_modifier,
        healing_dealt_modifier: character.healing_dealt_modifier,
        healing_taken_modifier: character.healing_taken_modifier,
        arrogance_damage_bonus_per_source: character_arrogance_damage_bonus_per_source(character),
        arrogance_damage_source_ids: Vec::new(),
        endless_pain_bonus_damage_per_stack: character_endless_pain_bonus_damage_per_stack(
            character,
        ),
        endless_pain_stacks: 0,
        infinite_focus_damage_bonus_per_stack: character_infinite_focus_damage_bonus_per_stack(
            character,
        ),
        infinite_focus_target_id: None,
        infinite_focus_stacks: 0,
        one_heart_healing_bonus_per_stack: character_one_heart_healing_bonus_per_stack(character),
        one_heart_target_id: None,
        one_heart_stacks: 0,
        inspiration_enabled: character_inspiration_available(character),
        inspiration_target_id: None,
        inspiration_sources: HashMap::new(),
        keen_evasion_enabled: character_keen_evasion_available(character),
        keen_evasion_available: character_keen_evasion_available(character),
        arcane_shield: character_arcane_shield_amount(character),
        overhealing_shield_cap_rate: character_overhealing_shield_cap_rate(character),
        overhealing_shield: 0.0,
        overhealing_shield_turns_remaining: 0,
        undying_rage_enabled: character_undying_rage_available(character),
        undying_rage_used: false,
        undying_rage_active: false,
        hope_avatar_enabled: character_hope_avatar_available(character),
        hope_avatar_used: false,
        hope_avatar_rounds_remaining: 0,
        liquid_body_damage_delay_rate: character_liquid_body_damage_delay_rate(character),
        liquid_body_self_healing_rate: character_liquid_body_self_healing_rate(character),
        calm_heart_healing_rate: character_calm_heart_healing_rate(character),
        combat_damage_taken_total: 0.0,
        champion_damage_bonus_per_stack: character_champion_damage_bonus_per_stack(character),
        champion_damage_reduction_per_stack: character_champion_damage_reduction_per_stack(
            character,
        ),
        champion_stacks: 0,
        dominion_max_hp_gain_rate: character_dominion_max_hp_gain_rate(character),
        dominion_max_hp_bonus_cap: character_dominion_max_hp_bonus_cap(character),
        dominion_max_hp_bonus: 0.0,
        sin_on_sin_exp_bonus_per_stack: character_sin_on_sin_exp_bonus_per_stack(character),
        sin_on_sin_recovery_rate: character_sin_on_sin_recovery_rate(character),
        sin_on_sin_stacks: 0,
        penance_healing_bonus_percent: character_penance_healing_bonus_percent(character),
        penance_kill_assist_count: 0,
        damage_contributors: Vec::new(),
        wound_healing_taken_turns: 0,
        delayed_damage_ticks: Vec::new(),
        delayed_healing_ticks: Vec::new(),
        damage_taken_this_turn: character.damage_taken_this_turn,
        healing_taken_this_turn: character.healing_taken_this_turn,
        skill_last_used_turns: HashMap::new(),
    }
}

fn participant_from_unit_template(
    target_id: &str,
    unit_id: &str,
    unit: &UnitPoolEntry,
) -> BattleParticipantSnapshot {
    let character = &unit.character;
    let status = character.status.combined(&character.extra_status);
    let (speed, low_survivor_speed) = character_battle_speeds(character);
    BattleParticipantSnapshot {
        target_id: target_id.to_owned(),
        display_name: unit_participant_display_name(target_id, unit_id, unit),
        unit_template_id: Some(unit_id.to_owned()),
        unit_character: Some(character.clone()),
        player_character: false,
        turn: 0,
        str_: status.str_,
        agi: status.agi,
        dex: status.dex,
        int_: status.int_,
        wis: status.wis,
        action_done: false,
        alive: character.hp > 0.0,
        negative_layers: 0,
        pending_negative: false,
        hp: character.hp,
        max_hp: character.max_hp,
        mp: character.mp,
        max_mp: character.max_mp,
        hp_regen: character.hp_regen,
        mp_regen: character.mp_regen,
        speed,
        low_survivor_speed,
        damage_dealt_modifier: character.damage_dealt_modifier,
        damage_taken_modifier: character.damage_taken_modifier,
        healing_dealt_modifier: character.healing_dealt_modifier,
        healing_taken_modifier: character.healing_taken_modifier,
        arrogance_damage_bonus_per_source: character_arrogance_damage_bonus_per_source(character),
        arrogance_damage_source_ids: Vec::new(),
        endless_pain_bonus_damage_per_stack: character_endless_pain_bonus_damage_per_stack(
            character,
        ),
        endless_pain_stacks: 0,
        infinite_focus_damage_bonus_per_stack: character_infinite_focus_damage_bonus_per_stack(
            character,
        ),
        infinite_focus_target_id: None,
        infinite_focus_stacks: 0,
        one_heart_healing_bonus_per_stack: character_one_heart_healing_bonus_per_stack(character),
        one_heart_target_id: None,
        one_heart_stacks: 0,
        inspiration_enabled: character_inspiration_available(character),
        inspiration_target_id: None,
        inspiration_sources: HashMap::new(),
        keen_evasion_enabled: character_keen_evasion_available(character),
        keen_evasion_available: character_keen_evasion_available(character),
        arcane_shield: character_arcane_shield_amount(character),
        overhealing_shield_cap_rate: character_overhealing_shield_cap_rate(character),
        overhealing_shield: 0.0,
        overhealing_shield_turns_remaining: 0,
        undying_rage_enabled: character_undying_rage_available(character),
        undying_rage_used: false,
        undying_rage_active: false,
        hope_avatar_enabled: character_hope_avatar_available(character),
        hope_avatar_used: false,
        hope_avatar_rounds_remaining: 0,
        liquid_body_damage_delay_rate: character_liquid_body_damage_delay_rate(character),
        liquid_body_self_healing_rate: character_liquid_body_self_healing_rate(character),
        calm_heart_healing_rate: character_calm_heart_healing_rate(character),
        combat_damage_taken_total: 0.0,
        champion_damage_bonus_per_stack: character_champion_damage_bonus_per_stack(character),
        champion_damage_reduction_per_stack: character_champion_damage_reduction_per_stack(
            character,
        ),
        champion_stacks: 0,
        dominion_max_hp_gain_rate: character_dominion_max_hp_gain_rate(character),
        dominion_max_hp_bonus_cap: character_dominion_max_hp_bonus_cap(character),
        dominion_max_hp_bonus: 0.0,
        sin_on_sin_exp_bonus_per_stack: character_sin_on_sin_exp_bonus_per_stack(character),
        sin_on_sin_recovery_rate: character_sin_on_sin_recovery_rate(character),
        sin_on_sin_stacks: 0,
        penance_healing_bonus_percent: character_penance_healing_bonus_percent(character),
        penance_kill_assist_count: 0,
        damage_contributors: Vec::new(),
        wound_healing_taken_turns: 0,
        delayed_damage_ticks: Vec::new(),
        delayed_healing_ticks: Vec::new(),
        damage_taken_this_turn: character.damage_taken_this_turn,
        healing_taken_this_turn: character.healing_taken_this_turn,
        skill_last_used_turns: HashMap::new(),
    }
}

fn participant_from_target(
    target_id: &str,
    manager: &NapcatMessageManager,
) -> BattleParticipantSnapshot {
    if let Some(character) = manager.player_characters.get(target_id) {
        return participant_from_character(target_id, character, manager);
    }

    BattleParticipantSnapshot {
        target_id: target_id.to_owned(),
        display_name: fallback_target_display_name(target_id, manager),
        unit_template_id: None,
        unit_character: None,
        player_character: false,
        turn: 0,
        str_: 0,
        agi: 0,
        dex: 0,
        int_: 0,
        wis: 0,
        action_done: false,
        alive: true,
        negative_layers: 0,
        pending_negative: false,
        hp: 1.0,
        max_hp: 1.0,
        mp: 0.0,
        max_mp: 0.0,
        hp_regen: 0.0,
        mp_regen: 0.0,
        speed: 0.0,
        low_survivor_speed: 0.0,
        damage_dealt_modifier: 1.0,
        damage_taken_modifier: 1.0,
        healing_dealt_modifier: 1.0,
        healing_taken_modifier: 1.0,
        arrogance_damage_bonus_per_source: 0.0,
        arrogance_damage_source_ids: Vec::new(),
        endless_pain_bonus_damage_per_stack: 0.0,
        endless_pain_stacks: 0,
        infinite_focus_damage_bonus_per_stack: 0.0,
        infinite_focus_target_id: None,
        infinite_focus_stacks: 0,
        one_heart_healing_bonus_per_stack: 0.0,
        one_heart_target_id: None,
        one_heart_stacks: 0,
        inspiration_enabled: false,
        inspiration_target_id: None,
        inspiration_sources: HashMap::new(),
        keen_evasion_enabled: false,
        keen_evasion_available: false,
        arcane_shield: 0.0,
        overhealing_shield_cap_rate: 0.0,
        overhealing_shield: 0.0,
        overhealing_shield_turns_remaining: 0,
        undying_rage_enabled: false,
        undying_rage_used: false,
        undying_rage_active: false,
        hope_avatar_enabled: false,
        hope_avatar_used: false,
        hope_avatar_rounds_remaining: 0,
        liquid_body_damage_delay_rate: 0.0,
        liquid_body_self_healing_rate: 0.0,
        calm_heart_healing_rate: 0.0,
        combat_damage_taken_total: 0.0,
        champion_damage_bonus_per_stack: 0.0,
        champion_damage_reduction_per_stack: 0.0,
        champion_stacks: 0,
        dominion_max_hp_gain_rate: 0.0,
        dominion_max_hp_bonus_cap: 0.0,
        dominion_max_hp_bonus: 0.0,
        sin_on_sin_exp_bonus_per_stack: 0.0,
        sin_on_sin_recovery_rate: 0.0,
        sin_on_sin_stacks: 0,
        penance_healing_bonus_percent: 0.0,
        penance_kill_assist_count: 0,
        damage_contributors: Vec::new(),
        wound_healing_taken_turns: 0,
        delayed_damage_ticks: Vec::new(),
        delayed_healing_ticks: Vec::new(),
        damage_taken_this_turn: 0.0,
        healing_taken_this_turn: 0.0,
        skill_last_used_turns: HashMap::new(),
    }
}

fn sync_participant_from_manager(
    participant: &mut BattleParticipantSnapshot,
    manager: &NapcatMessageManager,
) {
    if let Some(unit_id) = participant.unit_template_id.as_deref() {
        if let Some(unit) = manager.unit_pool.get(unit_id) {
            let mut character = character_for_participant(participant, manager)
                .unwrap_or_else(|| unit.character.clone());
            let stat_config = manager.character_stat_config_for_target(&participant.target_id);
            let mut rule_engine_state = RuleEngineState::default();
            sync_character_buffs(
                &participant.target_id,
                &mut character,
                &stat_config,
                &mut rule_engine_state,
                &manager.skill_pool,
            );
            let status = character.status.combined(&character.extra_status);
            let (speed, low_survivor_speed) = character_battle_speeds(&character);
            participant.display_name =
                unit_participant_display_name(&participant.target_id, unit_id, unit);
            participant.player_character = false;
            participant.max_hp = character.max_hp;
            participant.max_mp = character.max_mp;
            participant.hp_regen = character.hp_regen;
            participant.mp_regen = character.mp_regen;
            participant.speed = speed;
            participant.low_survivor_speed = low_survivor_speed;
            participant.str_ = status.str_;
            participant.agi = status.agi;
            participant.dex = status.dex;
            participant.int_ = status.int_;
            participant.wis = status.wis;
            participant.damage_dealt_modifier = character.damage_dealt_modifier;
            participant.damage_taken_modifier = character.damage_taken_modifier;
            participant.healing_dealt_modifier = character.healing_dealt_modifier;
            participant.healing_taken_modifier = character.healing_taken_modifier;
            participant.arrogance_damage_bonus_per_source =
                character_arrogance_damage_bonus_per_source(&character);
            participant.endless_pain_bonus_damage_per_stack =
                character_endless_pain_bonus_damage_per_stack(&character);
            participant.infinite_focus_damage_bonus_per_stack =
                character_infinite_focus_damage_bonus_per_stack(&character);
            participant.one_heart_healing_bonus_per_stack =
                character_one_heart_healing_bonus_per_stack(&character);
            participant.inspiration_enabled = character_inspiration_available(&character);
            sync_participant_keen_evasion(
                participant,
                character_keen_evasion_available(&character),
            );
            participant.overhealing_shield_cap_rate =
                character_overhealing_shield_cap_rate(&character);
            sync_participant_undying_rage(
                participant,
                character_undying_rage_available(&character),
            );
            participant.hope_avatar_enabled = character_hope_avatar_available(&character);
            participant.liquid_body_damage_delay_rate =
                character_liquid_body_damage_delay_rate(&character);
            participant.liquid_body_self_healing_rate =
                character_liquid_body_self_healing_rate(&character);
            participant.calm_heart_healing_rate = character_calm_heart_healing_rate(&character);
            participant.champion_damage_bonus_per_stack =
                character_champion_damage_bonus_per_stack(&character);
            participant.champion_damage_reduction_per_stack =
                character_champion_damage_reduction_per_stack(&character);
            let dominion_gain_rate = character_dominion_max_hp_gain_rate(&character);
            let dominion_bonus_cap = character_dominion_max_hp_bonus_cap(&character);
            participant.dominion_max_hp_gain_rate = dominion_gain_rate;
            participant.dominion_max_hp_bonus_cap = dominion_bonus_cap;
            participant.dominion_max_hp_bonus = if dominion_gain_rate > f32::EPSILON {
                participant
                    .dominion_max_hp_bonus
                    .clamp(0.0, dominion_bonus_cap)
            } else {
                0.0
            };
            participant.max_hp = character.max_hp + participant.dominion_max_hp_bonus;
            participant.sin_on_sin_exp_bonus_per_stack =
                character_sin_on_sin_exp_bonus_per_stack(&character);
            participant.sin_on_sin_recovery_rate = character_sin_on_sin_recovery_rate(&character);
            participant.penance_healing_bonus_percent =
                character_penance_healing_bonus_percent(&character);
            participant.hp = character.hp.clamp(0.0, participant.max_hp.max(0.0));
            participant.mp = character.mp.clamp(0.0, participant.max_mp.max(0.0));
            participant.alive = participant.hp > 0.0 || participant_hope_avatar_active(participant);
            character.hp = participant.hp;
            character.mp = participant.mp;
            character.damage_taken_this_turn = participant.damage_taken_this_turn;
            character.healing_taken_this_turn = participant.healing_taken_this_turn;
            character.skill_last_cast_turns = participant.skill_last_used_turns.clone();
            participant.unit_character = Some(character);
        }
        return;
    }

    if let Some(character) = manager.player_characters.get(&participant.target_id) {
        let status = character.status.combined(&character.extra_status);
        let (speed, low_survivor_speed) = character_battle_speeds(character);
        participant.display_name = character_display_name(
            &participant.target_id,
            character,
            manager,
        );
        participant.player_character = true;
        participant.max_hp = character.max_hp;
        participant.max_mp = character.max_mp;
        participant.hp_regen = character.hp_regen;
        participant.mp_regen = character.mp_regen;
        participant.speed = speed;
        participant.low_survivor_speed = low_survivor_speed;
        participant.str_ = status.str_;
        participant.agi = status.agi;
        participant.dex = status.dex;
        participant.int_ = status.int_;
        participant.wis = status.wis;
        participant.damage_dealt_modifier = character.damage_dealt_modifier;
        participant.damage_taken_modifier = character.damage_taken_modifier;
        participant.healing_dealt_modifier = character.healing_dealt_modifier;
        participant.healing_taken_modifier = character.healing_taken_modifier;
        participant.arrogance_damage_bonus_per_source =
            character_arrogance_damage_bonus_per_source(character);
        participant.endless_pain_bonus_damage_per_stack =
            character_endless_pain_bonus_damage_per_stack(character);
        participant.infinite_focus_damage_bonus_per_stack =
            character_infinite_focus_damage_bonus_per_stack(character);
        participant.one_heart_healing_bonus_per_stack =
            character_one_heart_healing_bonus_per_stack(character);
        participant.inspiration_enabled = character_inspiration_available(character);
        sync_participant_keen_evasion(
            participant,
            character_keen_evasion_available(character),
        );
        participant.overhealing_shield_cap_rate = character_overhealing_shield_cap_rate(character);
        sync_participant_undying_rage(
            participant,
            character_undying_rage_available(character),
        );
        participant.hope_avatar_enabled = character_hope_avatar_available(character);
        participant.liquid_body_damage_delay_rate =
            character_liquid_body_damage_delay_rate(character);
        participant.liquid_body_self_healing_rate =
            character_liquid_body_self_healing_rate(character);
        participant.calm_heart_healing_rate = character_calm_heart_healing_rate(character);
        participant.champion_damage_bonus_per_stack =
            character_champion_damage_bonus_per_stack(character);
        participant.champion_damage_reduction_per_stack =
            character_champion_damage_reduction_per_stack(character);
        let dominion_gain_rate = character_dominion_max_hp_gain_rate(character);
        let dominion_bonus_cap = character_dominion_max_hp_bonus_cap(character);
        participant.dominion_max_hp_gain_rate = dominion_gain_rate;
        participant.dominion_max_hp_bonus_cap = dominion_bonus_cap;
        participant.dominion_max_hp_bonus = if dominion_gain_rate > f32::EPSILON {
            participant
                .dominion_max_hp_bonus
                .clamp(0.0, dominion_bonus_cap)
        } else {
            0.0
        };
        participant.max_hp = character.max_hp + participant.dominion_max_hp_bonus;
        participant.sin_on_sin_exp_bonus_per_stack =
            character_sin_on_sin_exp_bonus_per_stack(character);
        participant.sin_on_sin_recovery_rate = character_sin_on_sin_recovery_rate(character);
        participant.penance_healing_bonus_percent =
            character_penance_healing_bonus_percent(character);
        participant.hp = participant.hp.min(participant.max_hp);
        participant.mp = participant.mp.min(participant.max_mp);
        participant.alive = participant.hp > 0.0 || participant_hope_avatar_active(participant);
    } else {
        participant.player_character = false;
        participant.low_survivor_speed = participant.speed.max(0.0);
        participant.arrogance_damage_bonus_per_source = 0.0;
        participant.endless_pain_bonus_damage_per_stack = 0.0;
        participant.infinite_focus_damage_bonus_per_stack = 0.0;
        participant.one_heart_healing_bonus_per_stack = 0.0;
        participant.inspiration_enabled = false;
        sync_participant_keen_evasion(participant, false);
        participant.overhealing_shield_cap_rate = 0.0;
        sync_participant_undying_rage(participant, false);
        participant.hope_avatar_enabled = false;
        participant.liquid_body_damage_delay_rate = 0.0;
        participant.liquid_body_self_healing_rate = 0.0;
        participant.calm_heart_healing_rate = 0.0;
        participant.champion_damage_bonus_per_stack = 0.0;
        participant.champion_damage_reduction_per_stack = 0.0;
        participant.dominion_max_hp_gain_rate = 0.0;
        participant.dominion_max_hp_bonus_cap = 0.0;
        participant.dominion_max_hp_bonus = 0.0;
        participant.sin_on_sin_exp_bonus_per_stack = 0.0;
        participant.sin_on_sin_recovery_rate = 0.0;
        participant.penance_healing_bonus_percent = 0.0;
        participant.display_name = participant_display_name(&participant.target_id, manager);
    }
}

fn sync_participant_from_manager_with_vitals(
    participant: &mut BattleParticipantSnapshot,
    manager: &NapcatMessageManager,
) {
    sync_participant_from_manager(participant, manager);
    let vitals = if participant.unit_template_id.is_some() {
        participant
            .unit_character
            .as_ref()
            .map(|character| (character.hp, character.mp))
    } else {
        manager
            .player_characters
            .get(&participant.target_id)
            .map(|character| (character.hp, character.mp))
    };
    if let Some((hp, mp)) = vitals {
        participant.hp = hp.clamp(0.0, participant.max_hp.max(0.0));
        participant.mp = mp.clamp(0.0, participant.max_mp.max(0.0));
        participant.alive = participant.hp > 0.0 || participant_hope_avatar_active(participant);
    }
}

fn refresh_unit_participant_from_template(
    participant: &mut BattleParticipantSnapshot,
    manager: &NapcatMessageManager,
) {
    let Some(unit_id) = participant.unit_template_id.as_deref() else {
        return;
    };
    let Some(unit) = manager.unit_pool.get(unit_id) else {
        return;
    };
    let current = character_for_participant(participant, manager);
    let mut refreshed = unit.character.clone();
    if let Some(current) = current {
        refreshed.active_buffs = current.active_buffs;
        refreshed.hp = current
            .buff_base_stats
            .as_ref()
            .map(|base| base.hp)
            .unwrap_or(participant.hp);
        refreshed.mp = current
            .buff_base_stats
            .as_ref()
            .map(|base| base.mp)
            .unwrap_or(participant.mp);
        refreshed.damage_taken_this_turn = participant.damage_taken_this_turn;
        refreshed.healing_taken_this_turn = participant.healing_taken_this_turn;
        refreshed.skill_last_cast_turns = participant.skill_last_used_turns.clone();
    }
    refreshed.buff_base_stats = None;
    participant.unit_character = Some(refreshed);
    sync_participant_from_manager(participant, manager);
}

fn living_player_participant_count(encounter: &BattleEncounter) -> usize {
    encounter
        .participants
        .iter()
        .filter(|participant| participant.player_character && participant.alive)
        .count()
}

fn participant_order_speed(
    participant: &BattleParticipantSnapshot,
    living_player_count: usize,
) -> f32 {
    let speed = participant.speed.max(0.0);
    let base_speed = if living_player_count > 0
        && living_player_count <= 3
        && participant.low_survivor_speed > speed
    {
        participant.low_survivor_speed
    } else {
        speed
    };
    base_speed * participant_inspiration_multiplier(participant)
}

fn ordered_participant_indices(encounter: &BattleEncounter) -> Vec<usize> {
    let mut indices = (0..encounter.participants.len()).collect::<Vec<_>>();
    if encounter.sort_by_turn {
        let living_player_count = living_player_participant_count(encounter);
        indices.sort_by(|left, right| {
            let left_participant = &encounter.participants[*left];
            let right_participant = &encounter.participants[*right];
            participant_order_speed(right_participant, living_player_count)
                .total_cmp(&participant_order_speed(
                    left_participant,
                    living_player_count,
                ))
                .then_with(|| right_participant.agi.cmp(&left_participant.agi))
                .then_with(|| {
                    left_participant
                        .action_done
                        .cmp(&right_participant.action_done)
                })
                .then_with(|| {
                    left_participant
                        .display_name
                        .cmp(&right_participant.display_name)
                })
        });
    } else {
        indices.sort_by(|left, right| {
            encounter.participants[*left]
                .display_name
                .cmp(&encounter.participants[*right].display_name)
        });
    }
    indices
}

fn current_actor_index(encounter: &BattleEncounter) -> Option<usize> {
    ordered_participant_indices(encounter)
        .into_iter()
        .find(|index| {
            let participant = &encounter.participants[*index];
            participant.alive && !participant.action_done
        })
}

fn normalize_encounter_after_edit(encounter: &mut BattleEncounter) {
    for participant in &mut encounter.participants {
        participant.max_hp = participant.max_hp.max(0.0);
        participant.hp = participant.hp.clamp(0.0, participant.max_hp);
        participant.max_mp = participant.max_mp.max(0.0);
        participant.mp = participant.mp.clamp(0.0, participant.max_mp);
        participant.damage_taken_this_turn = participant.damage_taken_this_turn.max(0.0);
        participant.healing_taken_this_turn = participant.healing_taken_this_turn.max(0.0);
        if participant.hp <= 0.0 && !participant_hope_avatar_active(participant) {
            participant.alive = false;
        }
    }
}

fn available_group_players(
    encounter: &BattleEncounter,
    manager: &NapcatMessageManager,
) -> Vec<(String, String)> {
    let existing = encounter
        .participants
        .iter()
        .map(|participant| participant.target_id.as_str())
        .collect::<HashSet<_>>();
    let mut candidate_ids = HashSet::new();

    if let Some(group_name) = encounter.trpg_group.as_deref() {
        if let Some(group) = manager.trpg_groups.get(group_name) {
            candidate_ids.extend(group.players.iter().cloned());
        }
    }
    candidate_ids.extend(manager.player_characters.keys().cloned());
    candidate_ids.extend(manager.chat_targets.keys().cloned());

    let mut candidates = candidate_ids
        .into_iter()
        .filter(|target_id| !existing.contains(target_id.as_str()))
        .map(|target_id| {
            let display_name = participant_display_name(&target_id, manager);
            (target_id, display_name)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    candidates
}

fn available_unit_templates(manager: &NapcatMessageManager) -> Vec<(String, String)> {
    let mut candidates = manager
        .unit_pool
        .iter()
        .map(|(unit_id, unit)| {
            (
                unit_id.clone(),
                unit_template_name(unit_id, unit),
            )
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    candidates
}

fn next_unit_participant_id(encounter: &BattleEncounter, unit_id: &str) -> String {
    let base = format!("unit:{unit_id}");
    if !encounter
        .participants
        .iter()
        .any(|participant| participant.target_id == base)
    {
        return base;
    }

    for index in 2.. {
        let candidate = format!("{base}#{index}");
        if !encounter
            .participants
            .iter()
            .any(|participant| participant.target_id == candidate)
        {
            return candidate;
        }
    }
    unreachable!("unbounded unit participant id search should always return")
}

fn character_for_participant(
    participant: &BattleParticipantSnapshot,
    manager: &NapcatMessageManager,
) -> Option<PlayerCharacter> {
    let mut character = if let Some(unit_id) = participant.unit_template_id.as_deref() {
        participant.unit_character.clone().or_else(|| {
            manager
                .unit_pool
                .get(unit_id)
                .map(|unit| unit.character.clone())
        })?
    } else {
        manager
            .player_characters
            .get(&participant.target_id)?
            .clone()
    };
    if let Some(base_stats) = character.buff_base_stats.as_mut() {
        base_stats.hp = (base_stats.hp + participant.hp - character.hp).max(0.0);
        base_stats.mp = (base_stats.mp + participant.mp - character.mp).max(0.0);
    }
    character.hp = participant.hp;
    character.mp = participant.mp;
    character.damage_taken_this_turn = participant.damage_taken_this_turn;
    character.healing_taken_this_turn = participant.healing_taken_this_turn;
    character.skill_last_cast_turns = participant.skill_last_used_turns.clone();
    if participant.unit_template_id.is_none() {
        character.max_hp = participant.max_hp;
        character.max_mp = participant.max_mp;
        character.hp_regen = participant.hp_regen;
        character.mp_regen = participant.mp_regen;
        character.speed = participant.speed;
        character.damage_dealt_modifier = participant.damage_dealt_modifier;
        character.damage_taken_modifier = participant.damage_taken_modifier;
        character.healing_dealt_modifier = participant.healing_dealt_modifier;
        character.healing_taken_modifier = participant.healing_taken_modifier;
        character.status.str_ = participant.str_;
        character.status.agi = participant.agi;
        character.status.dex = participant.dex;
        character.status.int_ = participant.int_;
        character.status.wis = participant.wis;
        character.extra_status.str_ = 0;
        character.extra_status.agi = 0;
        character.extra_status.dex = 0;
        character.extra_status.int_ = 0;
        character.extra_status.wis = 0;
    }
    Some(character)
}

fn character_skills(character: &PlayerCharacter) -> Vec<CharacterSkill> {
    character
        .skill_names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let display_name = if name.trim().is_empty() {
                format!("技能{}", index + 1)
            } else {
                name.trim().to_owned()
            };
            CharacterSkill {
                index,
                name: display_name,
                note: character
                    .skill_notes
                    .get(index)
                    .cloned()
                    .unwrap_or_default(),
                skill_type: character
                    .skill_metadata
                    .get(index)
                    .and_then(|metadata| metadata.skill_type.clone()),
                legacy_buff_machine_json: character
                    .skill_metadata
                    .get(index)
                    .and_then(|metadata| metadata.legacy_buff_machine_json.clone()),
                mp_cost: character
                    .skill_mp_costs
                    .get(index)
                    .copied()
                    .unwrap_or_default()
                    .max(0.0),
                cooldown_turns: character
                    .skill_cooldown_turns
                    .get(index)
                    .copied()
                    .unwrap_or_default(),
                cooldown_left: character
                    .skill_metadata
                    .get(index)
                    .and_then(|metadata| metadata.cooldown_left),
                target_count: character
                    .skill_metadata
                    .get(index)
                    .and_then(|metadata| metadata.target_count),
                target_class: character
                    .skill_metadata
                    .get(index)
                    .and_then(|metadata| metadata.target_class.clone()),
                range: character
                    .skill_metadata
                    .get(index)
                    .and_then(|metadata| metadata.range),
                arg_values: character
                    .skill_metadata
                    .get(index)
                    .map(|metadata| skill_rule_args(&metadata.args))
                    .unwrap_or_default(),
            }
        })
        .collect()
}

fn skill_cooldown_remaining(
    participant: &BattleParticipantSnapshot,
    skill_index: usize,
    cooldown_turns: u32,
    cooldown_left: Option<u32>,
) -> u32 {
    let Some(last_used_turn) = participant
        .skill_last_used_turns
        .get(&skill_index.to_string())
    else {
        return cooldown_left.unwrap_or_default();
    };
    cooldown_turns.saturating_sub(participant.turn.saturating_sub(*last_used_turn))
}

fn display_name_for_target(options: &[(String, String)], target_id: &str) -> String {
    options
        .iter()
        .find(|(id, _)| id == target_id)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| target_id.to_owned())
}

fn encounter_basic_config(
    encounter: &BattleEncounter,
    manager: &NapcatMessageManager,
    actor_id: &str,
) -> TrpgBasicConfig {
    encounter
        .trpg_group
        .as_deref()
        .and_then(|group_name| manager.trpg_groups.get(group_name))
        .map(|group| group.basic_config)
        .unwrap_or_else(|| manager.character_stat_config_for_target(actor_id))
}

fn participant_status(participant: &BattleParticipantSnapshot) -> CharacterStatus {
    CharacterStatus {
        str_: participant.str_,
        agi: participant.agi,
        dex: participant.dex,
        int_: participant.int_,
        wis: participant.wis,
        ..Default::default()
    }
}

fn participant_damage_multiplier(
    participant: &BattleParticipantSnapshot,
    character: Option<&PlayerCharacter>,
    config: &TrpgBasicConfig,
    completed_turns: u32,
    damage_type: DamageType,
) -> f32 {
    let status = participant_status(participant);
    let bonus_kind = trpg_damage_bonus_kind(damage_type);
    let talent_bonus = character
        .map(|character| {
            let typed_bonus =
                character_moonberry_talent_damage_attribute_bonus(character, &status, bonus_kind);
            if bonus_kind == TrpgDamageBonusKind::Range {
                typed_bonus
                    + character_range_magic_converter_damage_bonus(character, &status, config)
            } else {
                typed_bonus
            }
        })
        .unwrap_or_default();
    participant.damage_dealt_modifier
        * participant_inspiration_multiplier(participant)
        * participant_undying_rage_damage_multiplier(participant)
        * arrogance_damage_dealt_multiplier(
            participant.arrogance_damage_bonus_per_source,
            participant.arrogance_damage_source_ids.len() as u32,
        )
        * champion_damage_dealt_multiplier(
            participant.champion_damage_bonus_per_stack,
            participant.champion_stacks,
        )
        * low_hp_damage_multiplier(participant.hp, participant.max_hp)
        * (status_damage_attribute_multiplier(&status, config, bonus_kind) + talent_bonus)
        * character
            .map(character_chaos_output_variance)
            .map(moonberry_chaos_output_multiplier)
            .unwrap_or(1.0)
        * character
            .map(|character| {
                character_valorous_battle_damage_multiplier(character, completed_turns)
            })
            .unwrap_or(1.0)
}

fn participant_healing_multiplier(
    participant: &BattleParticipantSnapshot,
    character: Option<&PlayerCharacter>,
    config: &TrpgBasicConfig,
) -> f32 {
    let wounded_modifier = character
        .map(character_wounded_healing_dealt_modifier)
        .unwrap_or(1.0);
    penance_decayed_healing_dealt_modifier(
        participant.healing_dealt_modifier,
        participant.penance_healing_bonus_percent,
        participant.penance_kill_assist_count,
    ) * status_healing_attribute_multiplier(&participant_status(participant), config)
        * wounded_healing_dealt_multiplier(
            participant.hp,
            participant.max_hp,
            wounded_modifier,
        )
        * character
            .map(character_chaos_output_variance)
            .map(moonberry_chaos_output_multiplier)
            .unwrap_or(1.0)
}

fn participant_wound_healing_multiplier(participant: &BattleParticipantSnapshot) -> f32 {
    if participant.wound_healing_taken_turns > 0 {
        0.75
    } else {
        1.0
    }
}

fn battle_damage_type_label(damage_type: DamageType) -> &'static str {
    match damage_type {
        DamageType::Cursed => "诅咒",
        DamageType::Diseased => "疾病",
        DamageType::Bleed => "流血",
        DamageType::Range => "远程",
        DamageType::Poisoning => "中毒",
        DamageType::Physical => "物理",
        DamageType::Magical => "魔法",
        DamageType::None => "无类型",
    }
}

fn trpg_damage_bonus_kind(damage_type: DamageType) -> TrpgDamageBonusKind {
    match damage_type {
        DamageType::Magical => TrpgDamageBonusKind::Magical,
        DamageType::Physical => TrpgDamageBonusKind::Physical,
        DamageType::Range => TrpgDamageBonusKind::Range,
        DamageType::Cursed
        | DamageType::Diseased
        | DamageType::Bleed
        | DamageType::Poisoning
        | DamageType::None => TrpgDamageBonusKind::Other,
    }
}

fn trpg_damage_taken_kind(damage_type: DamageType) -> TrpgDamageTakenKind {
    match damage_type {
        DamageType::Magical => TrpgDamageTakenKind::Magical,
        DamageType::Diseased => TrpgDamageTakenKind::Diseased,
        DamageType::Poisoning => TrpgDamageTakenKind::Poisoning,
        DamageType::Physical
        | DamageType::Range
        | DamageType::Cursed
        | DamageType::Bleed
        | DamageType::None => TrpgDamageTakenKind::Other,
    }
}

enum SkillEffect {
    Damage {
        amount: f32,
        target: TargetSelector,
        damage_type: DamageType,
    },
    Heal {
        amount: f32,
        target: TargetSelector,
    },
    GrantBuff {
        target: TargetSelector,
        buff: RuleBuffTemplate,
    },
}

fn static_skill_effects(
    note: &str,
    arg_values: &SkillRuleArgs,
    skill_type: Option<&str>,
    legacy_buff_machine_json: Option<&str>,
) -> Vec<SkillEffect> {
    let Some(ast) = parse_rule_with_named_args(
        note,
        &arg_values.numeric_values,
        &arg_values.text_values,
    )
    .ok()
    .map(|ast| apply_skill_type_damage_default(ast, skill_type))
    .or_else(|| {
        legacy_buff_machine_json.and_then(|json| {
            legacy_moonberry_buff_machine_skill_cast_rule(
                json,
                &arg_values.numeric_values,
                skill_type,
            )
        })
    }) else {
        return Vec::new();
    };
    ast.actions
        .into_iter()
        .filter_map(|action| match action {
            Action::Damage {
                target,
                amount: ValueExpr::Number(amount),
                damage_type,
            } => Some(SkillEffect::Damage {
                amount: amount.max(0.0),
                target,
                damage_type,
            }),
            Action::Heal {
                target,
                amount: ValueExpr::Number(amount),
                ..
            } => Some(SkillEffect::Heal {
                amount: amount.max(0.0),
                target,
            }),
            Action::GrantBuff { target, buff } => Some(SkillEffect::GrantBuff { target, buff }),
            _ => None,
        })
        .collect()
}

fn resolve_skill_targets(
    target: TargetSelector,
    actor_id: &str,
    selected_target_id: &str,
    encounter: &BattleEncounter,
    scene_positions: Option<&SceneCharacterPositions>,
    fallback_radius: Option<f32>,
    target_class: Option<&str>,
) -> Vec<String> {
    let force_area =
        skill_target_class_is_area(target_class) && !matches!(target.actor, ActorRef::SelfActor);
    if target.area.is_some() || force_area {
        let radius = target
            .area
            .and_then(|area| area.radius_meters)
            .or(fallback_radius);
        let Some(radius) = radius else {
            return encounter
                .participants
                .iter()
                .filter(|participant| participant.alive && participant.target_id != actor_id)
                .map(|participant| participant.target_id.clone())
                .collect();
        };
        let Some(positions) = scene_positions else {
            return Vec::new();
        };
        let Some(actor_position) = positions.positions.get(actor_id) else {
            return Vec::new();
        };
        return encounter
            .participants
            .iter()
            .filter(|participant| participant.alive && participant.target_id != actor_id)
            .filter(|participant| {
                positions
                    .positions
                    .get(&participant.target_id)
                    .map(|position| actor_position.distance(*position) <= radius)
                    .unwrap_or(false)
            })
            .map(|participant| participant.target_id.clone())
            .collect();
    }

    let targets = match target.actor {
        ActorRef::SelfActor => vec![actor_id.to_owned()],
        ActorRef::Source | ActorRef::Target => vec![selected_target_id.to_owned()],
    };
    if matches!(target.actor, ActorRef::SelfActor) {
        targets
    } else {
        filter_battle_targets_by_range(
            actor_id,
            targets,
            scene_positions,
            fallback_radius,
        )
    }
}

fn character_display_name(
    target_id: &str,
    character: &PlayerCharacter,
    manager: &NapcatMessageManager,
) -> String {
    if !character.nickname.trim().is_empty() {
        return character.nickname.trim().to_owned();
    }
    if !character.name.trim().is_empty() {
        return character.name.trim().to_owned();
    }
    fallback_target_display_name(target_id, manager)
}

fn unit_template_name(unit_id: &str, unit: &UnitPoolEntry) -> String {
    if !unit.label.trim().is_empty() {
        return unit.label.trim().to_owned();
    }
    if !unit.character.nickname.trim().is_empty() {
        return unit.character.nickname.trim().to_owned();
    }
    if !unit.character.name.trim().is_empty() {
        return unit.character.name.trim().to_owned();
    }
    unit_id.to_owned()
}

fn unit_participant_display_name(target_id: &str, unit_id: &str, unit: &UnitPoolEntry) -> String {
    let base = unit_template_name(unit_id, unit);
    if let Some((_, suffix)) = target_id.rsplit_once('#') {
        if suffix
            .parse::<usize>()
            .ok()
            .filter(|index| *index > 1)
            .is_some()
        {
            return format!("{base} {suffix}");
        }
    }
    base
}

fn fallback_target_display_name(target_id: &str, manager: &NapcatMessageManager) -> String {
    manager
        .chat_targets
        .get(target_id)
        .map(|metadata| metadata.display_name.trim())
        .filter(|name| !name.is_empty())
        .or_else(|| message_sender_nickname(target_id, manager))
        .unwrap_or(target_id)
        .to_owned()
}

fn participant_display_name(target_id: &str, manager: &NapcatMessageManager) -> String {
    manager
        .player_characters
        .get(target_id)
        .map(|character| character_display_name(target_id, character, manager))
        .unwrap_or_else(|| fallback_target_display_name(target_id, manager))
}

fn participant_snapshot_display_name(
    participant: &BattleParticipantSnapshot,
    manager: &NapcatMessageManager,
) -> String {
    if let Some(unit_id) = participant.unit_template_id.as_deref() {
        if let Some(unit) = manager.unit_pool.get(unit_id) {
            return unit_participant_display_name(&participant.target_id, unit_id, unit);
        }
    }

    participant_display_name(&participant.target_id, manager)
}

fn message_sender_nickname<'a>(
    target_id: &str,
    manager: &'a NapcatMessageManager,
) -> Option<&'a str> {
    let mut nickname = None;
    for message in manager
        .messages
        .values()
        .flat_map(|messages| messages.iter())
    {
        if message.data.sender.user_id.to_string() == target_id
            && !message.data.sender.nickname.trim().is_empty()
        {
            nickname = Some(message.data.sender.nickname.trim());
        }
    }
    nickname
}

fn mark_negative_candidates(encounter: &mut BattleEncounter) {
    for participant in &mut encounter.participants {
        participant.pending_negative = false;
    }

    let alive_count = encounter
        .participants
        .iter()
        .filter(|participant| participant.alive)
        .count();
    if alive_count < 2 {
        return;
    }

    let min_turn = encounter
        .participants
        .iter()
        .filter(|participant| participant.alive)
        .map(|participant| participant.turn)
        .min()
        .unwrap_or_default();
    let lagging_count = encounter
        .participants
        .iter()
        .filter(|participant| participant.alive && participant.turn == min_turn)
        .count();
    let advanced_count = alive_count - lagging_count;
    let half = alive_count.div_ceil(2);
    if advanced_count < half {
        return;
    }

    for participant in &mut encounter.participants {
        if participant.alive && participant.turn == min_turn {
            participant.pending_negative = true;
        }
    }
}

fn limit_skill_targets(mut targets: Vec<String>, target_count: Option<u32>) -> Vec<String> {
    if let Some(target_count) = target_count {
        targets.truncate(target_count as usize);
    }
    targets
}

fn infinite_focus_eligible_target_id(
    target: TargetSelector,
    actor_id: &str,
    target_ids: &[String],
    target_class: Option<&str>,
) -> Option<String> {
    if target.area.is_some()
        || matches!(target.actor, ActorRef::SelfActor)
        || skill_target_class_is_area(target_class)
    {
        return None;
    }
    if matches!(
        target_class.map(str::trim),
        Some("无目标" | "多目标" | "范围")
    ) {
        return None;
    }
    let [target_id] = target_ids else {
        return None;
    };
    (target_id != actor_id).then(|| target_id.clone())
}

fn one_heart_eligible_target_id(
    target: TargetSelector,
    target_ids: &[String],
    target_class: Option<&str>,
) -> Option<String> {
    if target.area.is_some() || skill_target_class_is_area(target_class) {
        return None;
    }
    if matches!(
        target_class.map(str::trim),
        Some("无目标" | "多目标" | "范围")
    ) {
        return None;
    }
    let [target_id] = target_ids else {
        return None;
    };
    Some(target_id.clone())
}

fn skill_target_limit(target_count: Option<u32>, target_class: Option<&str>) -> Option<u32> {
    match target_class.map(str::trim) {
        Some("无目标") => Some(0),
        Some("单目标") => Some(target_count.unwrap_or(1).min(1)),
        _ => target_count,
    }
}

fn skill_target_class_is_area(target_class: Option<&str>) -> bool {
    matches!(
        target_class.map(str::trim),
        Some("范围")
    )
}

fn skill_range_radius(range: Option<i32>) -> Option<f32> {
    range.filter(|range| *range > 0).map(|range| range as f32)
}

fn battle_skill_damage_range_radius(
    skill_range: Option<i32>,
    actor_character: Option<&PlayerCharacter>,
    damage_type: DamageType,
    skill_type: Option<&str>,
) -> Option<f32> {
    let minimum_range = if damage_type == DamageType::Range {
        actor_character
            .map(character_minimum_range_meters)
            .unwrap_or(0.0)
    } else {
        0.0
    };
    let range_multiplier = if moonberry_skill_type_is_spell(skill_type) {
        actor_character
            .map(character_spell_range_multiplier)
            .unwrap_or(1.0)
    } else {
        1.0
    };
    moonberry_effective_skill_range_radius_with_multiplier(
        skill_range,
        minimum_range,
        range_multiplier,
    )
}

fn filter_battle_targets_by_range(
    actor_id: &str,
    targets: Vec<String>,
    scene_positions: Option<&SceneCharacterPositions>,
    radius: Option<f32>,
) -> Vec<String> {
    let Some(radius) = radius else {
        return targets;
    };
    let Some(positions) = scene_positions else {
        return Vec::new();
    };
    let Some(actor_position) = positions.positions.get(actor_id) else {
        return Vec::new();
    };
    targets
        .into_iter()
        .filter(|target_id| {
            positions
                .positions
                .get(target_id)
                .map(|position| actor_position.distance(*position) <= radius)
                .unwrap_or(false)
        })
        .collect()
}

fn format_number(value: f32) -> String {
    if value.fract().abs() < f32::EPSILON {
        format!("{}", value as i32)
    } else {
        format!("{value:.1}")
    }
}

#[cfg(test)]
mod area_tests {
    use super::*;
    use crate::rule_engine::AreaSelector;

    #[test]
    fn area_skill_targets_use_scene_character_positions() {
        let encounter = BattleEncounter {
            participants: vec![
                battle_participant("actor"),
                battle_participant("near"),
                battle_participant("far"),
            ],
            ..default()
        };
        let positions = SceneCharacterPositions {
            positions: HashMap::from([
                ("actor".to_owned(), Vec3::ZERO),
                (
                    "near".to_owned(),
                    Vec3::new(2.9, 0.0, 0.0),
                ),
                (
                    "far".to_owned(),
                    Vec3::new(3.1, 0.0, 0.0),
                ),
            ]),
        };

        let targets = resolve_skill_targets(
            TargetSelector {
                actor: ActorRef::Target,
                area: Some(AreaSelector {
                    radius_meters: Some(3.0),
                }),
            },
            "actor",
            "far",
            &encounter,
            Some(&positions),
            None,
            None,
        );

        assert_eq!(targets, vec!["near".to_owned()]);
    }

    #[test]
    fn range_target_class_expands_single_target_rule() {
        let encounter = BattleEncounter {
            participants: vec![
                battle_participant("actor"),
                battle_participant("near"),
                battle_participant("far"),
            ],
            ..default()
        };
        let positions = SceneCharacterPositions {
            positions: HashMap::from([
                ("actor".to_owned(), Vec3::ZERO),
                (
                    "near".to_owned(),
                    Vec3::new(2.9, 0.0, 0.0),
                ),
                (
                    "far".to_owned(),
                    Vec3::new(3.1, 0.0, 0.0),
                ),
            ]),
        };

        let targets = resolve_skill_targets(
            TargetSelector {
                actor: ActorRef::Target,
                area: None,
            },
            "actor",
            "far",
            &encounter,
            Some(&positions),
            Some(3.0),
            Some("范围"),
        );

        assert_eq!(targets, vec!["near".to_owned()]);
    }

    fn battle_participant(target_id: &str) -> BattleParticipantSnapshot {
        BattleParticipantSnapshot {
            target_id: target_id.to_owned(),
            display_name: target_id.to_owned(),
            unit_template_id: None,
            unit_character: None,
            player_character: false,
            turn: 0,
            str_: 0,
            agi: 0,
            dex: 0,
            int_: 0,
            wis: 0,
            action_done: false,
            alive: true,
            negative_layers: 0,
            pending_negative: false,
            hp: 10.0,
            max_hp: 10.0,
            mp: 0.0,
            max_mp: 0.0,
            hp_regen: 0.0,
            mp_regen: 0.0,
            speed: 0.0,
            low_survivor_speed: 0.0,
            damage_dealt_modifier: 1.0,
            damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            healing_taken_modifier: 1.0,
            arrogance_damage_bonus_per_source: 0.0,
            arrogance_damage_source_ids: Vec::new(),
            endless_pain_bonus_damage_per_stack: 0.0,
            endless_pain_stacks: 0,
            infinite_focus_damage_bonus_per_stack: 0.0,
            infinite_focus_target_id: None,
            infinite_focus_stacks: 0,
            one_heart_healing_bonus_per_stack: 0.0,
            one_heart_target_id: None,
            one_heart_stacks: 0,
            inspiration_enabled: false,
            inspiration_target_id: None,
            inspiration_sources: HashMap::new(),
            keen_evasion_enabled: false,
            keen_evasion_available: false,
            arcane_shield: 0.0,
            overhealing_shield_cap_rate: 0.0,
            overhealing_shield: 0.0,
            overhealing_shield_turns_remaining: 0,
            undying_rage_enabled: false,
            undying_rage_used: false,
            undying_rage_active: false,
            hope_avatar_enabled: false,
            hope_avatar_used: false,
            hope_avatar_rounds_remaining: 0,
            liquid_body_damage_delay_rate: 0.0,
            liquid_body_self_healing_rate: 0.0,
            calm_heart_healing_rate: 0.0,
            combat_damage_taken_total: 0.0,
            champion_damage_bonus_per_stack: 0.0,
            champion_damage_reduction_per_stack: 0.0,
            champion_stacks: 0,
            dominion_max_hp_gain_rate: 0.0,
            dominion_max_hp_bonus_cap: 0.0,
            dominion_max_hp_bonus: 0.0,
            sin_on_sin_exp_bonus_per_stack: 0.0,
            sin_on_sin_recovery_rate: 0.0,
            sin_on_sin_stacks: 0,
            penance_healing_bonus_percent: 0.0,
            penance_kill_assist_count: 0,
            damage_contributors: Vec::new(),
            wound_healing_taken_turns: 0,
            delayed_damage_ticks: Vec::new(),
            delayed_healing_ticks: Vec::new(),
            damage_taken_this_turn: 0.0,
            healing_taken_this_turn: 0.0,
            skill_last_used_turns: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_manager() -> NapcatMessageManager {
        NapcatMessageManager {
            messages: HashMap::default(),
            chat_targets: HashMap::default(),
            player_characters: HashMap::default(),
            trpg_groups: HashMap::default(),
            current_trpg_group: None,
            groups: HashMap::default(),
            read_message_counts: HashMap::default(),
            summarized_message_counts: HashMap::default(),
            open_chat_targets: HashSet::default(),
            pending_chat_targets: HashSet::default(),
            rejected_chat_targets: HashSet::default(),
            random_pools: HashMap::default(),
            skill_pool: Vec::new(),
            unit_pool: HashMap::default(),
        }
    }

    fn participant(id: &str, turn: u32) -> BattleParticipantSnapshot {
        BattleParticipantSnapshot {
            target_id: id.to_owned(),
            display_name: id.to_owned(),
            unit_template_id: None,
            unit_character: None,
            player_character: false,
            turn,
            str_: 0,
            agi: 0,
            dex: 0,
            int_: 0,
            wis: 0,
            action_done: false,
            alive: true,
            negative_layers: 0,
            pending_negative: false,
            hp: 10.0,
            max_hp: 10.0,
            mp: 0.0,
            max_mp: 10.0,
            hp_regen: 1.0,
            mp_regen: 1.0,
            speed: 0.0,
            low_survivor_speed: 0.0,
            damage_dealt_modifier: 1.0,
            damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            healing_taken_modifier: 1.0,
            arrogance_damage_bonus_per_source: 0.0,
            arrogance_damage_source_ids: Vec::new(),
            endless_pain_bonus_damage_per_stack: 0.0,
            endless_pain_stacks: 0,
            infinite_focus_damage_bonus_per_stack: 0.0,
            infinite_focus_target_id: None,
            infinite_focus_stacks: 0,
            one_heart_healing_bonus_per_stack: 0.0,
            one_heart_target_id: None,
            one_heart_stacks: 0,
            inspiration_enabled: false,
            inspiration_target_id: None,
            inspiration_sources: HashMap::new(),
            keen_evasion_enabled: false,
            keen_evasion_available: false,
            arcane_shield: 0.0,
            overhealing_shield_cap_rate: 0.0,
            overhealing_shield: 0.0,
            overhealing_shield_turns_remaining: 0,
            undying_rage_enabled: false,
            undying_rage_used: false,
            undying_rage_active: false,
            hope_avatar_enabled: false,
            hope_avatar_used: false,
            hope_avatar_rounds_remaining: 0,
            liquid_body_damage_delay_rate: 0.0,
            liquid_body_self_healing_rate: 0.0,
            calm_heart_healing_rate: 0.0,
            combat_damage_taken_total: 0.0,
            champion_damage_bonus_per_stack: 0.0,
            champion_damage_reduction_per_stack: 0.0,
            champion_stacks: 0,
            dominion_max_hp_gain_rate: 0.0,
            dominion_max_hp_bonus_cap: 0.0,
            dominion_max_hp_bonus: 0.0,
            sin_on_sin_exp_bonus_per_stack: 0.0,
            sin_on_sin_recovery_rate: 0.0,
            sin_on_sin_stacks: 0,
            penance_healing_bonus_percent: 0.0,
            penance_kill_assist_count: 0,
            damage_contributors: Vec::new(),
            wound_healing_taken_turns: 0,
            delayed_damage_ticks: Vec::new(),
            delayed_healing_ticks: Vec::new(),
            damage_taken_this_turn: 0.0,
            healing_taken_this_turn: 0.0,
            skill_last_used_turns: HashMap::new(),
        }
    }

    #[test]
    fn new_battle_inherits_group_and_character_turn_clocks() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("a".to_owned(), PlayerCharacter {
                skill_last_cast_turns: HashMap::from([("0".to_owned(), 5)]),
                ..Default::default()
            });
        let group = TrpgGroup {
            players: vec!["a".to_owned()],
            world_turn: 7,
            player_turns: HashMap::from([(
                "a".to_owned(),
                crate::napcat::TrpgPlayerTurnState {
                    turns_passed: 6,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        manager
            .trpg_groups
            .insert("party".to_owned(), group.clone());
        let mut store = BattleRoundStore::default();

        let encounter_id = store.create_encounter_from_group(
            "test".to_owned(),
            "party".to_owned(),
            &group,
            &manager,
        );

        let encounter = &store.encounters[&encounter_id];
        assert_eq!(encounter.round, 7);
        assert_eq!(encounter.participants[0].turn, 6);
        assert_eq!(
            encounter.participants[0].skill_last_used_turns,
            HashMap::from([("0".to_owned(), 5)])
        );
    }

    #[test]
    fn battle_changes_sync_to_character_and_group_turn_state() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("a".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 8.0,
                max_mp: 8.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["a".to_owned()],
            world_turn: 3,
            player_turns: HashMap::from([(
                "a".to_owned(),
                crate::napcat::TrpgPlayerTurnState {
                    turns_passed: 3,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        let mut player = participant("a", 4);
        player.player_character = true;
        player.action_done = true;
        player.hp = 6.0;
        player.mp = 2.0;
        player.damage_taken_this_turn = 4.0;
        player.healing_taken_this_turn = 1.0;
        player.skill_last_used_turns = HashMap::from([("0".to_owned(), 4)]);
        let encounter = BattleEncounter {
            trpg_group: Some("party".to_owned()),
            round: 4,
            participants: vec![player],
            ..Default::default()
        };

        assert!(sync_encounter_to_manager(
            Some(&encounter),
            &mut manager
        ));

        let character = &manager.player_characters["a"];
        assert_eq!(character.hp, 6.0);
        assert_eq!(character.mp, 2.0);
        assert_eq!(character.damage_taken_this_turn, 4.0);
        assert_eq!(character.healing_taken_this_turn, 1.0);
        assert_eq!(
            character.skill_last_cast_turns,
            HashMap::from([("0".to_owned(), 4)])
        );
        let group = &manager.trpg_groups["party"];
        assert_eq!(group.world_turn, 4);
        assert_eq!(group.player_turns["a"].turns_passed, 4);
        assert!(group.player_turns["a"].acted);
        assert!(!group.player_turns["a"].skipped);
    }

    #[test]
    fn battle_order_uses_gale_force_low_survivor_speed_when_player_count_drops() {
        let mut manager = empty_manager();
        let gale = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            speed: 10.0,
            status: crate::napcat::CharacterStatus {
                agi: 99,
                ..Default::default()
            },
            skill_names: vec!["狂风恶浪".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        let faster_normal = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            speed: 13.0,
            ..Default::default()
        };
        let slow_player = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            speed: 1.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("gale".to_owned(), gale.clone());
        manager
            .player_characters
            .insert("fast".to_owned(), faster_normal.clone());
        manager
            .player_characters
            .insert("p3".to_owned(), slow_player.clone());
        manager
            .player_characters
            .insert("p4".to_owned(), slow_player.clone());
        let mut encounter = BattleEncounter {
            sort_by_turn: true,
            participants: vec![
                participant_from_character("gale", &gale, &manager),
                participant_from_character("fast", &faster_normal, &manager),
                participant_from_character("p3", &slow_player, &manager),
                participant_from_character("p4", &slow_player, &manager),
            ],
            ..Default::default()
        };

        let gale_participant = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "gale")
            .unwrap();
        assert_eq!(gale_participant.speed, 12.0);
        assert_eq!(
            gale_participant.low_survivor_speed,
            13.5
        );
        assert_eq!(
            ordered_participant_indices(&encounter)
                .into_iter()
                .map(|index| encounter.participants[index].target_id.as_str())
                .collect::<Vec<_>>(),
            vec!["fast", "gale", "p3", "p4"]
        );

        encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "p4")
            .unwrap()
            .alive = false;

        assert_eq!(
            living_player_participant_count(&encounter),
            3
        );
        assert_eq!(
            ordered_participant_indices(&encounter)
                .into_iter()
                .map(|index| encounter.participants[index].target_id.as_str())
                .collect::<Vec<_>>(),
            vec!["gale", "fast", "p3", "p4"]
        );
    }

    #[test]
    fn unit_template_participant_uses_template_stats_and_skills() {
        let mut manager = empty_manager();
        let unit = UnitPoolEntry {
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            character: PlayerCharacter {
                hp: 8.0,
                max_hp: 12.0,
                mp: 3.0,
                max_mp: 5.0,
                status: crate::napcat::CharacterStatus {
                    agi: 7,
                    ..Default::default()
                },
                skill_names: vec!["黏液喷吐".to_owned()],
                skill_notes: vec!["造成3点伤害".to_owned()],
                skill_mp_costs: vec![1.0],
                skill_cooldown_turns: vec![2],
                ..Default::default()
            },
        };
        manager.unit_pool.insert("slime".to_owned(), unit.clone());

        let participant = participant_from_unit_template("unit:slime#2", "slime", &unit);

        assert_eq!(
            participant.unit_template_id.as_deref(),
            Some("slime")
        );
        assert_eq!(participant.display_name, "史莱姆 2");
        assert_eq!(participant.agi, 7);
        assert_eq!(participant.hp, 8.0);
        assert_eq!(participant.max_hp, 12.0);

        let skills = character_for_participant(&participant, &manager)
            .as_ref()
            .map(|character| character_skills(character))
            .unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "黏液喷吐");
        assert_eq!(skills[0].mp_cost, 1.0);
        assert_eq!(skills[0].cooldown_turns, 2);
    }

    #[test]
    fn battle_buffs_are_isolated_per_unit_instance_and_expire() {
        let mut manager = empty_manager();
        let caster = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("caster".to_owned(), caster.clone());
        let unit = UnitPoolEntry {
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            character: PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                ..Default::default()
            },
        };
        manager.unit_pool.insert("slime".to_owned(), unit.clone());
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![
                    participant_from_character("caster", &caster, &manager),
                    participant_from_unit_template("unit:slime#1", "slime", &unit),
                    participant_from_unit_template("unit:slime#2", "slime", &unit),
                ],
                ..Default::default()
            });
        let guard = CharacterSkill {
            index: 0,
            name: "守护术".to_owned(),
            note: "主动使用给予目标2回合守护状态使承伤设为0.5".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let damage = CharacterSkill {
            index: 1,
            name: "打击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use_with_buffs(
            "battle",
            "caster",
            "unit:slime#1",
            &guard,
            &mut manager,
            None,
        ));

        assert!(manager.unit_pool["slime"].character.active_buffs.is_empty());
        assert!((manager.unit_pool["slime"].character.damage_taken_modifier - 1.0).abs() < 0.0001);
        let encounter = &store.encounters["battle"];
        let guarded = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "unit:slime#1")
            .unwrap();
        let unguarded = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "unit:slime#2")
            .unwrap();
        assert!((guarded.damage_taken_modifier - 0.5).abs() < 0.0001);
        assert_eq!(
            guarded.unit_character.as_ref().unwrap().active_buffs.len(),
            1
        );
        assert!((unguarded.damage_taken_modifier - 1.0).abs() < 0.0001);
        assert!(unguarded
            .unit_character
            .as_ref()
            .unwrap()
            .active_buffs
            .is_empty());
        let serialized = serde_json::to_string(&store).unwrap();
        let restored = serde_json::from_str::<BattleRoundStore>(&serialized).unwrap();
        let restored_guarded = restored.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "unit:slime#1")
            .unwrap();
        assert_eq!(
            restored_guarded
                .unit_character
                .as_ref()
                .unwrap()
                .active_buffs
                .len(),
            1
        );

        assert!(store.record_skill_use(
            "battle",
            "caster",
            "unit:slime#1",
            &damage,
            &manager,
            None,
        ));
        assert!(store.record_skill_use(
            "battle",
            "caster",
            "unit:slime#2",
            &damage,
            &manager,
            None,
        ));
        let encounter = &store.encounters["battle"];
        assert!(
            (encounter
                .participants
                .iter()
                .find(|participant| participant.target_id == "unit:slime#1")
                .unwrap()
                .hp
                - 15.0)
                .abs()
                < 0.0001
        );
        assert!(
            (encounter
                .participants
                .iter()
                .find(|participant| participant.target_id == "unit:slime#2")
                .unwrap()
                .hp
                - 10.0)
                .abs()
                < 0.0001
        );

        let mut rule_engine_state = RuleEngineState::default();
        assert!(store.next_round("battle"));
        assert!(sync_battle_round_buff_advancement(
            &mut store,
            "battle",
            0,
            &mut manager,
            &mut rule_engine_state,
        ));
        let guarded = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "unit:slime#1")
            .unwrap();
        assert_eq!(
            guarded.unit_character.as_ref().unwrap().active_buffs[0].turns_remaining,
            1
        );

        assert!(store.next_round("battle"));
        assert!(sync_battle_round_buff_advancement(
            &mut store,
            "battle",
            1,
            &mut manager,
            &mut rule_engine_state,
        ));
        let guarded = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "unit:slime#1")
            .unwrap();
        assert!(guarded
            .unit_character
            .as_ref()
            .unwrap()
            .active_buffs
            .is_empty());
        assert!((guarded.damage_taken_modifier - 1.0).abs() < 0.0001);
    }

    #[test]
    fn unit_instance_buff_ticks_damage_without_mutating_template() {
        let mut manager = empty_manager();
        let source = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("source".to_owned(), source.clone());
        let unit = UnitPoolEntry {
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            character: PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                ..Default::default()
            },
        };
        manager.unit_pool.insert("slime".to_owned(), unit.clone());
        let mut target = participant_from_unit_template("unit:slime", "slime", &unit);
        target
            .unit_character
            .as_mut()
            .unwrap()
            .active_buffs
            .push(BuffSpec {
                name: "灼烧".to_owned(),
                kind: BuffKind::Magic,
                priority: 0,
                turns_remaining: 2,
                source_id: "source".to_owned(),
                beneficial: false,
                effects: vec![BuffEffect {
                    field: BuffField::DamageTakenModifier,
                    value: BuffValue::Set(1.0),
                }],
                tick_actions: vec![BuffTickAction::Damage {
                    amount: 4.0,
                    damage_type: DamageType::Magical,
                }],
            });
        sync_participant_from_manager(&mut target, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![
                    participant_from_character("source", &source, &manager),
                    target,
                ],
                ..Default::default()
            });

        assert!(store.next_round("battle"));
        assert!(sync_battle_round_buff_advancement(
            &mut store,
            "battle",
            0,
            &mut manager,
            &mut RuleEngineState::default(),
        ));

        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "unit:slime")
            .unwrap();
        assert!((target.hp - 16.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 4.0).abs() < 0.0001);
        assert_eq!(
            target.unit_character.as_ref().unwrap().active_buffs[0].turns_remaining,
            1
        );
        assert!(manager.unit_pool["slime"].character.active_buffs.is_empty());
        assert_eq!(
            manager.unit_pool["slime"].character.hp,
            20.0
        );
    }

    #[test]
    fn unit_instance_hp_buff_recomputes_from_base_without_stacking() {
        let mut manager = empty_manager();
        let unit = UnitPoolEntry {
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            character: PlayerCharacter {
                hp: 10.0,
                max_hp: 20.0,
                ..Default::default()
            },
        };
        manager.unit_pool.insert("slime".to_owned(), unit.clone());
        let mut participant = participant_from_unit_template("unit:slime", "slime", &unit);
        participant
            .unit_character
            .as_mut()
            .unwrap()
            .active_buffs
            .push(BuffSpec {
                name: "生命祝福".to_owned(),
                kind: BuffKind::Magic,
                priority: 0,
                turns_remaining: 2,
                source_id: "source".to_owned(),
                beneficial: true,
                effects: vec![BuffEffect {
                    field: BuffField::Hp,
                    value: BuffValue::Add(5.0),
                }],
                tick_actions: Vec::new(),
            });

        sync_participant_from_manager(&mut participant, &manager);
        assert!((participant.hp - 15.0).abs() < 0.0001);
        sync_participant_from_manager(&mut participant, &manager);
        assert!((participant.hp - 15.0).abs() < 0.0001);

        participant.hp = 12.0;
        sync_participant_from_manager(&mut participant, &manager);
        assert!((participant.hp - 12.0).abs() < 0.0001);
        participant
            .unit_character
            .as_mut()
            .unwrap()
            .active_buffs
            .clear();
        sync_participant_from_manager(&mut participant, &manager);
        assert!((participant.hp - 7.0).abs() < 0.0001);
        assert_eq!(
            manager.unit_pool["slime"].character.hp,
            10.0
        );
    }

    #[test]
    fn refresh_encounter_players_keeps_and_syncs_unit_templates() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["pc".to_owned()],
            ..Default::default()
        });
        manager
            .player_characters
            .insert("pc".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager.unit_pool.insert("slime".to_owned(), UnitPoolEntry {
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            character: PlayerCharacter {
                hp: 4.0,
                max_hp: 6.0,
                mp: 1.0,
                max_mp: 2.0,
                status: crate::napcat::CharacterStatus {
                    agi: 2,
                    ..Default::default()
                },
                ..Default::default()
            },
        });
        let unit = manager.unit_pool["slime"].clone();
        let mut encounter = BattleEncounter {
            name: "battle".to_owned(),
            trpg_group: Some("table".to_owned()),
            participants: vec![
                participant_from_unit_template("unit:slime", "slime", &unit),
                participant("old", 0),
            ],
            ..Default::default()
        };
        manager.unit_pool.get_mut("slime").unwrap().character.max_hp = 9.0;

        assert!(refresh_encounter_players(
            &mut encounter,
            &manager
        ));

        assert!(encounter
            .participants
            .iter()
            .any(|participant| participant.target_id == "pc"));
        assert!(!encounter
            .participants
            .iter()
            .any(|participant| participant.target_id == "old"));
        let unit_participant = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "unit:slime")
            .unwrap();
        assert_eq!(
            unit_participant.unit_template_id.as_deref(),
            Some("slime")
        );
        assert_eq!(unit_participant.max_hp, 9.0);
        assert_eq!(unit_participant.agi, 2);
    }

    #[test]
    fn group_battle_defaults_apply_to_new_encounters() {
        let manager = empty_manager();
        let group = TrpgGroup {
            players: vec!["a".to_owned()],
            battle_sort_by_turn: false,
            battle_negative_enabled: true,
            ..Default::default()
        };
        let mut store = BattleRoundStore::default();

        let encounter_id = store.create_encounter_from_group(
            "战斗".to_owned(),
            "party".to_owned(),
            &group,
            &manager,
        );

        let encounter = &store.encounters[&encounter_id];
        assert!(!encounter.sort_by_turn);
        assert!(encounter.negative_enabled);
        assert_eq!(encounter.participants.len(), 1);
    }

    #[test]
    fn half_party_advance_marks_lagging_participants_negative() {
        let mut encounter = BattleEncounter {
            name: "test".to_owned(),
            negative_enabled: true,
            participants: vec![
                participant("a", 1),
                participant("b", 1),
                participant("c", 0),
                participant("d", 0),
            ],
            ..Default::default()
        };

        mark_negative_candidates(&mut encounter);

        assert!(!encounter.participants[0].pending_negative);
        assert!(!encounter.participants[1].pending_negative);
        assert!(encounter.participants[2].pending_negative);
        assert!(encounter.participants[3].pending_negative);
    }

    #[test]
    fn active_battle_turn_suppresses_hp_regen_but_keeps_mp_regen() {
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                active: true,
                participants: vec![participant("a", 0)],
                ..Default::default()
            });
        store.encounters.get_mut("battle").unwrap().participants[0].hp = 5.0;

        assert!(store.advance_participant("battle", "a", false));

        let participant = &store.encounters["battle"].participants[0];
        assert_eq!(participant.turn, 1);
        assert_eq!(participant.hp, 5.0);
        assert_eq!(participant.mp, 1.0);
    }

    #[test]
    fn battle_damage_and_heal_track_turn_totals_until_next_round() {
        let manager = empty_manager();
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 20.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), target],
                ..Default::default()
            });

        assert!(store.apply_action("battle", "a", "b", "普通攻击", 3.0));
        let heal = CharacterSkill {
            index: 0,
            name: "治疗".to_owned(),
            note: "主动使用对目标回复2点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        assert!(store.record_skill_use("battle", "a", "b", &heal, &manager, None));

        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 9.0);
        assert_eq!(target.damage_taken_this_turn, 3.0);
        assert_eq!(target.healing_taken_this_turn, 2.0);

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.damage_taken_this_turn, 0.0);
        assert_eq!(target.healing_taken_this_turn, 0.0);
    }

    #[test]
    fn parsed_battle_skill_grants_buff_to_canonical_character_and_encounter() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![
                    participant_from_character("a", &actor_character, &manager),
                    participant_from_character("b", &target_character, &manager),
                ],
                ..Default::default()
            });
        let guard = CharacterSkill {
            index: 0,
            name: "守护术".to_owned(),
            note: "主动使用给予目标2回合守护状态使承伤设为0.5".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let damage = CharacterSkill {
            index: 1,
            name: "打击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use_with_buffs(
            "battle",
            "a",
            "b",
            &guard,
            &mut manager,
            None,
        ));

        let target_character = &manager.player_characters["b"];
        assert_eq!(target_character.active_buffs.len(), 1);
        assert_eq!(
            target_character.active_buffs[0].name,
            "守护"
        );
        assert!((target_character.damage_taken_modifier - 0.5).abs() < 0.0001);
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.damage_taken_modifier - 0.5).abs() < 0.0001);

        assert!(store.record_skill_use("battle", "a", "b", &damage, &manager, None));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 15.0).abs() < 0.0001);

        let mut rule_engine_state = RuleEngineState::default();
        assert!(store.next_round("battle"));
        assert!(sync_battle_round_buff_advancement(
            &mut store,
            "battle",
            0,
            &mut manager,
            &mut rule_engine_state,
        ));
        assert_eq!(
            manager.player_characters["b"].active_buffs[0].turns_remaining,
            1
        );
        assert!((manager.player_characters["b"].damage_taken_modifier - 0.5).abs() < 0.0001);

        assert!(store.next_round("battle"));
        assert!(sync_battle_round_buff_advancement(
            &mut store,
            "battle",
            1,
            &mut manager,
            &mut rule_engine_state,
        ));
        assert!(manager.player_characters["b"].active_buffs.is_empty());
        assert!((manager.player_characters["b"].damage_taken_modifier - 1.0).abs() < 0.0001);
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.damage_taken_modifier - 1.0).abs() < 0.0001);

        let vitality = CharacterSkill {
            index: 2,
            name: "活力".to_owned(),
            note: "旧规则".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: Some(
                r#"{"buffMachine":{"技能释放":[{"name":"活力","life":2,"effect":["hp"],"from":"技能目标","benifit":true,"value":["5"]}]}}"#
                    .to_owned(),
            ),
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        assert!(store.record_skill_use_with_buffs(
            "battle",
            "a",
            "b",
            &vitality,
            &mut manager,
            None,
        ));
        assert!((manager.player_characters["b"].hp - 20.0).abs() < 0.0001);
        assert!(
            (store.encounters["battle"]
                .participants
                .iter()
                .find(|participant| participant.target_id == "b")
                .unwrap()
                .hp
                - 20.0)
                .abs()
                < 0.0001
        );
    }

    #[test]
    fn rest_advance_restores_vitals() {
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0)],
                ..Default::default()
            });
        store.encounters.get_mut("battle").unwrap().participants[0].hp = 1.0;

        assert!(store.advance_participant("battle", "a", true));

        let participant = &store.encounters["battle"].participants[0];
        assert_eq!(participant.hp, 10.0);
        assert_eq!(participant.mp, 10.0);
        assert!(participant.alive);
    }

    #[test]
    fn parsed_battle_skill_uses_group_attribute_and_combat_modifiers() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            basic_config: TrpgBasicConfig {
                str_damage_bonus: 0.25,
                agi_damage_bonus: 0.5,
                dex_damage_bonus: 0.1,
                ..Default::default()
            },
            ..Default::default()
        });
        let mut actor = participant("a", 0);
        actor.str_ = 4;
        actor.agi = 51;
        actor.dex = 3;
        actor.damage_dealt_modifier = 2.0;
        let mut target = participant("b", 0);
        target.hp = 20.0;
        target.max_hp = 20.0;
        target.damage_taken_modifier = 0.5;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                trpg_group: Some("party".to_owned()),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "旋风斩".to_owned(),
            note: "主动使用对目标造成2点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));

        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 14.4).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_magic_skill_uses_archmage_talent_bonus() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            basic_config: TrpgBasicConfig {
                int_damage_bonus: 0.1,
                ..Default::default()
            },
            ..Default::default()
        });
        let character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            status: CharacterStatus {
                int_: 10,
                ..Default::default()
            },
            skill_names: vec!["大魔法师".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), character.clone());
        let actor = participant_from_character("a", &character, &manager);
        let mut target = participant("b", 0);
        target.hp = 50.0;
        target.max_hp = 50.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                trpg_group: Some("party".to_owned()),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "奥术冲击".to_owned(),
            note: "主动使用对目标造成10点魔法伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));

        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 29.5).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_skill_applies_typed_damage_taken_talents() {
        let mut manager = empty_manager();
        let target_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            skill_names: vec!["人类基因工程".to_owned(), "抗魔体质".to_owned()],
            skill_metadata: vec![
                crate::napcat::CharacterSkillMetadata::talent("normal_talent", "天赋"),
                crate::napcat::CharacterSkillMetadata::talent("support_talent", "辅助天赋"),
            ],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let mut actor = participant("a", 0);
        actor.hp = 10.0;
        actor.max_hp = 10.0;
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let disease = CharacterSkill {
            index: 0,
            name: "病灶".to_owned(),
            note: "主动使用对目标造成10点疾病伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let magic = CharacterSkill {
            index: 1,
            name: "魔弹".to_owned(),
            note: "主动使用对目标造成10点魔法伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &disease, &manager, None,));
        assert!(store.record_skill_use("battle", "a", "b", &magic, &manager, None,));

        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 2.5).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 17.5).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_skill_applies_wound_healing_taken_debuff() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            skill_names: vec!["溃伤".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let mut target = participant("b", 0);
        target.hp = 20.0;
        target.max_hp = 20.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let damage = CharacterSkill {
            index: 0,
            name: "切割".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let heal = CharacterSkill {
            index: 1,
            name: "治疗".to_owned(),
            note: "主动使用对目标治疗4点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &damage, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.wound_healing_taken_turns, 1);

        assert!(store.record_skill_use("battle", "a", "b", &heal, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 13.0).abs() < 0.0001);

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.wound_healing_taken_turns, 0);

        assert!(store.record_skill_use("battle", "a", "b", &heal, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 17.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_physical_damage_applies_lifesteal_talent() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 9.0,
            max_hp: 10.0,
            skill_names: vec!["禅宗古训".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let mut target = participant("b", 0);
        target.hp = 20.0;
        target.max_hp = 20.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "切割".to_owned(),
            note: "主动使用对目标造成4点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!((actor.hp - 9.6).abs() < 0.0001);
        assert!((actor.healing_taken_this_turn - 0.6).abs() < 0.0001);
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 16.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 4.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_physical_damage_schedules_sousas_claw_followup() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            skill_names: vec!["苏萨斯之爪".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let mut target = participant("b", 0);
        target.hp = 20.0;
        target.max_hp = 20.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "切割".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 10.0).abs() < 0.0001);
        assert_eq!(target.delayed_damage_ticks.len(), 1);
        assert_eq!(
            target.delayed_damage_ticks[0].name,
            "苏萨斯之爪"
        );

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 6.5).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 3.5).abs() < 0.0001);
        assert_eq!(target.delayed_damage_ticks.len(), 1);

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!(target.delayed_damage_ticks.is_empty());
        assert!((target.hp - 6.5).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_skill_applies_large_hit_damage_reduction_talent() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            skill_names: vec!["过度免疫".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "重击".to_owned(),
            note: "主动使用对目标造成5点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 16.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 4.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_skill_applies_fighting_spirit_turn_damage_reduction_talent() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["斗志昂扬".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "试探攻击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        for (turn, expected_damage) in [(0, 5.0), (1, 9.0), (2, 9.8), (3, 10.0)] {
            {
                let target = store
                    .encounters
                    .get_mut("battle")
                    .unwrap()
                    .participants
                    .iter_mut()
                    .find(|participant| participant.target_id == "b")
                    .unwrap();
                target.turn = turn;
                target.hp = 100.0;
                target.damage_taken_this_turn = 0.0;
            }

            assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
            let target = store.encounters["battle"]
                .participants
                .iter()
                .find(|participant| participant.target_id == "b")
                .unwrap();
            assert!((target.damage_taken_this_turn - expected_damage).abs() < 0.0001);
            assert!((target.hp - (100.0 - expected_damage)).abs() < 0.0001);
        }
    }

    #[test]
    fn parsed_battle_skill_applies_minimum_damage_floor_talent() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            level: 4,
            skill_names: vec!["菜鸡猛啄".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            damage_taken_modifier: 0.1,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "轻击".to_owned(),
            note: "主动使用对目标造成2点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 16.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 4.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_skill_applies_valorous_turn_damage_talent() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            skill_names: vec!["越战越勇".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let mut actor = participant_from_character("a", &actor_character, &manager);
        actor.turn = 5;
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "越战斩".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 89.0).abs() < 0.0001);

        {
            let encounter = store.encounters.get_mut("battle").unwrap();
            encounter
                .participants
                .iter_mut()
                .find(|participant| participant.target_id == "a")
                .unwrap()
                .turn = 10;
            let target = encounter
                .participants
                .iter_mut()
                .find(|participant| participant.target_id == "b")
                .unwrap();
            target.hp = 100.0;
            target.damage_taken_this_turn = 0.0;
        }

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 88.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 12.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_arrogance_talent_stacks_from_unique_damage_sources() {
        let mut manager = empty_manager();
        let arrogant_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["狂妄".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager.player_characters.insert(
            "a".to_owned(),
            arrogant_character.clone(),
        );
        let arrogant = participant_from_character("a", &arrogant_character, &manager);
        let mut target = participant("target", 0);
        target.hp = 100.0;
        target.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![
                    arrogant,
                    participant("b", 0),
                    participant("c", 0),
                    participant("d", 0),
                    participant("e", 0),
                    target,
                ],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "反击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.apply_action("battle", "b", "a", "试探", 1.0));
        assert!(store.apply_action("battle", "b", "a", "试探", 1.0));
        assert!(store.apply_action("battle", "c", "a", "试探", 1.0));
        assert!(store.apply_action("battle", "d", "a", "试探", 1.0));
        assert!(store.apply_action("battle", "e", "a", "试探", 1.0));
        let arrogant = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert_eq!(
            arrogant.arrogance_damage_source_ids,
            vec!["b".to_owned(), "c".to_owned(), "d".to_owned()]
        );

        assert!(store.record_skill_use("battle", "a", "target", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        assert!((target.hp - 87.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 13.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_endless_pain_talent_stacks_and_consumes_next_hit_damage() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            level: 4,
            skill_names: vec!["无尽痛楚".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let mut target = participant("target", 0);
        target.hp = 100.0;
        target.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![
                    actor,
                    participant("b", 0),
                    participant("c", 0),
                    participant("d", 0),
                    target,
                ],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "痛楚反击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.apply_action("battle", "b", "a", "试探", 1.0));
        assert!(store.apply_action("battle", "c", "a", "试探", 1.0));
        assert!(store.apply_action("battle", "d", "a", "试探", 1.0));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert_eq!(actor.endless_pain_stacks, 2);

        assert!(store.record_skill_use("battle", "a", "target", &skill, &manager, None,));
        let encounter = &store.encounters["battle"];
        let actor = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        assert_eq!(actor.endless_pain_stacks, 0);
        assert!((target.hp - 78.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 22.0).abs() < 0.0001);

        assert!(store.record_skill_use("battle", "a", "target", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        assert!((target.hp - 68.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 32.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_liquid_body_talent_delays_damage_and_heals_previous_turn_damage() {
        let mut manager = empty_manager();
        let liquid_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            skill_names: vec!["液态躯体".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("b".to_owned(), liquid_character.clone());
        let actor = participant("a", 0);
        let target = participant_from_character("b", &liquid_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                active: true,
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "液态测试".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 15.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 5.0).abs() < 0.0001);
        assert_eq!(target.delayed_damage_ticks.len(), 1);
        assert_eq!(
            target.delayed_damage_ticks[0].name,
            "液态躯体"
        );
        assert_eq!(
            target.delayed_damage_ticks[0].source_id,
            "a"
        );
        assert!((target.delayed_damage_ticks[0].amount - 5.0).abs() < 0.0001);
        assert!(store.encounters["battle"]
            .action_log
            .iter()
            .any(|entry| entry.contains("触发液态躯体")));

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 10.25).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 0.25).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 5.0).abs() < 0.0001);
        assert_eq!(target.delayed_damage_ticks.len(), 1);

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 10.5).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 0.25).abs() < 0.0001);
        assert_eq!(target.damage_taken_this_turn, 0.0);
        assert!(target.delayed_damage_ticks.is_empty());
    }

    #[test]
    fn parsed_battle_keen_evasion_talent_dodges_first_area_damage() {
        let mut manager = empty_manager();
        let keen_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            skill_names: vec!["敏锐".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("b".to_owned(), keen_character.clone());
        let actor = participant("a", 0);
        let target = participant_from_character("b", &keen_character, &manager);
        let bystander = participant("c", 0);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                active: true,
                participants: vec![actor, target, bystander],
                ..Default::default()
            });
        let direct_skill = CharacterSkill {
            index: 0,
            name: "单点测试".to_owned(),
            note: "主动使用对目标造成4点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let area_skill = CharacterSkill {
            index: 1,
            name: "范围测试".to_owned(),
            note: "主动使用对范围内目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("范围".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &direct_skill,
            &manager,
            None
        ));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 16.0).abs() < 0.0001);
        assert!(target.keen_evasion_enabled);
        assert!(target.keen_evasion_available);

        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &area_skill,
            &manager,
            None
        ));
        let encounter = &store.encounters["battle"];
        let target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 16.0).abs() < 0.0001);
        assert!(!target.keen_evasion_available);
        let bystander = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert_eq!(bystander.hp, 0.0);
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("触发敏锐")));

        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &area_skill,
            &manager,
            None
        ));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 6.0).abs() < 0.0001);
        assert!(!target.keen_evasion_available);
    }

    #[test]
    fn arcane_shield_absorbs_battle_damage_before_hp() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                mp: 30.0,
                max_mp: 50.0,
                skill_names: vec!["奥术护盾".to_owned()],
                skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                    "support_talent",
                    "辅助天赋",
                )],
                ..Default::default()
            });
        let mut participant = participant_from_target("target", &manager);

        assert!((participant.arcane_shield - 5.0).abs() < 0.0001);
        let resolution = apply_participant_damage_for_battle(&mut participant, 3.0, "enemy", true);
        assert!(resolution.defeat_outcome.is_none());
        assert!((resolution.damage_applied - 0.0).abs() < 0.0001);
        assert!((participant.arcane_shield - 2.0).abs() < 0.0001);
        assert!((participant.hp - 20.0).abs() < 0.0001);
        assert!((participant.damage_taken_this_turn - 0.0).abs() < 0.0001);
        assert!(participant.damage_contributors.is_empty());

        let resolution = apply_participant_damage_for_battle(&mut participant, 4.0, "enemy", true);
        assert!(resolution.defeat_outcome.is_none());
        assert!((resolution.damage_applied - 2.0).abs() < 0.0001);
        assert!((participant.arcane_shield - 0.0).abs() < 0.0001);
        assert!((participant.hp - 18.0).abs() < 0.0001);
        assert!((participant.damage_taken_this_turn - 2.0).abs() < 0.0001);
        assert_eq!(participant.damage_contributors, vec![
            "enemy".to_owned()
        ]);

        let persisted = serde_json::to_string(&participant).unwrap();
        let restored: BattleParticipantSnapshot = serde_json::from_str(&persisted).unwrap();
        assert!((restored.arcane_shield - participant.arcane_shield).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_undying_rage_negates_one_lethal_round_and_boosts_damage() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            skill_names: vec!["不死者之怒".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let mut target = participant("b", 0);
        target.hp = 100.0;
        target.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });

        let actor = store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let resolution = apply_participant_damage_for_battle(actor, 20.0, "enemy", true);
        assert!(resolution.defeat_outcome.is_none());
        assert!(resolution.undying_rage_triggered);
        assert!((actor.hp - 20.0).abs() < 0.0001);
        assert!(actor.undying_rage_used);
        assert!(actor.undying_rage_active);
        assert!((actor.damage_taken_this_turn - 0.0).abs() < 0.0001);
        assert!(actor.damage_contributors.is_empty());

        let persisted = serde_json::to_string(actor).unwrap();
        let restored: BattleParticipantSnapshot = serde_json::from_str(&persisted).unwrap();
        assert!(restored.undying_rage_used);
        assert!(restored.undying_rage_active);

        let skill = CharacterSkill {
            index: 0,
            name: "怒击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 89.0).abs() < 0.0001);

        let actor = store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let resolution = apply_participant_damage_for_battle(actor, 20.0, "enemy", true);
        assert!(resolution.defeat_outcome.is_none());
        assert!((resolution.damage_applied - 0.0).abs() < 0.0001);
        assert!((actor.hp - 20.0).abs() < 0.0001);

        assert!(store.next_round("battle"));
        let actor = store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!(!actor.undying_rage_active);
        let resolution = apply_participant_damage_for_battle(actor, 20.0, "enemy", true);
        assert!(resolution.defeat_outcome.is_some());
        assert!(!actor.alive);

        let mut oversized = participant_from_character("a", &actor_character, &manager);
        let resolution = apply_participant_damage_for_battle(&mut oversized, 21.0, "enemy", true);
        assert!(resolution.defeat_outcome.is_some());
        assert!(!oversized.alive);
        assert!(!oversized.undying_rage_used);
    }

    #[test]
    fn calm_heart_heals_active_combat_damage_once_on_battle_exit() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            damage_dealt_modifier: 1.0,
            damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            healing_taken_modifier: 1.0,
            skill_names: vec!["息心".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        assert!((actor.calm_heart_healing_rate - 0.5).abs() < 0.0001);

        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, participant("enemy", 0)],
                ..Default::default()
            });

        assert!(store.apply_action("battle", "enemy", "a", "攻击", 30.0));
        let actor = &store.encounters["battle"].participants[0];
        assert!((actor.hp - 70.0).abs() < 0.0001);
        assert!((actor.combat_damage_taken_total - 30.0).abs() < 0.0001);
        let restored: BattleParticipantSnapshot =
            serde_json::from_str(&serde_json::to_string(actor).unwrap()).unwrap();
        assert!((restored.calm_heart_healing_rate - 0.5).abs() < 0.0001);
        assert!((restored.combat_damage_taken_total - 30.0).abs() < 0.0001);

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, false
        ));
        assert!((encounter.participants[0].hp - 85.0).abs() < 0.0001);
        assert!((encounter.participants[0].combat_damage_taken_total - 0.0).abs() < 0.0001);
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("触发息心，回复15点生命值")));
        assert!(!set_encounter_active_state(
            encounter, false
        ));
        assert!((encounter.participants[0].hp - 85.0).abs() < 0.0001);

        assert!(store.apply_action("battle", "enemy", "a", "休整攻击", 10.0));
        assert!((store.encounters["battle"].participants[0].hp - 75.0).abs() < 0.0001);
        assert!(
            (store.encounters["battle"].participants[0].combat_damage_taken_total - 0.0).abs()
                < 0.0001
        );

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, true
        ));
        assert!(store.apply_action("battle", "enemy", "a", "攻击", 10.0));
        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!((encounter.participants[0].combat_damage_taken_total - 10.0).abs() < 0.0001);
        assert!(set_encounter_active_state(
            encounter, false
        ));
        assert!((encounter.participants[0].hp - 70.0).abs() < 0.0001);

        assert!(set_encounter_active_state(
            encounter, true
        ));
        encounter.participants[0].hp = 5.0;
        assert!(store.apply_action("battle", "enemy", "a", "致命攻击", 10.0));
        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(!encounter.participants[0].alive);
        assert!(set_encounter_active_state(
            encounter, false
        ));
        assert!((encounter.participants[0].hp - 0.0).abs() < 0.0001);
        assert!((encounter.participants[0].combat_damage_taken_total - 0.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_hope_avatar_survives_two_rounds_for_healing_then_dies() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            mp: 20.0,
            max_mp: 20.0,
            damage_dealt_modifier: 1.0,
            damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            healing_taken_modifier: 1.0,
            skill_names: vec!["希望化身".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let mut target = participant("b", 0);
        target.hp = 5.0;
        target.max_hp = 20.0;
        let enemy = participant("enemy", 0);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target, enemy],
                ..Default::default()
            });

        assert!(store.apply_action("battle", "enemy", "a", "致命攻击", 10.0));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!((actor.hp - 0.0).abs() < 0.0001);
        assert!(actor.alive);
        assert!(actor.hope_avatar_used);
        assert_eq!(actor.hope_avatar_rounds_remaining, 2);
        assert!(store.encounters["battle"]
            .action_log
            .iter()
            .any(|entry| entry.contains("触发希望化身")));
        let restored: BattleParticipantSnapshot =
            serde_json::from_str(&serde_json::to_string(actor).unwrap()).unwrap();
        assert!(restored.hope_avatar_used);
        assert_eq!(restored.hope_avatar_rounds_remaining, 2);

        assert!(!store.apply_action("battle", "a", "enemy", "普通攻击", 5.0));
        let damage = CharacterSkill {
            index: 0,
            name: "天使之怒".to_owned(),
            note: "主动使用对目标造成5点魔法伤害".to_owned(),
            skill_type: Some("法术".to_owned()),
            legacy_buff_machine_json: None,
            mp_cost: 5.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        assert!(!store.record_skill_use("battle", "a", "enemy", &damage, &manager, None));
        let heal = CharacterSkill {
            index: 1,
            name: "希望治愈".to_owned(),
            note: "主动使用对目标恢复10点生命值".to_owned(),
            skill_type: Some("法术".to_owned()),
            legacy_buff_machine_json: None,
            mp_cost: 5.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        assert!(store.record_skill_use("battle", "a", "b", &heal, &manager, None));
        let encounter = &store.encounters["battle"];
        let actor = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((actor.mp - 15.0).abs() < 0.0001);
        assert!((target.hp - 15.0).abs() < 0.0001);

        store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap()
            .liquid_body_damage_delay_rate = 0.5;
        let immune_damage = CharacterSkill {
            name: "追击".to_owned(),
            note: "主动使用对目标造成999点物理伤害".to_owned(),
            mp_cost: 0.0,
            ..damage.clone()
        };
        assert!(store.record_skill_use(
            "battle",
            "enemy",
            "a",
            &immune_damage,
            &manager,
            None,
        ));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!((actor.hp - 0.0).abs() < 0.0001);
        assert!(actor.alive);
        assert!(actor.delayed_damage_ticks.is_empty());
        assert!(store.encounters["battle"]
            .action_log
            .iter()
            .any(|entry| entry.contains("处于希望化身，免疫本次伤害")));

        assert!(store.next_round("battle"));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!(actor.alive);
        assert_eq!(actor.hope_avatar_rounds_remaining, 1);

        assert!(store.next_round("battle"));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!(!actor.alive);
        assert!((actor.hp - 0.0).abs() < 0.0001);
        assert_eq!(actor.hope_avatar_rounds_remaining, 0);
        assert!(store.encounters["battle"]
            .action_log
            .iter()
            .any(|entry| entry.contains("希望化身结束，角色死亡")));
    }

    #[test]
    fn shield_absorption_gates_post_hit_talents_and_logs_applied_damage() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 90.0,
            max_hp: 100.0,
            level: 4,
            skill_names: vec![
                "溃伤".to_owned(),
                "禅宗古训".to_owned(),
                "苏萨斯之爪".to_owned(),
                "无限专注".to_owned(),
                "无尽痛楚".to_owned(),
            ],
            skill_metadata: (0..5)
                .map(|_| crate::napcat::CharacterSkillMetadata::talent("normal_talent", "天赋"))
                .collect(),
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        let mut actor = participant_from_character("a", &actor_character, &manager);
        actor.endless_pain_stacks = 2;
        let mut fully_shielded = participant("b", 0);
        fully_shielded.hp = 100.0;
        fully_shielded.max_hp = 100.0;
        fully_shielded.arcane_shield = 50.0;
        let mut partly_shielded = participant("c", 0);
        partly_shielded.hp = 100.0;
        partly_shielded.max_hp = 100.0;
        partly_shielded.arcane_shield = 5.0;
        let mut liquid_body_shielded = participant("d", 0);
        liquid_body_shielded.hp = 100.0;
        liquid_body_shielded.max_hp = 100.0;
        liquid_body_shielded.arcane_shield = 11.0;
        liquid_body_shielded.liquid_body_damage_delay_rate = 0.5;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, fully_shielded, partly_shielded, liquid_body_shielded],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "护盾测试击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        let encounter = &store.encounters["battle"];
        let actor = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((actor.hp - 90.0).abs() < 0.0001);
        assert_eq!(actor.endless_pain_stacks, 2);
        assert_eq!(actor.infinite_focus_stacks, 0);
        assert!((target.hp - 100.0).abs() < 0.0001);
        assert_eq!(target.wound_healing_taken_turns, 0);
        assert!(target.delayed_damage_ticks.is_empty());
        assert!(
            encounter.action_log.iter().any(|entry| {
                entry.contains("护盾测试击") && entry.contains("造成0点伤害")
            })
        );
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("吸收22点伤害")));

        store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap()
            .endless_pain_stacks = 0;
        assert!(store.record_skill_use("battle", "a", "c", &skill, &manager, None));
        let encounter = &store.encounters["battle"];
        let actor = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert!((actor.hp - 90.75).abs() < 0.0001);
        assert_eq!(
            actor.infinite_focus_target_id.as_deref(),
            Some("c")
        );
        assert_eq!(actor.infinite_focus_stacks, 1);
        assert!((target.hp - 95.0).abs() < 0.0001);
        assert_eq!(target.wound_healing_taken_turns, 1);
        assert_eq!(target.delayed_damage_ticks.len(), 1);
        assert!((target.delayed_damage_ticks[0].amount - 1.75).abs() < 0.0001);
        assert!(
            encounter.action_log.iter().any(|entry| {
                entry.contains("护盾测试击") && entry.contains("造成5点伤害")
            })
        );

        store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap()
            .endless_pain_stacks = 2;
        assert!(store.record_skill_use("battle", "a", "d", &skill, &manager, None));
        let encounter = &store.encounters["battle"];
        let actor = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "d")
            .unwrap();
        assert!((actor.hp - 90.75).abs() < 0.0001);
        assert_eq!(actor.endless_pain_stacks, 0);
        assert_eq!(
            actor.infinite_focus_target_id.as_deref(),
            Some("c")
        );
        assert_eq!(actor.infinite_focus_stacks, 1);
        assert!((target.hp - 100.0).abs() < 0.0001);
        assert_eq!(target.wound_healing_taken_turns, 0);
        assert_eq!(target.delayed_damage_ticks.len(), 1);
        assert!((target.delayed_damage_ticks[0].amount - 11.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_overhealing_talent_grants_capped_expiring_shield() {
        let mut manager = empty_manager();
        let healer_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            skill_names: vec!["过度治疗".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 95.0,
            max_hp: 100.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), healer_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let healer = participant_from_character("a", &healer_character, &manager);
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![healer, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "过量治疗测试".to_owned(),
            note: "主动使用对目标回复20点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        let target = store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 100.0).abs() < 0.0001);
        assert!((target.overhealing_shield - 30.0).abs() < 0.0001);
        assert_eq!(
            target.overhealing_shield_turns_remaining,
            2
        );
        let resolution = apply_participant_damage_for_battle(target, 20.0, "enemy", true);
        assert!(resolution.defeat_outcome.is_none());
        assert!((resolution.damage_applied - 0.0).abs() < 0.0001);
        assert!((target.hp - 100.0).abs() < 0.0001);
        assert!((target.overhealing_shield - 10.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 0.0).abs() < 0.0001);

        let persisted = serde_json::to_string(target).unwrap();
        let restored: BattleParticipantSnapshot = serde_json::from_str(&persisted).unwrap();
        assert!((restored.overhealing_shield - 10.0).abs() < 0.0001);
        assert_eq!(
            restored.overhealing_shield_turns_remaining,
            2
        );

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.overhealing_shield - 10.0).abs() < 0.0001);
        assert_eq!(
            target.overhealing_shield_turns_remaining,
            1
        );

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.overhealing_shield - 0.0).abs() < 0.0001);
        assert_eq!(
            target.overhealing_shield_turns_remaining,
            0
        );

        let target = store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        schedule_participant_delayed_healing(
            target,
            "a",
            "healer",
            "延迟治疗",
            10.0,
            0.30,
            1,
        );
        let persisted = serde_json::to_string(target).unwrap();
        let restored: BattleParticipantSnapshot = serde_json::from_str(&persisted).unwrap();
        assert!(
            (restored.delayed_healing_ticks[0].overhealing_shield_cap_rate - 0.30).abs() < 0.0001
        );
        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.overhealing_shield - 10.0).abs() < 0.0001);
        assert_eq!(
            target.overhealing_shield_turns_remaining,
            2
        );
    }

    #[test]
    fn parsed_battle_infinite_focus_talent_stacks_on_repeated_single_target_hits() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["无限专注".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let mut target_b = participant("b", 0);
        target_b.hp = 100.0;
        target_b.max_hp = 100.0;
        let mut target_c = participant("c", 0);
        target_c.hp = 100.0;
        target_c.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target_b, target_c],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "专注打击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let encounter = &store.encounters["battle"];
        let actor = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target_b = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(
            actor.infinite_focus_target_id.as_deref(),
            Some("b")
        );
        assert_eq!(actor.infinite_focus_stacks, 1);
        assert!((target_b.hp - 90.0).abs() < 0.0001);

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let encounter = &store.encounters["battle"];
        let actor = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target_b = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(actor.infinite_focus_stacks, 2);
        assert!((target_b.hp - 79.0).abs() < 0.0001);

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target_b = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target_b.hp - 67.0).abs() < 0.0001);

        assert!(store.record_skill_use("battle", "a", "c", &skill, &manager, None,));
        let encounter = &store.encounters["battle"];
        let actor = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target_c = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert_eq!(
            actor.infinite_focus_target_id.as_deref(),
            Some("c")
        );
        assert_eq!(actor.infinite_focus_stacks, 1);
        assert!((target_c.hp - 90.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_champion_talent_stacks_from_player_eliminations() {
        let mut manager = empty_manager();
        let champion_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["总冠军".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        let victim_character = PlayerCharacter {
            hp: 5.0,
            max_hp: 5.0,
            ..Default::default()
        };
        let attacker_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            ..Default::default()
        };
        manager.player_characters.insert(
            "a".to_owned(),
            champion_character.clone(),
        );
        manager.player_characters.insert(
            "victim".to_owned(),
            victim_character.clone(),
        );
        manager.player_characters.insert(
            "attacker".to_owned(),
            attacker_character.clone(),
        );
        let champion = participant_from_character("a", &champion_character, &manager);
        let victim = participant_from_character("victim", &victim_character, &manager);
        let attacker = participant_from_character(
            "attacker",
            &attacker_character,
            &manager,
        );
        let mut target = participant("target", 0);
        target.hp = 100.0;
        target.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![champion, victim, attacker, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "冠军击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "victim", &skill, &manager, None,));
        let champion = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let victim = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "victim")
            .unwrap();
        assert_eq!(champion.champion_stacks, 1);
        assert!(!victim.alive);

        assert!(store.record_skill_use("battle", "a", "target", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        assert!((target.hp - 89.8).abs() < 0.0001);

        assert!(store.record_skill_use("battle", "attacker", "a", &skill, &manager, None,));
        let champion = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!((champion.hp - 90.1).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_dominion_talent_gains_capped_max_hp_when_any_target_dies() {
        let mut manager = empty_manager();
        let dominion_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["役于我手".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        let attacker_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            ..Default::default()
        };
        let victim_character = PlayerCharacter {
            hp: 5.0,
            max_hp: 50.0,
            ..Default::default()
        };
        manager.player_characters.insert(
            "a".to_owned(),
            dominion_character.clone(),
        );
        manager.player_characters.insert(
            "cap".to_owned(),
            dominion_character.clone(),
        );
        manager.player_characters.insert(
            "killer".to_owned(),
            attacker_character.clone(),
        );
        manager.player_characters.insert(
            "victim".to_owned(),
            victim_character.clone(),
        );
        let fresh_holder = participant_from_character("a", &dominion_character, &manager);
        let mut capped_holder = participant_from_character("cap", &dominion_character, &manager);
        capped_holder.dominion_max_hp_bonus = 19.0;
        capped_holder.max_hp += 19.0;
        let attacker = participant_from_character("killer", &attacker_character, &manager);
        let victim = participant_from_character("victim", &victim_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![fresh_holder, capped_holder, attacker, victim],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "收割".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "killer", "victim", &skill, &manager, None,));

        let encounter = &store.encounters["battle"];
        let fresh_holder = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!((fresh_holder.dominion_max_hp_bonus - 2.5).abs() < 0.0001);
        assert!((fresh_holder.max_hp - 102.5).abs() < 0.0001);
        assert!((fresh_holder.hp - 100.0).abs() < 0.0001);

        let capped_holder = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "cap")
            .unwrap();
        assert!((capped_holder.dominion_max_hp_bonus - 20.0).abs() < 0.0001);
        assert!((capped_holder.max_hp - 120.0).abs() < 0.0001);

        let defeated = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "victim")
            .unwrap();
        assert!(!defeated.alive);
        assert!(defeated.damage_contributors.is_empty());
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("触发役于我手")));
    }

    #[test]
    fn parsed_battle_sin_on_sin_talent_recovers_missing_resources_on_kill_participation() {
        let mut manager = empty_manager();
        let killer_character = PlayerCharacter {
            hp: 50.0,
            max_hp: 100.0,
            mp: 20.0,
            max_mp: 60.0,
            skill_names: vec!["罪上加罪".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        let assistant_character = PlayerCharacter {
            hp: 80.0,
            max_hp: 100.0,
            mp: 40.0,
            max_mp: 60.0,
            skill_names: vec!["罪上加罪".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        let victim_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), killer_character.clone());
        manager.player_characters.insert(
            "c".to_owned(),
            assistant_character.clone(),
        );
        manager
            .player_characters
            .insert("b".to_owned(), victim_character.clone());
        let mut killer = participant_from_character("a", &killer_character, &manager);
        killer.sin_on_sin_stacks = 4;
        let assistant = participant_from_character("c", &assistant_character, &manager);
        let victim = participant_from_character("b", &victim_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![killer, assistant, victim],
                ..Default::default()
            });
        let assist_damage = CharacterSkill {
            index: 0,
            name: "助攻".to_owned(),
            note: "主动使用对目标造成4点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let killing_damage = CharacterSkill {
            index: 1,
            name: "终击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use(
            "battle",
            "c",
            "b",
            &assist_damage,
            &manager,
            None
        ));
        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &killing_damage,
            &manager,
            None
        ));

        let encounter = &store.encounters["battle"];
        let killer = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert_eq!(killer.sin_on_sin_stacks, 5);
        assert!((killer.hp - 55.0).abs() < 0.0001);
        assert!((killer.mp - 24.0).abs() < 0.0001);
        assert!((killer.healing_taken_this_turn - 5.0).abs() < 0.0001);
        assert!(
            (sin_on_sin_exp_bonus_percent(
                killer.sin_on_sin_exp_bonus_per_stack,
                killer.sin_on_sin_stacks,
            ) - 10.0)
                .abs()
                < 0.0001
        );

        let assistant = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert_eq!(assistant.sin_on_sin_stacks, 1);
        assert!((assistant.hp - 82.0).abs() < 0.0001);
        assert!((assistant.mp - 42.0).abs() < 0.0001);
        assert!((assistant.healing_taken_this_turn - 2.0).abs() < 0.0001);
        assert!(
            (sin_on_sin_exp_bonus_percent(
                assistant.sin_on_sin_exp_bonus_per_stack,
                assistant.sin_on_sin_stacks,
            ) - 2.5)
                .abs()
                < 0.0001
        );

        let defeated = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!(!defeated.alive);
        assert!(defeated.damage_contributors.is_empty());
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("触发罪上加罪") && entry.contains("经验加成10%")));
    }

    #[test]
    fn parsed_battle_skill_applies_chaos_output_variance_talent() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["混沌无序".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 50.0,
            max_hp: 100.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let damage_skill = CharacterSkill {
            index: 0,
            name: "混沌击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let heal_skill = CharacterSkill {
            name: "混沌疗".to_owned(),
            note: "主动使用对目标治疗10点生命值".to_owned(),
            ..damage_skill.clone()
        };

        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &damage_skill,
            &manager,
            None,
        ));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!(
            (8.5..=11.5).contains(&target.damage_taken_this_turn),
            "damage roll out of range: {}",
            target.damage_taken_this_turn
        );

        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &heal_skill,
            &manager,
            None,
        ));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!(
            (8.5..=11.5).contains(&target.healing_taken_this_turn),
            "healing roll out of range: {}",
            target.healing_taken_this_turn
        );
    }

    #[test]
    fn parsed_battle_skill_applies_dying_target_healing_talent() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 4.0,
            max_hp: 20.0,
            skill_names: vec!["生死时速".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "急救".to_owned(),
            note: "主动使用对目标治疗4点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 10.0).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 6.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_skill_applies_wounded_healing_dealt_talent() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 16.0,
            max_hp: 20.0,
            skill_names: vec!["火源之力".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 0.0,
            max_hp: 30.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "火疗".to_owned(),
            note: "主动使用对目标治疗10点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 12.0).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 12.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_penance_healing_bonus_decays_on_kill_or_assist() {
        let mut manager = empty_manager();
        let penitent = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            healing_dealt_modifier: 1.25,
            skill_names: vec!["忏悔".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), penitent.clone());
        let actor = participant_from_character("a", &penitent, &manager);
        let mut assistant = participant("c", 0);
        assistant.hp = 10.0;
        assistant.max_hp = 10.0;
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 10.0;
        let mut heal_target = participant("d", 0);
        heal_target.hp = 0.0;
        heal_target.max_hp = 20.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, assistant, target, heal_target],
                ..Default::default()
            });
        let assist_damage = CharacterSkill {
            index: 0,
            name: "助攻".to_owned(),
            note: "主动使用对目标造成4点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let killing_damage = CharacterSkill {
            index: 1,
            name: "终击".to_owned(),
            note: "主动使用对目标造成6点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let heal = CharacterSkill {
            index: 2,
            name: "忏悔治疗".to_owned(),
            note: "主动使用对目标治疗10点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use(
            "battle",
            "c",
            "b",
            &assist_damage,
            &manager,
            None
        ));
        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &killing_damage,
            &manager,
            None
        ));
        let encounter = &store.encounters["battle"];
        let actor = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert_eq!(
            actor.penance_healing_bonus_percent,
            25.0
        );
        assert_eq!(actor.penance_kill_assist_count, 1);
        let assistant = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert_eq!(assistant.penance_kill_assist_count, 1);
        let defeated = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!(!defeated.alive);
        assert!(defeated.damage_contributors.is_empty());

        assert!(store.record_skill_use("battle", "a", "d", &heal, &manager, None));
        let heal_target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "d")
            .unwrap();
        assert!((heal_target.hp - 11.5).abs() < 0.0001);
        assert!((heal_target.healing_taken_this_turn - 11.5).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_skill_applies_mutual_aid_healing_feedback_talent() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 20.0,
            skill_names: vec!["互帮互助".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 0.0,
            max_hp: 20.0,
            skill_names: vec!["互帮互助".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "互助治疗".to_owned(),
            note: "主动使用对目标治疗4点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!((actor.hp - 14.0).abs() < 0.0001);
        assert!((actor.healing_taken_this_turn - 4.0).abs() < 0.0001);
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 4.0).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 4.0).abs() < 0.0001);
    }

    #[test]
    fn parsed_battle_one_heart_talent_stacks_same_target_healing_and_resets_on_switch() {
        let mut manager = empty_manager();
        let healer_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            skill_names: vec!["一心".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), healer_character.clone());
        let healer = participant_from_character("a", &healer_character, &manager);
        let mut target_b = participant("b", 0);
        target_b.hp = 0.0;
        target_b.max_hp = 100.0;
        let mut target_c = participant("c", 0);
        target_c.hp = 0.0;
        target_c.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![healer, target_b, target_c],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "专注治疗".to_owned(),
            note: "主动使用对目标治疗10点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        let encounter = &store.encounters["battle"];
        let healer = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target_b = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(
            healer.one_heart_target_id.as_deref(),
            Some("b")
        );
        assert_eq!(healer.one_heart_stacks, 1);
        assert!((target_b.hp - 10.0).abs() < 0.0001);

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        let encounter = &store.encounters["battle"];
        let healer = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target_b = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(healer.one_heart_stacks, 2);
        assert!((target_b.hp - 20.5).abs() < 0.0001);

        assert!(store.record_skill_use("battle", "a", "c", &skill, &manager, None));
        let encounter = &store.encounters["battle"];
        let healer = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target_c = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert_eq!(
            healer.one_heart_target_id.as_deref(),
            Some("c")
        );
        assert_eq!(healer.one_heart_stacks, 1);
        assert!((target_c.hp - 10.0).abs() < 0.0001);
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("触发一心")));
    }

    #[test]
    fn parsed_battle_inspiration_transfers_single_target_bonus_and_expires() {
        let mut manager = empty_manager();
        let healer_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            damage_dealt_modifier: 1.0,
            damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            healing_taken_modifier: 1.0,
            skill_names: vec!["振奋".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), healer_character.clone());
        let mut healer = participant_from_character("a", &healer_character, &manager);
        healer.speed = 1.0;
        let mut target_b = participant("b", 0);
        target_b.hp = 10.0;
        target_b.max_hp = 20.0;
        target_b.speed = 10.0;
        let mut target_c = participant("c", 0);
        target_c.hp = 10.0;
        target_c.max_hp = 20.0;
        target_c.speed = 10.5;
        let mut damage_target = participant("d", 0);
        damage_target.hp = 100.0;
        damage_target.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![healer, target_b, target_c, damage_target],
                ..Default::default()
            });
        let heal = CharacterSkill {
            index: 0,
            name: "振奋治疗".to_owned(),
            note: "主动使用对目标恢复10点生命值".to_owned(),
            skill_type: Some("法术".to_owned()),
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let damage = CharacterSkill {
            index: 0,
            name: "测试攻击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &heal, &manager, None));
        let encounter = &store.encounters["battle"];
        let healer = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target_b = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(
            healer.inspiration_target_id.as_deref(),
            Some("b")
        );
        assert_eq!(
            target_b.inspiration_sources.get("a"),
            Some(&1)
        );
        assert!((participant_inspiration_multiplier(target_b) - 1.10).abs() < 0.0001);
        let restored: BattleParticipantSnapshot =
            serde_json::from_str(&serde_json::to_string(target_b).unwrap()).unwrap();
        assert_eq!(
            restored.inspiration_sources.get("a"),
            Some(&1)
        );
        let order = ordered_participant_indices(encounter);
        let b_index = encounter
            .participants
            .iter()
            .position(|participant| participant.target_id == "b")
            .unwrap();
        let c_index = encounter
            .participants
            .iter()
            .position(|participant| participant.target_id == "c")
            .unwrap();
        assert!(
            order.iter().position(|index| *index == b_index).unwrap()
                < order.iter().position(|index| *index == c_index).unwrap()
        );
        assert!(store.record_skill_use("battle", "b", "d", &damage, &manager, None));
        assert!((store.encounters["battle"].participants[3].hp - 89.0).abs() < 0.0001);

        assert!(store.record_skill_use("battle", "a", "c", &heal, &manager, None));
        let encounter = &store.encounters["battle"];
        let target_b = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        let target_c = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert!(target_b.inspiration_sources.is_empty());
        assert_eq!(
            target_c.inspiration_sources.get("a"),
            Some(&1)
        );
        let mut multiply_inspired = target_c.clone();
        multiply_inspired
            .inspiration_sources
            .insert("other-healer".to_owned(), 1);
        assert!((participant_inspiration_multiplier(&multiply_inspired) - 1.10).abs() < 0.0001);
        assert!(store.record_skill_use("battle", "b", "d", &damage, &manager, None));
        assert!(store.record_skill_use("battle", "c", "d", &damage, &manager, None));
        let damage_target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "d")
            .unwrap();
        assert!((damage_target.hp - 68.0).abs() < 0.0001);

        let area_heal = CharacterSkill {
            index: 1,
            name: "范围治疗".to_owned(),
            note: "主动使用对周围3米内的目标恢复1点生命值".to_owned(),
            target_class: Some("范围".to_owned()),
            ..heal.clone()
        };
        let positions = SceneCharacterPositions {
            positions: HashMap::from([
                ("a".to_owned(), Vec3::ZERO),
                ("b".to_owned(), Vec3::new(1.0, 0.0, 0.0)),
                ("c".to_owned(), Vec3::new(2.0, 0.0, 0.0)),
                (
                    "d".to_owned(),
                    Vec3::new(10.0, 0.0, 0.0),
                ),
            ]),
        };
        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &area_heal,
            &manager,
            Some(&positions),
        ));
        let target_c = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert_eq!(
            target_c.inspiration_sources.get("a"),
            Some(&1)
        );

        assert!(store.next_round("battle"));
        let encounter = &store.encounters["battle"];
        let healer = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target_c = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert!(healer.inspiration_target_id.is_none());
        assert!(target_c.inspiration_sources.is_empty());
        assert!(store.record_skill_use("battle", "c", "d", &damage, &manager, None));
        let damage_target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "d")
            .unwrap();
        assert!((damage_target.hp - 58.0).abs() < 0.0001);
        assert!(store.encounters["battle"]
            .action_log
            .iter()
            .any(|entry| entry.contains("触发振奋")));
    }

    #[test]
    fn parsed_battle_echoing_memory_talent_schedules_single_target_healing_echoes() {
        let mut manager = empty_manager();
        let healer_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            skill_names: vec!["千万回忆".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), healer_character.clone());
        let healer = participant_from_character("a", &healer_character, &manager);
        let mut target_b = participant("b", 0);
        target_b.hp = 0.0;
        target_b.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                active: true,
                participants: vec![healer, target_b],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "回忆治疗".to_owned(),
            note: "主动使用对目标治疗20点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        let encounter = &store.encounters["battle"];
        let target_b = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target_b.hp - 20.0).abs() < 0.0001);
        assert_eq!(target_b.delayed_healing_ticks.len(), 2);
        assert_eq!(
            target_b.delayed_healing_ticks[0].name,
            "千万回忆"
        );
        assert_eq!(
            target_b.delayed_healing_ticks[0].source_id,
            "a"
        );
        assert!((target_b.delayed_healing_ticks[0].amount - 3.0).abs() < 0.0001);
        assert_eq!(
            target_b.delayed_healing_ticks[0].turns_remaining,
            1
        );
        assert!((target_b.delayed_healing_ticks[1].amount - 1.0).abs() < 0.0001);
        assert_eq!(
            target_b.delayed_healing_ticks[1].turns_remaining,
            2
        );

        assert!(store.next_round("battle"));
        let encounter = &store.encounters["battle"];
        let target_b = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target_b.hp - 23.0).abs() < 0.0001);
        assert!((target_b.healing_taken_this_turn - 3.0).abs() < 0.0001);
        assert_eq!(target_b.delayed_healing_ticks.len(), 1);
        assert!((target_b.delayed_healing_ticks[0].amount - 1.0).abs() < 0.0001);
        assert_eq!(
            target_b.delayed_healing_ticks[0].turns_remaining,
            1
        );
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("触发千万回忆")));

        assert!(store.next_round("battle"));
        let target_b = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target_b.hp - 24.0).abs() < 0.0001);
        assert!((target_b.healing_taken_this_turn - 1.0).abs() < 0.0001);
        assert!(target_b.delayed_healing_ticks.is_empty());
    }

    #[test]
    fn parsed_battle_skill_applies_low_hp_damage_penalty() {
        let manager = empty_manager();
        let mut actor = participant("a", 0);
        actor.hp = 5.0;
        actor.max_hp = 10.0;
        let mut target = participant("b", 0);
        target.hp = 20.0;
        target.max_hp = 20.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "旋风斩".to_owned(),
            note: "主动使用对目标造成4点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));

        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 17.0);
    }

    #[test]
    fn battle_skill_executes_multiple_damage_and_healing_actions_in_order() {
        let manager = empty_manager();
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 20.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "连段".to_owned(),
            note: "主动使用对目标造成3点物理伤害，对目标回复2点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: Some(1),
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));

        let encounter = &store.encounters["battle"];
        let target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 9.0);
        let effect_logs = encounter
            .action_log
            .iter()
            .filter(|entry| entry.contains("使用连段"))
            .collect::<Vec<_>>();
        assert_eq!(effect_logs.len(), 2);
        assert!(effect_logs[0].contains("造成3点伤害"));
        assert!(effect_logs[1].contains("回复2点生命值"));
    }

    #[test]
    fn battle_skill_uses_numeric_skill_args_in_amounts() {
        let manager = empty_manager();
        let character = PlayerCharacter {
            skill_names: vec!["变量伤害".to_owned()],
            skill_notes: vec!["主动使用对目标造成伤害值点物理伤害".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata {
                args: vec![crate::napcat::SkillPoolArg {
                    name: "伤害值".to_owned(),
                    kind: "数字".to_owned(),
                    value: "3".to_owned(),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let skill = character_skills(&character).remove(0);
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 10.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), target],
                ..Default::default()
            });

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 7.0);
    }

    #[test]
    fn battle_skill_uses_text_skill_args_in_rule_text() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            basic_config: TrpgBasicConfig {
                dex_range_damage_bonus: 0.5,
                ..Default::default()
            },
            ..Default::default()
        });
        let character = PlayerCharacter {
            skill_names: vec!["变量类型".to_owned()],
            skill_notes: vec!["主动使用对目标造成2点伤害类型伤害".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata {
                args: vec![crate::napcat::SkillPoolArg {
                    name: "伤害类型".to_owned(),
                    kind: "字符串".to_owned(),
                    value: "远程".to_owned(),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let skill = character_skills(&character).remove(0);
        let mut actor = participant("a", 0);
        actor.dex = 4;
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 10.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                trpg_group: Some("party".to_owned()),
                participants: vec![actor, target],
                ..Default::default()
            });

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 4.0);
    }

    #[test]
    fn battle_skill_uses_legacy_buff_machine_heal_when_note_unparsed() {
        let manager = empty_manager();
        let character = PlayerCharacter {
            skill_names: vec!["旧蓝图治疗".to_owned()],
            skill_notes: vec!["旧月莓图形技能".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata {
                args: vec![crate::napcat::SkillPoolArg {
                    name: "治疗量".to_owned(),
                    kind: "数字".to_owned(),
                    value: "3".to_owned(),
                }],
                legacy_has_buff_machine: true,
                legacy_buff_machine_json: Some(
                    r#"{"技能释放":[{"name":"治疗术","effect":["治疗"],"type":0,"from":"技能目标","value":["治疗量"]}]}"#
                        .to_owned(),
                ),
                ..Default::default()
            }],
            ..Default::default()
        };
        let skill = character_skills(&character).remove(0);
        let mut target = participant("b", 0);
        target.hp = 4.0;
        target.max_hp = 10.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), target],
                ..Default::default()
            });

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 7.0);
        assert_eq!(target.healing_taken_this_turn, 3.0);
    }

    #[test]
    fn battle_skill_uses_skill_type_as_default_damage_type() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            basic_config: TrpgBasicConfig {
                dex_range_damage_bonus: 0.5,
                ..Default::default()
            },
            ..Default::default()
        });
        let character = PlayerCharacter {
            skill_names: vec!["远程伤害".to_owned()],
            skill_notes: vec!["主动使用对目标造成2点伤害".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata {
                skill_type: Some("远程".to_owned()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let skill = character_skills(&character).remove(0);
        let mut actor = participant("a", 0);
        actor.dex = 4;
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 10.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                trpg_group: Some("party".to_owned()),
                participants: vec![actor, target],
                ..Default::default()
            });

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 4.0);
    }

    #[test]
    fn battle_range_damage_uses_converter_magic_bonus_talent() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            basic_config: TrpgBasicConfig {
                dex_range_damage_bonus: 0.5,
                int_damage_bonus: 0.2,
                ..Default::default()
            },
            ..Default::default()
        });
        let character = PlayerCharacter {
            status: CharacterStatus {
                dex: 4,
                int_: 5,
                ..Default::default()
            },
            skill_names: vec!["远程伤害".to_owned(), "数魔转换器".to_owned()],
            skill_notes: vec!["主动使用对目标造成2点伤害".to_owned(), String::new()],
            skill_metadata: vec![
                crate::napcat::CharacterSkillMetadata {
                    skill_type: Some("远程".to_owned()),
                    ..Default::default()
                },
                crate::napcat::CharacterSkillMetadata::talent("normal_talent", "天赋"),
            ],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), character.clone());
        let skill = character_skills(&character).remove(0);
        let actor = participant_from_character("a", &character, &manager);
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 10.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                trpg_group: Some("party".to_owned()),
                participants: vec![actor, target],
                ..Default::default()
            });

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 2.0);
    }

    #[test]
    fn battle_skill_respects_imported_cooldown_left() {
        let mut store = BattleRoundStore::default();
        let manager = empty_manager();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), participant("b", 0)],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "护盾".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: Some(2),
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(!store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        assert!(store.encounters["battle"].action_log[0].contains("冷却还剩2轮"));
    }

    #[test]
    fn battle_skill_limits_targets_by_metadata_target_count() {
        let mut store = BattleRoundStore::default();
        let manager = empty_manager();
        let mut first = participant("b", 0);
        first.hp = 10.0;
        first.max_hp = 10.0;
        let mut second = participant("c", 0);
        second.hp = 10.0;
        second.max_hp = 10.0;
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), first, second],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "范围测试".to_owned(),
            note: "主动使用对范围内目标造成1点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: Some(1),
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let encounter = &store.encounters["battle"];
        let first = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        let second = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert_eq!(first.hp, 9.0);
        assert_eq!(second.hp, 10.0);
    }

    #[test]
    fn battle_skill_no_target_class_blocks_targets() {
        let mut store = BattleRoundStore::default();
        let manager = empty_manager();
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 10.0;
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "无目标测试".to_owned(),
            note: "主动使用对目标造成1点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: Some(1),
            target_class: Some("无目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 10.0);
    }

    #[test]
    fn battle_skill_uses_metadata_range_when_area_omits_radius() {
        let mut store = BattleRoundStore::default();
        let manager = empty_manager();
        let mut first = participant("b", 0);
        first.hp = 10.0;
        first.max_hp = 10.0;
        let mut second = participant("c", 0);
        second.hp = 10.0;
        second.max_hp = 10.0;
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), first, second],
                ..Default::default()
            });
        let positions = SceneCharacterPositions {
            positions: HashMap::from([
                ("a".to_owned(), Vec3::ZERO),
                ("b".to_owned(), Vec3::new(2.9, 0.0, 0.0)),
                ("c".to_owned(), Vec3::new(3.1, 0.0, 0.0)),
            ]),
        };
        let skill = CharacterSkill {
            index: 0,
            name: "范围测试".to_owned(),
            note: "主动使用对范围内目标造成1点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: Some(3),
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &skill,
            &manager,
            Some(&positions),
        ));
        let encounter = &store.encounters["battle"];
        let first = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        let second = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "c")
            .unwrap();
        assert_eq!(first.hp, 9.0);
        assert_eq!(second.hp, 10.0);
    }

    #[test]
    fn battle_skill_single_target_respects_metadata_range() {
        let mut store = BattleRoundStore::default();
        let manager = empty_manager();
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 10.0;
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), target],
                ..Default::default()
            });
        let positions = SceneCharacterPositions {
            positions: HashMap::from([
                ("a".to_owned(), Vec3::ZERO),
                ("b".to_owned(), Vec3::new(3.1, 0.0, 0.0)),
            ]),
        };
        let skill = CharacterSkill {
            index: 0,
            name: "射程测试".to_owned(),
            note: "主动使用对目标造成1点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: Some(3),
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &skill,
            &manager,
            Some(&positions),
        ));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 10.0);
    }

    #[test]
    fn battle_range_damage_uses_tex30_minimum_range_talent() {
        let mut manager = empty_manager();
        let character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            mp: 10.0,
            max_mp: 10.0,
            level: 2,
            skill_names: vec!["远程伤害".to_owned(), "瞄准镜Tex-30".to_owned()],
            skill_notes: vec!["主动使用对目标造成1点伤害".to_owned(), String::new()],
            skill_metadata: vec![
                crate::napcat::CharacterSkillMetadata {
                    skill_type: Some("远程".to_owned()),
                    range: Some(3),
                    ..Default::default()
                },
                crate::napcat::CharacterSkillMetadata::talent("normal_talent", "天赋"),
            ],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), character.clone());
        let skill = character_skills(&character).remove(0);
        let actor = participant_from_character("a", &character, &manager);
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 10.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let positions = SceneCharacterPositions {
            positions: HashMap::from([
                ("a".to_owned(), Vec3::ZERO),
                (
                    "b".to_owned(),
                    Vec3::new(20.0, 0.0, 0.0),
                ),
            ]),
        };

        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &skill,
            &manager,
            Some(&positions),
        ));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 9.0);
    }

    #[test]
    fn battle_spell_skill_uses_magic_web_range_talent() {
        let mut manager = empty_manager();
        let character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            mp: 10.0,
            max_mp: 10.0,
            skill_names: vec!["法术伤害".to_owned(), "魔网延伸".to_owned()],
            skill_notes: vec!["主动使用对目标造成1点伤害".to_owned(), String::new()],
            skill_metadata: vec![
                crate::napcat::CharacterSkillMetadata {
                    skill_type: Some("法术".to_owned()),
                    range: Some(10),
                    ..Default::default()
                },
                crate::napcat::CharacterSkillMetadata::talent("normal_talent", "天赋"),
            ],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), character.clone());
        let skill = character_skills(&character).remove(0);
        let actor = participant_from_character("a", &character, &manager);
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 10.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, target],
                ..Default::default()
            });
        let positions = SceneCharacterPositions {
            positions: HashMap::from([
                ("a".to_owned(), Vec3::ZERO),
                (
                    "b".to_owned(),
                    Vec3::new(10.4, 0.0, 0.0),
                ),
            ]),
        };

        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &skill,
            &manager,
            Some(&positions),
        ));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert_eq!(target.hp, 9.0);
    }

    #[test]
    fn skill_cooldown_starts_after_skill_action_finishes() {
        let mut store = BattleRoundStore::default();
        let manager = empty_manager();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), participant("b", 0)],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "旋风斩".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 1,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        assert!(store.finish_actor_action("battle", "a"));

        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert_eq!(actor.turn, 1);
        assert_eq!(
            skill_cooldown_remaining(
                actor,
                skill.index,
                skill.cooldown_turns,
                skill.cooldown_left
            ),
            1
        );
    }
}
