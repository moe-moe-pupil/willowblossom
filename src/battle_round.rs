mod post_match;

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
    hidden_roles::{
        effective_hidden_role_character,
        hidden_role_battle_state,
        hidden_role_intrinsic_opening_shield,
        protective_suit_opening_magic_shield,
        HiddenRoleBattleState,
    },
    napcat::{
        arrogance_damage_dealt_multiplier,
        champion_damage_dealt_multiplier,
        champion_damage_taken_multiplier,
        character_arcane_shield_amount,
        character_arcane_shield_rate,
        character_arrogance_damage_bonus_per_source,
        character_butterfly_available,
        character_calm_heart_healing_rate,
        character_champion_damage_bonus_per_stack,
        character_champion_damage_reduction_per_stack,
        character_chaos_output_variance,
        character_damage_attribute_multiplier,
        character_damage_dealt_talent_buffs,
        character_damage_taken_attribute_multiplier,
        character_dominion_max_hp_bonus_cap,
        character_dominion_max_hp_gain_rate,
        character_dying_target_healing_modifier,
        character_echoing_memory_healing_rates,
        character_effective_skill_mp_cost,
        character_endless_pain_bonus_damage_per_stack,
        character_fatigue_walker_available,
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
        character_low_hp_damage_multiplier,
        character_minimum_damage_floor,
        character_minimum_range_meters,
        character_mirror_coat_available,
        character_moonberry_talent_damage_attribute_bonus,
        character_mutual_aid_healing_rate,
        character_next_level_exp,
        character_one_heart_healing_bonus_per_stack,
        character_overhealing_shield_cap_rate,
        character_penance_healing_bonus_percent,
        character_physical_damage_followup_rate,
        character_physical_damage_lifesteal,
        character_range_magic_converter_damage_bonus,
        character_redeemed_needle_state,
        character_rest_then_fight_healing_rate,
        character_sin_on_sin_exp_bonus_per_stack,
        character_sin_on_sin_recovery_rate,
        character_spell_range_multiplier,
        character_summon_damage_multiplier,
        character_sunset_available,
        character_undying_rage_available,
        character_valorous_battle_damage_multiplier,
        character_wounded_healing_dealt_modifier,
        dying_target_healing_multiplier,
        endless_pain_bonus_damage,
        infinite_focus_damage_dealt_multiplier,
        large_hit_damage_taken_multiplier,
        low_hp_damage_multiplier_with_fatigue,
        moonberry_chaos_output_multiplier,
        moonberry_effective_skill_range_radius_with_multiplier,
        moonberry_skill_type_is_spell,
        one_heart_healing_dealt_multiplier,
        penance_decayed_healing_dealt_modifier,
        sin_on_sin_exp_bonus_percent,
        skill_rule_args,
        status_damage_attribute_multiplier,
        status_healing_attribute_multiplier,
        trpg_config_with_weave,
        update_character_from_status_with_config,
        wounded_healing_dealt_multiplier,
        CharacterSkillMetadata,
        CharacterSkillSourceKind,
        CharacterStatus,
        NapcatMessageManager,
        PlayerCharacter,
        ProtectiveSuitKind,
        RedeemedNeedleState,
        SkillRuleArgs,
        Summon,
        SummonKind,
        TrpgBasicConfig,
        TrpgDamageBonusKind,
        TrpgDamageTakenKind,
        TrpgGlobalCombatModifiers,
        TrpgGroup,
        UnitInstance,
        UnitPoolEntry,
        UnitRarity,
        STATUS_POINTS_PER_LEVEL,
    },
    rule_engine::{
        apply_skill_type_damage_default,
        current_redeemed_numeric_skill_range,
        current_redeemed_skill_modes,
        legacy_moonberry_buff_machine_skill_cast_rule,
        parse_rule_with_named_args,
        Action,
        ActorRef,
        BuffTickAction,
        DamageType,
        RuleAmountResolution,
        RuleBuffTemplate,
        RuleEngineState,
        RuleModifier,
        TargetSelector,
        ValueExpr,
    },
    scene::SceneCharacterPositions,
    ui::{
        advance_buffs_for_players,
        sync_character_buffs,
    },
};

const MAX_GROUP_CLOCK_CATCH_UP_ROUNDS_PER_FRAME: u32 = 64;
const SUPPORT_TALENT_EXPERIENCE_BONUS_RATE: f32 = 0.15;

pub struct BattleRoundPlugin;

impl Plugin for BattleRoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BattleRoundUiState>()
            .init_resource::<BattleTargetingLine>()
            .add_systems(Startup, setup_battle_round_store)
            .add_systems(
                Update,
                (
                    sync_battle_round_entities,
                    draw_battle_targeting_line,
                ),
            )
            .add_systems(
                EguiPrimaryContextPass,
                battle_round_panel,
            );
    }
}

#[derive(Resource, Default, Debug, Clone)]
pub struct BattleTargetingLine {
    pub actor_id: Option<String>,
    pub target_id: Option<String>,
}

fn draw_battle_targeting_line(
    targeting: Res<BattleTargetingLine>,
    positions: Option<Res<SceneCharacterPositions>>,
    mut gizmos: Gizmos,
) {
    let Some(positions) = positions else {
        return;
    };
    let (Some(actor_id), Some(target_id)) = (
        targeting.actor_id.as_deref(),
        targeting.target_id.as_deref(),
    ) else {
        return;
    };
    let (Some(start), Some(end)) = (
        positions.positions.get(actor_id),
        positions.positions.get(target_id),
    ) else {
        return;
    };
    let lift = Vec3::Y * 0.35;
    gizmos.line(
        *start + lift,
        *end + lift,
        Color::srgb(1.0, 0.16, 0.08),
    );
    gizmos.sphere(
        Isometry3d::from_translation(*end + lift),
        0.18,
        Color::srgb(1.0, 0.75, 0.1),
    );
}

#[derive(Resource, Default)]
pub struct BattleRoundUiState {
    panel_open: bool,
    combat_log_open: bool,
    post_match: post_match::PostMatchSummaryUiState,
    new_encounter_name: String,
    selected_group: String,
    selected_add_player: HashMap<String, String>,
    selected_add_unit: HashMap<String, String>,
    selected_add_summon: HashMap<String, (String, usize)>,
    selected_action_target: HashMap<String, String>,
    selected_action_actor: HashMap<String, String>,
    selected_skill_index: HashMap<String, usize>,
    selected_item_index: HashMap<String, usize>,
    selected_item_skill_index: HashMap<String, usize>,
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
    #[serde(skip)]
    scene_positions: HashMap<String, Vec3>,
}

impl BattleRoundStore {
    pub fn add_unit_instance_to_encounter(
        &mut self,
        encounter_id: &str,
        instance_id: &str,
        manager: &NapcatMessageManager,
    ) -> Result<bool, String> {
        let instance_id = instance_id.trim();
        let instance = manager
            .unit_instances
            .get(instance_id)
            .ok_or_else(|| "NPC实例不存在".to_owned())?;
        let template = manager
            .unit_pool
            .get(&instance.template_id)
            .ok_or_else(|| "NPC实例对应的单位模板不存在".to_owned())?;
        let encounter = self
            .encounters
            .get_mut(encounter_id)
            .ok_or_else(|| "战斗轮不存在".to_owned())?;
        if encounter
            .participants
            .iter()
            .any(|participant| participant.target_id == instance_id)
        {
            return Ok(false);
        }
        let mut participant = participant_from_unit_instance(instance_id, instance, template);
        participant.group_modifiers = encounter_group_combat_modifiers(encounter, manager);
        initialize_participant_clock(
            &mut participant,
            encounter.trpg_group.as_deref(),
            manager,
        );
        encounter.participants.push(participant);
        Ok(true)
    }

    pub fn remove_unit_template_data(&mut self, template_id: &str) -> usize {
        let mut removed = 0;
        for encounter in self.encounters.values_mut() {
            let previous_len = encounter.participants.len();
            encounter
                .participants
                .retain(|participant| participant.unit_template_id.as_deref() != Some(template_id));
            removed += previous_len - encounter.participants.len();
        }
        removed
    }

    pub fn remove_player_data(&mut self, target_id: &str) -> usize {
        let mut removed = 0;
        for encounter in self.encounters.values_mut() {
            let previous_participant_len = encounter.participants.len();
            encounter.participants.retain(|participant| {
                participant.target_id != target_id
                    && !(participant.is_summon
                        && participant.summon_owner_id.as_deref() == Some(target_id))
            });
            removed += previous_participant_len - encounter.participants.len();

            for participant in &mut encounter.participants {
                participant
                    .arrogance_damage_source_ids
                    .retain(|source_id| source_id != target_id);
                if participant.infinite_focus_target_id.as_deref() == Some(target_id) {
                    participant.infinite_focus_target_id = None;
                    participant.infinite_focus_stacks = 0;
                }
                if participant.one_heart_target_id.as_deref() == Some(target_id) {
                    participant.one_heart_target_id = None;
                    participant.one_heart_stacks = 0;
                }
                if participant.inspiration_target_id.as_deref() == Some(target_id) {
                    participant.inspiration_target_id = None;
                }
                participant.inspiration_sources.remove(target_id);
                participant
                    .damage_contributors
                    .retain(|source_id| source_id != target_id);
                participant.damage_contribution_amounts.remove(target_id);
                participant
                    .delayed_damage_ticks
                    .retain(|tick| tick.source_id != target_id);
                participant
                    .delayed_healing_ticks
                    .retain(|tick| tick.source_id != target_id);
                participant
                    .corrosion_stacks
                    .retain(|stack| stack.source_id != target_id);
            }
            encounter
                .combat_log
                .retain(|entry| entry.source_id != target_id && entry.target_id != target_id);
            encounter.combat_log_start = encounter.combat_log_start.min(encounter.combat_log.len());
        }
        removed
    }
}

pub const BATTLE_ROUND_EXPORT_VERSION: u32 = 1;

#[derive(Serialize)]
struct BattleRoundStoreExportRef<'a> {
    version: u32,
    export_type: &'static str,
    store: &'a BattleRoundStore,
}

#[derive(Deserialize)]
struct BattleRoundStoreExportOwned {
    version: u32,
    export_type: String,
    store: BattleRoundStore,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BattleEncounter {
    pub name: String,
    #[serde(default)]
    pub trpg_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trpg_campaign_id: Option<String>,
    #[serde(default)]
    pub manager_sync_quarantined: bool,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default = "default_true")]
    pub sort_by_turn: bool,
    #[serde(default)]
    pub negative_enabled: bool,
    #[serde(default)]
    pub round: u32,
    #[serde(default)]
    pub combat_completed_turns: u32,
    #[serde(default)]
    pub participants: Vec<BattleParticipantSnapshot>,
    #[serde(default)]
    pub action_log: Vec<String>,
    #[serde(default)]
    pub combat_log: Vec<CombatLogEntry>,
    #[serde(default)]
    pub combat_log_start: usize,
}

impl Default for BattleEncounter {
    fn default() -> Self {
        Self {
            name: String::new(),
            trpg_group: None,
            trpg_campaign_id: None,
            manager_sync_quarantined: false,
            active: true,
            sort_by_turn: true,
            negative_enabled: false,
            round: 0,
            combat_completed_turns: 0,
            participants: Vec::new(),
            action_log: Vec::new(),
            combat_log: Vec::new(),
            combat_log_start: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CombatLogKind {
    Damage,
    Healing,
    Buff,
    Resource,
    Experience,
    Elimination,
    Assist,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CombatLogEntry {
    pub round: u32,
    pub kind: CombatLogKind,
    pub source_id: String,
    pub source_name: String,
    pub target_id: String,
    pub target_name: String,
    pub action_name: String,
    pub base_amount: f32,
    pub effective_amount: f32,
    #[serde(default)]
    pub modifiers: Vec<RuleModifier>,
    #[serde(default)]
    pub benefits: Vec<String>,
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
    /// 召唤物参与者：等级=主人等级，生命=召唤物数据，不受低血伤害减少。
    #[serde(default)]
    pub is_summon: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summon_owner_id: Option<String>,
    #[serde(default)]
    pub summon_kind: SummonKind,
    #[serde(default = "default_participant_level")]
    pub level: i32,
    #[serde(default)]
    pub exp: i32,
    #[serde(default)]
    pub support_talent_experience_bonus_rate: f32,
    #[serde(default)]
    pub base_damage: f32,
    #[serde(default)]
    pub unit_rarity: UnitRarity,
    #[serde(default)]
    pub turn: u32,
    #[serde(default)]
    pub combat_turns_completed: u32,
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
    /// TRPG-group-wide multipliers for every participant. Kept separate
    /// from character/equipment/buff modifiers so group setting changes do not
    /// get baked into persisted character, NPC, or summon state.
    #[serde(default, alias = "npc_group_modifiers")]
    pub group_modifiers: TrpgGlobalCombatModifiers,
    #[serde(default)]
    pub hidden_role: HiddenRoleBattleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protective_suit_kind: Option<ProtectiveSuitKind>,
    #[serde(default)]
    pub protective_suit_shield: f32,
    #[serde(default)]
    pub protective_suit_shield_max: f32,
    #[serde(default)]
    pub protective_suit_magic_only: bool,
    #[serde(default)]
    pub protective_suit_passive_active: bool,
    #[serde(default)]
    pub protective_suit_active_available: bool,
    #[serde(default)]
    pub protective_suit_speed_bonus: f32,
    #[serde(default)]
    pub protective_suit_slow_rounds_remaining: u32,
    #[serde(default)]
    pub protective_suit_blind_rounds_remaining: u32,
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
    pub arcane_shield_rate: f32,
    #[serde(default)]
    pub overhealing_shield_cap_rate: f32,
    #[serde(default)]
    pub overhealing_shield: f32,
    #[serde(default)]
    pub overhealing_shield_turns_remaining: u32,
    #[serde(default)]
    pub revenge_soul_shield_rate: f32,
    #[serde(default)]
    pub revenge_soul_shield: f32,
    #[serde(default)]
    pub construct_shield: f32,
    #[serde(default)]
    pub construct_shield_max: f32,
    #[serde(default)]
    pub construct_shield_repair_rounds_remaining: u32,
    #[serde(default)]
    pub construct_repair_channel_rounds_remaining: u32,
    #[serde(default)]
    pub paralyzed_rounds_remaining: u32,
    #[serde(default)]
    pub commissar_proficiency: u32,
    #[serde(default)]
    pub next_attack_bonus_physical: f32,
    #[serde(default)]
    pub natural_hp_regen_suppressed: bool,
    #[serde(default)]
    pub flying_needles_enabled: bool,
    #[serde(default)]
    pub flying_needles_ready: u8,
    #[serde(default)]
    pub needle_case_enabled: bool,
    #[serde(default)]
    pub needle_case_ready: u8,
    #[serde(default)]
    pub needle_case_progress_noncombat_rounds: u8,
    #[serde(default)]
    pub redeemed_invisibility_rounds_remaining: u32,
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
    pub mirror_coat_enabled: bool,
    #[serde(default)]
    pub mirror_coat_layers: u32,
    #[serde(default)]
    pub mirror_coat_cooldown_remaining: u32,
    #[serde(default)]
    pub mirror_coat_cleanup_pending: bool,
    #[serde(default)]
    pub sunset_enabled: bool,
    #[serde(default)]
    pub sunset_death_time_reset_pending: bool,
    #[serde(default)]
    pub goose_channeling_turns: u32,
    #[serde(default)]
    pub butterfly_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub butterfly_target_id: Option<String>,
    #[serde(default)]
    pub butterfly_effect_on_holder: bool,
    #[serde(default)]
    pub butterfly_initialized: bool,
    #[serde(default)]
    pub butterfly_locked: bool,
    #[serde(default)]
    pub butterfly_effect: bool,
    #[serde(default)]
    pub liquid_body_damage_delay_rate: f32,
    #[serde(default)]
    pub liquid_body_self_healing_rate: f32,
    #[serde(default)]
    pub calm_heart_healing_rate: f32,
    #[serde(default)]
    pub combat_damage_taken_total: f32,
    #[serde(default)]
    pub rest_then_fight_healing_rate: f32,
    #[serde(default)]
    pub rest_then_fight_turns: u32,
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
    pub damage_contribution_amounts: HashMap<String, f32>,
    #[serde(default)]
    pub wound_healing_taken_turns: i32,
    #[serde(default)]
    pub delayed_damage_ticks: Vec<BattleDelayedDamageTick>,
    #[serde(default)]
    pub delayed_healing_ticks: Vec<BattleDelayedHealingTick>,
    #[serde(default)]
    pub corrosion_stacks: Vec<BattleCorrosionStack>,
    #[serde(default)]
    pub damage_taken_this_turn: f32,
    #[serde(default)]
    pub healing_taken_this_turn: f32,
    #[serde(default)]
    pub skill_last_used_turns: HashMap<String, u32>,
    #[serde(default)]
    pub skill_cooldown_ready_turns: HashMap<String, u32>,
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BattleCorrosionKind {
    Poison,
    Rust,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BattleCorrosionStack {
    pub source_id: String,
    pub source_name: String,
    pub kind: BattleCorrosionKind,
    pub damage_per_turn: f32,
    pub turns_remaining: u32,
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

/// 「镜像外衣」：每层隐身持续 1 个战斗轮。
pub const MIRROR_COAT_LAYERS_PER_MP_UNIT: f32 = 5.0;
/// 「镜像外衣」：除保命自带的一层外，最多用魔法值叠加的额外层数（15 魔法值 / 5 = 3）。
pub const MIRROR_COAT_MAX_EXTRA_LAYERS: u32 = 3;
/// 「镜像外衣」冷却回合数。
pub const MIRROR_COAT_COOLDOWN_ROUNDS: u32 = 6;

/// 「日薄崦嵫」距离减免的起始距离（码）。
pub const SUNSET_DAMAGE_DISTANCE_THRESHOLD: f32 = 10.0;
/// 「日薄崦嵫」超出起始距离后每码减免的伤害点数。
pub const SUNSET_DAMAGE_REDUCTION_PER_YARD: f32 = 1.0;
/// 「日薄崦嵫」单次伤害减免上限比例。
pub const SUNSET_DAMAGE_REDUCTION_CAP: f32 = 0.20;
/// 「日薄崦嵫」死亡后重置的世界时间（18:00 的分钟数）。
pub const SUNSET_TIME_RESET_MINUTES: u32 = 18 * 60;

/// 「精美烧鹅」引导回合数。
pub const GOOSE_CHANNEL_TURNS: u32 = 5;
/// 「精美烧鹅」每回合回复的最大生命/魔法比例。
pub const GOOSE_CHANNEL_HEAL_RATE: f32 = 0.20;

/// 「复仇之魂」：造成有效治疗后，治疗者获得治疗量一半的战斗护盾，最多 5 点。
pub const REVENGE_SOUL_SHIELD_RATE: f32 = 0.50;
pub const REVENGE_SOUL_SHIELD_CAP: f32 = 5.0;
const CORROSION_WAVE_POISON_DAMAGE: f32 = 3.0;
const CORROSION_WAVE_POISON_TURNS: u32 = 4;
const CORROSION_WAVE_RUST_DAMAGE: f32 = 6.0;
const CORROSION_WAVE_RUST_TURNS: u32 = 2;
const CORROSION_WAVE_STACK_MULTIPLIER: f32 = 0.75;

fn trpg_group_campaign_id(group: &TrpgGroup) -> &str {
    let campaign_id = group.campaign_id.trim();
    if campaign_id.is_empty() {
        "default"
    } else {
        campaign_id
    }
}

fn default_true() -> bool { true }

fn default_combat_modifier() -> f32 { 1.0 }

fn default_participant_level() -> i32 { 1 }

fn character_revenge_soul_shield_rate(character: &PlayerCharacter) -> f32 {
    character
        .skill_names
        .iter()
        .enumerate()
        .any(|(index, name)| {
            name.trim() == "复仇之魂"
                && character
                    .skill_metadata
                    .get(index)
                    .is_some_and(|metadata| metadata.is_approved())
                && character.skill_notes.get(index).is_some_and(|note| {
                    note.contains("治疗数值一半的护盾") && note.contains("最多5点")
                })
        })
        .then_some(REVENGE_SOUL_SHIELD_RATE)
        .unwrap_or(0.0)
}

fn character_has_corrosion_wave(character: &PlayerCharacter) -> bool {
    character
        .skill_notes
        .iter()
        .enumerate()
        .any(|(index, note)| {
            character
                .skill_metadata
                .get(index)
                .is_some_and(|metadata| metadata.is_approved())
                && (character
                    .skill_names
                    .get(index)
                    .is_some_and(|name| name.trim() == "腐蚀波")
                    || note.contains("腐蚀波"))
                && note.contains("每次攻击后")
                && note.contains("所有敌人")
                && note.contains("中毒")
                && note.contains("锈蚀")
        })
}

fn participant_is_player_side(
    participant: &BattleParticipantSnapshot,
    manager: &NapcatMessageManager,
) -> bool {
    participant.player_character
        || participant
            .summon_owner_id
            .as_deref()
            .is_some_and(|owner_id| manager.player_characters.contains_key(owner_id))
}

fn participant_is_mechanical(
    participant: &BattleParticipantSnapshot,
    manager: &NapcatMessageManager,
) -> bool {
    if participant.summon_kind == SummonKind::Mech {
        return true;
    }
    participant
        .unit_template_id
        .as_deref()
        .and_then(|unit_id| manager.unit_pool.get(unit_id))
        .is_some_and(|unit| {
            unit.category.contains("机械")
                || unit.label.contains("机械")
                || unit.note.contains("机械")
        })
        || participant.display_name.contains("机械")
}

fn apply_corrosion_wave_after_attack(
    encounter: &mut BattleEncounter,
    actor_id: &str,
    actor_character: Option<&PlayerCharacter>,
    manager: &NapcatMessageManager,
) -> usize {
    if !encounter.active || !actor_character.is_some_and(character_has_corrosion_wave) {
        return 0;
    }
    let Some(actor) = encounter
        .participants
        .iter()
        .find(|participant| participant.target_id == actor_id)
    else {
        return 0;
    };
    let actor_name = actor.display_name.clone();
    let actor_player_side = participant_is_player_side(actor, manager);
    let mut logs = Vec::new();
    for target in &mut encounter.participants {
        if !target.alive
            || target.target_id == actor_id
            || participant_is_player_side(target, manager) == actor_player_side
        {
            continue;
        }
        let kind = if participant_is_mechanical(target, manager) {
            BattleCorrosionKind::Rust
        } else {
            BattleCorrosionKind::Poison
        };
        let active_layers = target
            .corrosion_stacks
            .iter()
            .filter(|stack| stack.source_id == actor_id)
            .count();
        let (base_damage, turns, label) = match kind {
            BattleCorrosionKind::Poison => (
                CORROSION_WAVE_POISON_DAMAGE,
                CORROSION_WAVE_POISON_TURNS,
                "中毒",
            ),
            BattleCorrosionKind::Rust => (
                CORROSION_WAVE_RUST_DAMAGE,
                CORROSION_WAVE_RUST_TURNS,
                "锈蚀",
            ),
        };
        let damage_per_turn = if active_layers == 0 {
            base_damage
        } else {
            base_damage * CORROSION_WAVE_STACK_MULTIPLIER
        };
        target.corrosion_stacks.push(BattleCorrosionStack {
            source_id: actor_id.to_owned(),
            source_name: actor_name.clone(),
            kind,
            damage_per_turn,
            turns_remaining: turns,
        });
        logs.push(format!(
            "{}触发腐蚀波，使{}获得第{}层{}（每回合{}点物理伤害，持续{}回合）",
            actor_name,
            target.display_name,
            active_layers + 1,
            label,
            format_number(damage_per_turn),
            turns
        ));
    }
    let applied = logs.len();
    encounter.action_log.extend(logs);
    applied
}

fn grant_participant_revenge_soul_shield(
    participant: &mut BattleParticipantSnapshot,
    effective_healing: f32,
) -> f32 {
    let previous = participant.revenge_soul_shield.max(0.0);
    if participant.revenge_soul_shield_rate <= f32::EPSILON || effective_healing <= f32::EPSILON {
        return 0.0;
    }
    participant.revenge_soul_shield = (previous
        + effective_healing.max(0.0) * participant.revenge_soul_shield_rate)
        .min(REVENGE_SOUL_SHIELD_CAP);
    (participant.revenge_soul_shield - previous).max(0.0)
}

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

#[derive(Clone, Copy, Debug, Default)]
struct BattleHealingResolution {
    hp_restored: f32,
    shield_gained: f32,
}

impl BattleHealingResolution {
    fn effective_amount(self) -> f32 { self.hp_restored + self.shield_gained }
}

fn apply_participant_healing_for_battle(
    participant: &mut BattleParticipantSnapshot,
    amount: f32,
    overhealing_shield_cap_rate: f32,
) -> BattleHealingResolution {
    let amount = amount.max(0.0);
    if amount <= f32::EPSILON {
        return BattleHealingResolution::default();
    }
    let missing_hp = (participant.max_hp - participant.hp).max(0.0);
    let applied_healing = amount.min(missing_hp);
    participant.hp = (participant.hp + applied_healing).min(participant.max_hp);
    participant.alive = participant.hp > 0.0;

    let overhealing = (amount - applied_healing).max(0.0);
    let shield_cap = participant.max_hp.max(0.0) * overhealing_shield_cap_rate.max(0.0);
    let previous_shield = participant.overhealing_shield.max(0.0);
    if overhealing > f32::EPSILON && shield_cap > f32::EPSILON {
        participant.overhealing_shield = (previous_shield + overhealing).min(shield_cap);
        participant.overhealing_shield_turns_remaining = 2;
    }
    let resolution = BattleHealingResolution {
        hp_restored: applied_healing,
        shield_gained: (participant.overhealing_shield - previous_shield).max(0.0),
    };
    record_participant_healing_taken(
        participant,
        resolution.effective_amount(),
    );
    resolution
}

fn set_encounter_active_state(encounter: &mut BattleEncounter, active: bool) -> bool {
    if encounter.active == active {
        return false;
    }

    if active {
        encounter.combat_log_start = encounter.combat_log.len();
        encounter.combat_completed_turns = 0;
        let mut logs = Vec::new();
        let mut combat_entries = Vec::new();
        for participant in &mut encounter.participants {
            participant.combat_turns_completed = 0;
            participant.combat_damage_taken_total = 0.0;
            participant.damage_contributors.clear();
            participant.damage_contribution_amounts.clear();
            participant.arrogance_damage_source_ids.clear();
            participant.endless_pain_stacks = 0;
            participant.infinite_focus_target_id = None;
            participant.infinite_focus_stacks = 0;
            participant.one_heart_target_id = None;
            participant.one_heart_stacks = 0;
            participant.inspiration_target_id = None;
            participant.inspiration_sources.clear();
            participant.keen_evasion_available = participant.keen_evasion_enabled;
            participant.hidden_role.free_action_available =
                participant.hidden_role.free_action_skill.is_some();
            participant.hidden_role.second_wind_used = false;
            participant.undying_rage_used = false;
            participant.undying_rage_active = false;
            participant.hope_avatar_used = false;
            participant.hope_avatar_rounds_remaining = 0;
            participant.mirror_coat_layers = 0;
            participant.mirror_coat_cooldown_remaining = 0;
            participant.mirror_coat_cleanup_pending = false;
            participant.sunset_death_time_reset_pending = false;
            participant.goose_channeling_turns = 0;
            participant.corrosion_stacks.clear();
            participant.arcane_shield =
                participant.max_mp.max(0.0) * participant.arcane_shield_rate.max(0.0);
            participant.revenge_soul_shield = 0.0;
            let previous_hp = participant.hp;
            if let Some(log) = apply_participant_rest_then_fight_healing(participant) {
                logs.push(log);
                let effective_amount = (participant.hp - previous_hp).max(0.0);
                combat_entries.push(CombatLogEntry {
                    round: encounter.round,
                    kind: CombatLogKind::Healing,
                    source_id: participant.target_id.clone(),
                    source_name: participant.display_name.clone(),
                    target_id: participant.target_id.clone(),
                    target_name: participant.display_name.clone(),
                    action_name: "以逸待劳".to_owned(),
                    base_amount: effective_amount,
                    effective_amount,
                    modifiers: Vec::new(),
                    benefits: Vec::new(),
                });
            }
        }
        encounter.action_log.extend(logs);
        encounter.combat_log.extend(combat_entries);
    } else {
        encounter.combat_completed_turns = 0;
        let mut logs = Vec::new();
        let mut combat_entries = Vec::new();
        let mut defeat_outcomes = Vec::new();
        for participant in &mut encounter.participants {
            participant.combat_turns_completed = 0;
            participant.keen_evasion_available = false;
            participant.hidden_role.free_action_available = false;
            participant.hidden_role.second_wind_used = false;
            participant.undying_rage_active = false;
            participant.arcane_shield = 0.0;
            participant.revenge_soul_shield = 0.0;
            participant.arrogance_damage_source_ids.clear();
            participant.endless_pain_stacks = 0;
            participant.infinite_focus_target_id = None;
            participant.infinite_focus_stacks = 0;
            participant.one_heart_target_id = None;
            participant.one_heart_stacks = 0;
            participant.inspiration_target_id = None;
            participant.inspiration_sources.clear();
            participant.mirror_coat_layers = 0;
            participant.mirror_coat_cooldown_remaining = 0;
            participant.mirror_coat_cleanup_pending = false;
            participant.sunset_death_time_reset_pending = false;
            participant.goose_channeling_turns = 0;
            participant.corrosion_stacks.clear();
            if participant_hope_avatar_active(participant) {
                let was_alive = participant.alive;
                participant.hp = 0.0;
                participant.alive = false;
                participant.hope_avatar_rounds_remaining = 0;
                logs.push(format!(
                    "{}的希望化身随战斗结束，角色死亡",
                    participant.display_name
                ));
                if let Some(outcome) = participant_defeat_outcome(participant, was_alive, None) {
                    defeat_outcomes.push(outcome);
                }
            }
            let healing = participant.combat_damage_taken_total.max(0.0)
                * participant.calm_heart_healing_rate.max(0.0);
            participant.combat_damage_taken_total = 0.0;
            if !participant.alive || healing <= f32::EPSILON {
                continue;
            }
            let shield_cap_rate = participant.overhealing_shield_cap_rate;
            let resolution =
                apply_participant_healing_for_battle(participant, healing, shield_cap_rate);
            let effective_amount = resolution.effective_amount();
            let mut benefits = Vec::new();
            if resolution.shield_gained > f32::EPSILON {
                benefits.push(format!(
                    "过量治疗转化{}点护盾",
                    format_number(resolution.shield_gained)
                ));
            }
            logs.push(format!(
                "{}触发息心，回复{}点生命值",
                participant.display_name,
                format_number(effective_amount)
            ));
            combat_entries.push(CombatLogEntry {
                round: encounter.round,
                kind: CombatLogKind::Healing,
                source_id: participant.target_id.clone(),
                source_name: participant.display_name.clone(),
                target_id: participant.target_id.clone(),
                target_name: participant.display_name.clone(),
                action_name: "息心".to_owned(),
                base_amount: healing,
                effective_amount,
                modifiers: Vec::new(),
                benefits,
            });
        }
        encounter.action_log.extend(logs);
        encounter.combat_log.extend(combat_entries);
        encounter.active = false;
        for outcome in defeat_outcomes {
            apply_battle_defeat_outcome(encounter, outcome);
        }
        for participant in &mut encounter.participants {
            participant.damage_contributors.clear();
            participant.damage_contribution_amounts.clear();
        }
    }
    encounter.active = active;
    true
}

fn clear_participant_dominion_bonus(participant: &mut BattleParticipantSnapshot) {
    let bonus = participant.dominion_max_hp_bonus.max(0.0);
    if bonus <= f32::EPSILON {
        return;
    }
    participant.max_hp = (participant.max_hp - bonus).max(0.0);
    participant.hp = participant.hp.min(participant.max_hp);
    participant.dominion_max_hp_bonus = 0.0;
}

/// 移除角色身上会造成伤害的有害buff（伤害/固定伤害类持续效果）。
fn remove_character_damage_dealing_buffs(character: &mut PlayerCharacter) -> usize {
    let before = character.active_buffs.len();
    character.active_buffs.retain(|buff| {
        buff.beneficial
            || !buff.tick_actions.iter().any(|action| {
                matches!(
                    action,
                    BuffTickAction::Damage { .. } | BuffTickAction::FixedDamage { .. }
                )
            })
    });
    before - character.active_buffs.len()
}

/// 结团时清空某个TRPG组所有战斗轮内「役于我手」的剩余生命上限加成。
pub fn clear_trpg_group_dominion_bonuses(store: &mut BattleRoundStore, group_name: &str) -> usize {
    let mut cleared = 0;
    for encounter in store.encounters.values_mut() {
        if encounter.trpg_group.as_deref() != Some(group_name) {
            continue;
        }
        for participant in &mut encounter.participants {
            let before = participant.dominion_max_hp_bonus.max(0.0);
            clear_participant_dominion_bonus(participant);
            if before > f32::EPSILON {
                cleared += 1;
            }
        }
    }
    cleared
}

/// 判断某个玩家角色当前是否处于本团/本活动的激活战斗轮中。
pub fn character_in_active_encounter(
    store: &BattleRoundStore,
    group_name: &str,
    campaign_id: &str,
    target_id: &str,
) -> bool {
    store.encounters.values().any(|encounter| {
        encounter.active
            && (encounter.trpg_group.as_deref() == Some(group_name)
                || encounter.trpg_campaign_id.as_deref() == Some(campaign_id))
            && encounter
                .participants
                .iter()
                .any(|participant| participant.target_id == target_id)
    })
}

fn advance_participant_rest_then_fight(participant: &mut BattleParticipantSnapshot) {
    if participant.alive && participant.rest_then_fight_healing_rate > f32::EPSILON {
        participant.rest_then_fight_turns =
            participant.rest_then_fight_turns.saturating_add(1).min(10);
    }
}

fn apply_participant_rest_then_fight_healing(
    participant: &mut BattleParticipantSnapshot,
) -> Option<String> {
    let turns = std::mem::take(&mut participant.rest_then_fight_turns).min(10);
    if !participant.alive || turns == 0 || participant.rest_then_fight_healing_rate <= f32::EPSILON
    {
        return None;
    }
    let healing = participant.max_hp.max(0.0)
        * (participant.rest_then_fight_healing_rate.max(0.0) * turns as f32).min(0.50);
    let previous_hp = participant.hp;
    participant.hp = (participant.hp + healing).min(participant.max_hp);
    let restored = (participant.hp - previous_hp).max(0.0);
    (restored > f32::EPSILON).then(|| {
        format!(
            "{}触发以逸待劳，回复{}点生命值",
            participant.display_name,
            format_number(restored)
        )
    })
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
    amount: f32,
) {
    let amount = amount.max(0.0);
    if source_id.trim().is_empty() || participant.target_id == source_id || amount <= f32::EPSILON {
        return;
    }
    *participant
        .damage_contribution_amounts
        .entry(source_id.to_owned())
        .or_default() += amount;
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

fn skill_effects_allow_selected_target(
    effects: &[SkillEffect],
    target_class: Option<&str>,
    selected_target_alive: Option<bool>,
) -> bool {
    if effects.is_empty()
        || matches!(
            target_class.map(str::trim),
            Some("无目标" | "范围")
        )
    {
        return true;
    }

    effects.iter().all(|effect| {
        let (target, healing) = match effect {
            SkillEffect::Damage { target, .. } => (*target, false),
            SkillEffect::Heal { target, .. } => (*target, true),
            SkillEffect::GrantBuff { target, .. } => (*target, false),
        };
        matches!(target.actor, ActorRef::SelfActor)
            || target.area.is_some()
            || selected_target_alive
                .map(|alive| alive || healing)
                .unwrap_or(false)
    })
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
    encounter_active: bool,
) -> bool {
    if amount <= f32::EPSILON
        || !encounter_active
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
    round: u32,
) -> Option<(String, CombatLogEntry)> {
    if !participant.alive || participant.liquid_body_self_healing_rate <= f32::EPSILON {
        return None;
    }
    let healing = previous_damage_taken.max(0.0) * participant.liquid_body_self_healing_rate;
    if healing <= f32::EPSILON {
        return None;
    }
    let shield_cap_rate = participant.overhealing_shield_cap_rate;
    let resolution = apply_participant_healing_for_battle(participant, healing, shield_cap_rate);
    let effective_amount = resolution.effective_amount();
    let mut benefits = Vec::new();
    if resolution.shield_gained > f32::EPSILON {
        benefits.push(format!(
            "过量治疗转化{}点护盾",
            format_number(resolution.shield_gained)
        ));
    }
    Some((
        format!(
            "{}触发液态躯体，回复{}点生命值",
            participant.display_name,
            format_number(effective_amount)
        ),
        CombatLogEntry {
            round,
            kind: CombatLogKind::Healing,
            source_id: participant.target_id.clone(),
            source_name: participant.display_name.clone(),
            target_id: participant.target_id.clone(),
            target_name: participant.display_name.clone(),
            action_name: "液态躯体".to_owned(),
            base_amount: healing,
            effective_amount,
            modifiers: Vec::new(),
            benefits,
        },
    ))
}

struct BattleDefeatOutcome {
    contributors: Vec<String>,
    contribution_amounts: HashMap<String, f32>,
    killer_id: Option<String>,
    defeated_id: String,
    defeated_player_character: bool,
    defeated_level: i32,
    defeated_max_hp: f32,
    defeated_base_damage: f32,
    defeated_rarity: UnitRarity,
}

struct BattleDamageResolution {
    damage_applied: f32,
    damage_absorbed: f32,
    undying_rage_triggered: bool,
    hope_avatar_triggered: bool,
    mirror_coat_triggered: bool,
    hope_avatar_immune: bool,
    defeat_outcome: Option<BattleDefeatOutcome>,
}

fn participant_defeat_outcome(
    participant: &mut BattleParticipantSnapshot,
    was_alive: bool,
    killer_id: Option<&str>,
) -> Option<BattleDefeatOutcome> {
    if !was_alive || participant.alive {
        return None;
    }
    let contributors = std::mem::take(&mut participant.damage_contributors);
    let contribution_amounts = std::mem::take(&mut participant.damage_contribution_amounts);
    let killer_id = killer_id
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| contributors.last().cloned());
    Some(BattleDefeatOutcome {
        contributors,
        contribution_amounts,
        killer_id,
        defeated_id: participant.target_id.clone(),
        defeated_player_character: participant.player_character,
        defeated_level: participant.level.max(1),
        defeated_max_hp: participant.max_hp,
        defeated_base_damage: participant.base_damage,
        defeated_rarity: participant.unit_rarity,
    })
}

fn apply_participant_damage_for_battle(
    participant: &mut BattleParticipantSnapshot,
    amount: f32,
    source_id: &str,
    encounter_active: bool,
) -> BattleDamageResolution {
    let incoming_amount = amount.max(0.0);
    if encounter_active && participant_hope_avatar_active(participant) {
        return BattleDamageResolution {
            damage_applied: 0.0,
            damage_absorbed: incoming_amount,
            undying_rage_triggered: false,
            hope_avatar_triggered: false,
            mirror_coat_triggered: false,
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
    let available_revenge_soul_shield =
        if encounter_active { participant.revenge_soul_shield.max(0.0) } else { 0.0 };
    let revenge_soul_absorbed = available_revenge_soul_shield.min(after_overhealing_shield);
    participant.revenge_soul_shield = available_revenge_soul_shield - revenge_soul_absorbed;
    let after_revenge_soul_shield = (after_overhealing_shield - revenge_soul_absorbed).max(0.0);
    let available_construct_shield = participant.construct_shield.max(0.0);
    let construct_absorbed = available_construct_shield.min(after_revenge_soul_shield);
    participant.construct_shield = available_construct_shield - construct_absorbed;
    if available_construct_shield > f32::EPSILON
        && participant.construct_shield <= f32::EPSILON
        && participant.construct_shield_max > f32::EPSILON
    {
        participant.construct_shield = 0.0;
        participant.construct_shield_repair_rounds_remaining = 2;
    }
    let after_construct_shield = (after_revenge_soul_shield - construct_absorbed).max(0.0);
    let available_shield = if encounter_active { participant.arcane_shield.max(0.0) } else { 0.0 };
    let absorbed = available_shield.min(after_construct_shield);
    participant.arcane_shield = available_shield - absorbed;
    let mut final_amount = (after_construct_shield - absorbed).max(0.0);
    let mut undying_rage_triggered = false;
    let mut hope_avatar_triggered = false;
    let mut mirror_coat_triggered = false;
    let within_undying_rage_limit =
        participant.max_hp > f32::EPSILON && final_amount <= participant.max_hp + f32::EPSILON;
    if encounter_active && participant.undying_rage_active && within_undying_rage_limit {
        final_amount = 0.0;
    } else if encounter_active
        && participant.undying_rage_enabled
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
            mirror_coat_triggered,
            hope_avatar_immune: false,
            defeat_outcome: None,
        };
    }
    let was_alive = participant.alive;
    let previous_hp = participant.hp.max(0.0);
    let damage_applied = final_amount.min(previous_hp);
    record_participant_damage_taken(participant, damage_applied);
    if encounter_active {
        participant.combat_damage_taken_total += damage_applied;
    }
    if was_alive && damage_applied > f32::EPSILON {
        record_participant_damage_contributor(participant, source_id, damage_applied);
        if encounter_active {
            record_participant_arrogance_damage_source(participant, source_id);
            record_participant_endless_pain_stack(participant);
        }
    }
    participant.hp = (previous_hp - damage_applied).max(0.0);
    participant.alive = participant.hp > 0.0;
    if encounter_active
        && participant.alive
        && !participant.hidden_role.second_wind_used
        && participant.hidden_role.second_wind_heal > f32::EPSILON
        && participant.hp <= participant.max_hp * 0.5 + f32::EPSILON
    {
        participant.hp =
            (participant.hp + participant.hidden_role.second_wind_heal).min(participant.max_hp);
        participant.hidden_role.second_wind_used = true;
    }
    if participant.mirror_coat_layers > 0 && damage_applied > f32::EPSILON {
        // 隐身状态下再次受到伤害会立刻脱离隐身并回到战斗。
        participant.mirror_coat_layers = 0;
    }
    if participant.goose_channeling_turns > 0 && damage_applied > f32::EPSILON {
        // 食用烧鹅的引导被伤害打断，烧鹅消失。
        participant.goose_channeling_turns = 0;
    }
    if participant.construct_repair_channel_rounds_remaining > 0 && damage_applied > f32::EPSILON {
        participant.construct_repair_channel_rounds_remaining = 0;
    }
    if encounter_active
        && !participant.alive
        && participant.mirror_coat_enabled
        && participant.mirror_coat_cooldown_remaining == 0
    {
        // 镜像外衣：致命伤害改为移除伤害性有害buff、生命值变为1并获得镜像外衣。
        participant.hp = 1.0;
        participant.alive = true;
        let extra = ((participant.mp.max(0.0) / MIRROR_COAT_LAYERS_PER_MP_UNIT).floor() as u32)
            .min(MIRROR_COAT_MAX_EXTRA_LAYERS);
        participant.mirror_coat_layers = 1 + extra;
        participant.mp = (participant.mp - extra as f32 * MIRROR_COAT_LAYERS_PER_MP_UNIT).max(0.0);
        participant.mirror_coat_cooldown_remaining = MIRROR_COAT_COOLDOWN_ROUNDS;
        participant.mirror_coat_cleanup_pending = true;
        mirror_coat_triggered = true;
    }
    if encounter_active
        && !participant.alive
        && participant.hope_avatar_enabled
        && !participant.hope_avatar_used
    {
        participant.alive = true;
        participant.hope_avatar_used = true;
        participant.hope_avatar_rounds_remaining = 2;
        hope_avatar_triggered = true;
    }
    BattleDamageResolution {
        damage_applied,
        damage_absorbed: (incoming_amount - final_amount).max(0.0),
        undying_rage_triggered,
        hope_avatar_triggered,
        mirror_coat_triggered,
        hope_avatar_immune: false,
        defeat_outcome: participant_defeat_outcome(participant, was_alive, Some(source_id)),
    }
}

fn is_current_redeemed_void_break(skill: &CharacterSkill) -> bool {
    skill.name.trim() == "玄空破"
        && skill.note.contains("从一个4米内的可视目标内部")
        && skill.note.contains("最多优先将生命值降低为1")
        && skill.note.contains("爆炸造成7点物理伤害")
}

fn is_current_redeemed_commissar_war_cry(skill: &CharacterSkill) -> bool {
    skill.name.trim() == "政委"
        && skill.note.contains("为了帝皇")
        && skill.note.contains("熟练度")
        && skill.note.contains("每次使用前熟练度+1")
        && skill.note.contains("下次攻击伤害附带")
        && skill.note.contains("物理伤害")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProtectiveSuitAction {
    MedicalInjection,
    PhotonFlash,
    CryogenicRelease,
    AcidVial,
    AcidDoor,
    MaintenanceSaw,
    MaintenanceDoor,
}

fn apply_participant_typed_damage_for_battle(
    participant: &mut BattleParticipantSnapshot,
    amount: f32,
    source_id: &str,
    encounter_active: bool,
    damage_type: DamageType,
) -> BattleDamageResolution {
    let incoming_amount = amount.max(0.0);
    let suit_can_absorb = participant.protective_suit_passive_active
        && participant.protective_suit_shield > f32::EPSILON
        && (!participant.protective_suit_magic_only || damage_type == DamageType::Magical);
    let suit_absorbed = if suit_can_absorb {
        participant.protective_suit_shield.min(incoming_amount)
    } else {
        0.0
    };
    participant.protective_suit_shield =
        (participant.protective_suit_shield - suit_absorbed).max(0.0);
    if suit_can_absorb && participant.protective_suit_shield <= f32::EPSILON {
        participant.protective_suit_shield = 0.0;
        if participant.protective_suit_kind != Some(ProtectiveSuitKind::Radiation) {
            participant.protective_suit_passive_active = false;
        }
        if participant.protective_suit_speed_bonus > f32::EPSILON {
            participant.speed =
                (participant.speed - participant.protective_suit_speed_bonus).max(0.0);
            participant.low_survivor_speed =
                (participant.low_survivor_speed - participant.protective_suit_speed_bonus)
                    .max(0.0);
            participant.protective_suit_speed_bonus = 0.0;
        }
    }
    let mut resolution = apply_participant_damage_for_battle(
        participant,
        (incoming_amount - suit_absorbed).max(0.0),
        source_id,
        encounter_active,
    );
    resolution.damage_absorbed += suit_absorbed;
    resolution
}

fn apply_participant_shield_piercing_damage_for_battle(
    participant: &mut BattleParticipantSnapshot,
    amount: f32,
    source_id: &str,
    encounter_active: bool,
) -> BattleDamageResolution {
    let saved_suit_shield = participant.protective_suit_shield;
    let saved_overhealing_shield = participant.overhealing_shield;
    let saved_overhealing_turns = participant.overhealing_shield_turns_remaining;
    let saved_revenge_soul_shield = participant.revenge_soul_shield;
    let saved_construct_shield = participant.construct_shield;
    let saved_construct_repair = participant.construct_shield_repair_rounds_remaining;
    let saved_arcane_shield = participant.arcane_shield;
    participant.protective_suit_shield = 0.0;
    participant.overhealing_shield = 0.0;
    participant.overhealing_shield_turns_remaining = 0;
    participant.revenge_soul_shield = 0.0;
    participant.construct_shield = 0.0;
    participant.arcane_shield = 0.0;
    let resolution =
        apply_participant_damage_for_battle(participant, amount, source_id, encounter_active);
    participant.protective_suit_shield = saved_suit_shield;
    participant.overhealing_shield = saved_overhealing_shield;
    participant.overhealing_shield_turns_remaining = saved_overhealing_turns;
    participant.revenge_soul_shield = saved_revenge_soul_shield;
    participant.construct_shield = saved_construct_shield;
    participant.construct_shield_repair_rounds_remaining = saved_construct_repair;
    participant.arcane_shield = saved_arcane_shield;
    resolution
}

fn is_current_redeemed_chainsword(skill: &CharacterSkill) -> bool {
    skill.name.trim() == "链锯剑"
        && skill.note.contains("近身攻击会造成5点物理伤害")
        && skill.note.contains("撕裂")
        && skill.note.contains("抑制目标的自然生命回复效果")
}

fn is_current_redeemed_bolter(skill: &CharacterSkill) -> bool {
    skill.note.contains("爆失枪（1分）")
        && skill.note.contains("只能在6米内造成1点物理伤害")
        && skill
            .note
            .contains("每次开枪都会广播给周围玩家这把枪的描述")
}

fn is_current_redeemed_flying_needle(skill: &CharacterSkill) -> bool {
    skill.name.trim() == "飞针"
        && skill.note.contains("常态具备5根飞针")
        && skill.note.contains("每根飞针造成1点远程物理伤害")
        && skill.note.contains("射程6米无距离衰减")
}

fn is_current_redeemed_blow_needles(skill: &CharacterSkill) -> bool {
    skill.name.trim() == "吹针术"
        && skill.note.contains("一次性射出当前持有的所有飞针")
        && skill.note.contains("命中的飞针数量+2的远程物理伤害")
}

fn current_redeemed_invisibility_rounds(skill: &CharacterSkill) -> Option<u32> {
    if skill.name.trim() != "隐身术"
        || !skill.note.contains("冷却2")
        || !skill.note.contains("无消耗")
    {
        return None;
    }
    let duration = skill
        .note
        .split_once("持续")?
        .1
        .chars()
        .skip_while(|character| !character.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse::<u32>()
        .ok()?;
    (duration > 0).then_some(duration)
}

fn skill_has_dedicated_battle_resolution(skill: &CharacterSkill) -> bool {
    is_current_redeemed_commissar_war_cry(skill)
        || is_current_redeemed_blow_needles(skill)
        || current_redeemed_invisibility_rounds(skill).is_some()
        || matches!(
            skill.name.as_str(),
            "食用精美烧鹅" | "维修机甲"
        )
}

fn apply_void_break_damage_for_battle(
    participant: &mut BattleParticipantSnapshot,
    amount: f32,
    source_id: &str,
    encounter_active: bool,
) -> BattleDamageResolution {
    let incoming_amount = amount.max(0.0);
    let hp_first_amount = incoming_amount.min((participant.hp.max(0.0) - 1.0).max(0.0));
    let saved_overhealing_shield = participant.overhealing_shield;
    let saved_overhealing_turns = participant.overhealing_shield_turns_remaining;
    let saved_revenge_soul_shield = participant.revenge_soul_shield;
    let saved_arcane_shield = participant.arcane_shield;
    participant.overhealing_shield = 0.0;
    participant.overhealing_shield_turns_remaining = 0;
    participant.revenge_soul_shield = 0.0;
    participant.arcane_shield = 0.0;

    let mut resolution = apply_participant_damage_for_battle(
        participant,
        hp_first_amount,
        source_id,
        encounter_active,
    );
    participant.overhealing_shield = saved_overhealing_shield;
    participant.overhealing_shield_turns_remaining = saved_overhealing_turns;
    participant.revenge_soul_shield = saved_revenge_soul_shield;
    participant.arcane_shield = saved_arcane_shield;

    if resolution.damage_applied + f32::EPSILON < hp_first_amount {
        resolution.damage_absorbed = incoming_amount;
        return resolution;
    }

    let mut remaining = (incoming_amount - resolution.damage_applied).max(0.0);
    let overhealing_absorbed = participant.overhealing_shield.max(0.0).min(remaining);
    participant.overhealing_shield =
        (participant.overhealing_shield.max(0.0) - overhealing_absorbed).max(0.0);
    remaining = (remaining - overhealing_absorbed).max(0.0);
    if participant.overhealing_shield <= f32::EPSILON {
        participant.overhealing_shield = 0.0;
        participant.overhealing_shield_turns_remaining = 0;
    }

    if encounter_active {
        let revenge_absorbed = participant.revenge_soul_shield.max(0.0).min(remaining);
        participant.revenge_soul_shield =
            (participant.revenge_soul_shield.max(0.0) - revenge_absorbed).max(0.0);
        remaining = (remaining - revenge_absorbed).max(0.0);
        let arcane_absorbed = participant.arcane_shield.max(0.0).min(remaining);
        participant.arcane_shield = (participant.arcane_shield.max(0.0) - arcane_absorbed).max(0.0);
    }

    resolution.damage_absorbed = (incoming_amount - resolution.damage_applied).max(0.0);
    resolution
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
        participant_defeat_outcome(participant, was_alive, None),
    )
}

/// 每轮推进「镜像外衣」：隐身层数递减，冷却回合递减。
fn advance_participant_mirror_coat(participant: &mut BattleParticipantSnapshot) {
    if participant.mirror_coat_layers > 0 {
        participant.mirror_coat_layers -= 1;
    }
    if participant.mirror_coat_cooldown_remaining > 0 {
        participant.mirror_coat_cooldown_remaining -= 1;
    }
}

/// 每轮推进「精美烧鹅」引导：回复20%最大生命/魔法，引导结束或被打断后烧鹅消失。
fn advance_participant_goose_channel(
    participant: &mut BattleParticipantSnapshot,
    round: u32,
) -> Option<(String, CombatLogEntry)> {
    if participant.goose_channeling_turns == 0 {
        return None;
    }
    let turns_left = participant.goose_channeling_turns;
    participant.goose_channeling_turns -= 1;
    let hp_heal = participant.max_hp.max(0.0) * GOOSE_CHANNEL_HEAL_RATE;
    let mp_heal = participant.max_mp.max(0.0) * GOOSE_CHANNEL_HEAL_RATE;
    let previous_hp = participant.hp;
    let previous_mp = participant.mp;
    participant.hp = (participant.hp + hp_heal).min(participant.max_hp);
    participant.mp = (participant.mp + mp_heal).min(participant.max_mp);
    let hp_restored = (participant.hp - previous_hp).max(0.0);
    let mp_restored = (participant.mp - previous_mp).max(0.0);
    let log = format!(
        "{}食用精美烧鹅，回复{}点生命、{}点魔法（引导{}回合）",
        participant.display_name,
        format_number(hp_restored),
        format_number(mp_restored),
        turns_left
    );
    let entry = CombatLogEntry {
        round,
        kind: CombatLogKind::Healing,
        source_id: participant.target_id.clone(),
        source_name: participant.display_name.clone(),
        target_id: participant.target_id.clone(),
        target_name: participant.display_name.clone(),
        action_name: "精美烧鹅".to_owned(),
        base_amount: hp_restored,
        effective_amount: hp_restored,
        modifiers: Vec::new(),
        benefits: vec![format!("魔法回复{}", format_number(mp_restored))],
    };
    Some((log, entry))
}

fn advance_redeemed_construct_state(
    participant: &mut BattleParticipantSnapshot,
    encounter_active: bool,
) -> Vec<String> {
    let mut logs = Vec::new();
    if !encounter_active && participant.construct_shield_repair_rounds_remaining > 0 {
        participant.construct_shield_repair_rounds_remaining -= 1;
        if participant.construct_shield_repair_rounds_remaining == 0 {
            participant.construct_shield = participant.construct_shield_max.max(0.0);
            logs.push(format!(
                "{}的全伤害护盾维修完成，恢复{}点护盾",
                participant.display_name,
                format_number(participant.construct_shield)
            ));
        }
    }
    if participant.construct_repair_channel_rounds_remaining > 0 {
        participant.construct_repair_channel_rounds_remaining -= 1;
        if participant.construct_repair_channel_rounds_remaining == 0 {
            participant.hp = participant.max_hp.max(0.0);
            participant.alive = participant.hp > 0.0;
            logs.push(format!(
                "{}完成维修，生命值完全恢复",
                participant.display_name
            ));
        }
    }
    logs
}

fn advance_redeemed_needles_noncombat(participant: &mut BattleParticipantSnapshot) -> bool {
    if !participant.flying_needles_enabled {
        return false;
    }
    let mut changed = false;
    if participant.flying_needles_ready < 5 {
        participant.flying_needles_ready += 1;
        changed = true;
    }
    if participant.needle_case_enabled && participant.needle_case_ready < 2 {
        participant.needle_case_progress_noncombat_rounds += 1;
        if participant.needle_case_progress_noncombat_rounds >= 2 {
            participant.needle_case_progress_noncombat_rounds = 0;
            participant.needle_case_ready += 1;
        }
        changed = true;
    }
    changed
}

/// 推进「蝴蝶效应」：效果在持有者与指定目标间每回合互换；一方死亡后常驻存活方。
fn sync_butterfly_effects(encounter: &mut BattleEncounter) {
    for index in 0..encounter.participants.len() {
        if !encounter.participants[index].butterfly_enabled {
            continue;
        }
        let Some(target_id) = encounter.participants[index].butterfly_target_id.clone() else {
            continue;
        };
        let Some(target_index) = encounter
            .participants
            .iter()
            .position(|participant| participant.target_id == target_id)
        else {
            continue;
        };
        if encounter.participants[index].butterfly_locked {
            continue;
        }
        let holder_alive = encounter.participants[index].alive;
        let target_alive = encounter.participants[target_index].alive;
        if !holder_alive || !target_alive {
            // 一方死亡：效果常驻存活一方，停止互换。
            let on_holder = holder_alive;
            encounter.participants[index].butterfly_locked = true;
            encounter.participants[index].butterfly_effect_on_holder = on_holder;
            encounter.participants[index].butterfly_initialized = true;
            encounter.participants[index].butterfly_effect = on_holder;
            encounter.participants[target_index].butterfly_effect = !on_holder;
            continue;
        }
        if !encounter.participants[index].butterfly_initialized {
            // 第一轮：指定目标先持有效果。
            encounter.participants[index].butterfly_initialized = true;
            encounter.participants[index].butterfly_effect_on_holder = false;
        } else {
            let next_on_holder = !encounter.participants[index].butterfly_effect_on_holder;
            encounter.participants[index].butterfly_effect_on_holder = next_on_holder;
        }
        let on_holder = encounter.participants[index].butterfly_effect_on_holder;
        encounter.participants[index].butterfly_effect = on_holder;
        encounter.participants[target_index].butterfly_effect = !on_holder;
    }
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
    let mut combat_entries = Vec::new();
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
            let resolution = apply_participant_healing_for_battle(
                participant,
                hp_recovered,
                shield_cap_rate,
            );
            let hp_effective = resolution.effective_amount();
            let mut benefits = Vec::new();
            if resolution.shield_gained > f32::EPSILON {
                benefits.push(format!(
                    "过量治疗转化{}点护盾",
                    format_number(resolution.shield_gained)
                ));
            }
            combat_entries.push(CombatLogEntry {
                round: encounter.round,
                kind: CombatLogKind::Healing,
                source_id: participant.target_id.clone(),
                source_name: participant.display_name.clone(),
                target_id: participant.target_id.clone(),
                target_name: participant.display_name.clone(),
                action_name: "罪上加罪".to_owned(),
                base_amount: hp_recovered,
                effective_amount: hp_effective,
                modifiers: Vec::new(),
                benefits,
            });
        }
        let previous_mp = participant.mp;
        if mp_recovered > f32::EPSILON {
            participant.mp = (participant.mp + mp_recovered).min(participant.max_mp);
            combat_entries.push(CombatLogEntry {
                round: encounter.round,
                kind: CombatLogKind::Resource,
                source_id: participant.target_id.clone(),
                source_name: participant.display_name.clone(),
                target_id: participant.target_id.clone(),
                target_name: participant.display_name.clone(),
                action_name: "罪上加罪·魔法回复".to_owned(),
                base_amount: mp_recovered,
                effective_amount: (participant.mp - previous_mp).max(0.0),
                modifiers: Vec::new(),
                benefits: Vec::new(),
            });
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
    encounter.combat_log.extend(combat_entries);
}

fn grant_participant_experience(participant: &mut BattleParticipantSnapshot, amount: i32) -> i32 {
    if amount <= 0 {
        return 0;
    }
    participant.level = participant.level.max(1);
    participant.exp = participant.exp.saturating_add(amount);
    let mut level_ups = 0;
    while participant.level < 999 {
        let required = character_next_level_exp(participant.level);
        if participant.exp < required {
            break;
        }
        participant.exp -= required;
        participant.level += 1;
        level_ups += 1;
    }
    level_ups
}

fn experience_with_bonus(amount: i32, bonus_rate: f32) -> i32 {
    if amount <= 0 {
        return 0;
    }
    ((amount as f64) * (1.0 + bonus_rate.max(0.0) as f64))
        .round()
        .clamp(1.0, i32::MAX as f64) as i32
}

fn battle_defeat_total_experience(
    encounter: &BattleEncounter,
    outcome: &BattleDefeatOutcome,
) -> i32 {
    let killer_level = outcome
        .killer_id
        .as_deref()
        .and_then(|killer_id| {
            encounter
                .participants
                .iter()
                .find(|participant| participant.target_id == killer_id)
        })
        .map(|participant| participant.level.max(1))
        .unwrap_or(outcome.defeated_level);
    let pve_level_scale = if outcome.defeated_player_character {
        1.0
    } else {
        killer_level as f32 / outcome.defeated_level.max(1) as f32
    };
    let threat = outcome.defeated_max_hp.max(0.0) * pve_level_scale
        + outcome.defeated_base_damage.max(0.0) * pve_level_scale * 5.0
        + outcome.defeated_level.max(1) as f32 * pve_level_scale * 3.0;
    let mode_multiplier = if outcome.defeated_player_character {
        TrpgBasicConfig::default().exp_gain_per_level_pvp
    } else {
        outcome.defeated_rarity.experience_multiplier()
    };
    (threat * mode_multiplier)
        .round()
        .clamp(1.0, i32::MAX as f32) as i32
}

fn apply_battle_experience_reward(encounter: &mut BattleEncounter, outcome: &BattleDefeatOutcome) {
    let Some(killer_id) = outcome.killer_id.as_deref() else {
        return;
    };
    let total_exp = battle_defeat_total_experience(encounter, outcome);
    let player_ids = encounter
        .participants
        .iter()
        .filter(|participant| {
            participant.player_character && participant.target_id != outcome.defeated_id
        })
        .map(|participant| participant.target_id.clone())
        .collect::<HashSet<_>>();
    if !player_ids.contains(killer_id) {
        return;
    }

    let killer_max_hp = encounter
        .participants
        .iter()
        .find(|participant| participant.target_id == killer_id)
        .map(|participant| participant.max_hp.max(1.0))
        .unwrap_or(1.0);
    let mut weights = outcome
        .contribution_amounts
        .iter()
        .filter(|(source_id, amount)| player_ids.contains(*source_id) && **amount > f32::EPSILON)
        .map(|(source_id, amount)| (source_id.clone(), *amount))
        .collect::<HashMap<_, _>>();

    // Healing and beneficial buffs only assist when they were applied to the eventual killer.
    for entry in encounter.combat_log.iter().skip(encounter.combat_log_start) {
        if entry.target_id != killer_id || !player_ids.contains(&entry.source_id) {
            continue;
        }
        let support = match entry.kind {
            CombatLogKind::Healing => entry.effective_amount.max(0.0),
            CombatLogKind::Buff if entry.benefits.iter().any(|benefit| benefit == "有益") => {
                killer_max_hp * 0.05
            },
            _ => 0.0,
        };
        if support > f32::EPSILON {
            *weights.entry(entry.source_id.clone()).or_default() += support;
        }
    }
    // The killing blow matters without overpowering sustained contribution.
    let existing_weight = weights.values().copied().sum::<f32>().max(1.0);
    *weights.entry(killer_id.to_owned()).or_default() += existing_weight * 0.20;
    weights.retain(|source_id, weight| player_ids.contains(source_id) && *weight > f32::EPSILON);
    if weights.is_empty() {
        return;
    }

    let weight_sum = weights.values().copied().sum::<f32>();
    let mut shares = weights
        .into_iter()
        .map(|(source_id, weight)| {
            let exact = total_exp as f32 * weight / weight_sum;
            (
                source_id,
                exact.floor() as i32,
                exact.fract(),
            )
        })
        .collect::<Vec<_>>();
    let assigned = shares.iter().map(|(_, amount, _)| *amount).sum::<i32>();
    let mut remainder = total_exp.saturating_sub(assigned);
    shares.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| left.0.cmp(&right.0))
    });
    for (_, amount, _) in &mut shares {
        if remainder == 0 {
            break;
        }
        *amount += 1;
        remainder -= 1;
    }

    for (source_id, amount, _) in shares {
        if amount <= 0 {
            continue;
        }
        if let Some(participant) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == source_id)
        {
            let base_amount = amount;
            let bonus_rate = participant.support_talent_experience_bonus_rate;
            let amount = experience_with_bonus(base_amount, bonus_rate);
            let bonus_note = if bonus_rate > f32::EPSILON {
                format!(
                    "（辅助天赋经验加成{}%，基础{}）",
                    format_number((bonus_rate * 100.0).round()),
                    base_amount
                )
            } else {
                String::new()
            };
            let level_ups = grant_participant_experience(participant, amount);
            let experience_event = CombatLogEntry {
                round: encounter.round,
                kind: CombatLogKind::Experience,
                source_id: source_id.clone(),
                source_name: participant.display_name.clone(),
                target_id: source_id.clone(),
                target_name: participant.display_name.clone(),
                action_name: "战斗经验".to_owned(),
                base_amount: amount as f32,
                effective_amount: amount as f32,
                modifiers: Vec::new(),
                benefits: (level_ups > 0)
                    .then(|| format!("提升{level_ups}级"))
                    .into_iter()
                    .collect(),
            };
            encounter.action_log.push(if level_ups > 0 {
                format!(
                    "{}获得{}经验{}并提升{}级",
                    participant.display_name, amount, bonus_note, level_ups
                )
            } else {
                format!(
                    "{}获得{}经验{}",
                    participant.display_name, amount, bonus_note
                )
            });
            encounter.combat_log.push(experience_event);
        }
    }
}

fn battle_participant_display_name(encounter: &BattleEncounter, target_id: &str) -> String {
    encounter
        .participants
        .iter()
        .find(|participant| participant.target_id == target_id)
        .map(|participant| participant.display_name.clone())
        .unwrap_or_else(|| target_id.to_owned())
}

fn record_battle_defeat_events(encounter: &mut BattleEncounter, outcome: &BattleDefeatOutcome) {
    let defeated_name = battle_participant_display_name(encounter, &outcome.defeated_id);
    let killer_id = outcome
        .killer_id
        .as_deref()
        .filter(|killer_id| *killer_id != outcome.defeated_id);
    let (killer_source_id, killer_name) = killer_id
        .map(|killer_id| {
            (
                killer_id.to_owned(),
                battle_participant_display_name(encounter, killer_id),
            )
        })
        .unwrap_or_else(|| ("system".to_owned(), "环境".to_owned()));
    encounter.combat_log.push(CombatLogEntry {
        round: encounter.round,
        kind: CombatLogKind::Elimination,
        source_id: killer_source_id,
        source_name: killer_name,
        target_id: outcome.defeated_id.clone(),
        target_name: defeated_name.clone(),
        action_name: "击败".to_owned(),
        base_amount: 1.0,
        effective_amount: 1.0,
        modifiers: Vec::new(),
        benefits: Vec::new(),
    });

    let Some(killer_id) = killer_id else {
        return;
    };
    let mut assistants = HashMap::<String, &'static str>::new();
    for contributor_id in &outcome.contributors {
        if contributor_id != killer_id && contributor_id != &outcome.defeated_id {
            assistants.insert(contributor_id.clone(), "伤害贡献");
        }
    }
    for entry in encounter.combat_log.iter().skip(encounter.combat_log_start) {
        if entry.target_id != killer_id
            || entry.source_id == killer_id
            || entry.source_id == outcome.defeated_id
        {
            continue;
        }
        let support_kind = match entry.kind {
            CombatLogKind::Healing => Some("治疗支援"),
            CombatLogKind::Buff if entry.benefits.iter().any(|benefit| benefit == "有益") => {
                Some("有益状态支援")
            },
            _ => None,
        };
        if let Some(support_kind) = support_kind {
            assistants
                .entry(entry.source_id.clone())
                .or_insert(support_kind);
        }
    }

    let mut assistants = assistants.into_iter().collect::<Vec<_>>();
    assistants.sort_by(|left, right| left.0.cmp(&right.0));
    for (assistant_id, reason) in assistants {
        encounter.combat_log.push(CombatLogEntry {
            round: encounter.round,
            kind: CombatLogKind::Assist,
            source_name: battle_participant_display_name(encounter, &assistant_id),
            source_id: assistant_id,
            target_id: outcome.defeated_id.clone(),
            target_name: defeated_name.clone(),
            action_name: "助攻".to_owned(),
            base_amount: 1.0,
            effective_amount: 1.0,
            modifiers: Vec::new(),
            benefits: vec![reason.to_owned()],
        });
    }
}

fn apply_battle_defeat_outcome(encounter: &mut BattleEncounter, outcome: BattleDefeatOutcome) {
    record_battle_defeat_events(encounter, &outcome);
    apply_battle_experience_reward(encounter, &outcome);
    if encounter.active {
        apply_dominion_target_death(encounter, outcome.defeated_max_hp);
    }
    let contributors = outcome.contributors.into_iter().collect::<HashSet<_>>();
    if !contributors.is_empty() {
        apply_penance_kill_assists(encounter, contributors.iter().cloned());
        apply_sin_on_sin_kill_participation(encounter, &contributors);
    }
    if outcome.defeated_player_character {
        apply_champion_player_elimination(encounter);
    }
    if let Some(defeated) = encounter
        .participants
        .iter_mut()
        .find(|participant| participant.target_id == outcome.defeated_id)
    {
        if defeated.sunset_enabled {
            defeated.sunset_death_time_reset_pending = true;
        }
    }
}

/// 「日薄崦嵫」持有者被击败后，把本团世界时间重置为当天18:00。
fn apply_sunset_world_time_reset(
    encounter: &mut BattleEncounter,
    manager: &mut NapcatMessageManager,
) {
    let mut pending_names = Vec::new();
    for participant in &mut encounter.participants {
        if participant.sunset_death_time_reset_pending {
            participant.sunset_death_time_reset_pending = false;
            pending_names.push(participant.display_name.clone());
        }
    }
    if pending_names.is_empty() {
        return;
    }
    let group_name = encounter.trpg_group.clone().or_else(|| {
        encounter
            .trpg_campaign_id
            .as_deref()
            .and_then(|campaign_id| {
                manager
                    .trpg_groups
                    .iter()
                    .find(|(_, group)| group.campaign_id.trim() == campaign_id)
                    .map(|(name, _)| name.clone())
            })
    });
    let Some(group_name) = group_name else {
        return;
    };
    let Some(group) = manager.trpg_groups.get_mut(&group_name) else {
        return;
    };
    let day_start = (group.world_time_minutes / crate::napcat::WORLD_DAY_MINUTES)
        * crate::napcat::WORLD_DAY_MINUTES;
    group.world_time_minutes = day_start + SUNSET_TIME_RESET_MINUTES;
    encounter.action_log.push(format!(
        "{}触发日薄崦嵫，世界时间重置为晚上6点",
        pending_names.join("、")
    ));
}

fn reset_participant_turn_totals(participant: &mut BattleParticipantSnapshot) -> bool {
    let changed = participant.damage_taken_this_turn.abs() > f32::EPSILON
        || participant.healing_taken_this_turn.abs() > f32::EPSILON;
    participant.damage_taken_this_turn = 0.0;
    participant.healing_taken_this_turn = 0.0;
    changed
}

fn completed_combat_turns(encounter: &BattleEncounter) -> u32 { encounter.combat_completed_turns }

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
    combat_log: Vec<CombatLogEntry>,
    defeat_outcomes: Vec<BattleDefeatOutcome>,
}

fn advance_radiation_protective_suits(
    encounter: &mut BattleEncounter,
    scene_positions: &HashMap<String, Vec3>,
) -> BattleDelayedDamageAdvance {
    if !encounter.active {
        return BattleDelayedDamageAdvance::default();
    }
    let emitters = encounter
        .participants
        .iter()
        .filter(|participant| {
            participant.alive
                && participant.protective_suit_kind == Some(ProtectiveSuitKind::Radiation)
        })
        .map(|participant| {
            (
                participant.target_id.clone(),
                participant.display_name.clone(),
                scene_positions.get(&participant.target_id).copied(),
            )
        })
        .collect::<Vec<_>>();
    let mut events = Vec::new();
    for (source_id, source_name, source_position) in emitters {
        for (target_index, target) in encounter.participants.iter().enumerate() {
            if !target.alive {
                continue;
            }
            let in_range = target.target_id == source_id
                || source_position.is_some_and(|source_position| {
                    scene_positions
                        .get(&target.target_id)
                        .is_some_and(|target_position| {
                            source_position.distance(*target_position) <= 3.0
                        })
                });
            if in_range {
                events.push((
                    source_id.clone(),
                    source_name.clone(),
                    target_index,
                ));
            }
        }
    }

    let mut advance = BattleDelayedDamageAdvance::default();
    for (source_id, source_name, target_index) in events {
        if !encounter.participants[target_index].alive {
            continue;
        }
        let target_id = encounter.participants[target_index].target_id.clone();
        let target_name = encounter.participants[target_index].display_name.clone();
        let resolution = apply_participant_shield_piercing_damage_for_battle(
            &mut encounter.participants[target_index],
            1.0,
            &source_id,
            true,
        );
        advance.combat_log.push(CombatLogEntry {
            round: encounter.round,
            kind: CombatLogKind::Damage,
            source_id: source_id.clone(),
            source_name: source_name.clone(),
            target_id,
            target_name: target_name.clone(),
            action_name: "辐射防护服".to_owned(),
            base_amount: 1.0,
            effective_amount: resolution.damage_applied,
            modifiers: Vec::new(),
            benefits: vec!["穿透护盾".to_owned()],
        });
        advance.logs.push(format!(
            "{}的辐射防护服使{}失去{}点生命值（穿透护盾）",
            source_name,
            target_name,
            format_number(resolution.damage_applied)
        ));
        if let Some(outcome) = resolution.defeat_outcome {
            advance.defeat_outcomes.push(outcome);
        }
    }
    advance
}

fn advance_participant_delayed_damage_ticks(
    participant: &mut BattleParticipantSnapshot,
    encounter_active: bool,
    round: u32,
) -> BattleDelayedDamageAdvance {
    if participant.delayed_damage_ticks.is_empty() {
        return BattleDelayedDamageAdvance::default();
    }
    let mut advance = BattleDelayedDamageAdvance::default();
    let display_name = participant.display_name.clone();
    let mut due = Vec::new();
    participant.delayed_damage_ticks.retain_mut(|tick| {
        // Persisted ticks use 2 before their one execution; older builds left 1 behind after it.
        if tick.turns_remaining <= 0 {
            return false;
        }
        tick.turns_remaining -= 1;
        if tick.turns_remaining > 0 {
            due.push(tick.clone());
        }
        false
    });
    if !participant.alive {
        return advance;
    }
    for tick in due {
        let final_amount = tick.amount.max(0.0);
        if final_amount <= f32::EPSILON {
            continue;
        }
        let resolution = apply_participant_typed_damage_for_battle(
            participant,
            final_amount,
            &tick.source_id,
            encounter_active,
            tick.damage_type,
        );
        let mut benefits = Vec::new();
        if resolution.damage_absorbed > f32::EPSILON {
            benefits.push(format!(
                "护盾/免疫吸收{}点",
                format_number(resolution.damage_absorbed)
            ));
        }
        advance.combat_log.push(CombatLogEntry {
            round,
            kind: CombatLogKind::Damage,
            source_id: tick.source_id.clone(),
            source_name: tick.source_name.clone(),
            target_id: participant.target_id.clone(),
            target_name: display_name.clone(),
            action_name: tick.name.clone(),
            base_amount: final_amount,
            effective_amount: resolution.damage_applied,
            modifiers: Vec::new(),
            benefits,
        });
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

fn advance_participant_corrosion(
    participant: &mut BattleParticipantSnapshot,
    encounter_active: bool,
    round: u32,
) -> BattleDelayedDamageAdvance {
    if !encounter_active || !participant.alive || participant.corrosion_stacks.is_empty() {
        if !encounter_active || !participant.alive {
            participant.corrosion_stacks.clear();
        }
        return BattleDelayedDamageAdvance::default();
    }

    let display_name = participant.display_name.clone();
    let mut advance = BattleDelayedDamageAdvance::default();
    let stacks = std::mem::take(&mut participant.corrosion_stacks);
    let mut remaining = Vec::with_capacity(stacks.len());
    for mut stack in stacks {
        if !participant.alive {
            break;
        }
        let label = match stack.kind {
            BattleCorrosionKind::Poison => "中毒",
            BattleCorrosionKind::Rust => "锈蚀",
        };
        let damage = stack.damage_per_turn.max(0.0);
        let resolution = apply_participant_typed_damage_for_battle(
            participant,
            damage,
            &stack.source_id,
            encounter_active,
            DamageType::Physical,
        );
        let mut benefits = vec![format!("腐蚀波{label}")];
        if resolution.damage_absorbed > f32::EPSILON {
            benefits.push(format!(
                "护盾/免疫吸收{}点",
                format_number(resolution.damage_absorbed)
            ));
        }
        advance.combat_log.push(CombatLogEntry {
            round,
            kind: CombatLogKind::Damage,
            source_id: stack.source_id.clone(),
            source_name: stack.source_name.clone(),
            target_id: participant.target_id.clone(),
            target_name: display_name.clone(),
            action_name: format!("腐蚀波·{label}"),
            base_amount: damage,
            effective_amount: resolution.damage_applied,
            modifiers: Vec::new(),
            benefits,
        });
        advance.logs.push(format!(
            "{}的腐蚀波{}对{}造成{}点物理伤害",
            stack.source_name,
            label,
            display_name,
            format_number(resolution.damage_applied)
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
        if let Some(outcome) = resolution.defeat_outcome {
            advance.defeat_outcomes.push(outcome);
        }
        stack.turns_remaining = stack.turns_remaining.saturating_sub(1);
        if stack.turns_remaining > 0 && participant.alive {
            remaining.push(stack);
        }
    }
    participant.corrosion_stacks = remaining;
    advance
}

#[derive(Default)]
struct BattleDelayedHealingAdvance {
    logs: Vec<String>,
    combat_log: Vec<CombatLogEntry>,
}

fn advance_participant_delayed_healing_ticks(
    participant: &mut BattleParticipantSnapshot,
    round: u32,
) -> BattleDelayedHealingAdvance {
    if participant.delayed_healing_ticks.is_empty() {
        return BattleDelayedHealingAdvance::default();
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
    if !participant.alive {
        return BattleDelayedHealingAdvance::default();
    }
    let mut advance = BattleDelayedHealingAdvance::default();
    for tick in due {
        let final_amount = tick.amount.max(0.0);
        if final_amount <= f32::EPSILON {
            continue;
        }
        let resolution = apply_participant_healing_for_battle(
            participant,
            final_amount,
            tick.overhealing_shield_cap_rate,
        );
        let effective_amount = resolution.effective_amount();
        let mut benefits = Vec::new();
        if resolution.shield_gained > f32::EPSILON {
            benefits.push(format!(
                "过量治疗转化{}点护盾",
                format_number(resolution.shield_gained)
            ));
        }
        advance.combat_log.push(CombatLogEntry {
            round,
            kind: CombatLogKind::Healing,
            source_id: tick.source_id.clone(),
            source_name: tick.source_name.clone(),
            target_id: participant.target_id.clone(),
            target_name: display_name.clone(),
            action_name: tick.name.clone(),
            base_amount: final_amount,
            effective_amount,
            modifiers: Vec::new(),
            benefits,
        });
        advance.logs.push(format!(
            "{}触发{}，为{}回复{}点生命值",
            tick.source_name,
            tick.name,
            display_name,
            format_number(effective_amount)
        ));
    }
    advance
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
    store: Option<ResMut<Persistent<BattleRoundStore>>>,
    existing: Query<Entity, With<BattleRoundRuntime>>,
    mut last_signature: Local<u64>,
) {
    let Some(mut store) = store else {
        return;
    };
    if store.repair_duplicate_participants() {
        if let Err(error) = store.persist() {
            eprintln!("failed to persist repaired battle participant identities: {error}");
        }
    }
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
        encounter.trpg_campaign_id.hash(&mut hasher);
        encounter.manager_sync_quarantined.hash(&mut hasher);
        encounter.active.hash(&mut hasher);
        encounter.sort_by_turn.hash(&mut hasher);
        encounter.negative_enabled.hash(&mut hasher);
        encounter.round.hash(&mut hasher);
        encounter.combat_completed_turns.hash(&mut hasher);
        for entry in &encounter.action_log {
            entry.hash(&mut hasher);
        }
        for participant in &encounter.participants {
            participant.target_id.hash(&mut hasher);
            participant.display_name.hash(&mut hasher);
            participant.unit_template_id.hash(&mut hasher);
            participant.player_character.hash(&mut hasher);
            participant.is_summon.hash(&mut hasher);
            participant.summon_owner_id.hash(&mut hasher);
            participant.turn.hash(&mut hasher);
            participant.combat_turns_completed.hash(&mut hasher);
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
                .group_modifiers
                .damage_dealt
                .to_bits()
                .hash(&mut hasher);
            participant
                .group_modifiers
                .damage_taken
                .to_bits()
                .hash(&mut hasher);
            participant
                .group_modifiers
                .healing_dealt
                .to_bits()
                .hash(&mut hasher);
            participant
                .group_modifiers
                .healing_taken
                .to_bits()
                .hash(&mut hasher);
            participant.hidden_role.cocooning.hash(&mut hasher);
            participant.hidden_role.free_action_skill.hash(&mut hasher);
            participant
                .hidden_role
                .free_action_available
                .hash(&mut hasher);
            participant
                .hidden_role
                .second_wind_heal
                .to_bits()
                .hash(&mut hasher);
            participant.hidden_role.second_wind_used.hash(&mut hasher);
            participant.hidden_role.immune_diseased.hash(&mut hasher);
            participant.hidden_role.immune_poisoning.hash(&mut hasher);
            participant.hidden_role.immune_bleed.hash(&mut hasher);
            participant.protective_suit_kind.hash(&mut hasher);
            participant.protective_suit_shield.to_bits().hash(&mut hasher);
            participant
                .protective_suit_passive_active
                .hash(&mut hasher);
            participant
                .protective_suit_active_available
                .hash(&mut hasher);
            participant
                .protective_suit_slow_rounds_remaining
                .hash(&mut hasher);
            participant
                .protective_suit_blind_rounds_remaining
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
            participant.arcane_shield_rate.to_bits().hash(&mut hasher);
            participant
                .overhealing_shield_cap_rate
                .to_bits()
                .hash(&mut hasher);
            participant.overhealing_shield.to_bits().hash(&mut hasher);
            participant
                .overhealing_shield_turns_remaining
                .hash(&mut hasher);
            participant
                .revenge_soul_shield_rate
                .to_bits()
                .hash(&mut hasher);
            participant.revenge_soul_shield.to_bits().hash(&mut hasher);
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
                .rest_then_fight_healing_rate
                .to_bits()
                .hash(&mut hasher);
            participant.rest_then_fight_turns.hash(&mut hasher);
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
            for stack in &participant.corrosion_stacks {
                stack.source_id.hash(&mut hasher);
                stack.source_name.hash(&mut hasher);
                stack.kind.hash(&mut hasher);
                stack.damage_per_turn.to_bits().hash(&mut hasher);
                stack.turns_remaining.hash(&mut hasher);
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
    mut targeting_line: ResMut<BattleTargetingLine>,
) {
    if !ui_state.panel_open {
        targeting_line.actor_id = None;
        targeting_line.target_id = None;
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
    store.scene_positions = scene_positions
        .as_deref()
        .map(|positions| positions.positions.clone())
        .unwrap_or_default();

    let mut panel_open = ui_state.panel_open;
    let mut changed = false;
    let mut manager_changed = false;
    let mut close_requested = false;
    targeting_line.actor_id = None;
    targeting_line.target_id = None;

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
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("打开GM战斗明细").clicked() {
                            ui_state.combat_log_open = true;
                        }
                        if ui.button("打开GM赛后统计").clicked() {
                            ui_state
                                .post_match
                                .open_for(store.active_encounter_id.as_deref());
                        }
                    });
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
                            &mut targeting_line,
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

    gm_combat_log_window(
        ctx,
        &mut ui_state.combat_log_open,
        store,
    );
    post_match::show_window(ctx, &mut ui_state.post_match, store);
    ui_state.panel_open = panel_open && !close_requested;
    if changed {
        store.persist().ok();
    }
    if manager_changed {
        manager.persist().ok();
    }
}

fn gm_combat_log_window(ctx: &egui::Context, open: &mut bool, store: &BattleRoundStore) {
    if !*open {
        return;
    }
    egui::Window::new("GM战斗明细")
        .default_width(640.0)
        .resizable(true)
        .open(open)
        .show(ctx, |ui| {
            ui.small("仅显示在GM本地界面；每条伤害、治疗及其属性/BUFF来源都会展开。");
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut encounters = store.encounters.iter().collect::<Vec<_>>();
                    encounters.sort_by(|left, right| left.1.name.cmp(&right.1.name));
                    for (encounter_id, encounter) in encounters {
                        ui.collapsing(
                            format!(
                                "{} · {}条",
                                encounter.name,
                                encounter.combat_log.len()
                            ),
                            |ui| {
                                if encounter.combat_log.is_empty() {
                                    ui.small("还没有结算明细。");
                                }
                                for (index, entry) in encounter.combat_log.iter().enumerate().rev()
                                {
                                    ui.push_id((encounter_id, index), |ui| {
                                        let verb = match entry.kind {
                                            CombatLogKind::Damage => "伤害",
                                            CombatLogKind::Healing => "治疗",
                                            CombatLogKind::Buff => "状态",
                                            CombatLogKind::Resource => "资源",
                                            CombatLogKind::Experience => "经验",
                                            CombatLogKind::Elimination => "击杀",
                                            CombatLogKind::Assist => "助攻",
                                        };
                                        ui.collapsing(
                                            format!(
                                                "R{} · {} → {} · {} {}",
                                                entry.round,
                                                entry.source_name,
                                                entry.target_name,
                                                format_number(entry.effective_amount),
                                                verb,
                                            ),
                                            |ui| {
                                                ui.label(format!("来源：{}", entry.action_name));
                                                ui.label(format!(
                                                    "基础值 {} → 结算值 {}",
                                                    format_number(entry.base_amount),
                                                    format_number(entry.effective_amount),
                                                ));
                                                for modifier in &entry.modifiers {
                                                    ui.small(format!(
                                                        "{}：×{}",
                                                        modifier.source,
                                                        format_number(modifier.multiplier),
                                                    ));
                                                }
                                                for benefit in &entry.benefits {
                                                    ui.colored_label(
                                                        egui::Color32::LIGHT_GREEN,
                                                        benefit,
                                                    );
                                                }
                                            },
                                        );
                                    });
                                }
                            },
                        );
                    }
                });
        });
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
                let encounter_id =
                    store.create_encounter_from_group(name, group_name.to_owned(), group);
                store.active_encounter_id = Some(encounter_id);
                ui_state.new_encounter_name.clear();
                changed = true;
            }
        }
    });
    ui.small("创建后名单为空，可在战斗轮内逐个添加玩家、单位和召唤物。");

    changed
}

fn encounter_ui(
    ui: &mut egui::Ui,
    ui_state: &mut BattleRoundUiState,
    store: &mut BattleRoundStore,
    manager: &mut NapcatMessageManager,
    rule_engine_state: &mut RuleEngineState,
    scene_positions: Option<&SceneCharacterPositions>,
    targeting_line: &mut BattleTargetingLine,
    encounter_entity: &BattleEncounterEntity,
) -> bool {
    let mut changed = false;
    let encounter_id = encounter_entity.id.as_str();
    if !store.encounters.contains_key(encounter_id) {
        return false;
    }
    if store
        .encounters
        .get(encounter_id)
        .is_some_and(|encounter| encounter.manager_sync_quarantined)
    {
        return quarantined_encounter_ui(
            ui,
            store,
            manager,
            encounter_id,
            &encounter_entity.name,
        );
    }
    let linked_group_name = store
        .encounters
        .get(encounter_id)
        .and_then(|encounter| encounter.trpg_group.as_deref())
        .map(str::to_owned);
    let mut linked_campaign_id = None;
    if let Some(group_name) = linked_group_name.as_deref() {
        let Some(group) = manager.trpg_groups.get(group_name) else {
            return locked_encounter_ui(
                ui,
                store,
                encounter_id,
                &encounter_entity.name,
                &format!(
                    "绑定的TRPG组“{group_name}”已不存在；此战斗轮已锁定，不会再覆盖角色状态。"
                ),
            );
        };
        let campaign_id = trpg_group_campaign_id(group).to_owned();
        let bound_campaign_id = store
            .encounters
            .get(encounter_id)
            .and_then(|encounter| encounter.trpg_campaign_id.as_deref());
        if let Some(bound_campaign_id) = bound_campaign_id {
            if bound_campaign_id != campaign_id {
                return locked_encounter_ui(
                    ui,
                    store,
                    encounter_id,
                    &encounter_entity.name,
                    &format!(
                        "此战斗轮属于活动“{bound_campaign_id}”，当前同名TRPG组属于“{campaign_id}”；已锁定以防跨活动覆盖角色状态。"
                    ),
                );
            }
        } else {
            changed |= store.bind_legacy_encounter_campaign(encounter_id, &campaign_id);
        }
        linked_campaign_id = Some(campaign_id);
    }
    let canonical_encounter_id = store
        .encounters
        .get(encounter_id)
        .and_then(|encounter| encounter.trpg_group.as_deref())
        .and_then(|group_name| {
            store.canonical_encounter_id_for_group(
                group_name,
                linked_campaign_id.as_deref(),
            )
        })
        .filter(|canonical_id| *canonical_id != encounter_id)
        .map(str::to_owned);
    if let Some(canonical_encounter_id) = canonical_encounter_id {
        let canonical_name = store
            .encounters
            .get(&canonical_encounter_id)
            .map(|encounter| encounter.name.as_str())
            .unwrap_or(canonical_encounter_id.as_str());
        return locked_encounter_ui(
            ui,
            store,
            encounter_id,
            &encounter_entity.name,
            &format!(
                "此战斗轮与“{canonical_name}”绑定到同一TRPG组，已锁定以防重复结算或覆盖角色状态。"
            ),
        );
    }
    if let Some(encounter) = store.encounters.get_mut(encounter_id) {
        changed |= prune_unbound_group_participants(encounter, manager);
    }
    let initial_round = store
        .encounters
        .get(encounter_id)
        .map(|encounter| encounter.round)
        .unwrap_or_default();
    let mut remove = false;
    changed |= sync_encounter_from_group_clock(store, encounter_id, manager);
    let group_rounds_remaining = group_rounds_ahead_of_encounter(store, encounter_id, manager);

    ui.group(|ui| {
        ui.set_width(ui.available_width());
        if group_rounds_remaining > 0 {
            let encounter = store
                .encounters
                .get(encounter_id)
                .expect("encounter existence checked");
            ui.horizontal_wrapped(|ui| {
                ui.heading(&encounter_entity.name);
                ui.small(format!("第{}轮", encounter.round));
            });
            ui.colored_label(
                egui::Color32::YELLOW,
                format!(
                    "正在同步TRPG组轮次，还差{group_rounds_remaining}轮；完成前不会开放战斗操作。"
                ),
            );
            return;
        }
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
                    let match_ended = encounter.active && !active;
                    changed |= set_encounter_active_state(encounter, active);
                    if match_ended {
                        ui_state.post_match.open_for(Some(encounter_id));
                    }
                }
                changed |= ui
                    .checkbox(&mut encounter.negative_enabled, "消极")
                    .changed();
                changed |= ui
                    .checkbox(&mut encounter.sort_by_turn, "排序")
                    .on_hover_text("仅按AGI排序行动顺序。")
                    .changed();
                if ui
                    .button("刷新玩家")
                    .on_hover_text("从TRPG组重新同步所有玩家；未在名单中的玩家会被重新加入。")
                    .clicked()
                {
                    changed |= refresh_encounter_players(encounter, manager);
                }
                if ui.button("下一轮").clicked() {
                    next_round_requested = true;
                }
                if ui.button("统计").clicked() {
                    ui_state.post_match.open_for(Some(encounter_id));
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
            targeting_line,
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

fn locked_encounter_ui(
    ui: &mut egui::Ui,
    store: &mut BattleRoundStore,
    encounter_id: &str,
    encounter_name: &str,
    reason: &str,
) -> bool {
    let mut remove = false;
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.heading(encounter_name);
        ui.colored_label(egui::Color32::YELLOW, reason);
        if ui.button("删除此战斗轮").clicked() {
            remove = true;
        }
    });
    if !remove {
        return false;
    }
    store.encounters.remove(encounter_id);
    if store.active_encounter_id.as_deref() == Some(encounter_id) {
        store.active_encounter_id = None;
    }
    true
}

fn quarantined_encounter_ui(
    ui: &mut egui::Ui,
    store: &mut BattleRoundStore,
    manager: &NapcatMessageManager,
    encounter_id: &str,
    encounter_name: &str,
) -> bool {
    let can_reconnect = store
        .encounters
        .get(encounter_id)
        .is_some_and(|encounter| encounter_can_reconnect_to_manager(encounter, manager));
    let mut reconnect = false;
    let mut remove = false;
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.heading(encounter_name);
        ui.colored_label(
            egui::Color32::YELLOW,
            "主TRPG数据已被完整替换；此战斗轮已隔离，不会覆盖新导入角色的HP、MP、回合、计数或冷却。",
        );
        if can_reconnect {
            ui.small("仅在确认此战斗轮与当前主数据属于同一份备份时重新连接；连接会立即以战斗状态更新角色。");
            if ui
                .button("确认连接当前主数据（会覆盖角色战斗状态）")
                .clicked()
            {
                reconnect = true;
            }
        } else {
            ui.small("当前TRPG组或活动身份不匹配。请导入配套战斗备份，或删除此旧战斗轮。");
        }
        if ui.button("删除此战斗轮").clicked() {
            remove = true;
        }
    });

    if reconnect {
        if let Some(encounter) = store.encounters.get_mut(encounter_id) {
            if encounter.trpg_campaign_id.is_none() {
                encounter.trpg_campaign_id = encounter
                    .trpg_group
                    .as_deref()
                    .and_then(|group_name| manager.trpg_groups.get(group_name))
                    .map(|group| trpg_group_campaign_id(group).to_owned());
            }
            encounter.manager_sync_quarantined = false;
            return true;
        }
    }
    if remove {
        store.encounters.remove(encounter_id);
        if store.active_encounter_id.as_deref() == Some(encounter_id) {
            store.active_encounter_id = None;
        }
        return true;
    }
    false
}

fn encounter_can_reconnect_to_manager(
    encounter: &BattleEncounter,
    manager: &NapcatMessageManager,
) -> bool {
    let Some(group_name) = encounter.trpg_group.as_deref() else {
        return true;
    };
    let Some(group) = manager.trpg_groups.get(group_name) else {
        return false;
    };
    encounter
        .trpg_campaign_id
        .as_deref()
        .is_none_or(|campaign_id| campaign_id == trpg_group_campaign_id(group))
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
    let mut completion_target = None;
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
            let mut requested_done = participant.action_done;
            if ui
                .add_enabled(
                    participant_can_act(participant),
                    egui::Checkbox::new(&mut requested_done, ""),
                )
                .changed()
                && requested_done
            {
                completion_target = Some(participant.target_id.clone());
            }
            changed |= ui
                .text_edit_singleline(&mut participant.display_name)
                .changed();
            ui.small(&participant.target_id);
            let effective_speed = participant_order_speed(
                participant,
                living_player_count,
                encounter.active,
            );
            let speed_escalated = (effective_speed - participant.speed).abs() > f32::EPSILON;
            if speed_escalated {
                ui.label(format!(
                    "速度 {}",
                    format_number(effective_speed)
                ))
                .on_hover_text("存活玩家≤3，狂风恶浪移速加成提升至35%");
            } else {
                ui.label("速度");
            }
            changed |= ui
                .add(egui::DragValue::new(&mut participant.speed).speed(0.5))
                .changed();
            if participant.penance_healing_bonus_percent > f32::EPSILON
                && participant.penance_kill_assist_count > 0
            {
                ui.small(format!(
                    "忏悔{}次",
                    participant.penance_kill_assist_count
                ));
            }
            if encounter.active
                && participant.arrogance_damage_bonus_per_source > f32::EPSILON
                && !participant.arrogance_damage_source_ids.is_empty()
            {
                ui.small(format!(
                    "狂妄{}层",
                    participant.arrogance_damage_source_ids.len()
                ));
            }
            if encounter.active
                && participant.endless_pain_bonus_damage_per_stack > f32::EPSILON
                && participant.endless_pain_stacks > 0
            {
                ui.small(format!(
                    "无尽痛楚{}层",
                    participant.endless_pain_stacks
                ));
            }
            if encounter.active
                && participant.infinite_focus_damage_bonus_per_stack > f32::EPSILON
                && participant.infinite_focus_stacks > 0
            {
                ui.small(format!(
                    "无限专注{}层",
                    participant.infinite_focus_stacks
                ));
            }
            if encounter.active
                && participant.one_heart_healing_bonus_per_stack > f32::EPSILON
                && participant.one_heart_stacks > 0
            {
                ui.small(format!(
                    "一心{}层",
                    participant.one_heart_stacks
                ));
            }
            if encounter.active
                && participant_inspiration_multiplier(participant) > 1.0 + f32::EPSILON
            {
                ui.small("振奋：速度与伤害+10%");
            }
            if encounter.active
                && participant.keen_evasion_enabled
                && participant.keen_evasion_available
            {
                ui.small("敏锐待机");
            }
            if participant.hidden_role.cocooning {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    "结茧中·无法行动",
                );
            }
            if encounter.active && participant.hidden_role.free_action_available {
                if let Some(skill) = participant.hidden_role.free_action_skill.as_deref() {
                    ui.small(format!("免费行动待机：{skill}"));
                }
            }
            if participant.hidden_role.second_wind_heal > f32::EPSILON {
                ui.small(
                    if participant.hidden_role.second_wind_used {
                        "适应恢复已触发"
                    } else {
                        "适应恢复待机"
                    },
                );
            }
            if encounter.active && participant.arcane_shield > f32::EPSILON {
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
            if encounter.active && participant.revenge_soul_shield > f32::EPSILON {
                ui.small(format!(
                    "复仇之魂护盾{}",
                    format_number(participant.revenge_soul_shield)
                ));
            }
            if participant.construct_shield_max > f32::EPSILON {
                ui.small(format!(
                    "全伤害护盾{}/{}",
                    format_number(participant.construct_shield),
                    format_number(participant.construct_shield_max)
                ));
            }
            if participant.construct_shield_repair_rounds_remaining > 0 {
                ui.small(format!(
                    "护盾维修：剩余{}回合",
                    participant.construct_shield_repair_rounds_remaining
                ));
            }
            if participant.construct_repair_channel_rounds_remaining > 0 {
                ui.small(format!(
                    "机甲维修引导：剩余{}回合",
                    participant.construct_repair_channel_rounds_remaining
                ));
            }
            if participant.paralyzed_rounds_remaining > 0 {
                ui.small(format!(
                    "麻痹：剩余{}回合",
                    participant.paralyzed_rounds_remaining
                ));
            }
            if let Some(kind) = participant.protective_suit_kind {
                ui.small(format!(
                    "{}护盾{}/{}{}",
                    kind.label(),
                    format_number(participant.protective_suit_shield),
                    format_number(participant.protective_suit_shield_max),
                    if participant.protective_suit_magic_only {
                        "（仅魔法）"
                    } else {
                        ""
                    }
                ));
                if !participant.protective_suit_passive_active
                    && kind != ProtectiveSuitKind::Radiation
                {
                    ui.small("防护服被动已失效");
                }
                if participant.protective_suit_active_available {
                    ui.small("防护服主动效果可用");
                }
            }
            if participant.protective_suit_blind_rounds_remaining > 0 {
                ui.small("致盲：本轮无法行动");
            }
            if participant.protective_suit_slow_rounds_remaining > 0 {
                ui.small("急冻：本轮移速减半");
            }
            if participant.commissar_proficiency > 0 {
                ui.small(format!(
                    "为了帝皇熟练度 {}/5",
                    participant.commissar_proficiency.min(5)
                ));
            }
            if participant.next_attack_bonus_physical > f32::EPSILON {
                ui.small(format!(
                    "下次物理攻击附带{}点伤害",
                    format_number(participant.next_attack_bonus_physical)
                ));
            }
            if participant.natural_hp_regen_suppressed {
                ui.small("撕裂：自然生命回复受抑制");
            }
            if participant.flying_needles_enabled {
                ui.small(format!(
                    "飞针 {}/5 · 针匣 {}/{}{}",
                    participant.flying_needles_ready,
                    participant.needle_case_ready,
                    if participant.needle_case_enabled { 2 } else { 0 },
                    if participant.needle_case_enabled
                        && participant.needle_case_progress_noncombat_rounds > 0
                    {
                        "（生产1/2）"
                    } else {
                        ""
                    }
                ));
            }
            if participant.redeemed_invisibility_rounds_remaining > 0 {
                ui.small(format!(
                    "隐身术：剩余{}回合",
                    participant.redeemed_invisibility_rounds_remaining
                ));
            }
            if encounter.active {
                if participant.undying_rage_active {
                    ui.small("不死者之怒生效");
                } else if participant.undying_rage_enabled && participant.undying_rage_used {
                    ui.small("不死者之怒已触发");
                }
            }
            if encounter.active && participant_hope_avatar_active(participant) {
                ui.small(format!(
                    "希望化身：剩余{}回合",
                    participant.hope_avatar_rounds_remaining
                ));
            } else if encounter.active
                && participant.hope_avatar_enabled
                && participant.hope_avatar_used
            {
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
            if participant.rest_then_fight_healing_rate > f32::EPSILON {
                ui.small(format!(
                    "以逸待劳{}层/{}%",
                    participant.rest_then_fight_turns,
                    format_number(
                        (participant.rest_then_fight_healing_rate
                            * participant.rest_then_fight_turns as f32)
                            .min(0.50)
                            * 100.0
                    )
                ));
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
            if !participant.corrosion_stacks.is_empty() {
                ui.small(format!(
                    "腐蚀{}层",
                    participant.corrosion_stacks.len()
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
            if encounter.active
                && participant.dominion_max_hp_gain_rate > f32::EPSILON
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
            let mut alive = participant.alive;
            if ui.checkbox(&mut alive, "存活").changed() {
                set_participant_alive_after_manual_edit(participant, alive);
                changed = true;
            }
            ui.small(format!(
                "本轮承伤 {} / 受疗 {}",
                format_number(participant.damage_taken_this_turn),
                format_number(participant.healing_taken_this_turn)
            ));
            if participant.action_done {
                ui.small("已完成");
            }
            if ui.button("移出").on_hover_text("移出战斗轮").clicked() {
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
                participant.group_modifiers = encounter_group_combat_modifiers(encounter, manager);
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
                    let mut participant = participant_from_unit_template(&target_id, unit_id, unit);
                    participant.group_modifiers =
                        encounter_group_combat_modifiers(encounter, manager);
                    encounter.participants.push(participant);
                    changed = true;
                }
            }
        });
    }

    let mut remove_unit_id: Option<String> = None;
    let joined_units = encounter
        .participants
        .iter()
        .filter(|participant| participant.unit_template_id.is_some() && !participant.is_summon)
        .map(|participant| {
            (
                participant.target_id.clone(),
                participant.display_name.clone(),
            )
        })
        .collect::<Vec<_>>();
    if !joined_units.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label("已加入单位");
            for (target_id, display_name) in &joined_units {
                ui.group(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(display_name);
                        ui.small(target_id);
                        if ui.button("移出战斗轮").clicked() {
                            remove_unit_id = Some(target_id.clone());
                        }
                    });
                });
            }
        });
    }

    let summon_candidates = available_summon_candidates(encounter, manager);
    if !summon_candidates.is_empty() {
        let selected = ui_state
            .selected_add_summon
            .entry(encounter_id.to_owned())
            .or_insert_with(|| {
                (
                    summon_candidates[0].0.clone(),
                    summon_candidates[0].1,
                )
            });
        if !summon_candidates
            .iter()
            .any(|(owner_id, index, _)| owner_id == &selected.0 && *index == selected.1)
        {
            *selected = (
                summon_candidates[0].0.clone(),
                summon_candidates[0].1,
            );
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("添加召唤物");
            egui::ComboBox::from_id_salt(format!(
                "battle_add_summon_{encounter_id}"
            ))
            .selected_text(
                summon_candidates
                    .iter()
                    .find(|(owner_id, index, _)| owner_id == &selected.0 && *index == selected.1)
                    .map(|(_, _, label)| label.as_str())
                    .unwrap_or(selected.0.as_str()),
            )
            .show_ui(ui, |ui| {
                for (owner_id, index, label) in &summon_candidates {
                    ui.selectable_value(
                        selected,
                        (owner_id.clone(), *index),
                        label,
                    );
                }
            });
            if ui.button("添加").clicked() {
                if let Some((owner_id, index)) = summon_candidates
                    .iter()
                    .find(|(owner_id, index, _)| owner_id == &selected.0 && *index == selected.1)
                    .map(|(owner_id, index, _)| (owner_id.clone(), *index))
                {
                    if let (Some(owner), Some(summon)) = (
                        manager.player_characters.get(&owner_id),
                        manager
                            .player_characters
                            .get(&owner_id)
                            .and_then(|owner| owner.summons.get(index)),
                    ) {
                        let summon = summon.clone();
                        let owner = owner.clone();
                        let mut participant = participant_from_summon(
                            &owner_id, index, &summon, &owner, manager,
                        );
                        participant.group_modifiers =
                            encounter_group_combat_modifiers(encounter, manager);
                        encounter.participants.push(participant);
                        changed = true;
                    }
                }
            }
        });
    }

    if let Some(target_id) = remove_unit_id {
        encounter
            .participants
            .retain(|participant| participant.target_id != target_id);
        changed = true;
    }
    if changed {
        normalize_encounter_after_edit(encounter);
    }
    if let Some(target_id) = completion_target {
        changed |= set_roster_action_done(store, encounter_id, &target_id, true);
    }
    changed
}

fn set_roster_action_done(
    store: &mut BattleRoundStore,
    encounter_id: &str,
    target_id: &str,
    done: bool,
) -> bool {
    done && store.finish_actor_action(encounter_id, target_id)
}

fn encounter_action_ui(
    ui: &mut egui::Ui,
    ui_state: &mut BattleRoundUiState,
    encounter_id: &str,
    store: &mut BattleRoundStore,
    manager: &mut NapcatMessageManager,
    scene_positions: Option<&SceneCharacterPositions>,
    targeting_line: &mut BattleTargetingLine,
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
    let encounter_active = encounter.active;
    let target_options = encounter
        .participants
        .iter()
        .filter(|participant| participant.mirror_coat_layers == 0)
        .map(|participant| {
            (
                participant.target_id.clone(),
                if participant.alive {
                    participant.display_name.clone()
                } else {
                    format!("{}（倒下）", participant.display_name)
                },
            )
        })
        .collect::<Vec<_>>();
    let living_target_ids = encounter
        .participants
        .iter()
        .filter(|participant| participant.alive && participant.mirror_coat_layers == 0)
        .map(|participant| participant.target_id.clone())
        .collect::<HashSet<_>>();
    let skills = character_for_participant(&actor, manager)
        .as_ref()
        .map(|character| character_skills(character))
        .unwrap_or_default();
    let item_skills = character_for_participant(&actor, manager)
        .as_ref()
        .map(item_skill_groups)
        .unwrap_or_default();

    ui.label(format!(
        "当前行动者：{}",
        actor.display_name
    ));
    let actor_changed = ui_state
        .selected_action_actor
        .insert(
            encounter_id.to_owned(),
            actor.target_id.clone(),
        )
        .as_deref()
        != Some(actor.target_id.as_str());
    let target = ui_state
        .selected_action_target
        .entry(encounter_id.to_owned())
        .or_insert_with(|| {
            default_action_target_id(
                &target_options,
                &living_target_ids,
                &actor.target_id,
            )
        });
    update_action_target_for_actor(
        target,
        &actor.target_id,
        actor_changed,
        &target_options,
        &living_target_ids,
    );
    targeting_line.actor_id = Some(actor.target_id.clone());
    targeting_line.target_id = (!target.is_empty()).then(|| target.clone());
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
        let target_alive = living_target_ids.contains(target.as_str());
        if ui
            .add_enabled(
                target_alive && (!encounter_active || !participant_hope_avatar_active(&actor)),
                egui::Button::new("普通攻击"),
            )
            .clicked()
        {
            changed |= store.apply_common_attack_and_finish(
                encounter_id,
                &actor.target_id,
                target,
                "普通攻击",
                *amount,
                manager,
            );
        }
        if ui.button("标记完成").clicked() {
            changed |= store.finish_actor_action(encounter_id, &actor.target_id);
        }
        if ui.button("跳过+消极").clicked() {
            changed |= store.skip_negative_participant(encounter_id, &actor.target_id);
        }
    });

    if let Some(kind) = actor.protective_suit_kind {
        ui.horizontal_wrapped(|ui| {
            ui.label("防护服");
            let available = actor.protective_suit_active_available;
            let mut use_action = |label: &str, action: ProtectiveSuitAction| {
                if ui
                    .add_enabled(available, egui::Button::new(label))
                    .clicked()
                {
                    changed |= store.use_protective_suit_action_and_finish(
                        encounter_id,
                        &actor.target_id,
                        target,
                        action,
                        manager,
                        scene_positions,
                    );
                }
            };
            match kind {
                ProtectiveSuitKind::Medical => {
                    use_action("使用治疗针（3点）", ProtectiveSuitAction::MedicalInjection);
                },
                ProtectiveSuitKind::Photon => {
                    use_action("释放强光", ProtectiveSuitAction::PhotonFlash);
                },
                ProtectiveSuitKind::Cryogenic => {
                    use_action("释放急冻气体", ProtectiveSuitAction::CryogenicRelease);
                },
                ProtectiveSuitKind::Scientist => {
                    use_action("投掷酸液", ProtectiveSuitAction::AcidVial);
                    use_action("腐蚀锁门", ProtectiveSuitAction::AcidDoor);
                },
                ProtectiveSuitKind::Maintenance => {
                    use_action("维修锯攻击", ProtectiveSuitAction::MaintenanceSaw);
                    use_action("拆除锁门", ProtectiveSuitAction::MaintenanceDoor);
                },
                ProtectiveSuitKind::Normal
                | ProtectiveSuitKind::DarkMatter
                | ProtectiveSuitKind::Electronic
                | ProtectiveSuitKind::Radiation
                | ProtectiveSuitKind::Heavy => {
                    ui.small("无主动效果");
                },
            }
            if kind.has_consumable_active() && !available {
                ui.small("一次性装置已使用");
            }
        });
    }

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
            let hope_avatar_allows = !encounter_active
                || !participant_hope_avatar_active(&actor)
                || skill_effects_are_hope_avatar_healing(&effects);
            let target_alive = living_target_ids.contains(target.as_str());
            let target_allows = skill_effects_allow_selected_target(
                &effects,
                skill.target_class.as_deref(),
                Some(target_alive),
            );
            let requires_gm_resolution =
                effects.is_empty() && !skill_has_dedicated_battle_resolution(skill);
            let can_use = cooldown_remaining == 0 && can_pay && hope_avatar_allows && target_allows;
            let use_label = if requires_gm_resolution {
                "GM裁定并记录"
            } else {
                "使用技能"
            };
            let response = ui.add_enabled(can_use, egui::Button::new(use_label));
            if response.clicked() {
                changed |= store.record_skill_use_with_buffs_and_finish(
                    encounter_id,
                    &actor.target_id,
                    target,
                    skill,
                    manager,
                    scene_positions,
                );
            }
            if !hope_avatar_allows {
                ui.small("希望化身期间只能释放治疗技能");
            } else if !target_allows {
                ui.small("倒下目标只能接受单目标治疗");
            } else if !can_pay {
                ui.small(format!(
                    "需要{} MP",
                    format_number(skill.mp_cost.max(0.0))
                ));
            } else if cooldown_remaining > 0 {
                ui.small(format!(
                    "冷却还剩{cooldown_remaining}轮"
                ));
            } else if requires_gm_resolution {
                ui.small("规则引擎无法安全自动结算此技能；确认后记录使用、消耗和冷却，具体效果由GM处理。");
            }
        });
    } else {
        ui.small("这个角色没有技能。");
    }

    if !item_skills.is_empty() {
        let selected_item = ui_state
            .selected_item_index
            .entry(encounter_id.to_owned())
            .or_insert(0);
        if *selected_item >= item_skills.len() {
            *selected_item = 0;
        }
        let selected_item_skill = ui_state
            .selected_item_skill_index
            .entry(encounter_id.to_owned())
            .or_insert(0);
        if *selected_item_skill >= item_skills[*selected_item].1.len() {
            *selected_item_skill = 0;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("施法物品");
            egui::ComboBox::from_id_salt(format!("battle_item_{encounter_id}"))
                .selected_text(item_skills[*selected_item].0.as_str())
                .show_ui(ui, |ui| {
                    for (index, (item_name, _)) in item_skills.iter().enumerate() {
                        ui.selectable_value(selected_item, index, item_name);
                    }
                });
            let skills = &item_skills[*selected_item].1;
            if *selected_item_skill >= skills.len() {
                *selected_item_skill = 0;
            }
            egui::ComboBox::from_id_salt(format!(
                "battle_item_skill_{encounter_id}"
            ))
            .selected_text(skills[*selected_item_skill].name.as_str())
            .show_ui(ui, |ui| {
                for (index, skill) in skills.iter().enumerate() {
                    ui.selectable_value(selected_item_skill, index, &skill.name);
                }
            });
            let skill = &skills[*selected_item_skill];
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
            let target_alive = living_target_ids.contains(target.as_str());
            let target_allows = skill_effects_allow_selected_target(
                &effects,
                skill.target_class.as_deref(),
                Some(target_alive),
            );
            let hope_avatar_allows = !encounter_active
                || !participant_hope_avatar_active(&actor)
                || skill_effects_are_hope_avatar_healing(&effects);
            if ui
                .add_enabled(
                    cooldown_remaining == 0 && can_pay && target_allows && hope_avatar_allows,
                    egui::Button::new("使用物品施法"),
                )
                .clicked()
            {
                changed |= store.record_skill_use_with_buffs_and_finish(
                    encounter_id,
                    &actor.target_id,
                    target,
                    skill,
                    manager,
                    scene_positions,
                );
            }
            if skill_item_consumes_on_cast(
                character_for_participant(&actor, manager).as_ref(),
                skill.index,
            ) {
                ui.small("消耗1个");
            }
        });
    }

    changed
}

fn default_action_target_id(
    target_options: &[(String, String)],
    living_target_ids: &HashSet<String>,
    actor_id: &str,
) -> String {
    target_options
        .iter()
        .find(|(target_id, _)| target_id != actor_id && living_target_ids.contains(target_id))
        .or_else(|| {
            target_options
                .iter()
                .find(|(target_id, _)| target_id != actor_id)
        })
        .or_else(|| target_options.first())
        .map(|(target_id, _)| target_id.clone())
        .unwrap_or_default()
}

fn update_action_target_for_actor(
    target: &mut String,
    actor_id: &str,
    actor_changed: bool,
    target_options: &[(String, String)],
    living_target_ids: &HashSet<String>,
) {
    let target_exists = target_options
        .iter()
        .any(|(target_id, _)| target_id == target);
    if !target_exists || (actor_changed && target == actor_id) {
        *target = default_action_target_id(
            target_options,
            living_target_ids,
            actor_id,
        );
    }
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
    pub fn to_export_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&BattleRoundStoreExportRef {
            version: BATTLE_ROUND_EXPORT_VERSION,
            export_type: "battle_rounds",
            store: self,
        })
        .map_err(|err| err.to_string())
    }

    pub fn from_export_json(text: &str) -> Result<Self, String> {
        let export: BattleRoundStoreExportOwned =
            serde_json::from_str(text).map_err(|err| err.to_string())?;
        if export.version != BATTLE_ROUND_EXPORT_VERSION {
            return Err(format!(
                "unsupported battle round export version {}; expected {}",
                export.version, BATTLE_ROUND_EXPORT_VERSION
            ));
        }
        if export.export_type != "battle_rounds" {
            return Err(format!(
                "unsupported battle round export type {}",
                export.export_type
            ));
        }

        let mut store = export.store;
        for (encounter_id, encounter) in &store.encounters {
            if encounter_id.trim().is_empty() {
                return Err("battle round export contains an empty encounter id".to_owned());
            }
            if encounter
                .participants
                .iter()
                .any(|participant| participant.target_id.trim().is_empty())
            {
                return Err(format!(
                    "battle round export encounter {encounter_id} contains an empty participant id"
                ));
            }
        }
        store.repair_duplicate_participants();
        for encounter in store.encounters.values_mut() {
            normalize_encounter_after_edit(encounter);
        }
        if store
            .active_encounter_id
            .as_ref()
            .is_some_and(|encounter_id| !store.encounters.contains_key(encounter_id))
        {
            store.active_encounter_id = None;
        }
        Ok(store)
    }

    pub fn quarantine_manager_sync(&mut self) -> usize {
        let mut changed = 0;
        for encounter in self.encounters.values_mut() {
            if !encounter.manager_sync_quarantined {
                encounter.manager_sync_quarantined = true;
                changed += 1;
            }
        }
        changed
    }

    fn repair_duplicate_participants(&mut self) -> bool {
        let mut changed = false;
        for encounter in self.encounters.values_mut() {
            changed |= deduplicate_encounter_participants(encounter);
        }
        changed
    }

    fn create_encounter_from_group(
        &mut self,
        name: String,
        group_name: String,
        group: &TrpgGroup,
    ) -> String {
        let campaign_id = trpg_group_campaign_id(group);
        if let Some(encounter_id) =
            self.canonical_encounter_id_for_group(&group_name, Some(campaign_id))
        {
            return encounter_id.to_owned();
        }
        let encounter_id = self.allocate_encounter_id();

        self.encounters
            .insert(encounter_id.clone(), BattleEncounter {
                name,
                trpg_group: Some(group_name),
                trpg_campaign_id: Some(campaign_id.to_owned()),
                manager_sync_quarantined: false,
                active: true,
                sort_by_turn: group.battle_sort_by_turn,
                negative_enabled: group.battle_negative_enabled,
                round: group.world_turn,
                combat_completed_turns: 0,
                participants: Vec::new(),
                action_log: Vec::new(),
                combat_log: Vec::new(),
                combat_log_start: 0,
            });
        encounter_id
    }

    fn canonical_encounter_id_for_group<'a>(
        &'a self,
        group_name: &str,
        campaign_id: Option<&str>,
    ) -> Option<&'a str> {
        let max_round = self
            .encounters
            .values()
            .filter(|encounter| {
                encounter.trpg_group.as_deref() == Some(group_name)
                    && campaign_id.is_none_or(|campaign_id| {
                        encounter
                            .trpg_campaign_id
                            .as_deref()
                            .is_none_or(|bound_id| bound_id == campaign_id)
                    })
            })
            .map(|encounter| encounter.round)
            .max()?;

        if let Some(active_id) = self.active_encounter_id.as_deref() {
            if self.encounters.get(active_id).is_some_and(|encounter| {
                encounter.trpg_group.as_deref() == Some(group_name)
                    && encounter.round == max_round
                    && campaign_id.is_none_or(|campaign_id| {
                        encounter
                            .trpg_campaign_id
                            .as_deref()
                            .is_none_or(|bound_id| bound_id == campaign_id)
                    })
            }) {
                return Some(active_id);
            }
        }

        self.encounters
            .iter()
            .filter(|(_, encounter)| {
                encounter.trpg_group.as_deref() == Some(group_name)
                    && encounter.round == max_round
                    && campaign_id.is_none_or(|campaign_id| {
                        encounter
                            .trpg_campaign_id
                            .as_deref()
                            .is_none_or(|bound_id| bound_id == campaign_id)
                    })
            })
            .map(|(encounter_id, _)| encounter_id.as_str())
            .max()
    }

    fn encounter_is_canonical(&self, encounter_id: &str) -> bool {
        let Some(encounter) = self.encounters.get(encounter_id) else {
            return false;
        };
        let Some(group_name) = encounter.trpg_group.as_deref() else {
            return true;
        };
        self.canonical_encounter_id_for_group(
            group_name,
            encounter.trpg_campaign_id.as_deref(),
        ) == Some(encounter_id)
    }

    fn encounter_group_exists(&self, encounter_id: &str, manager: &NapcatMessageManager) -> bool {
        let Some(encounter) = self.encounters.get(encounter_id) else {
            return false;
        };
        let Some(group_name) = encounter.trpg_group.as_deref() else {
            return true;
        };
        let Some(group) = manager.trpg_groups.get(group_name) else {
            return false;
        };
        encounter
            .trpg_campaign_id
            .as_deref()
            .is_none_or(|bound_id| bound_id == trpg_group_campaign_id(group))
    }

    fn bind_legacy_encounter_campaign(&mut self, encounter_id: &str, campaign_id: &str) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        if encounter.trpg_group.is_none() || encounter.trpg_campaign_id.is_some() {
            return false;
        }
        encounter.trpg_campaign_id = Some(campaign_id.to_owned());
        true
    }

    fn allocate_encounter_id(&mut self) -> String {
        let first_index = self.next_encounter_index.max(1);
        let mut index = first_index;
        loop {
            let encounter_id = format!("battle-{index}");
            if !self.encounters.contains_key(&encounter_id) {
                self.next_encounter_index = index.checked_add(1).unwrap_or(1);
                return encounter_id;
            }
            index = index.checked_add(1).unwrap_or(1);
            assert_ne!(
                index, first_index,
                "all battle encounter identifiers are occupied"
            );
        }
    }

    fn next_round(&mut self, encounter_id: &str) -> bool {
        if !self.encounter_is_canonical(encounter_id) {
            return false;
        }
        let scene_positions = self.scene_positions.clone();
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        if encounter.round == u32::MAX {
            return false;
        }
        encounter.round = encounter.round.saturating_add(1);
        advance_encounter_inspiration(encounter);
        sync_butterfly_effects(encounter);
        let mut delayed_logs = Vec::new();
        let mut delayed_combat_log = Vec::new();
        let mut defeat_outcomes = Vec::new();
        let mut skipped_combat_turns = 0_u32;
        for participant in &mut encounter.participants {
            if participant.alive && !participant.action_done {
                participant.turn = participant.turn.saturating_add(1);
                if encounter.active {
                    participant.combat_turns_completed =
                        participant.combat_turns_completed.saturating_add(1);
                    skipped_combat_turns = skipped_combat_turns.saturating_add(1);
                }
            }
            participant.action_done = false;
            participant.hidden_role.free_action_available =
                encounter.active && participant.hidden_role.free_action_skill.is_some();
            participant.undying_rage_active = false;
            advance_participant_overhealing_shield(participant);
            let previous_damage_taken = participant.damage_taken_this_turn;
            reset_participant_turn_totals(participant);
            advance_participant_mirror_coat(participant);
            let (hope_log, hope_outcome) = advance_participant_hope_avatar(participant);
            if let Some(log) = hope_log {
                delayed_logs.push(log);
                if let Some(outcome) = hope_outcome {
                    defeat_outcomes.push(outcome);
                }
                continue;
            }
            if encounter.active {
                if let Some((log, combat_entry)) = apply_participant_liquid_body_healing(
                    participant,
                    previous_damage_taken,
                    encounter.round,
                ) {
                    delayed_logs.push(log);
                    delayed_combat_log.push(combat_entry);
                }
            }
            if participant.wound_healing_taken_turns > 0 {
                participant.wound_healing_taken_turns -= 1;
            }
            if participant.alive {
                if !encounter.active {
                    if !participant.natural_hp_regen_suppressed {
                        participant.hp =
                            (participant.hp + participant.hp_regen).min(participant.max_hp);
                    }
                    advance_participant_rest_then_fight(participant);
                }
                participant.mp = (participant.mp + participant.mp_regen).min(participant.max_mp);
            }
            if participant.goose_channeling_turns > 0 {
                if let Some((log, combat_entry)) =
                    advance_participant_goose_channel(participant, encounter.round)
                {
                    delayed_logs.push(log);
                    delayed_combat_log.push(combat_entry);
                }
            }
            delayed_logs.extend(advance_redeemed_construct_state(
                participant,
                encounter.active,
            ));
            if !encounter.active && advance_redeemed_needles_noncombat(participant) {
                delayed_logs.push(format!(
                    "{}补充飞针：常备{}/5，针匣{}/{}",
                    participant.display_name,
                    participant.flying_needles_ready,
                    participant.needle_case_ready,
                    if participant.needle_case_enabled { 2 } else { 0 }
                ));
            }
            if participant.paralyzed_rounds_remaining > 0 {
                participant.paralyzed_rounds_remaining -= 1;
            }
            if participant.protective_suit_blind_rounds_remaining > 0 {
                participant.protective_suit_blind_rounds_remaining -= 1;
            }
            if participant.protective_suit_slow_rounds_remaining > 0 {
                participant.protective_suit_slow_rounds_remaining -= 1;
            }
            if participant.redeemed_invisibility_rounds_remaining > 0 {
                participant.redeemed_invisibility_rounds_remaining -= 1;
            }
            let delayed = advance_participant_delayed_damage_ticks(
                participant,
                encounter.active,
                encounter.round,
            );
            delayed_logs.extend(delayed.logs);
            delayed_combat_log.extend(delayed.combat_log);
            defeat_outcomes.extend(delayed.defeat_outcomes);
            let corrosion = advance_participant_corrosion(
                participant,
                encounter.active,
                encounter.round,
            );
            delayed_logs.extend(corrosion.logs);
            delayed_combat_log.extend(corrosion.combat_log);
            defeat_outcomes.extend(corrosion.defeat_outcomes);
            let delayed_healing =
                advance_participant_delayed_healing_ticks(participant, encounter.round);
            delayed_logs.extend(delayed_healing.logs);
            delayed_combat_log.extend(delayed_healing.combat_log);
        }
        let radiation = advance_radiation_protective_suits(encounter, &scene_positions);
        delayed_logs.extend(radiation.logs);
        delayed_combat_log.extend(radiation.combat_log);
        defeat_outcomes.extend(radiation.defeat_outcomes);
        encounter.combat_completed_turns = encounter
            .combat_completed_turns
            .saturating_add(skipped_combat_turns);
        encounter.combat_log.extend(delayed_combat_log);
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
            .map(|encounter| encounter.participants.iter().any(participant_can_act))
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
        self.finish_actor_action_internal(encounter_id, target_id, false)
    }

    fn finish_resolved_actor_action(&mut self, encounter_id: &str, target_id: &str) -> bool {
        self.finish_actor_action_internal(encounter_id, target_id, true)
    }

    fn finish_actor_action_internal(
        &mut self,
        encounter_id: &str,
        target_id: &str,
        allow_defeated: bool,
    ) -> bool {
        if !self.encounter_is_canonical(encounter_id) {
            return false;
        }
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
        if participant.action_done || (!allow_defeated && !participant.alive) {
            return false;
        }
        participant.action_done = true;
        participant.turn = participant.turn.saturating_add(1);
        if encounter.active {
            participant.combat_turns_completed =
                participant.combat_turns_completed.saturating_add(1);
            encounter.combat_completed_turns = encounter.combat_completed_turns.saturating_add(1);
        }
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

    fn apply_action_and_finish(
        &mut self,
        encounter_id: &str,
        actor_id: &str,
        target_id: &str,
        action_name: &str,
        damage: f32,
    ) -> bool {
        if !self.apply_action(
            encounter_id,
            actor_id,
            target_id,
            action_name,
            damage,
        ) {
            return false;
        }
        self.finish_resolved_actor_action(encounter_id, actor_id)
    }

    fn apply_common_attack_and_finish(
        &mut self,
        encounter_id: &str,
        actor_id: &str,
        target_id: &str,
        action_name: &str,
        base_damage: f32,
        manager: &mut NapcatMessageManager,
    ) -> bool {
        let Some(encounter) = self.encounters.get(encounter_id) else {
            return false;
        };
        let Some(actor) = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == actor_id)
            .cloned()
        else {
            return false;
        };
        let Some(target) = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == target_id)
            .cloned()
        else {
            return false;
        };
        let config = encounter_basic_config(encounter, manager, actor_id);
        let actor_character = character_for_participant(&actor, manager);
        let target_character = character_for_participant(&target, manager);
        let mut source_notes = character_combat_source_notes(actor_character.as_ref(), "攻击者");
        source_notes.extend(character_combat_source_notes(
            target_character.as_ref(),
            "目标",
        ));
        let mut modifiers = participant_damage_modifiers(
            &actor,
            actor_character.as_ref(),
            &config,
            completed_combat_turns(encounter),
            DamageType::Physical,
            encounter.active,
        );
        modifiers.extend(participant_damage_taken_modifiers(
            &target,
            target_character.as_ref(),
            DamageType::Physical,
            encounter.active,
        ));
        let incoming = RuleAmountResolution::new(base_damage, modifiers.clone()).resolved_amount;
        let large_hit = target_character
            .as_ref()
            .map(character_large_hit_damage_taken_modifier)
            .map(|modifier| large_hit_damage_taken_multiplier(target.max_hp, incoming, modifier))
            .unwrap_or(1.0);
        if (large_hit - 1.0).abs() > f32::EPSILON {
            modifiers.push(RuleAmountResolution::factor(
                "大额伤害承伤",
                large_hit,
            ));
        }
        let resolution = RuleAmountResolution::new(base_damage, modifiers);
        let previous_hp = target.hp;
        if !self.apply_action(
            encounter_id,
            actor_id,
            target_id,
            action_name,
            resolution.resolved_amount,
        ) {
            return false;
        }
        let effective = if let Some(encounter) = self.encounters.get_mut(encounter_id) {
            let effective = encounter
                .participants
                .iter()
                .find(|participant| participant.target_id == target_id)
                .map(|participant| (previous_hp - participant.hp).max(0.0))
                .unwrap_or_default();
            encounter.combat_log.push(CombatLogEntry {
                round: encounter.round,
                kind: CombatLogKind::Damage,
                source_id: actor_id.to_owned(),
                source_name: actor.display_name,
                target_id: target_id.to_owned(),
                target_name: target.display_name,
                action_name: action_name.to_owned(),
                base_amount: base_damage.max(0.0),
                effective_amount: effective,
                modifiers: resolution.modifiers,
                benefits: source_notes,
            });
            effective
        } else {
            0.0
        };
        if effective > f32::EPSILON {
            if let Some(encounter) = self.encounters.get_mut(encounter_id) {
                apply_corrosion_wave_after_attack(
                    encounter,
                    actor_id,
                    actor_character.as_ref(),
                    manager,
                );
            }
        }
        if let Some(encounter) = self.encounters.get_mut(encounter_id) {
            apply_sunset_world_time_reset(encounter, manager);
        }
        self.finish_resolved_actor_action(encounter_id, actor_id)
    }

    fn use_protective_suit_action_and_finish(
        &mut self,
        encounter_id: &str,
        actor_id: &str,
        target_id: &str,
        action: ProtectiveSuitAction,
        manager: &mut NapcatMessageManager,
        scene_positions: Option<&SceneCharacterPositions>,
    ) -> bool {
        if !self.encounter_is_canonical(encounter_id) {
            return false;
        }
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        let Some(actor_index) = encounter
            .participants
            .iter()
            .position(|participant| participant.target_id == actor_id)
        else {
            return false;
        };
        if !participant_can_act(&encounter.participants[actor_index]) {
            return false;
        }
        let actor = encounter.participants[actor_index].clone();
        let expected_kind = match action {
            ProtectiveSuitAction::MedicalInjection => ProtectiveSuitKind::Medical,
            ProtectiveSuitAction::PhotonFlash => ProtectiveSuitKind::Photon,
            ProtectiveSuitAction::CryogenicRelease => ProtectiveSuitKind::Cryogenic,
            ProtectiveSuitAction::AcidVial | ProtectiveSuitAction::AcidDoor => {
                ProtectiveSuitKind::Scientist
            },
            ProtectiveSuitAction::MaintenanceSaw | ProtectiveSuitAction::MaintenanceDoor => {
                ProtectiveSuitKind::Maintenance
            },
        };
        if actor.protective_suit_kind != Some(expected_kind)
            || !actor.protective_suit_active_available
        {
            return false;
        }
        let consumes_active = expected_kind.has_consumable_active();
        let target_in_range = |from_id: &str, to_id: &str, radius: f32| {
            if from_id == to_id {
                return true;
            }
            scene_positions
                .and_then(|positions| {
                    Some((
                        positions.positions.get(from_id)?,
                        positions.positions.get(to_id)?,
                    ))
                })
                .is_none_or(|(from, to)| from.distance(*to) <= radius)
        };

        let mut defeat_outcomes = Vec::new();
        match action {
            ProtectiveSuitAction::MedicalInjection => {
                let Some(target_index) = encounter
                    .participants
                    .iter()
                    .position(|participant| participant.target_id == target_id)
                else {
                    return false;
                };
                if !encounter.participants[target_index].alive
                    || !target_in_range(actor_id, target_id, 1.0)
                {
                    return false;
                }
                let target_name = encounter.participants[target_index].display_name.clone();
                let resolution = apply_participant_healing_for_battle(
                    &mut encounter.participants[target_index],
                    3.0,
                    0.0,
                );
                encounter.action_log.push(format!(
                    "{}使用医疗防护服，为{}治疗{}点生命值",
                    actor.display_name,
                    target_name,
                    format_number(resolution.hp_restored)
                ));
                encounter.combat_log.push(CombatLogEntry {
                    round: encounter.round,
                    kind: CombatLogKind::Healing,
                    source_id: actor_id.to_owned(),
                    source_name: actor.display_name.clone(),
                    target_id: target_id.to_owned(),
                    target_name,
                    action_name: "医疗防护服".to_owned(),
                    base_amount: 3.0,
                    effective_amount: resolution.hp_restored,
                    modifiers: Vec::new(),
                    benefits: Vec::new(),
                });
            },
            ProtectiveSuitAction::PhotonFlash => {
                let actor_player_side = participant_is_player_side(&actor, manager);
                let mut affected = Vec::new();
                for target in &mut encounter.participants {
                    if target.alive
                        && target.target_id != actor_id
                        && participant_is_player_side(target, manager) != actor_player_side
                    {
                        target.protective_suit_blind_rounds_remaining =
                            target.protective_suit_blind_rounds_remaining.max(1);
                        affected.push(target.display_name.clone());
                    }
                }
                encounter.action_log.push(format!(
                    "{}释放光子防护服强光，致盲{}1回合",
                    actor.display_name,
                    if affected.is_empty() {
                        "无目标".to_owned()
                    } else {
                        affected.join("、")
                    }
                ));
            },
            ProtectiveSuitAction::CryogenicRelease => {
                let mut affected = Vec::new();
                for target in &mut encounter.participants {
                    if target.alive && target_in_range(actor_id, &target.target_id, 10.0) {
                        target.protective_suit_slow_rounds_remaining =
                            target.protective_suit_slow_rounds_remaining.max(1);
                        affected.push(target.display_name.clone());
                    }
                }
                encounter.action_log.push(format!(
                    "{}释放急冻气体，使{}移速减半1回合",
                    actor.display_name,
                    affected.join("、")
                ));
            },
            ProtectiveSuitAction::AcidVial => {
                let Some(center_index) = encounter
                    .participants
                    .iter()
                    .position(|participant| participant.target_id == target_id)
                else {
                    return false;
                };
                let center_name = encounter.participants[center_index].display_name.clone();
                let center_position = scene_positions
                    .and_then(|positions| positions.positions.get(target_id))
                    .copied();
                let target_indices = encounter
                    .participants
                    .iter()
                    .enumerate()
                    .filter(|(_, target)| target.alive)
                    .filter(|(_, target)| {
                        center_position.map_or(target.target_id == target_id, |center| {
                            scene_positions
                                .and_then(|positions| positions.positions.get(&target.target_id))
                                .is_some_and(|position| center.distance(*position) <= 2.0)
                        })
                    })
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                let mut affected = Vec::new();
                for target_index in target_indices {
                    let target = &mut encounter.participants[target_index];
                    let target_name = target.display_name.clone();
                    let resolution = apply_participant_typed_damage_for_battle(
                        target,
                        3.0,
                        actor_id,
                        encounter.active,
                        DamageType::Physical,
                    );
                    affected.push(format!(
                        "{}{}点",
                        target_name,
                        format_number(resolution.damage_applied)
                    ));
                    if let Some(outcome) = resolution.defeat_outcome {
                        defeat_outcomes.push(outcome);
                    }
                }
                encounter.action_log.push(format!(
                    "{}向{}投掷酸液：{}",
                    actor.display_name,
                    center_name,
                    affected.join("、")
                ));
            },
            ProtectiveSuitAction::AcidDoor => {
                encounter.action_log.push(format!(
                    "{}使用科学家服装的酸液腐蚀锁门（消耗1回合）",
                    actor.display_name
                ));
            },
            ProtectiveSuitAction::MaintenanceSaw => {
                let Some(target_index) = encounter
                    .participants
                    .iter()
                    .position(|participant| participant.target_id == target_id)
                else {
                    return false;
                };
                if !encounter.participants[target_index].alive
                    || !target_in_range(actor_id, target_id, 1.0)
                {
                    return false;
                }
                let target_name = encounter.participants[target_index].display_name.clone();
                let resolution = apply_participant_typed_damage_for_battle(
                    &mut encounter.participants[target_index],
                    2.0,
                    actor_id,
                    encounter.active,
                    DamageType::Physical,
                );
                encounter.action_log.push(format!(
                    "{}用维修锯撕裂{}，造成{}点物理伤害",
                    actor.display_name,
                    target_name,
                    format_number(resolution.damage_applied)
                ));
                if let Some(outcome) = resolution.defeat_outcome {
                    defeat_outcomes.push(outcome);
                }
            },
            ProtectiveSuitAction::MaintenanceDoor => {
                encounter.action_log.push(format!(
                    "{}使用维修锯拆除锁门（消耗1回合）",
                    actor.display_name
                ));
            },
        }
        if consumes_active {
            encounter.participants[actor_index].protective_suit_active_available = false;
        }
        for outcome in defeat_outcomes {
            apply_battle_defeat_outcome(encounter, outcome);
        }
        if consumes_active {
            if let Some(character) = manager.player_characters.get_mut(actor_id) {
                if character.protective_suit.kind == expected_kind {
                    let _ = character.protective_suit.consume_active();
                }
            }
        }
        self.finish_resolved_actor_action(encounter_id, actor_id)
    }

    fn apply_action(
        &mut self,
        encounter_id: &str,
        actor_id: &str,
        target_id: &str,
        action_name: &str,
        damage: f32,
    ) -> bool {
        if !self.encounter_is_canonical(encounter_id) {
            return false;
        }
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        let Some(actor) = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == actor_id)
        else {
            return false;
        };
        let actor_name = actor.display_name.clone();
        let actor_snapshot = actor.clone();
        if !participant_can_act(actor) {
            encounter.action_log.push(format!(
                "{}已经倒下或完成本轮行动，无法再次行动",
                actor_name
            ));
            return false;
        }
        let actor_hope_avatar_active = encounter.active && participant_hope_avatar_active(actor);
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
        if !target.alive {
            let target_name = target.display_name.clone();
            encounter.action_log.push(format!(
                "{}已经倒下，不能成为普通攻击目标",
                target_name
            ));
            return false;
        }
        let final_damage =
            damage.max(0.0) * personalized_pve_level_multiplier(&actor_snapshot, target);
        let resolution = apply_participant_typed_damage_for_battle(
            target,
            final_damage,
            actor_id,
            encounter.active,
            DamageType::Physical,
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
        if actor_snapshot.mirror_coat_layers > 0 && resolution.damage_applied > f32::EPSILON {
            let broke_stealth = encounter
                .participants
                .iter_mut()
                .find(|participant| participant.target_id == actor_id)
                .map(|actor| {
                    actor.mirror_coat_layers = 0;
                })
                .is_some();
            if broke_stealth {
                encounter.action_log.push(format!(
                    "{}造成伤害，脱离隐身",
                    actor_name
                ));
            }
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
        if !self.encounter_is_canonical(encounter_id)
            || !self.encounter_group_exists(encounter_id, manager)
        {
            return false;
        }
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
        let hidden_role_free_action =
            actor_snapshot.hidden_role.free_action_skill.as_deref() == Some(skill.name.trim());
        let can_use_hidden_role_free_action = hidden_role_free_action
            && actor_snapshot.alive
            && actor_snapshot.hidden_role.free_action_available
            && !actor_snapshot.hidden_role.cocooning;
        if !participant_can_act(&actor_snapshot) && !can_use_hidden_role_free_action {
            encounter.action_log.push(format!(
                "{}已经倒下或完成本轮行动，无法使用技能",
                actor_snapshot.display_name
            ));
            return false;
        }
        if hidden_role_free_action && !actor_snapshot.hidden_role.free_action_available {
            encounter.action_log.push(format!(
                "{}本轮已经使用过免费技能{}",
                actor_snapshot.display_name, skill.name
            ));
            return false;
        }
        let actor_character = character_for_participant(&actor_snapshot, manager);
        let mut effects = static_skill_effects(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            skill.legacy_buff_machine_json.as_deref(),
        );
        let available_needles = u16::from(actor_snapshot.flying_needles_ready)
            + u16::from(actor_snapshot.needle_case_ready);
        let uses_single_needle = is_current_redeemed_flying_needle(skill);
        let uses_all_needles = is_current_redeemed_blow_needles(skill);
        if (uses_single_needle || uses_all_needles) && available_needles == 0 {
            encounter.action_log.push(format!(
                "{}不能使用{}；当前没有飞针",
                actor_snapshot.display_name, skill.name
            ));
            return false;
        }
        if uses_all_needles {
            let mut found_damage = false;
            for effect in &mut effects {
                if let SkillEffect::Damage { amount, .. } = effect {
                    *amount = available_needles as f32 + 2.0;
                    found_damage = true;
                }
            }
            if !found_damage {
                effects.push(SkillEffect::Damage {
                    amount: available_needles as f32 + 2.0,
                    target: TargetSelector::single(ActorRef::Target),
                    damage_type: DamageType::Range,
                });
            }
        }
        let selected_target_alive = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == target_id)
            .map(|participant| participant.alive);
        if is_current_redeemed_commissar_war_cry(skill) && selected_target_alive != Some(true) {
            encounter.action_log.push(format!(
                "{}不能对所选目标使用{}；目标不存在或已经倒下",
                actor_snapshot.display_name, skill.name
            ));
            return false;
        }
        if !skill_effects_allow_selected_target(
            &effects,
            skill.target_class.as_deref(),
            selected_target_alive,
        ) {
            encounter.action_log.push(format!(
                "{}不能对所选目标使用{}；倒下目标只能接受单目标治疗",
                actor_snapshot.display_name, skill.name
            ));
            return false;
        }
        let actor_damage_dealt_buffs = actor_character
            .as_ref()
            .map(|character| character_damage_dealt_talent_buffs(character, actor_id))
            .unwrap_or_default();
        let mut actor_dealt_damage = false;
        let actor_source_notes = character_combat_source_notes(actor_character.as_ref(), "施法者");
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
        if encounter.active
            && participant_hope_avatar_active(&actor_snapshot)
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
        actor
            .skill_cooldown_ready_turns
            .remove(&skill.index.to_string());

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
        let mut pending_next_attack_bonus = actor_snapshot.next_attack_bonus_physical.max(0.0);
        let mut consumed_next_attack_bonus = false;
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
                    let actor_damage_modifiers = participant_damage_modifiers(
                        &actor_snapshot,
                        actor_character.as_ref(),
                        &basic_config,
                        completed_combat_turns(encounter),
                        damage_type,
                        encounter.active,
                    );
                    let actor_damage_multiplier =
                        actor_damage_modifiers.iter().fold(1.0, |value, modifier| {
                            value * modifier.multiplier
                        });
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
                        DefeatedTargetPolicy::Exclude,
                    );
                    let target_ids = limit_skill_targets(
                        target_ids,
                        skill_target_limit(
                            skill.target_count,
                            skill.target_class.as_deref(),
                        ),
                    );
                    let infinite_focus_target_id = if encounter.active {
                        infinite_focus_eligible_target_id(
                            target,
                            actor_id,
                            &target_ids,
                            skill.target_class.as_deref(),
                        )
                    } else {
                        None
                    };
                    if target_ids.is_empty() {
                        encounter.action_log.push(format!(
                            "{}使用{}，但范围内没有目标",
                            actor_name, skill.name
                        ));
                    }
                    let mut pending_actor_lifesteal = 0.0;
                    let mut pending_endless_pain_bonus_damage = if encounter.active {
                        endless_pain_bonus_damage(
                            actor_snapshot.endless_pain_bonus_damage_per_stack,
                            actor_snapshot.endless_pain_stacks,
                        )
                    } else {
                        0.0
                    };
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
                        let target_source_notes =
                            character_combat_source_notes(target_character.as_ref(), "目标");
                        let target_damage_modifiers = participant_damage_taken_modifiers(
                            target,
                            target_character.as_ref(),
                            damage_type,
                            encounter.active,
                        );
                        let target_damage_multiplier =
                            target_damage_modifiers.iter().fold(1.0, |value, modifier| {
                                value * modifier.multiplier
                            });
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
                        let attached_physical_damage = if damage_type == DamageType::Physical {
                            pending_next_attack_bonus
                        } else {
                            0.0
                        };
                        let incoming_amount = ((amount + attached_physical_damage)
                            * actor_damage_multiplier
                            * infinite_focus_multiplier
                            * personalized_pve_level_multiplier(&actor_snapshot, target)
                            * target_damage_multiplier)
                            .max(0.0);
                        let target_large_hit_modifier = target_character
                            .as_ref()
                            .map(character_large_hit_damage_taken_modifier)
                            .unwrap_or(1.0);
                        let large_hit_multiplier = large_hit_damage_taken_multiplier(
                            target.max_hp,
                            incoming_amount,
                            target_large_hit_modifier,
                        );
                        let typed_final_amount = (incoming_amount * large_hit_multiplier).max(0.0);
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
                            encounter.active,
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
                        let (mut final_amount, delayed_liquid_body_damage) =
                            if !encounter.active || participant_hope_avatar_active(target) {
                                (resolved_amount, 0.0)
                            } else {
                                participant_liquid_body_split_damage(target, resolved_amount)
                            };
                        let target_display_name = target.display_name.clone();
                        // 日薄崦嵫：距离超过10码后每1码减免1点伤害，至多减免20%。
                        if target.sunset_enabled {
                            let distance = scene_positions
                                .and_then(|positions| {
                                    let actor_position = positions.positions.get(actor_id)?;
                                    let target_position =
                                        positions.positions.get(&target.target_id)?;
                                    Some(actor_position.distance(*target_position))
                                })
                                .unwrap_or(0.0);
                            if distance > SUNSET_DAMAGE_DISTANCE_THRESHOLD {
                                let reduction = (distance - SUNSET_DAMAGE_DISTANCE_THRESHOLD)
                                    .min(final_amount.max(0.0) * SUNSET_DAMAGE_REDUCTION_CAP);
                                if reduction > f32::EPSILON {
                                    final_amount = (final_amount - reduction).max(0.0);
                                    encounter.action_log.push(format!(
                                        "{}距离{}码，日薄崦嵫减免{}点伤害",
                                        target_display_name,
                                        format_number(distance),
                                        format_number(reduction)
                                    ));
                                }
                            }
                        }
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
                        let resolution = if is_current_redeemed_void_break(skill) {
                            apply_void_break_damage_for_battle(
                                target,
                                final_amount,
                                actor_id,
                                encounter.active,
                            )
                        } else {
                            apply_participant_typed_damage_for_battle(
                                target,
                                final_amount,
                                actor_id,
                                encounter.active,
                                damage_type,
                            )
                        };
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
                        if resolution.damage_applied > f32::EPSILON {
                            actor_dealt_damage = true;
                            if attached_physical_damage > f32::EPSILON {
                                consumed_next_attack_bonus = true;
                                pending_next_attack_bonus = 0.0;
                            }
                            if actor_damage_dealt_buffs
                                .iter()
                                .any(|buff| buff.name == "溃伤")
                            {
                                target.wound_healing_taken_turns = 1;
                            }
                            if is_current_redeemed_chainsword(skill) {
                                target.natural_hp_regen_suppressed = true;
                            }
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
                        let mut modifiers = actor_damage_modifiers.clone();
                        modifiers.extend(target_damage_modifiers);
                        if (infinite_focus_multiplier - 1.0).abs() > f32::EPSILON {
                            modifiers.push(RuleAmountResolution::factor(
                                "无限专注",
                                infinite_focus_multiplier,
                            ));
                        }
                        if (large_hit_multiplier - 1.0).abs() > f32::EPSILON {
                            modifiers.push(RuleAmountResolution::factor(
                                "大额伤害承伤",
                                large_hit_multiplier,
                            ));
                        }
                        let mut benefits = actor_source_notes.clone();
                        benefits.extend(target_source_notes);
                        if resolution.damage_absorbed > f32::EPSILON {
                            let source = if is_current_redeemed_void_break(skill) {
                                "玄空破保留1点生命并由护盾吸收"
                            } else {
                                "护盾/免疫吸收"
                            };
                            benefits.push(format!(
                                "{}{}点",
                                source,
                                format_number(resolution.damage_absorbed)
                            ));
                        }
                        if endless_pain_bonus > f32::EPSILON {
                            benefits.push(format!(
                                "无尽痛楚追加{}点",
                                format_number(endless_pain_bonus)
                            ));
                        }
                        if delayed_liquid_body_damage > f32::EPSILON {
                            benefits.push(format!(
                                "液态躯体延后{}点",
                                format_number(delayed_liquid_body_damage)
                            ));
                        }
                        if attached_physical_damage > f32::EPSILON {
                            benefits.push(format!(
                                "为了帝皇附带{}点物理伤害",
                                format_number(attached_physical_damage)
                            ));
                        }
                        encounter.combat_log.push(CombatLogEntry {
                            round: encounter.round,
                            kind: CombatLogKind::Damage,
                            source_id: actor_id.to_owned(),
                            source_name: actor_name.clone(),
                            target_id: resolved_target_id.clone(),
                            target_name: target_display_name.clone(),
                            action_name: skill.name.clone(),
                            base_amount: amount,
                            effective_amount: resolution.damage_applied,
                            modifiers,
                            benefits,
                        });
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
                            let resolution = apply_participant_healing_for_battle(
                                actor,
                                pending_actor_lifesteal,
                                actor_snapshot.overhealing_shield_cap_rate,
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
                                "{}触发禅宗古训，回复{}点生命值",
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
                                action_name: "禅宗古训".to_owned(),
                                base_amount: pending_actor_lifesteal,
                                effective_amount,
                                modifiers: Vec::new(),
                                benefits,
                            });
                        }
                    }
                },
                SkillEffect::Heal { amount, target } => {
                    let actor_healing_modifiers = participant_healing_modifiers(
                        &actor_snapshot,
                        actor_character.as_ref(),
                        &basic_config,
                    );
                    let actor_healing_multiplier =
                        actor_healing_modifiers.iter().fold(1.0, |value, modifier| {
                            value * modifier.multiplier
                        });
                    let actor_mutual_aid_healing_rate = actor_character
                        .as_ref()
                        .map(character_mutual_aid_healing_rate)
                        .unwrap_or(0.0);
                    let actor_source_notes =
                        character_combat_source_notes(actor_character.as_ref(), "施法者");
                    let actor_echoing_memory_healing_rates = actor_character
                        .as_ref()
                        .and_then(|character| character_echoing_memory_healing_rates(character));
                    let actor_dying_target_healing_modifier = actor_character
                        .as_ref()
                        .map(character_dying_target_healing_modifier)
                        .unwrap_or(1.0);
                    let target_ids = resolve_skill_targets(
                        target,
                        actor_id,
                        target_id,
                        encounter,
                        scene_positions,
                        skill_range_radius(skill.range),
                        skill.target_class.as_deref(),
                        DefeatedTargetPolicy::AllowSingleTarget,
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
                        let target_source_notes =
                            character_combat_source_notes(target_character.as_ref(), "目标");
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
                            participant_healing_taken_multiplier(target)
                                * wound_multiplier
                                * dying_multiplier;
                        let one_heart_multiplier = if encounter.active
                            && single_heal_target_id.as_deref() == Some(resolved_target_id.as_str())
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
                        let healing_resolution = apply_participant_healing_for_battle(
                            target,
                            final_amount,
                            actor_snapshot.overhealing_shield_cap_rate,
                        );
                        let effective_amount = healing_resolution.effective_amount();
                        pending_actor_revenge_soul_healing += healing_resolution.hp_restored;
                        if resolved_target_id != actor_id && effective_amount > f32::EPSILON {
                            pending_actor_mutual_aid_healing += effective_amount
                                * (actor_mutual_aid_healing_rate + target_mutual_aid_healing_rate);
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
                            if let Some((first_echo_rate, second_echo_rate)) =
                                actor_echoing_memory_healing_rates
                            {
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
                            skill.name,
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
                            action_name: skill.name.clone(),
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
                        DefeatedTargetPolicy::Exclude,
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
                        if skill.name == "电能冲击"
                            && actor_snapshot.summon_kind == SummonKind::ArmedDrone
                        {
                            if let Some(target) = encounter
                                .participants
                                .iter_mut()
                                .find(|participant| participant.target_id == resolved_target_id)
                            {
                                target.paralyzed_rounds_remaining = target
                                    .paralyzed_rounds_remaining
                                    .max(buff.turns_remaining.max(1) as u32);
                            }
                        }
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
                        encounter.combat_log.push(CombatLogEntry {
                            round: encounter.round,
                            kind: CombatLogKind::Buff,
                            source_id: actor_id.to_owned(),
                            source_name: actor_name.clone(),
                            target_id: resolved_target_id,
                            target_name,
                            action_name: skill.name.clone(),
                            base_amount: 0.0,
                            effective_amount: 0.0,
                            modifiers: Vec::new(),
                            benefits: {
                                let mut benefits = vec![format!(
                                    "{}（{}回合）",
                                    buff.name, buff.turns_remaining,
                                )];
                                if buff.beneficial {
                                    benefits.push("有益".to_owned());
                                }
                                benefits
                            },
                        });
                    }
                },
            }
        }
        if consumed_next_attack_bonus {
            if let Some(actor) = encounter
                .participants
                .iter_mut()
                .find(|participant| participant.target_id == actor_id)
            {
                actor.next_attack_bonus_physical = 0.0;
            }
        }
        if is_current_redeemed_commissar_war_cry(skill) {
            let target_index = encounter
                .participants
                .iter()
                .position(|participant| participant.target_id == target_id)
                .expect("commissar war cry target was validated before applying effects");
            let proficiency = encounter
                .participants
                .iter_mut()
                .find(|participant| participant.target_id == actor_id)
                .map(|actor| {
                    actor.commissar_proficiency =
                        actor.commissar_proficiency.saturating_add(1).min(5);
                    actor.commissar_proficiency
                })
                .unwrap_or(0);
            if proficiency > 0 {
                let target = &mut encounter.participants[target_index];
                target.next_attack_bonus_physical = proficiency as f32;
                encounter.action_log.push(format!(
                    "{}高呼“为了帝皇”，熟练度提升至{}/5；{}的下次物理攻击附带{}点物理伤害",
                    actor_name, proficiency, target.display_name, proficiency
                ));
            }
        }
        if is_current_redeemed_bolter(skill) {
            encounter.action_log.push(format!(
                "广播给周围玩家：{}开火——{}",
                actor_name,
                skill.note.trim()
            ));
        }
        if uses_single_needle || uses_all_needles {
            let consumed_state = encounter
                .participants
                .iter_mut()
                .find(|participant| participant.target_id == actor_id)
                .map(|actor| {
                    let consumed = if uses_all_needles {
                        let consumed = u16::from(actor.flying_needles_ready)
                            + u16::from(actor.needle_case_ready);
                        actor.flying_needles_ready = 0;
                        actor.needle_case_ready = 0;
                        consumed
                    } else if actor.flying_needles_ready > 0 {
                        actor.flying_needles_ready -= 1;
                        1
                    } else {
                        actor.needle_case_ready = actor.needle_case_ready.saturating_sub(1);
                        1
                    };
                    (
                        consumed,
                        actor.flying_needles_ready,
                        actor.needle_case_ready,
                        actor.needle_case_enabled,
                    )
                });
            if let Some((consumed, ready, case_ready, case_enabled)) = consumed_state {
                encounter.action_log.push(format!(
                    "{}消耗{}根飞针（常备{}/5，针匣{}/{})",
                    actor_name,
                    consumed,
                    ready,
                    case_ready,
                    if case_enabled { 2 } else { 0 }
                ));
            }
        }
        if let Some(duration) = current_redeemed_invisibility_rounds(skill) {
            if let Some(actor) = encounter
                .participants
                .iter_mut()
                .find(|participant| participant.target_id == actor_id)
            {
                actor.redeemed_invisibility_rounds_remaining = duration;
                encounter.action_log.push(format!(
                    "{}进入隐身，持续{}回合",
                    actor_name, duration
                ));
            }
        }
        if mp_cost > 0.0 {
            encounter.action_log.push(format!(
                "{}消耗{} MP",
                actor_name,
                format_number(mp_cost)
            ));
        }
        if skill.name == "食用精美烧鹅" {
            if let Some(encounter) = self.encounters.get_mut(encounter_id) {
                let started = encounter
                    .participants
                    .iter_mut()
                    .find(|participant| participant.target_id == actor_id)
                    .map(|actor| {
                        actor.goose_channeling_turns = GOOSE_CHANNEL_TURNS;
                    })
                    .is_some();
                if started {
                    encounter.action_log.push(format!(
                        "{}开始食用精美烧鹅，引导{}回合",
                        actor_name, GOOSE_CHANNEL_TURNS
                    ));
                }
            }
        }
        if skill.name == "维修机甲" && actor_snapshot.summon_kind == SummonKind::Mech {
            if let Some(encounter) = self.encounters.get_mut(encounter_id) {
                let started = encounter
                    .participants
                    .iter_mut()
                    .find(|participant| participant.target_id == actor_id)
                    .map(|actor| {
                        actor.construct_repair_channel_rounds_remaining = 2;
                    })
                    .is_some();
                if started {
                    encounter.action_log.push(format!(
                        "{}开始维修机甲，引导2回合；受到生命伤害会打断维修",
                        actor_name
                    ));
                }
            }
        }
        if actor_dealt_damage {
            if let Some(encounter) = self.encounters.get_mut(encounter_id) {
                apply_corrosion_wave_after_attack(
                    encounter,
                    actor_id,
                    actor_character.as_ref(),
                    manager,
                );
            }
        }
        if actor_dealt_damage && skill.name.trim() == "异形震慑尖啸" {
            if let Some(encounter) = self.encounters.get_mut(encounter_id) {
                let actor_player_side = encounter
                    .participants
                    .iter()
                    .find(|participant| participant.target_id == actor_id)
                    .map(|participant| participant_is_player_side(participant, manager))
                    .unwrap_or(true);
                let actor_position = scene_positions
                    .and_then(|positions| positions.positions.get(actor_id))
                    .copied();
                let mut affected = Vec::new();
                for target in &mut encounter.participants {
                    if !target.alive
                        || target.target_id == actor_id
                        || participant_is_player_side(target, manager) == actor_player_side
                    {
                        continue;
                    }
                    let in_range = actor_position.is_none_or(|actor_position| {
                        scene_positions
                            .and_then(|positions| positions.positions.get(&target.target_id))
                            .is_some_and(|target_position| {
                                actor_position.distance(*target_position) <= 5.0
                            })
                    });
                    if in_range {
                        target.paralyzed_rounds_remaining =
                            target.paralyzed_rounds_remaining.max(1);
                        affected.push(target.display_name.clone());
                    }
                }
                if !affected.is_empty() {
                    encounter.action_log.push(format!(
                        "{}的震慑尖啸使{}麻痹1回合",
                        actor_name,
                        affected.join("、")
                    ));
                }
            }
        }
        if actor_dealt_damage && actor_snapshot.mirror_coat_layers > 0 {
            if let Some(encounter) = self.encounters.get_mut(encounter_id) {
                let broke_stealth = encounter
                    .participants
                    .iter_mut()
                    .find(|participant| participant.target_id == actor_id)
                    .map(|actor| {
                        actor.mirror_coat_layers = 0;
                    })
                    .is_some();
                if broke_stealth {
                    encounter.action_log.push(format!(
                        "{}造成伤害，脱离隐身",
                        actor_name
                    ));
                }
            }
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
                        DefeatedTargetPolicy::Exclude,
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

        let max_hp_adjustments = self
            .encounters
            .get(encounter_id)
            .map(|encounter| apply_battle_manager_max_hp_adjustments(encounter, manager))
            .unwrap_or_default();
        let _ = sync_encounter_to_manager(
            self.encounters.get(encounter_id),
            manager,
        );
        let skill_pool = manager.skill_pool.clone();
        let mut rule_engine_state = RuleEngineState::default();
        let mut refreshed_player_ids = HashSet::new();
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
                    refreshed_player_ids.insert(resolved_target_id.clone());
                }
            }
        }
        restore_battle_manager_max_hp_adjustments(&max_hp_adjustments, manager);
        if let Some(encounter) = self.encounters.get_mut(encounter_id) {
            for participant in encounter
                .participants
                .iter_mut()
                .filter(|participant| refreshed_player_ids.contains(&participant.target_id))
            {
                sync_participant_from_manager_with_vitals(participant, manager);
            }
        }
        let _ = sync_encounter_to_manager(
            self.encounters.get(encounter_id),
            manager,
        );
        true
    }

    fn record_skill_use_with_buffs_and_finish(
        &mut self,
        encounter_id: &str,
        actor_id: &str,
        target_id: &str,
        skill: &CharacterSkill,
        manager: &mut NapcatMessageManager,
        scene_positions: Option<&SceneCharacterPositions>,
    ) -> bool {
        if !self.record_skill_use_with_buffs(
            encounter_id,
            actor_id,
            target_id,
            skill,
            manager,
            scene_positions,
        ) {
            return false;
        }
        let consumed_item_name = manager
            .player_characters
            .get_mut(actor_id)
            .and_then(|character| consume_item_skill(character, skill.index));
        if let Some(item_name) = consumed_item_name {
            let actor_name = self
                .encounters
                .get(encounter_id)
                .and_then(|encounter| {
                    encounter
                        .participants
                        .iter()
                        .find(|participant| participant.target_id == actor_id)
                })
                .map(|participant| participant.display_name.clone())
                .unwrap_or_else(|| actor_id.to_owned());
            if let Some(encounter) = self.encounters.get_mut(encounter_id) {
                encounter.action_log.push(format!(
                    "{}消耗了1个{}",
                    actor_name, item_name
                ));
            }
        }
        if let Some(encounter) = self.encounters.get_mut(encounter_id) {
            apply_sunset_world_time_reset(encounter, manager);
        }
        let used_hidden_role_free_action = self
            .encounters
            .get_mut(encounter_id)
            .and_then(|encounter| {
                encounter
                    .participants
                    .iter_mut()
                    .find(|participant| participant.target_id == actor_id)
            })
            .is_some_and(|actor| {
                if actor.hidden_role.free_action_available
                    && actor.hidden_role.free_action_skill.as_deref() == Some(skill.name.trim())
                {
                    actor.hidden_role.free_action_available = false;
                    true
                } else {
                    false
                }
            });
        if used_hidden_role_free_action {
            return true;
        }
        self.finish_resolved_actor_action(encounter_id, actor_id)
    }

    fn advance_participant(&mut self, encounter_id: &str, target_id: &str, resume: bool) -> bool {
        if !self.encounter_is_canonical(encounter_id) {
            return false;
        }
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
                if !participant.natural_hp_regen_suppressed {
                    participant.hp =
                        (participant.hp + participant.hp_regen).min(participant.max_hp);
                }
                advance_participant_rest_then_fight(participant);
            }
            participant.mp = (participant.mp + participant.mp_regen).min(participant.max_mp);
        }
        let previous_damage_taken = participant.damage_taken_this_turn;
        reset_participant_turn_totals(participant);
        participant.undying_rage_active = false;
        advance_participant_overhealing_shield(participant);
        let mut delayed_logs = Vec::new();
        let mut delayed_combat_log = Vec::new();
        let (hope_log, hope_outcome) = advance_participant_hope_avatar(participant);
        let mut defeat_outcomes = hope_outcome.into_iter().collect::<Vec<_>>();
        participant.inspiration_sources.retain(|_, turns| {
            *turns = turns.saturating_sub(1);
            *turns > 0
        });
        if let Some(log) = hope_log {
            delayed_logs.push(log);
        } else {
            if encounter.active {
                if let Some((log, combat_entry)) = apply_participant_liquid_body_healing(
                    participant,
                    previous_damage_taken,
                    encounter.round,
                ) {
                    delayed_logs.push(log);
                    delayed_combat_log.push(combat_entry);
                }
            }
            if participant.wound_healing_taken_turns > 0 {
                participant.wound_healing_taken_turns -= 1;
            }
            let delayed = advance_participant_delayed_damage_ticks(
                participant,
                encounter.active,
                encounter.round,
            );
            delayed_logs.extend(delayed.logs);
            delayed_combat_log.extend(delayed.combat_log);
            defeat_outcomes.extend(delayed.defeat_outcomes);
            let corrosion = advance_participant_corrosion(
                participant,
                encounter.active,
                encounter.round,
            );
            delayed_logs.extend(corrosion.logs);
            delayed_combat_log.extend(corrosion.combat_log);
            defeat_outcomes.extend(corrosion.defeat_outcomes);
            let delayed_healing =
                advance_participant_delayed_healing_ticks(participant, encounter.round);
            delayed_logs.extend(delayed_healing.logs);
            delayed_combat_log.extend(delayed_healing.combat_log);
        }
        participant.turn = participant.turn.saturating_add(1);
        if encounter.active {
            participant.combat_turns_completed =
                participant.combat_turns_completed.saturating_add(1);
            encounter.combat_completed_turns = encounter.combat_completed_turns.saturating_add(1);
            participant.hidden_role.free_action_available =
                participant.hidden_role.free_action_skill.is_some();
        }
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
        encounter.combat_log.extend(delayed_combat_log);
        for outcome in defeat_outcomes {
            apply_battle_defeat_outcome(encounter, outcome);
        }
        encounter.action_log.extend(delayed_logs);
        true
    }

    fn skip_negative_participant(&mut self, encounter_id: &str, target_id: &str) -> bool {
        if !self.encounter_is_canonical(encounter_id) {
            return false;
        }
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
        if !participant_can_act(participant) {
            return false;
        }
        participant.negative_layers = participant.negative_layers.saturating_add(1);
        participant.pending_negative = false;
        let _ = participant;
        self.finish_actor_action(encounter_id, target_id)
    }
}

fn refresh_encounter_players(
    encounter: &mut BattleEncounter,
    manager: &NapcatMessageManager,
) -> bool {
    let Some(group_name) = encounter.trpg_group.clone() else {
        return false;
    };
    let Some(group) = manager.trpg_groups.get(&group_name) else {
        return false;
    };
    let group_modifiers = group
        .global_combat_modifiers
        .effective_at_world_turn(group.world_turn);

    let before_signature = encounter_participants_signature(&encounter.participants);
    deduplicate_encounter_participants(encounter);
    encounter.participants.retain(|participant| {
        participant.unit_template_id.is_some()
            || group.players.contains(&participant.target_id)
            || (participant.is_summon
                && participant
                    .summon_owner_id
                    .as_ref()
                    .is_some_and(|owner_id| group.players.contains(owner_id)))
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
                Some(&group_name),
                manager,
            );
            encounter.participants.push(participant);
        }
    }
    for participant in &mut encounter.participants {
        participant.group_modifiers = group_modifiers;
    }
    before_signature != encounter_participants_signature(&encounter.participants)
}

fn deduplicate_encounter_participants(encounter: &mut BattleEncounter) -> bool {
    let before_len = encounter.participants.len();
    let mut seen_target_ids = HashSet::new();
    encounter
        .participants
        .retain(|participant| seen_target_ids.insert(participant.target_id.clone()));
    before_len != encounter.participants.len()
}

fn prune_unbound_group_participants(
    encounter: &mut BattleEncounter,
    manager: &NapcatMessageManager,
) -> bool {
    let Some(group) = encounter
        .trpg_group
        .as_deref()
        .and_then(|group_name| manager.trpg_groups.get(group_name))
    else {
        return false;
    };
    let before_len = encounter.participants.len();
    encounter.participants.retain(|participant| {
        participant.unit_template_id.is_some()
            || group.players.contains(&participant.target_id)
            || (participant.is_summon
                && participant
                    .summon_owner_id
                    .as_ref()
                    .is_some_and(|owner_id| group.players.contains(owner_id)))
    });
    before_len != encounter.participants.len()
}

fn sync_encounter_from_group_clock(
    store: &mut BattleRoundStore,
    encounter_id: &str,
    manager: &NapcatMessageManager,
) -> bool {
    if !store.encounter_is_canonical(encounter_id)
        || !store.encounter_group_exists(encounter_id, manager)
    {
        return false;
    }
    let Some(group_name) = store
        .encounters
        .get(encounter_id)
        .and_then(|encounter| encounter.trpg_group.as_deref())
    else {
        return false;
    };
    let Some(group) = manager.trpg_groups.get(group_name) else {
        return false;
    };
    let group_round = group.world_turn;
    let group_turns = group.player_turns.clone();
    let mut changed = false;

    let encounter_round = store
        .encounters
        .get(encounter_id)
        .map(|encounter| encounter.round)
        .unwrap_or_default();
    if group_round > encounter_round {
        if let Some(encounter) = store.encounters.get_mut(encounter_id) {
            for participant in encounter
                .participants
                .iter_mut()
                .filter(|participant| participant.player_character)
            {
                sync_participant_from_manager_with_vitals(participant, manager);
            }
        }
        let rounds_to_advance = group_round
            .saturating_sub(encounter_round)
            .min(MAX_GROUP_CLOCK_CATCH_UP_ROUNDS_PER_FRAME);
        for _ in 0..rounds_to_advance {
            changed |= store.next_round(encounter_id);
            changed |= advance_unit_participant_buffs(store, encounter_id, manager);
        }
    }

    if store
        .encounters
        .get(encounter_id)
        .is_some_and(|encounter| encounter.round < group_round)
    {
        return changed;
    }

    let Some(encounter) = store.encounters.get_mut(encounter_id) else {
        return changed;
    };
    let encounter_active = encounter.active;
    let mut completed_combat_turns = 0_u32;
    for participant in encounter
        .participants
        .iter_mut()
        .filter(|participant| participant.player_character && participant.alive)
    {
        let Some(turn) = group_turns.get(&participant.target_id) else {
            continue;
        };
        let finished = turn.acted || turn.skipped;
        let effective_turn = turn.turns_passed.saturating_add(u32::from(finished));
        if effective_turn > participant.turn {
            let advanced = effective_turn - participant.turn;
            participant.turn = effective_turn;
            if encounter_active {
                participant.combat_turns_completed =
                    participant.combat_turns_completed.saturating_add(advanced);
                completed_combat_turns = completed_combat_turns.saturating_add(advanced);
            }
            changed = true;
        }
        if finished && effective_turn >= participant.turn && !participant.action_done {
            participant.action_done = true;
            participant.pending_negative = false;
            changed = true;
        }
    }
    if completed_combat_turns > 0 {
        encounter.combat_completed_turns = encounter
            .combat_completed_turns
            .saturating_add(completed_combat_turns);
    }
    if changed && encounter.negative_enabled {
        mark_negative_candidates(encounter);
    }
    changed
}

fn group_rounds_ahead_of_encounter(
    store: &BattleRoundStore,
    encounter_id: &str,
    manager: &NapcatMessageManager,
) -> u32 {
    let Some(encounter) = store.encounters.get(encounter_id) else {
        return 0;
    };
    let Some(group) = encounter
        .trpg_group
        .as_deref()
        .and_then(|group_name| manager.trpg_groups.get(group_name))
    else {
        return 0;
    };
    group.world_turn.saturating_sub(encounter.round)
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
    let mut cooldown_character = character.clone();
    crate::napcat::materialize_imported_skill_cooldowns(
        &mut cooldown_character,
        participant.turn,
    );
    participant.skill_cooldown_ready_turns = cooldown_character.skill_cooldown_ready_turns;
}

fn sync_encounter_to_manager(
    encounter: Option<&BattleEncounter>,
    manager: &mut NapcatMessageManager,
) -> bool {
    let Some(encounter) = encounter else {
        return false;
    };
    if encounter.manager_sync_quarantined {
        return false;
    }
    let linked_player_ids = if let Some(group_name) = encounter.trpg_group.as_deref() {
        let Some(group) = manager.trpg_groups.get(group_name) else {
            return false;
        };
        if encounter
            .trpg_campaign_id
            .as_deref()
            .is_some_and(|bound_id| bound_id != trpg_group_campaign_id(group))
        {
            return false;
        }
        Some(group.players.iter().cloned().collect::<HashSet<_>>())
    } else {
        None
    };
    let mut changed = false;

    for participant in &encounter.participants {
        if participant.is_summon {
            let Some(owner_id) = participant.summon_owner_id.as_deref() else {
                continue;
            };
            if linked_player_ids
                .as_ref()
                .is_some_and(|player_ids| !player_ids.contains(owner_id))
            {
                continue;
            }
            let Some(index) = crate::napcat::parse_summon_target_id(&participant.target_id)
                .map(|(_, index)| index)
            else {
                continue;
            };
            let stat_config = manager.character_stat_config_for_target(owner_id);
            let Some(character) = manager.player_characters.get_mut(owner_id) else {
                continue;
            };
            let _ = crate::napcat::sync_character_summons(character, &stat_config);
            if let Some(summon) = character.summons.get_mut(index) {
                let hp = participant.hp.clamp(0.0, summon.max_hp.max(0.0));
                if (summon.hp - hp).abs() > f32::EPSILON {
                    summon.hp = hp;
                    changed = true;
                }
                let shield = participant
                    .construct_shield
                    .clamp(0.0, summon.max_shield.max(0.0));
                if (summon.shield - shield).abs() > f32::EPSILON {
                    summon.shield = shield;
                    changed = true;
                }
                if summon.shield_repair_rounds_remaining
                    != participant.construct_shield_repair_rounds_remaining
                {
                    summon.shield_repair_rounds_remaining =
                        participant.construct_shield_repair_rounds_remaining;
                    changed = true;
                }
                if summon.repair_channel_rounds_remaining
                    != participant.construct_repair_channel_rounds_remaining
                {
                    summon.repair_channel_rounds_remaining =
                        participant.construct_repair_channel_rounds_remaining;
                    changed = true;
                }
            }
            continue;
        }
        if participant.unit_template_id.is_some() {
            if let Some(instance) = manager.unit_instances.get_mut(&participant.target_id) {
                let mut character = participant
                    .unit_character
                    .clone()
                    .unwrap_or_else(|| instance.character.clone());
                character.name = participant.display_name.clone();
                character.nickname = participant.display_name.clone();
                character.hp = participant.hp.clamp(0.0, participant.max_hp.max(0.0));
                character.max_hp = participant.max_hp.max(0.0);
                character.mp = participant.mp.clamp(0.0, participant.max_mp.max(0.0));
                character.max_mp = participant.max_mp.max(0.0);
                character.hp_regen = participant.hp_regen;
                character.mp_regen = participant.mp_regen;
                character.speed = participant.speed;
                character.damage_taken_this_turn = participant.damage_taken_this_turn;
                character.healing_taken_this_turn = participant.healing_taken_this_turn;
                character.skill_last_cast_turns = participant.skill_last_used_turns.clone();
                character.skill_cooldown_ready_turns =
                    participant.skill_cooldown_ready_turns.clone();
                instance.display_name = participant.display_name.clone();
                instance.character = character;
                changed = true;
            }
            continue;
        }
        if !participant.player_character {
            continue;
        }
        if linked_player_ids
            .as_ref()
            .is_some_and(|player_ids| !player_ids.contains(&participant.target_id))
        {
            continue;
        }
        let stat_config = manager.character_stat_config_for_target(&participant.target_id);
        let Some(character) = manager.player_characters.get_mut(&participant.target_id) else {
            continue;
        };
        if character.level != participant.level {
            let level_delta = (participant.level - character.level).max(0);
            character.level = participant.level.max(1);
            character.status_points = character
                .status_points
                .saturating_add(level_delta * STATUS_POINTS_PER_LEVEL);
            update_character_from_status_with_config(character, &stat_config);
            changed = true;
        }
        if character.exp != participant.exp {
            character.exp = participant.exp.max(0);
            changed = true;
        }
        let commissar_proficiency = participant.commissar_proficiency.min(5);
        if character.redeemed_commissar_proficiency != commissar_proficiency {
            character.redeemed_commissar_proficiency = commissar_proficiency;
            changed = true;
        }
        if participant.flying_needles_enabled
            && character_redeemed_needle_state(character).is_some()
        {
            let needle_state = RedeemedNeedleState {
                ready: participant.flying_needles_ready.min(5),
                case_ready: if participant.needle_case_enabled {
                    participant.needle_case_ready.min(2)
                } else {
                    0
                },
                case_progress_noncombat_rounds: if participant.needle_case_enabled {
                    participant.needle_case_progress_noncombat_rounds.min(1)
                } else {
                    0
                },
            };
            if character.redeemed_needles != Some(needle_state) {
                character.redeemed_needles = Some(needle_state);
                changed = true;
            }
        }
        let dominion_bonus = participant.dominion_max_hp_bonus.clamp(
            0.0,
            character_dominion_max_hp_bonus_cap(character).max(0.0),
        );
        if (character.dominion_max_hp_bonus - dominion_bonus).abs() > f32::EPSILON {
            character.dominion_max_hp_bonus = dominion_bonus;
            changed = true;
        }
        if participant.mirror_coat_cleanup_pending
            && remove_character_damage_dealing_buffs(character) > 0
        {
            changed = true;
        }
        let effective_max_hp = (character.max_hp + character.dominion_max_hp_bonus).max(0.0);
        let hp = participant.hp.clamp(0.0, effective_max_hp);
        let mp = participant.mp.clamp(0.0, character.max_mp.max(0.0));
        if (character.hp - hp).abs() > f32::EPSILON {
            if let Some(base_stats) = character.buff_base_stats.as_mut() {
                base_stats.hp = (base_stats.hp + hp - character.hp).max(0.0);
            }
            character.hp = hp;
            changed = true;
        }
        if (character.mp - mp).abs() > f32::EPSILON {
            if let Some(base_stats) = character.buff_base_stats.as_mut() {
                base_stats.mp = (base_stats.mp + mp - character.mp).max(0.0);
            }
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
        if character.skill_cooldown_ready_turns != participant.skill_cooldown_ready_turns {
            character.skill_cooldown_ready_turns = participant.skill_cooldown_ready_turns.clone();
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
    let manager_round_ahead = group.world_turn > encounter.round;
    let encounter_round_ahead = group.world_turn < encounter.round;
    if encounter_round_ahead {
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

        // The group clock stores completed rounds plus a current-round flag,
        // while a battle participant increments `turn` as soon as they act.
        // Do not write both the incremented turn and `acted`, which would count
        // the same action twice. An explicitly newer group clock/skip also wins
        // over a stale open encounter until that encounter catches up.
        if manager_round_ahead {
            continue;
        }
        if encounter_round_ahead {
            let completed_turns = participant
                .turn
                .saturating_sub(u32::from(participant.action_done));
            if turn.turns_passed <= completed_turns {
                if turn.turns_passed != completed_turns
                    || turn.acted != participant.action_done
                    || turn.skipped
                {
                    turn.turns_passed = completed_turns;
                    turn.acted = participant.action_done;
                    turn.skipped = false;
                    changed = true;
                }
            }
            continue;
        }

        let group_effective_turn = turn
            .turns_passed
            .saturating_add(u32::from(turn.acted || turn.skipped));
        if group_effective_turn >= participant.turn {
            continue;
        }
        let completed_turns = participant
            .turn
            .saturating_sub(u32::from(participant.action_done));
        if turn.turns_passed != completed_turns
            || turn.acted != participant.action_done
            || turn.skipped
        {
            turn.turns_passed = completed_turns;
            turn.acted = participant.action_done;
            turn.skipped = false;
            changed = true;
        }
    }
    changed |= group.refresh_legacy_negative_timers();
    changed
}

#[derive(Debug)]
struct BattleManagerMaxHpAdjustment {
    target_id: String,
    bonus: f32,
}

fn apply_battle_manager_max_hp_adjustments(
    encounter: &BattleEncounter,
    manager: &mut NapcatMessageManager,
) -> Vec<BattleManagerMaxHpAdjustment> {
    if !encounter.active {
        return Vec::new();
    }
    let mut adjustments = Vec::new();
    for participant in &encounter.participants {
        let bonus = participant.dominion_max_hp_bonus.max(0.0);
        if !participant.player_character
            || participant.unit_template_id.is_some()
            || bonus <= f32::EPSILON
        {
            continue;
        }
        let Some(character) = manager.player_characters.get_mut(&participant.target_id) else {
            continue;
        };
        character.max_hp += bonus;
        if let Some(base_stats) = character.buff_base_stats.as_mut() {
            base_stats.max_hp += bonus;
        }
        adjustments.push(BattleManagerMaxHpAdjustment {
            target_id: participant.target_id.clone(),
            bonus,
        });
    }
    adjustments
}

fn restore_battle_manager_max_hp_adjustments(
    adjustments: &[BattleManagerMaxHpAdjustment],
    manager: &mut NapcatMessageManager,
) {
    for adjustment in adjustments {
        let Some(character) = manager.player_characters.get_mut(&adjustment.target_id) else {
            continue;
        };
        character.max_hp = (character.max_hp - adjustment.bonus).max(0.0);
        if let Some(base_stats) = character.buff_base_stats.as_mut() {
            base_stats.max_hp = (base_stats.max_hp - adjustment.bonus).max(0.0);
        }
    }
}

fn sync_battle_round_buff_advancement(
    store: &mut BattleRoundStore,
    encounter_id: &str,
    previous_round: u32,
    manager: &mut NapcatMessageManager,
    rule_engine_state: &mut RuleEngineState,
) -> bool {
    if !store.encounter_is_canonical(encounter_id)
        || !store.encounter_group_exists(encounter_id, manager)
    {
        return false;
    }
    let Some(encounter) = store.encounters.get(encounter_id) else {
        return false;
    };
    if encounter.round <= previous_round {
        return false;
    }
    let canonical_round = if let Some(group_name) = encounter.trpg_group.as_deref() {
        let Some(group) = manager.trpg_groups.get(group_name) else {
            return false;
        };
        group.world_turn
    } else {
        previous_round
    };
    let rounds_to_advance = encounter
        .round
        .saturating_sub(canonical_round.max(previous_round));
    let player_ids = encounter
        .participants
        .iter()
        .filter(|participant| participant.player_character)
        .map(|participant| participant.target_id.clone())
        .collect::<Vec<_>>();

    if rounds_to_advance == 0 {
        return sync_encounter_to_manager(Some(encounter), manager);
    }
    let max_hp_adjustments = apply_battle_manager_max_hp_adjustments(encounter, manager);
    let _ = sync_encounter_to_manager(Some(encounter), manager);
    for _ in 0..rounds_to_advance {
        let _ = advance_buffs_for_players(manager, &player_ids, rule_engine_state);
        let _ = advance_unit_participant_buffs(store, encounter_id, manager);
    }
    restore_battle_manager_max_hp_adjustments(&max_hp_adjustments, manager);
    if let Some(encounter) = store.encounters.get_mut(encounter_id) {
        for participant in encounter
            .participants
            .iter_mut()
            .filter(|participant| participant.player_character)
        {
            sync_participant_from_manager_with_vitals(participant, manager);
        }
    }
    let _ = sync_encounter_to_manager(
        store.encounters.get(encounter_id),
        manager,
    );
    if let Some(encounter) = store.encounters.get_mut(encounter_id) {
        apply_sunset_world_time_reset(encounter, manager);
    }
    true
}

#[derive(Clone)]
struct BattleBuffTick {
    source_id: String,
    target_id: String,
    action_name: String,
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
                        action_name: buff.name.clone(),
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
        let source_index = encounter
            .participants
            .iter()
            .position(|participant| participant.target_id == tick.source_id);
        let source_character = source_index
            .and_then(|index| character_for_participant(&encounter.participants[index], manager))
            .or_else(|| manager.player_characters.get(&tick.source_id).cloned());
        let source_name = source_index
            .map(|index| encounter.participants[index].display_name.clone())
            .unwrap_or_else(|| tick.source_id.clone());
        let Some(target_index) = encounter
            .participants
            .iter()
            .position(|participant| participant.target_id == tick.target_id)
        else {
            continue;
        };
        if !encounter.participants[target_index].alive {
            continue;
        }
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
                let stat_config = encounter_basic_config(encounter, manager, &tick.source_id);
                let source_multiplier = source_index
                    .map(|index| {
                        participant_damage_multiplier(
                            &encounter.participants[index],
                            source_character.as_ref(),
                            &stat_config,
                            encounter.combat_completed_turns,
                            damage_type,
                            encounter.active,
                        )
                    })
                    .unwrap_or_else(|| {
                        source_character
                            .as_ref()
                            .map(|source| {
                                source.damage_dealt_modifier
                                    * character_low_hp_damage_multiplier(source)
                                    * character_damage_attribute_multiplier(
                                        source,
                                        &stat_config,
                                        trpg_damage_bonus_kind(damage_type),
                                    )
                            })
                            .unwrap_or(1.0)
                    });
                let target = &encounter.participants[target_index];
                let target_multiplier = participant_damage_taken_multiplier(
                    target,
                    target_character.as_ref(),
                    damage_type,
                    encounter.active,
                );
                let incoming_amount =
                    (amount.max(0.0) * source_multiplier * target_multiplier).max(0.0);
                let target_large_hit_modifier = target_character
                    .as_ref()
                    .map(character_large_hit_damage_taken_modifier)
                    .unwrap_or(1.0);
                let final_amount = (incoming_amount
                    * large_hit_damage_taken_multiplier(
                        target.max_hp,
                        incoming_amount,
                        target_large_hit_modifier,
                    ))
                .max(0.0);
                let resolution = apply_participant_typed_damage_for_battle(
                    &mut encounter.participants[target_index],
                    final_amount,
                    &tick.source_id,
                    encounter.active,
                    damage_type,
                );
                let mut benefits = Vec::new();
                if resolution.damage_absorbed > f32::EPSILON {
                    benefits.push(format!(
                        "护盾/免疫吸收{}点",
                        format_number(resolution.damage_absorbed)
                    ));
                }
                encounter.combat_log.push(CombatLogEntry {
                    round: encounter.round,
                    kind: CombatLogKind::Damage,
                    source_id: tick.source_id.clone(),
                    source_name: source_name.clone(),
                    target_id: tick.target_id.clone(),
                    target_name: target_name.clone(),
                    action_name: tick.action_name.clone(),
                    base_amount: amount.max(0.0),
                    effective_amount: resolution.damage_applied,
                    modifiers: Vec::new(),
                    benefits,
                });
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
            BuffTickAction::FixedDamage {
                amount,
                damage_type,
            } => {
                let final_amount = amount.max(0.0);
                let resolution = apply_participant_typed_damage_for_battle(
                    &mut encounter.participants[target_index],
                    final_amount,
                    &tick.source_id,
                    encounter.active,
                    damage_type,
                );
                let mut benefits = Vec::new();
                if resolution.damage_absorbed > f32::EPSILON {
                    benefits.push(format!(
                        "护盾/免疫吸收{}点",
                        format_number(resolution.damage_absorbed)
                    ));
                }
                encounter.combat_log.push(CombatLogEntry {
                    round: encounter.round,
                    kind: CombatLogKind::Damage,
                    source_id: tick.source_id.clone(),
                    source_name: source_name.clone(),
                    target_id: tick.target_id.clone(),
                    target_name: target_name.clone(),
                    action_name: tick.action_name.clone(),
                    base_amount: amount.max(0.0),
                    effective_amount: resolution.damage_applied,
                    modifiers: Vec::new(),
                    benefits,
                });
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
                let stat_config = encounter_basic_config(encounter, manager, &tick.source_id);
                let source_multiplier = source_index
                    .map(|index| {
                        participant_healing_multiplier(
                            &encounter.participants[index],
                            source_character.as_ref(),
                            &stat_config,
                        )
                    })
                    .unwrap_or_else(|| {
                        source_character
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
                            .unwrap_or(1.0)
                    });
                let target_multiplier =
                    participant_healing_taken_multiplier(&encounter.participants[target_index])
                        * participant_wound_healing_multiplier(
                            &encounter.participants[target_index],
                        )
                        * source_character
                            .as_ref()
                            .map(|source| {
                                dying_target_healing_multiplier(
                                    encounter.participants[target_index].hp,
                                    encounter.participants[target_index].max_hp,
                                    character_dying_target_healing_modifier(source),
                                )
                            })
                            .unwrap_or(1.0);
                let final_amount =
                    (amount.max(0.0) * source_multiplier * target_multiplier).max(0.0);
                let source_overhealing_shield_cap_rate = source_character
                    .as_ref()
                    .map(character_overhealing_shield_cap_rate)
                    .unwrap_or(0.0);
                let target = &mut encounter.participants[target_index];
                let resolution = apply_participant_healing_for_battle(
                    target,
                    final_amount,
                    source_overhealing_shield_cap_rate,
                );
                let effective_amount = resolution.effective_amount();
                let mut benefits = Vec::new();
                if resolution.shield_gained > f32::EPSILON {
                    benefits.push(format!(
                        "过量治疗转化{}点护盾",
                        format_number(resolution.shield_gained)
                    ));
                }
                encounter.combat_log.push(CombatLogEntry {
                    round: encounter.round,
                    kind: CombatLogKind::Healing,
                    source_id: tick.source_id.clone(),
                    source_name: source_name.clone(),
                    target_id: tick.target_id.clone(),
                    target_name: target_name.clone(),
                    action_name: tick.action_name.clone(),
                    base_amount: amount.max(0.0),
                    effective_amount,
                    modifiers: Vec::new(),
                    benefits,
                });
                let mutual_aid_healing =
                    if tick.source_id != tick.target_id && effective_amount > f32::EPSILON {
                        effective_amount
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
                encounter.action_log.push(format!(
                    "状态触发：{}为{}回复{}点生命值",
                    source_name,
                    target_name,
                    format_number(effective_amount)
                ));
                if mutual_aid_healing > f32::EPSILON {
                    if let Some(source) = encounter
                        .participants
                        .iter_mut()
                        .find(|participant| participant.target_id == tick.source_id)
                    {
                        let shield_cap_rate = source.overhealing_shield_cap_rate;
                        let resolution = apply_participant_healing_for_battle(
                            source,
                            mutual_aid_healing,
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
                            source_name,
                            format_number(effective_amount)
                        ));
                        encounter.combat_log.push(CombatLogEntry {
                            round: encounter.round,
                            kind: CombatLogKind::Healing,
                            source_id: tick.source_id.clone(),
                            source_name: source_name.clone(),
                            target_id: tick.source_id.clone(),
                            target_name: source_name.clone(),
                            action_name: "互帮互助".to_owned(),
                            base_amount: mutual_aid_healing,
                            effective_amount,
                            modifiers: Vec::new(),
                            benefits,
                        });
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
            .group_modifiers
            .damage_dealt
            .to_bits()
            .hash(&mut hasher);
        participant
            .group_modifiers
            .damage_taken
            .to_bits()
            .hash(&mut hasher);
        participant
            .group_modifiers
            .healing_dealt
            .to_bits()
            .hash(&mut hasher);
        participant
            .group_modifiers
            .healing_taken
            .to_bits()
            .hash(&mut hasher);
        participant.hidden_role.cocooning.hash(&mut hasher);
        participant.hidden_role.free_action_skill.hash(&mut hasher);
        participant
            .hidden_role
            .free_action_available
            .hash(&mut hasher);
        participant
            .hidden_role
            .second_wind_heal
            .to_bits()
            .hash(&mut hasher);
        participant.hidden_role.second_wind_used.hash(&mut hasher);
        participant.hidden_role.immune_diseased.hash(&mut hasher);
        participant.hidden_role.immune_poisoning.hash(&mut hasher);
        participant.hidden_role.immune_bleed.hash(&mut hasher);
        participant.protective_suit_kind.hash(&mut hasher);
        participant.protective_suit_shield.to_bits().hash(&mut hasher);
        participant
            .protective_suit_passive_active
            .hash(&mut hasher);
        participant
            .protective_suit_active_available
            .hash(&mut hasher);
        participant
            .protective_suit_slow_rounds_remaining
            .hash(&mut hasher);
        participant
            .protective_suit_blind_rounds_remaining
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
        participant.arcane_shield_rate.to_bits().hash(&mut hasher);
        participant
            .overhealing_shield_cap_rate
            .to_bits()
            .hash(&mut hasher);
        participant.overhealing_shield.to_bits().hash(&mut hasher);
        participant
            .overhealing_shield_turns_remaining
            .hash(&mut hasher);
        participant
            .revenge_soul_shield_rate
            .to_bits()
            .hash(&mut hasher);
        participant.revenge_soul_shield.to_bits().hash(&mut hasher);
        participant.summon_kind.hash(&mut hasher);
        participant.construct_shield.to_bits().hash(&mut hasher);
        participant.construct_shield_max.to_bits().hash(&mut hasher);
        participant
            .construct_shield_repair_rounds_remaining
            .hash(&mut hasher);
        participant
            .construct_repair_channel_rounds_remaining
            .hash(&mut hasher);
        participant.paralyzed_rounds_remaining.hash(&mut hasher);
        participant.commissar_proficiency.hash(&mut hasher);
        participant
            .next_attack_bonus_physical
            .to_bits()
            .hash(&mut hasher);
        participant.natural_hp_regen_suppressed.hash(&mut hasher);
        participant.flying_needles_enabled.hash(&mut hasher);
        participant.flying_needles_ready.hash(&mut hasher);
        participant.needle_case_enabled.hash(&mut hasher);
        participant.needle_case_ready.hash(&mut hasher);
        participant
            .needle_case_progress_noncombat_rounds
            .hash(&mut hasher);
        participant
            .redeemed_invisibility_rounds_remaining
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
            .rest_then_fight_healing_rate
            .to_bits()
            .hash(&mut hasher);
        participant.rest_then_fight_turns.hash(&mut hasher);
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
        for stack in &participant.corrosion_stacks {
            stack.source_id.hash(&mut hasher);
            stack.source_name.hash(&mut hasher);
            stack.kind.hash(&mut hasher);
            stack.damage_per_turn.to_bits().hash(&mut hasher);
            stack.turns_remaining.hash(&mut hasher);
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

fn character_support_talent_experience_bonus_rate(character: &PlayerCharacter) -> f32 {
    if character.skill_metadata.iter().any(|metadata| {
        metadata.is_approved()
            && metadata.source == CharacterSkillSourceKind::Talent
            && (metadata.source_pool_id.as_deref() == Some("support_talent")
                || metadata.source_pool_label.as_deref() == Some("辅助天赋"))
    }) {
        SUPPORT_TALENT_EXPERIENCE_BONUS_RATE
    } else {
        0.0
    }
}

fn participant_from_character(
    target_id: &str,
    character: &PlayerCharacter,
    manager: &NapcatMessageManager,
) -> BattleParticipantSnapshot {
    let effective_character = effective_hidden_role_character(
        target_id,
        character,
        &manager.hidden_roles,
    );
    let character = &effective_character;
    let hidden_role = hidden_role_battle_state(manager.hidden_roles.get(target_id));
    let opening_shield = hidden_role_intrinsic_opening_shield(
        manager.hidden_roles.get(target_id),
    );
    let protective_suit_kind = character
        .protective_suit
        .is_intact()
        .then_some(character.protective_suit.kind);
    let protective_suit_all_shield = protective_suit_kind
        .map(ProtectiveSuitKind::all_damage_shield)
        .unwrap_or_default();
    let protective_suit_magic_shield =
        protective_suit_opening_magic_shield(&character.protective_suit);
    let protective_suit_shield = protective_suit_all_shield.max(protective_suit_magic_shield);
    let status = character.status.combined(&character.extra_status);
    let (mut speed, mut low_survivor_speed) = character_battle_speeds(character);
    let protective_suit_speed_bonus = if protective_suit_kind == Some(ProtectiveSuitKind::Electronic)
    {
        1.5
    } else {
        0.0
    };
    speed += protective_suit_speed_bonus;
    low_survivor_speed += protective_suit_speed_bonus;
    let dominion_gain_rate = character_dominion_max_hp_gain_rate(character);
    let dominion_bonus = if dominion_gain_rate > f32::EPSILON {
        character.dominion_max_hp_bonus.clamp(
            0.0,
            character_dominion_max_hp_bonus_cap(character),
        )
    } else {
        0.0
    };
    let redeemed_needles = character_redeemed_needle_state(character);
    BattleParticipantSnapshot {
        target_id: target_id.to_owned(),
        display_name: character_display_name(target_id, character, manager),
        unit_template_id: None,
        unit_character: None,
        player_character: true,
        is_summon: false,
        summon_owner_id: None,
        summon_kind: SummonKind::Custom,
        level: character.level.max(1),
        exp: character.exp.max(0),
        support_talent_experience_bonus_rate: character_support_talent_experience_bonus_rate(
            character,
        ),
        base_damage: status.str_.max(status.dex).max(status.int_).max(1) as f32,
        unit_rarity: UnitRarity::Normal,
        turn: 0,
        combat_turns_completed: 0,
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
        max_hp: character.max_hp + dominion_bonus,
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
        group_modifiers: TrpgGlobalCombatModifiers::default(),
        hidden_role,
        protective_suit_kind,
        protective_suit_shield,
        protective_suit_shield_max: protective_suit_shield,
        protective_suit_magic_only: protective_suit_magic_shield > f32::EPSILON,
        protective_suit_passive_active: protective_suit_kind.is_some(),
        protective_suit_active_available: protective_suit_kind.is_some_and(|kind| {
            kind == ProtectiveSuitKind::Maintenance
                || (kind.has_consumable_active() && !character.protective_suit.active_used)
        }),
        protective_suit_speed_bonus,
        protective_suit_slow_rounds_remaining: 0,
        protective_suit_blind_rounds_remaining: 0,
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
        arcane_shield_rate: character_arcane_shield_rate(character),
        overhealing_shield_cap_rate: character_overhealing_shield_cap_rate(character),
        overhealing_shield: 0.0,
        overhealing_shield_turns_remaining: 0,
        revenge_soul_shield_rate: character_revenge_soul_shield_rate(character),
        revenge_soul_shield: 0.0,
        construct_shield: opening_shield,
        construct_shield_max: opening_shield,
        construct_shield_repair_rounds_remaining: 0,
        construct_repair_channel_rounds_remaining: 0,
        paralyzed_rounds_remaining: 0,
        commissar_proficiency: character.redeemed_commissar_proficiency.min(5),
        next_attack_bonus_physical: 0.0,
        natural_hp_regen_suppressed: false,
        flying_needles_enabled: redeemed_needles.is_some(),
        flying_needles_ready: redeemed_needles.map(|(state, _)| state.ready).unwrap_or(0),
        needle_case_enabled: redeemed_needles
            .map(|(_, enabled)| enabled)
            .unwrap_or(false),
        needle_case_ready: redeemed_needles
            .map(|(state, _)| state.case_ready)
            .unwrap_or(0),
        needle_case_progress_noncombat_rounds: redeemed_needles
            .map(|(state, _)| state.case_progress_noncombat_rounds)
            .unwrap_or(0),
        redeemed_invisibility_rounds_remaining: 0,
        undying_rage_enabled: character_undying_rage_available(character),
        undying_rage_used: false,
        undying_rage_active: false,
        hope_avatar_enabled: character_hope_avatar_available(character),
        hope_avatar_used: false,
        hope_avatar_rounds_remaining: 0,
        mirror_coat_enabled: character_mirror_coat_available(character),
        mirror_coat_layers: 0,
        mirror_coat_cooldown_remaining: 0,
        mirror_coat_cleanup_pending: false,
        sunset_enabled: character_sunset_available(character),
        sunset_death_time_reset_pending: false,
        goose_channeling_turns: 0,
        butterfly_enabled: character_butterfly_available(character),
        butterfly_target_id: None,
        butterfly_effect_on_holder: false,
        butterfly_initialized: false,
        butterfly_locked: false,
        butterfly_effect: false,
        liquid_body_damage_delay_rate: character_liquid_body_damage_delay_rate(character),
        liquid_body_self_healing_rate: character_liquid_body_self_healing_rate(character),
        calm_heart_healing_rate: character_calm_heart_healing_rate(character),
        combat_damage_taken_total: 0.0,
        rest_then_fight_healing_rate: character_rest_then_fight_healing_rate(character),
        rest_then_fight_turns: 0,
        champion_damage_bonus_per_stack: character_champion_damage_bonus_per_stack(character),
        champion_damage_reduction_per_stack: character_champion_damage_reduction_per_stack(
            character,
        ),
        champion_stacks: 0,
        dominion_max_hp_gain_rate: dominion_gain_rate,
        dominion_max_hp_bonus_cap: character_dominion_max_hp_bonus_cap(character),
        dominion_max_hp_bonus: dominion_bonus,
        sin_on_sin_exp_bonus_per_stack: character_sin_on_sin_exp_bonus_per_stack(character),
        sin_on_sin_recovery_rate: character_sin_on_sin_recovery_rate(character),
        sin_on_sin_stacks: 0,
        penance_healing_bonus_percent: character_penance_healing_bonus_percent(character),
        penance_kill_assist_count: 0,
        damage_contributors: Vec::new(),
        damage_contribution_amounts: HashMap::new(),
        wound_healing_taken_turns: 0,
        delayed_damage_ticks: Vec::new(),
        delayed_healing_ticks: Vec::new(),
        corrosion_stacks: Vec::new(),
        damage_taken_this_turn: character.damage_taken_this_turn,
        healing_taken_this_turn: character.healing_taken_this_turn,
        skill_last_used_turns: HashMap::new(),
        skill_cooldown_ready_turns: HashMap::new(),
    }
}

fn participant_from_unit_template(
    target_id: &str,
    unit_id: &str,
    unit: &UnitPoolEntry,
) -> BattleParticipantSnapshot {
    let character = &unit.character;
    let mut cooldown_character = character.clone();
    crate::napcat::materialize_imported_skill_cooldowns(&mut cooldown_character, 0);
    let status = character.status.combined(&character.extra_status);
    let (speed, low_survivor_speed) = character_battle_speeds(character);
    BattleParticipantSnapshot {
        target_id: target_id.to_owned(),
        display_name: unit_participant_display_name(target_id, unit_id, unit),
        unit_template_id: Some(unit_id.to_owned()),
        unit_character: Some(character.clone()),
        player_character: false,
        is_summon: false,
        summon_owner_id: None,
        summon_kind: SummonKind::Custom,
        level: character.level.max(1),
        exp: 0,
        support_talent_experience_bonus_rate: 0.0,
        base_damage: unit.base_damage.max(0.0),
        unit_rarity: unit.rarity,
        turn: 0,
        combat_turns_completed: 0,
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
        group_modifiers: TrpgGlobalCombatModifiers::default(),
        hidden_role: HiddenRoleBattleState::default(),
        protective_suit_kind: None,
        protective_suit_shield: 0.0,
        protective_suit_shield_max: 0.0,
        protective_suit_magic_only: false,
        protective_suit_passive_active: false,
        protective_suit_active_available: false,
        protective_suit_speed_bonus: 0.0,
        protective_suit_slow_rounds_remaining: 0,
        protective_suit_blind_rounds_remaining: 0,
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
        arcane_shield_rate: character_arcane_shield_rate(character),
        overhealing_shield_cap_rate: character_overhealing_shield_cap_rate(character),
        overhealing_shield: 0.0,
        overhealing_shield_turns_remaining: 0,
        revenge_soul_shield_rate: character_revenge_soul_shield_rate(character),
        revenge_soul_shield: 0.0,
        construct_shield: 0.0,
        construct_shield_max: 0.0,
        construct_shield_repair_rounds_remaining: 0,
        construct_repair_channel_rounds_remaining: 0,
        paralyzed_rounds_remaining: 0,
        commissar_proficiency: 0,
        next_attack_bonus_physical: 0.0,
        natural_hp_regen_suppressed: false,
        flying_needles_enabled: false,
        flying_needles_ready: 0,
        needle_case_enabled: false,
        needle_case_ready: 0,
        needle_case_progress_noncombat_rounds: 0,
        redeemed_invisibility_rounds_remaining: 0,
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
        rest_then_fight_healing_rate: character_rest_then_fight_healing_rate(character),
        rest_then_fight_turns: 0,
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
        damage_contribution_amounts: HashMap::new(),
        wound_healing_taken_turns: 0,
        delayed_damage_ticks: Vec::new(),
        delayed_healing_ticks: Vec::new(),
        corrosion_stacks: Vec::new(),
        mirror_coat_enabled: false,
        mirror_coat_layers: 0,
        mirror_coat_cooldown_remaining: 0,
        mirror_coat_cleanup_pending: false,
        sunset_enabled: character_sunset_available(character),
        sunset_death_time_reset_pending: false,
        goose_channeling_turns: 0,
        butterfly_enabled: character_butterfly_available(character),
        butterfly_target_id: None,
        butterfly_effect_on_holder: false,
        butterfly_initialized: false,
        butterfly_locked: false,
        butterfly_effect: false,
        damage_taken_this_turn: character.damage_taken_this_turn,
        healing_taken_this_turn: character.healing_taken_this_turn,
        skill_last_used_turns: HashMap::new(),
        skill_cooldown_ready_turns: cooldown_character.skill_cooldown_ready_turns,
    }
}

fn participant_from_unit_instance(
    instance_id: &str,
    instance: &UnitInstance,
    template: &UnitPoolEntry,
) -> BattleParticipantSnapshot {
    let mut instance_template = template.clone();
    instance_template.character = instance.character.clone();
    let mut participant = participant_from_unit_template(
        instance_id,
        &instance.template_id,
        &instance_template,
    );
    participant.display_name = if instance.display_name.trim().is_empty() {
        unit_template_name(&instance.template_id, template)
    } else {
        instance.display_name.trim().to_owned()
    };
    if let Some(character) = participant.unit_character.as_mut() {
        character.name = participant.display_name.clone();
        character.nickname = participant.display_name.clone();
    }
    participant
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
        is_summon: false,
        summon_owner_id: None,
        summon_kind: SummonKind::Custom,
        level: 1,
        exp: 0,
        support_talent_experience_bonus_rate: 0.0,
        base_damage: 0.0,
        unit_rarity: UnitRarity::Normal,
        turn: 0,
        combat_turns_completed: 0,
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
        group_modifiers: TrpgGlobalCombatModifiers::default(),
        hidden_role: HiddenRoleBattleState::default(),
        protective_suit_kind: None,
        protective_suit_shield: 0.0,
        protective_suit_shield_max: 0.0,
        protective_suit_magic_only: false,
        protective_suit_passive_active: false,
        protective_suit_active_available: false,
        protective_suit_speed_bonus: 0.0,
        protective_suit_slow_rounds_remaining: 0,
        protective_suit_blind_rounds_remaining: 0,
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
        arcane_shield_rate: 0.0,
        overhealing_shield_cap_rate: 0.0,
        overhealing_shield: 0.0,
        overhealing_shield_turns_remaining: 0,
        revenge_soul_shield_rate: 0.0,
        revenge_soul_shield: 0.0,
        construct_shield: 0.0,
        construct_shield_max: 0.0,
        construct_shield_repair_rounds_remaining: 0,
        construct_repair_channel_rounds_remaining: 0,
        paralyzed_rounds_remaining: 0,
        commissar_proficiency: 0,
        next_attack_bonus_physical: 0.0,
        natural_hp_regen_suppressed: false,
        flying_needles_enabled: false,
        flying_needles_ready: 0,
        needle_case_enabled: false,
        needle_case_ready: 0,
        needle_case_progress_noncombat_rounds: 0,
        redeemed_invisibility_rounds_remaining: 0,
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
        rest_then_fight_healing_rate: 0.0,
        rest_then_fight_turns: 0,
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
        damage_contribution_amounts: HashMap::new(),
        wound_healing_taken_turns: 0,
        delayed_damage_ticks: Vec::new(),
        delayed_healing_ticks: Vec::new(),
        corrosion_stacks: Vec::new(),
        mirror_coat_enabled: false,
        mirror_coat_layers: 0,
        mirror_coat_cooldown_remaining: 0,
        mirror_coat_cleanup_pending: false,
        sunset_enabled: false,
        sunset_death_time_reset_pending: false,
        goose_channeling_turns: 0,
        butterfly_enabled: false,
        butterfly_target_id: None,
        butterfly_effect_on_holder: false,
        butterfly_initialized: false,
        butterfly_locked: false,
        butterfly_effect: false,
        damage_taken_this_turn: 0.0,
        healing_taken_this_turn: 0.0,
        skill_last_used_turns: HashMap::new(),
        skill_cooldown_ready_turns: HashMap::new(),
    }
}

/// 召唤物参与者：等级=主人等级，生命=召唤物当前值，伤害修正包含主人魅力的
/// 召唤物伤害加成（每点魅力+2%），且不受自身低血伤害减少影响。
fn participant_from_summon(
    owner_id: &str,
    summon_index: usize,
    summon: &Summon,
    owner: &PlayerCharacter,
    manager: &NapcatMessageManager,
) -> BattleParticipantSnapshot {
    let mut participant = participant_from_target(owner_id, manager);
    let target_id = crate::napcat::summon_target_id(owner_id, summon_index);
    let owner_name = character_display_name(owner_id, owner, manager);
    let summon_name = summon_display_name(summon);
    let mut unit_character = PlayerCharacter::default();
    unit_character.name = format!("{owner_name}的{summon_name}");
    unit_character.level = owner.level.max(1);
    unit_character.hp = summon.hp;
    unit_character.max_hp = summon.max_hp;
    configure_redeemed_summon_character(
        &mut unit_character,
        summon.kind,
        owner.level,
    );
    unit_character.inventory = summon.inventory.clone();
    let summon_status = unit_character.status.combined(&unit_character.extra_status);

    participant.target_id = target_id.clone();
    participant.display_name = unit_character.name.clone();
    participant.unit_template_id = None;
    participant.unit_character = Some(unit_character);
    participant.player_character = false;
    participant.is_summon = true;
    participant.protective_suit_kind = None;
    participant.protective_suit_shield = 0.0;
    participant.protective_suit_shield_max = 0.0;
    participant.protective_suit_magic_only = false;
    participant.protective_suit_passive_active = false;
    participant.protective_suit_active_available = false;
    participant.protective_suit_speed_bonus = 0.0;
    participant.protective_suit_slow_rounds_remaining = 0;
    participant.protective_suit_blind_rounds_remaining = 0;
    participant.summon_owner_id = Some(owner_id.to_owned());
    participant.summon_kind = summon.kind;
    participant.level = owner.level.max(1);
    participant.exp = 0;
    participant.support_talent_experience_bonus_rate = 0.0;
    participant.base_damage = summon_status
        .str_
        .max(summon_status.dex)
        .max(summon_status.int_)
        .max(1) as f32;
    participant.str_ = summon_status.str_;
    participant.agi = summon_status.agi;
    participant.dex = summon_status.dex;
    participant.int_ = summon_status.int_;
    participant.wis = summon_status.wis;
    participant.alive = summon.hp > 0.0;
    participant.hp = summon.hp;
    participant.max_hp = summon.max_hp;
    participant.mp = 0.0;
    participant.max_mp = 0.0;
    participant.hp_regen = 0.0;
    participant.mp_regen = 0.0;
    participant.speed = 0.0;
    participant.low_survivor_speed = 0.0;
    participant.damage_dealt_modifier = character_summon_damage_multiplier(owner);
    participant.damage_taken_modifier = 1.0;
    participant.healing_dealt_modifier = 1.0;
    participant.healing_taken_modifier = 1.0;
    participant.construct_shield = summon.shield;
    participant.construct_shield_max = summon.max_shield;
    participant.construct_shield_repair_rounds_remaining = summon.shield_repair_rounds_remaining;
    participant.construct_repair_channel_rounds_remaining = summon.repair_channel_rounds_remaining;
    participant.skill_last_used_turns = HashMap::new();
    participant.skill_cooldown_ready_turns = HashMap::new();
    participant
}

fn summon_skill_metadata(
    skill_type: &str,
    target_class: &str,
    range: i32,
) -> CharacterSkillMetadata {
    CharacterSkillMetadata {
        skill_type: Some(skill_type.to_owned()),
        target_class: Some(target_class.to_owned()),
        range: Some(range),
        ..Default::default()
    }
}

fn configure_redeemed_summon_character(
    character: &mut PlayerCharacter,
    kind: SummonKind,
    owner_level: i32,
) {
    character.inited = true;
    character.level = owner_level.max(1);
    character.status = CharacterStatus::default();
    character.extra_status = CharacterStatus::default();
    character.skill_names.clear();
    character.skill_notes.clear();
    character.skill_mp_costs.clear();
    character.skill_cooldown_turns.clear();
    character.skill_metadata.clear();
    match kind {
        SummonKind::ArmedDrone => {
            character.status.vit = 3;
            character.status.dex = 3;
            character.status.k = 3;
            character.skill_names = vec!["无人机枪械射击".to_owned(), "电能冲击".to_owned()];
            character.skill_notes = vec![
                "主动使用对20米内的目标造成5点物理伤害".to_owned(),
                "主动使用对周围3米内的目标施加1回合麻痹状态".to_owned(),
            ];
            character.skill_mp_costs = vec![0.0, 0.0];
            character.skill_cooldown_turns = vec![0, 0];
            character.skill_metadata = vec![
                summon_skill_metadata("远程", "单目标", 20),
                summon_skill_metadata("动作", "范围", 3),
            ];
        },
        SummonKind::Mech => {
            character.skill_names = vec!["巨型金属刀横扫".to_owned(), "维修机甲".to_owned()];
            character.skill_notes = vec![
                "主动使用对周围2米内的目标造成5点物理伤害".to_owned(),
                "主动使用引导2回合后完全恢复自身生命值".to_owned(),
            ];
            character.skill_mp_costs = vec![0.0, 0.0];
            character.skill_cooldown_turns = vec![0, 0];
            character.skill_metadata = vec![
                summon_skill_metadata("动作", "范围", 2),
                summon_skill_metadata("动作", "无目标", 0),
            ];
        },
        SummonKind::PortableTurret => {
            let damage = owner_level.max(1);
            character.skill_names = vec!["便携小炮塔射击".to_owned()];
            character.skill_notes = vec![format!("主动使用对6米内的目标造成{damage}点物理伤害")];
            character.skill_mp_costs = vec![0.0];
            character.skill_cooldown_turns = vec![0];
            character.skill_metadata = vec![summon_skill_metadata("远程", "单目标", 6)];
        },
        SummonKind::Custom => {},
    }
}

fn summon_display_name(summon: &Summon) -> &str {
    if summon.name.trim().is_empty() {
        "召唤物"
    } else {
        summon.name.trim()
    }
}

fn sync_participant_from_manager(
    participant: &mut BattleParticipantSnapshot,
    manager: &NapcatMessageManager,
) {
    if participant.is_summon {
        let Some(owner_id) = participant.summon_owner_id.clone() else {
            return;
        };
        let Some(owner) = manager.player_characters.get(&owner_id) else {
            return;
        };
        let Some(index) =
            crate::napcat::parse_summon_target_id(&participant.target_id).map(|(_, index)| index)
        else {
            return;
        };
        let Some(summon) = owner.summons.get(index) else {
            return;
        };
        reset_non_player_participant_bonus_fields(participant);
        let owner_name = character_display_name(&owner_id, owner, manager);
        let summon_name = summon_display_name(summon);
        participant.display_name = format!("{owner_name}的{summon_name}");
        participant.player_character = false;
        participant.summon_kind = summon.kind;
        participant.level = owner.level.max(1);
        participant.exp = 0;
        participant.base_damage = 0.0;
        participant.max_hp = summon.max_hp;
        participant.max_mp = 0.0;
        participant.hp_regen = 0.0;
        participant.mp_regen = 0.0;
        participant.speed = 0.0;
        participant.low_survivor_speed = 0.0;
        participant.str_ = 0;
        participant.agi = 0;
        participant.dex = 0;
        participant.int_ = 0;
        participant.wis = 0;
        participant.damage_dealt_modifier = character_summon_damage_multiplier(owner);
        participant.damage_taken_modifier = 1.0;
        participant.healing_dealt_modifier = 1.0;
        participant.healing_taken_modifier = 1.0;
        participant.construct_shield = summon.shield;
        participant.construct_shield_max = summon.max_shield;
        participant.construct_shield_repair_rounds_remaining =
            summon.shield_repair_rounds_remaining;
        participant.construct_repair_channel_rounds_remaining =
            summon.repair_channel_rounds_remaining;
        participant.hp = summon.hp.clamp(0.0, participant.max_hp.max(0.0));
        participant.mp = 0.0;
        participant.alive = participant.hp > 0.0;
        let mut unit_character = participant.unit_character.clone().unwrap_or_default();
        unit_character.name = participant.display_name.clone();
        unit_character.level = owner.level.max(1);
        unit_character.hp = participant.hp;
        unit_character.max_hp = participant.max_hp;
        configure_redeemed_summon_character(
            &mut unit_character,
            summon.kind,
            owner.level,
        );
        unit_character.inventory = summon.inventory.clone();
        let status = unit_character.status.combined(&unit_character.extra_status);
        participant.base_damage = status.str_.max(status.dex).max(status.int_).max(1) as f32;
        participant.str_ = status.str_;
        participant.agi = status.agi;
        participant.dex = status.dex;
        participant.int_ = status.int_;
        participant.wis = status.wis;
        participant.unit_character = Some(unit_character);
        return;
    }

    if let Some(unit_id) = participant.unit_template_id.as_deref() {
        if let Some(unit) = manager.unit_pool.get(unit_id) {
            let instance = manager.unit_instances.get(&participant.target_id);
            let mut character = instance
                .map(|instance| instance.character.clone())
                .or_else(|| character_for_participant(participant, manager))
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
            participant.display_name = instance
                .map(|instance| instance.display_name.trim())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    unit_participant_display_name(&participant.target_id, unit_id, unit)
                });
            participant.player_character = false;
            participant.level = character.level.max(1);
            participant.support_talent_experience_bonus_rate = 0.0;
            participant.base_damage = unit.base_damage.max(0.0);
            participant.unit_rarity = unit.rarity;
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
            participant.arcane_shield_rate = character_arcane_shield_rate(&character);
            participant.overhealing_shield_cap_rate =
                character_overhealing_shield_cap_rate(&character);
            participant.revenge_soul_shield_rate = character_revenge_soul_shield_rate(&character);
            sync_participant_undying_rage(
                participant,
                character_undying_rage_available(&character),
            );
            participant.hope_avatar_enabled = character_hope_avatar_available(&character);
            participant.mirror_coat_enabled = character_mirror_coat_available(&character);
            participant.sunset_enabled = character_sunset_available(&character);
            participant.butterfly_enabled = character_butterfly_available(&character);
            participant.liquid_body_damage_delay_rate =
                character_liquid_body_damage_delay_rate(&character);
            participant.liquid_body_self_healing_rate =
                character_liquid_body_self_healing_rate(&character);
            participant.calm_heart_healing_rate = character_calm_heart_healing_rate(&character);
            participant.rest_then_fight_healing_rate =
                character_rest_then_fight_healing_rate(&character);
            if participant.rest_then_fight_healing_rate <= f32::EPSILON {
                participant.rest_then_fight_turns = 0;
            }
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
            character.skill_cooldown_ready_turns = participant.skill_cooldown_ready_turns.clone();
            participant.unit_character = Some(character);
        }
        return;
    }

    if let Some(base_character) = manager.player_characters.get(&participant.target_id) {
        let effective_character = effective_hidden_role_character(
            &participant.target_id,
            base_character,
            &manager.hidden_roles,
        );
        let character = &effective_character;
        let status = character.status.combined(&character.extra_status);
        let (speed, low_survivor_speed) = character_battle_speeds(character);
        let protective_suit_speed_bonus =
            sync_participant_protective_suit(participant, character);
        participant.display_name = character_display_name(
            &participant.target_id,
            character,
            manager,
        );
        participant.player_character = true;
        participant.level = character.level.max(1);
        participant.exp = character.exp.max(0);
        participant.commissar_proficiency = character.redeemed_commissar_proficiency.min(5);
        if let Some((state, case_enabled)) = character_redeemed_needle_state(character) {
            participant.flying_needles_enabled = true;
            participant.flying_needles_ready = state.ready.min(5);
            participant.needle_case_enabled = case_enabled;
            participant.needle_case_ready = if case_enabled { state.case_ready.min(2) } else { 0 };
            participant.needle_case_progress_noncombat_rounds =
                if case_enabled { state.case_progress_noncombat_rounds.min(1) } else { 0 };
        } else {
            participant.flying_needles_enabled = false;
            participant.flying_needles_ready = 0;
            participant.needle_case_enabled = false;
            participant.needle_case_ready = 0;
            participant.needle_case_progress_noncombat_rounds = 0;
        }
        participant.support_talent_experience_bonus_rate =
            character_support_talent_experience_bonus_rate(character);
        let total = character.status.combined(&character.extra_status);
        participant.base_damage = total.str_.max(total.dex).max(total.int_).max(1) as f32;
        participant.unit_rarity = UnitRarity::Normal;
        participant.max_hp = character.max_hp;
        participant.max_mp = character.max_mp;
        participant.hp_regen = character.hp_regen;
        participant.mp_regen = character.mp_regen;
        participant.speed = speed + protective_suit_speed_bonus;
        participant.low_survivor_speed = low_survivor_speed + protective_suit_speed_bonus;
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
        participant.arcane_shield_rate = character_arcane_shield_rate(character);
        participant.overhealing_shield_cap_rate = character_overhealing_shield_cap_rate(character);
        participant.revenge_soul_shield_rate = character_revenge_soul_shield_rate(character);
        sync_participant_undying_rage(
            participant,
            character_undying_rage_available(character),
        );
        participant.hope_avatar_enabled = character_hope_avatar_available(character);
        participant.mirror_coat_enabled = character_mirror_coat_available(character);
        participant.sunset_enabled = character_sunset_available(character);
        participant.butterfly_enabled = character_butterfly_available(character);
        participant.butterfly_target_id = manager
            .trpg_groups
            .values()
            .find(|group| {
                group
                    .players
                    .iter()
                    .any(|player| player == &participant.target_id)
            })
            .and_then(|group| group.butterfly_targets.get(&participant.target_id).cloned());
        participant.liquid_body_damage_delay_rate =
            character_liquid_body_damage_delay_rate(character);
        participant.liquid_body_self_healing_rate =
            character_liquid_body_self_healing_rate(character);
        participant.calm_heart_healing_rate = character_calm_heart_healing_rate(character);
        participant.rest_then_fight_healing_rate =
            character_rest_then_fight_healing_rate(character);
        if participant.rest_then_fight_healing_rate <= f32::EPSILON {
            participant.rest_then_fight_turns = 0;
        }
        participant.champion_damage_bonus_per_stack =
            character_champion_damage_bonus_per_stack(character);
        participant.champion_damage_reduction_per_stack =
            character_champion_damage_reduction_per_stack(character);
        let dominion_gain_rate = character_dominion_max_hp_gain_rate(character);
        let dominion_bonus_cap = character_dominion_max_hp_bonus_cap(character);
        participant.dominion_max_hp_gain_rate = dominion_gain_rate;
        participant.dominion_max_hp_bonus_cap = dominion_bonus_cap;
        participant.dominion_max_hp_bonus = if dominion_gain_rate > f32::EPSILON {
            character
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
        reset_non_player_participant_bonus_fields(participant);
        participant.display_name = participant_display_name(&participant.target_id, manager);
    }
}

fn reset_non_player_participant_bonus_fields(participant: &mut BattleParticipantSnapshot) {
    participant.player_character = false;
    participant.protective_suit_kind = None;
    participant.protective_suit_shield = 0.0;
    participant.protective_suit_shield_max = 0.0;
    participant.protective_suit_magic_only = false;
    participant.protective_suit_passive_active = false;
    participant.protective_suit_active_available = false;
    participant.protective_suit_speed_bonus = 0.0;
    participant.protective_suit_slow_rounds_remaining = 0;
    participant.protective_suit_blind_rounds_remaining = 0;
    participant.support_talent_experience_bonus_rate = 0.0;
    participant.low_survivor_speed = participant.speed.max(0.0);
    participant.arrogance_damage_bonus_per_source = 0.0;
    participant.endless_pain_bonus_damage_per_stack = 0.0;
    participant.infinite_focus_damage_bonus_per_stack = 0.0;
    participant.one_heart_healing_bonus_per_stack = 0.0;
    participant.inspiration_enabled = false;
    sync_participant_keen_evasion(participant, false);
    participant.arcane_shield_rate = 0.0;
    participant.overhealing_shield_cap_rate = 0.0;
    participant.revenge_soul_shield_rate = 0.0;
    participant.revenge_soul_shield = 0.0;
    sync_participant_undying_rage(participant, false);
    participant.hope_avatar_enabled = false;
    participant.mirror_coat_enabled = false;
    participant.sunset_enabled = false;
    participant.butterfly_enabled = false;
    participant.butterfly_target_id = None;
    participant.liquid_body_damage_delay_rate = 0.0;
    participant.liquid_body_self_healing_rate = 0.0;
    participant.calm_heart_healing_rate = 0.0;
    participant.rest_then_fight_healing_rate = 0.0;
    participant.rest_then_fight_turns = 0;
    participant.champion_damage_bonus_per_stack = 0.0;
    participant.champion_damage_reduction_per_stack = 0.0;
    participant.dominion_max_hp_gain_rate = 0.0;
    participant.dominion_max_hp_bonus_cap = 0.0;
    participant.dominion_max_hp_bonus = 0.0;
    participant.sin_on_sin_exp_bonus_per_stack = 0.0;
    participant.sin_on_sin_recovery_rate = 0.0;
    participant.penance_healing_bonus_percent = 0.0;
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
            .map(|character| {
                let effective = effective_hidden_role_character(
                    &participant.target_id,
                    character,
                    &manager.hidden_roles,
                );
                (effective.hp, effective.mp)
            })
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
    let instance = manager.unit_instances.get(&participant.target_id);
    let current = character_for_participant(participant, manager);
    let mut refreshed = instance
        .map(|instance| instance.character.clone())
        .unwrap_or_else(|| unit.character.clone());
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
        refreshed.skill_cooldown_ready_turns = participant.skill_cooldown_ready_turns.clone();
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
    encounter_active: bool,
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
    let inspiration_multiplier = if encounter_active {
        participant_inspiration_multiplier(participant)
    } else {
        1.0
    };
    let protective_suit_slow_multiplier =
        if participant.protective_suit_slow_rounds_remaining > 0 {
            0.5
        } else {
            1.0
        };
    base_speed * inspiration_multiplier * protective_suit_slow_multiplier
}

/// WoW-style PvE normalization: a mob is evaluated at each interacting player's
/// level. The encounter stores one shared health percentage, so player damage is
/// converted back into the template mob's health units. Mob damage is scaled up
/// or down for the particular player it hits. Player-versus-player is unchanged.
fn personalized_pve_level_multiplier(
    source: &BattleParticipantSnapshot,
    target: &BattleParticipantSnapshot,
) -> f32 {
    if source.player_character == target.player_character {
        return 1.0;
    }
    let mob = if source.player_character { target } else { source };
    if mob.unit_template_id.is_none() {
        return 1.0;
    }
    target.level.max(1) as f32 / source.level.max(1) as f32
}

fn ordered_participant_indices(encounter: &BattleEncounter) -> Vec<usize> {
    let mut indices = (0..encounter.participants.len()).collect::<Vec<_>>();
    if encounter.sort_by_turn {
        indices.sort_by(|left, right| {
            let left_participant = &encounter.participants[*left];
            let right_participant = &encounter.participants[*right];
            right_participant
                .agi
                .cmp(&left_participant.agi)
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
        .find(|index| participant_can_act(&encounter.participants[*index]))
}

fn participant_can_act(participant: &BattleParticipantSnapshot) -> bool {
    participant.alive
        && !participant.action_done
        && participant.goose_channeling_turns == 0
        && participant.construct_repair_channel_rounds_remaining == 0
        && participant.paralyzed_rounds_remaining == 0
        && participant.protective_suit_blind_rounds_remaining == 0
        && !participant.hidden_role.cocooning
}

fn sync_participant_protective_suit(
    participant: &mut BattleParticipantSnapshot,
    character: &PlayerCharacter,
) -> f32 {
    let new_kind = character
        .protective_suit
        .is_intact()
        .then_some(character.protective_suit.kind);
    if participant.protective_suit_kind != new_kind {
        participant.protective_suit_kind = new_kind;
        let all_shield = new_kind
            .map(ProtectiveSuitKind::all_damage_shield)
            .unwrap_or_default();
        let magic_shield = protective_suit_opening_magic_shield(&character.protective_suit);
        let shield = all_shield.max(magic_shield);
        participant.protective_suit_shield = shield;
        participant.protective_suit_shield_max = shield;
        participant.protective_suit_magic_only = magic_shield > f32::EPSILON;
        participant.protective_suit_passive_active = new_kind.is_some();
        participant.protective_suit_active_available = new_kind.is_some_and(|kind| {
            kind == ProtectiveSuitKind::Maintenance
                || (kind.has_consumable_active() && !character.protective_suit.active_used)
        });
        participant.protective_suit_slow_rounds_remaining = 0;
        participant.protective_suit_blind_rounds_remaining = 0;
    } else if character.protective_suit.active_used {
        participant.protective_suit_active_available = false;
    }
    let speed_bonus = if new_kind == Some(ProtectiveSuitKind::Electronic)
        && participant.protective_suit_passive_active
    {
        1.5
    } else {
        0.0
    };
    participant.protective_suit_speed_bonus = speed_bonus;
    speed_bonus
}

fn normalize_encounter_after_edit(encounter: &mut BattleEncounter) {
    for participant in &mut encounter.participants {
        participant.max_hp = participant.max_hp.max(0.0);
        participant.hp = participant.hp.clamp(0.0, participant.max_hp);
        participant.max_mp = participant.max_mp.max(0.0);
        participant.mp = participant.mp.clamp(0.0, participant.max_mp);
        participant.construct_shield_max = participant.construct_shield_max.max(0.0);
        participant.construct_shield = participant
            .construct_shield
            .clamp(0.0, participant.construct_shield_max);
        participant.protective_suit_shield_max =
            participant.protective_suit_shield_max.max(0.0);
        participant.protective_suit_shield = participant
            .protective_suit_shield
            .clamp(0.0, participant.protective_suit_shield_max);
        participant.flying_needles_ready = participant.flying_needles_ready.min(5);
        participant.needle_case_ready = if participant.needle_case_enabled {
            participant.needle_case_ready.min(2)
        } else {
            0
        };
        participant.needle_case_progress_noncombat_rounds = if participant.needle_case_enabled {
            participant.needle_case_progress_noncombat_rounds.min(1)
        } else {
            0
        };
        participant.damage_taken_this_turn = participant.damage_taken_this_turn.max(0.0);
        participant.healing_taken_this_turn = participant.healing_taken_this_turn.max(0.0);
        participant.alive = participant.hp > 0.0 || participant_hope_avatar_active(participant);
    }
}

fn set_participant_alive_after_manual_edit(
    participant: &mut BattleParticipantSnapshot,
    alive: bool,
) {
    if alive {
        participant.hp = participant.hp.max(1.0).min(participant.max_hp.max(0.0));
        participant.alive = participant.hp > 0.0;
        return;
    }

    participant.hp = 0.0;
    participant.alive = false;
    participant.hope_avatar_rounds_remaining = 0;
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

    match encounter.trpg_group.as_deref() {
        Some(group_name) => {
            if let Some(group) = manager.trpg_groups.get(group_name) {
                candidate_ids.extend(group.players.iter().cloned());
            }
        },
        None => {
            candidate_ids.extend(manager.player_characters.keys().cloned());
            candidate_ids.extend(manager.chat_targets.keys().cloned());
        },
    }

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

/// 所有可加入遭遇的召唤物（主人、索引、显示名），排除已在遭遇中的。
fn available_summon_candidates(
    encounter: &BattleEncounter,
    manager: &NapcatMessageManager,
) -> Vec<(String, usize, String)> {
    let mut candidates = Vec::new();
    for (owner_id, character) in &manager.player_characters {
        for (index, summon) in character.summons.iter().enumerate() {
            let target_id = crate::napcat::summon_target_id(owner_id, index);
            if encounter
                .participants
                .iter()
                .any(|participant| participant.target_id == target_id)
            {
                continue;
            }
            let owner_name = character_display_name(owner_id, character, manager);
            let summon_name = summon_display_name(summon);
            candidates.push((
                owner_id.clone(),
                index,
                format!("{owner_name}：{summon_name}"),
            ));
        }
    }
    candidates.sort_by(|left, right| {
        left.2
            .cmp(&right.2)
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
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

fn encounter_group_combat_modifiers(
    encounter: &BattleEncounter,
    manager: &NapcatMessageManager,
) -> TrpgGlobalCombatModifiers {
    encounter
        .trpg_group
        .as_deref()
        .and_then(|group_name| manager.trpg_groups.get(group_name))
        .map(|group| {
            group
                .global_combat_modifiers
                .effective_at_world_turn(group.world_turn)
        })
        .unwrap_or_default()
}

fn character_for_participant(
    participant: &BattleParticipantSnapshot,
    manager: &NapcatMessageManager,
) -> Option<PlayerCharacter> {
    let mut character = if participant.is_summon {
        participant.unit_character.clone()?
    } else if let Some(unit_id) = participant.unit_template_id.as_deref() {
        participant.unit_character.clone().or_else(|| {
            manager
                .unit_pool
                .get(unit_id)
                .map(|unit| unit.character.clone())
        })?
    } else {
        effective_hidden_role_character(
            &participant.target_id,
            manager.player_characters.get(&participant.target_id)?,
            &manager.hidden_roles,
        )
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
    character.skill_cooldown_ready_turns = participant.skill_cooldown_ready_turns.clone();
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
    let skills = character
        .skill_names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let display_name = if name.trim().is_empty() {
                format!("技能{}", index + 1)
            } else {
                name.trim().to_owned()
            };
            let note = character
                .skill_notes
                .get(index)
                .cloned()
                .unwrap_or_default();
            CharacterSkill {
                index,
                name: display_name,
                note: note.clone(),
                skill_type: character
                    .skill_metadata
                    .get(index)
                    .and_then(|metadata| metadata.skill_type.clone()),
                legacy_buff_machine_json: character
                    .skill_metadata
                    .get(index)
                    .and_then(|metadata| metadata.legacy_buff_machine_json.clone()),
                mp_cost: character_effective_skill_mp_cost(
                    character,
                    character
                        .skill_mp_costs
                        .get(index)
                        .copied()
                        .unwrap_or_default(),
                    character
                        .skill_metadata
                        .get(index)
                        .and_then(|metadata| metadata.skill_type.as_deref()),
                ),
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
                    .and_then(|metadata| metadata.range)
                    .or_else(|| current_redeemed_numeric_skill_range(&note)),
                arg_values: character
                    .skill_metadata
                    .get(index)
                    .map(|metadata| skill_rule_args(&metadata.args))
                    .unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    skills
        .into_iter()
        .flat_map(|skill| {
            let Some(modes) = current_redeemed_skill_modes(&skill.name, &skill.note) else {
                return vec![skill];
            };
            modes
                .iter()
                .map(|mode| CharacterSkill {
                    name: format!("{}（{}）", skill.name, mode.label),
                    note: mode.rule.to_owned(),
                    skill_type: Some(mode.skill_type.to_owned()),
                    mp_cost: mode.mp_cost,
                    cooldown_turns: mode.cooldown_turns,
                    cooldown_left: None,
                    target_count: mode.target_count,
                    target_class: Some(mode.target_class.to_owned()),
                    range: Some(mode.range),
                    ..skill.clone()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn item_skill_groups(character: &PlayerCharacter) -> Vec<(String, Vec<CharacterSkill>)> {
    let mut next_index = character.skill_names.len();
    let mut groups = Vec::new();
    for item in &character.inventory.items {
        let mut skills = Vec::new();
        for item_skill in &item.skills {
            let index = next_index;
            next_index += 1;
            if !item_skill.metadata.is_approved() {
                continue;
            }
            let skill_name = if item_skill.name.trim().is_empty() {
                format!("物品技能{}", skills.len() + 1)
            } else {
                item_skill.name.trim().to_owned()
            };
            skills.push(CharacterSkill {
                index,
                name: skill_name,
                note: item_skill.note.clone(),
                skill_type: item_skill.metadata.skill_type.clone(),
                legacy_buff_machine_json: item_skill.metadata.legacy_buff_machine_json.clone(),
                mp_cost: character_effective_skill_mp_cost(
                    character,
                    item_skill.mp_cost,
                    item_skill.metadata.skill_type.as_deref(),
                ),
                cooldown_turns: item_skill.cooldown_turns,
                cooldown_left: item_skill.metadata.cooldown_left,
                target_count: item_skill.metadata.target_count,
                target_class: item_skill.metadata.target_class.clone(),
                range: item_skill.metadata.range,
                arg_values: skill_rule_args(&item_skill.metadata.args),
            });
        }
        if !skills.is_empty() {
            groups.push((
                item_display_name_for_battle(item),
                skills,
            ));
        }
    }
    groups
}

fn item_display_name_for_battle(item: &crate::napcat::InventoryItem) -> String {
    let name = if item.name.trim().is_empty() { "未命名物品" } else { item.name.trim() };
    if item.stack > 1 {
        format!("{name} x{}", item.stack)
    } else {
        name.to_owned()
    }
}

fn item_skill_source(
    character: &PlayerCharacter,
    synthetic_skill_index: usize,
) -> Option<(usize, usize)> {
    let mut next_index = character.skill_names.len();
    for (item_index, item) in character.inventory.items.iter().enumerate() {
        for item_skill_index in 0..item.skills.len() {
            if next_index == synthetic_skill_index {
                return Some((item_index, item_skill_index));
            }
            next_index += 1;
        }
    }
    None
}

fn skill_item_consumes_on_cast(
    character: Option<&PlayerCharacter>,
    synthetic_skill_index: usize,
) -> bool {
    let Some(character) = character else {
        return false;
    };
    let Some((item_index, item_skill_index)) = item_skill_source(character, synthetic_skill_index)
    else {
        return false;
    };
    character.inventory.items[item_index].skills[item_skill_index].consume_item
}

fn consume_item_skill(
    character: &mut PlayerCharacter,
    synthetic_skill_index: usize,
) -> Option<String> {
    let (item_index, item_skill_index) = item_skill_source(character, synthetic_skill_index)?;
    if !character.inventory.items[item_index].skills[item_skill_index].consume_item {
        return None;
    }
    let item_name = {
        let name = character.inventory.items[item_index].name.trim();
        if name.is_empty() {
            "未命名物品".to_owned()
        } else {
            name.to_owned()
        }
    };
    if character.inventory.items[item_index].stack > 1 {
        character.inventory.items[item_index].stack -= 1;
    } else {
        character.inventory.items.remove(item_index);
        for slot in &mut character.inventory.hotbar {
            *slot = match *slot {
                crate::napcat::CharacterHotbarSlot::Item(index) if index == item_index => {
                    crate::napcat::CharacterHotbarSlot::Empty
                },
                crate::napcat::CharacterHotbarSlot::Item(index) if index > item_index => {
                    crate::napcat::CharacterHotbarSlot::Item(index - 1)
                },
                other => other,
            };
        }
    }
    Some(item_name)
}

fn skill_cooldown_remaining(
    participant: &BattleParticipantSnapshot,
    skill_index: usize,
    cooldown_turns: u32,
    cooldown_left: Option<u32>,
) -> u32 {
    let skill_key = skill_index.to_string();
    if let Some(last_used_turn) = participant.skill_last_used_turns.get(&skill_key) {
        return cooldown_turns.saturating_sub(participant.turn.saturating_sub(*last_used_turn));
    }
    participant
        .skill_cooldown_ready_turns
        .get(&skill_key)
        .map(|ready_turn| ready_turn.saturating_sub(participant.turn))
        .unwrap_or_else(|| cooldown_left.unwrap_or_default())
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
    let group_config = encounter
        .trpg_group
        .as_deref()
        .and_then(|group_name| manager.trpg_groups.get(group_name))
        .map(|group| {
            (
                group.basic_config,
                group.campaign_id.as_str(),
            )
        });
    if let Some((config, campaign_id)) = group_config {
        let int_ = manager
            .player_characters
            .get(actor_id)
            .map(|character| character.status.int_ + character.extra_status.int_)
            .unwrap_or_default();
        trpg_config_with_weave(config, campaign_id, int_)
    } else {
        manager.character_stat_config_for_target(actor_id)
    }
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

fn participant_group_modifiers(
    participant: &BattleParticipantSnapshot,
) -> TrpgGlobalCombatModifiers {
    participant.group_modifiers
}

fn participant_healing_taken_multiplier(participant: &BattleParticipantSnapshot) -> f32 {
    participant.healing_taken_modifier * participant_group_modifiers(participant).healing_taken
}

fn participant_damage_multiplier(
    participant: &BattleParticipantSnapshot,
    character: Option<&PlayerCharacter>,
    config: &TrpgBasicConfig,
    completed_turns: u32,
    damage_type: DamageType,
    encounter_active: bool,
) -> f32 {
    participant_damage_modifiers(
        participant,
        character,
        config,
        completed_turns,
        damage_type,
        encounter_active,
    )
    .into_iter()
    .fold(1.0, |value, modifier| {
        value * modifier.multiplier
    })
}

fn participant_damage_modifiers(
    participant: &BattleParticipantSnapshot,
    character: Option<&PlayerCharacter>,
    config: &TrpgBasicConfig,
    completed_turns: u32,
    damage_type: DamageType,
    encounter_active: bool,
) -> Vec<RuleModifier> {
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
    let inspiration_multiplier = if encounter_active {
        participant_inspiration_multiplier(participant)
    } else {
        1.0
    };
    let arrogance_multiplier = if encounter_active {
        arrogance_damage_dealt_multiplier(
            participant.arrogance_damage_bonus_per_source,
            participant.arrogance_damage_source_ids.len() as u32,
        )
    } else {
        1.0
    };
    let undying_rage_multiplier = if encounter_active {
        participant_undying_rage_damage_multiplier(participant)
    } else {
        1.0
    };
    let attribute_multiplier =
        status_damage_attribute_multiplier(&status, config, bonus_kind) + talent_bonus;
    let champion_multiplier = champion_damage_dealt_multiplier(
        participant.champion_damage_bonus_per_stack,
        participant.champion_stacks,
    );
    // 召唤物不会因为自身生命值低下而受到伤害减少。
    let low_hp_multiplier = if participant.is_summon {
        1.0
    } else {
        low_hp_damage_multiplier_with_fatigue(
            participant.hp,
            participant.max_hp,
            character
                .map(character_fatigue_walker_available)
                .unwrap_or(false),
        )
    };
    let chaos_multiplier = character
        .map(character_chaos_output_variance)
        .map(moonberry_chaos_output_multiplier)
        .unwrap_or(1.0);
    let valorous_multiplier = if encounter_active {
        character
            .map(|character| {
                character_valorous_battle_damage_multiplier(character, completed_turns)
            })
            .unwrap_or(1.0)
    } else {
        1.0
    };
    [
        (
            "造成伤害修正（角色/装备/BUFF）",
            participant.damage_dealt_modifier,
        ),
        (
            "TRPG组全局造成伤害修正",
            participant_group_modifiers(participant).damage_dealt,
        ),
        ("属性伤害加成", attribute_multiplier),
        ("振奋", inspiration_multiplier),
        ("不死者之怒", undying_rage_multiplier),
        ("傲慢", arrogance_multiplier),
        ("强者", champion_multiplier),
        ("低生命/疲惫行者", low_hp_multiplier),
        ("混沌输出", chaos_multiplier),
        ("勇战", valorous_multiplier),
        (
            "蝴蝶效应",
            if participant.butterfly_effect { 1.05 } else { 1.0 },
        ),
    ]
    .into_iter()
    .filter(|(source, multiplier)| {
        *source == "属性伤害加成" || (*multiplier - 1.0).abs() > f32::EPSILON
    })
    .map(|(source, multiplier)| RuleAmountResolution::factor(source, multiplier))
    .collect()
}

fn participant_damage_taken_multiplier(
    participant: &BattleParticipantSnapshot,
    character: Option<&PlayerCharacter>,
    damage_type: DamageType,
    encounter_active: bool,
) -> f32 {
    participant_damage_taken_modifiers(
        participant,
        character,
        damage_type,
        encounter_active,
    )
    .into_iter()
    .fold(1.0, |value, modifier| {
        value * modifier.multiplier
    })
}

fn participant_damage_taken_modifiers(
    participant: &BattleParticipantSnapshot,
    character: Option<&PlayerCharacter>,
    damage_type: DamageType,
    encounter_active: bool,
) -> Vec<RuleModifier> {
    let champion = champion_damage_taken_multiplier(
        participant.champion_damage_reduction_per_stack,
        participant.champion_stacks,
    );
    let typed = character
        .map(|character| {
            character_damage_taken_attribute_multiplier(
                character,
                trpg_damage_taken_kind(damage_type),
            )
        })
        .unwrap_or(1.0);
    let fighting_spirit = if encounter_active {
        character
            .map(|character| {
                character_fighting_spirit_damage_taken_multiplier(
                    character,
                    participant.combat_turns_completed,
                )
            })
            .unwrap_or(1.0)
    } else {
        1.0
    };
    let hidden_role_immunity = match damage_type {
        DamageType::Diseased if participant.hidden_role.immune_diseased => 0.0,
        DamageType::Poisoning if participant.hidden_role.immune_poisoning => 0.0,
        DamageType::Bleed if participant.hidden_role.immune_bleed => 0.0,
        _ => 1.0,
    };
    [
        (
            "承受伤害修正（角色/装备/BUFF）",
            participant.damage_taken_modifier,
        ),
        (
            "TRPG组全局承受伤害修正",
            participant_group_modifiers(participant).damage_taken,
        ),
        ("强者减伤", champion),
        ("伤害类型抗性", typed),
        ("战意", fighting_spirit),
        (
            "仿生体生物伤害免疫",
            hidden_role_immunity,
        ),
        (
            "蝴蝶效应",
            if participant.butterfly_effect { 0.95 } else { 1.0 },
        ),
    ]
    .into_iter()
    .filter(|(_, multiplier)| (*multiplier - 1.0).abs() > f32::EPSILON)
    .map(|(source, multiplier)| RuleAmountResolution::factor(source, multiplier))
    .collect()
}

fn participant_healing_multiplier(
    participant: &BattleParticipantSnapshot,
    character: Option<&PlayerCharacter>,
    config: &TrpgBasicConfig,
) -> f32 {
    participant_healing_modifiers(participant, character, config)
        .into_iter()
        .fold(1.0, |value, modifier| {
            value * modifier.multiplier
        })
}

fn participant_healing_modifiers(
    participant: &BattleParticipantSnapshot,
    character: Option<&PlayerCharacter>,
    config: &TrpgBasicConfig,
) -> Vec<RuleModifier> {
    let wounded_modifier = character
        .map(character_wounded_healing_dealt_modifier)
        .unwrap_or(1.0);
    let dealt = penance_decayed_healing_dealt_modifier(
        participant.healing_dealt_modifier,
        participant.penance_healing_bonus_percent,
        participant.penance_kill_assist_count,
    );
    let attribute = status_healing_attribute_multiplier(&participant_status(participant), config);
    let wounded = wounded_healing_dealt_multiplier(
        participant.hp,
        participant.max_hp,
        wounded_modifier,
    );
    let chaos = character
        .map(character_chaos_output_variance)
        .map(moonberry_chaos_output_multiplier)
        .unwrap_or(1.0);
    [
        (
            "造成治疗修正（角色/装备/BUFF/忏悔）",
            dealt,
        ),
        (
            "TRPG组全局造成治疗修正",
            participant_group_modifiers(participant).healing_dealt,
        ),
        ("WIS/INT治疗属性加成", attribute),
        ("背水疗愈", wounded),
        ("混沌输出", chaos),
    ]
    .into_iter()
    .filter(|(source, multiplier)| {
        *source == "WIS/INT治疗属性加成" || (*multiplier - 1.0).abs() > f32::EPSILON
    })
    .map(|(source, multiplier)| RuleAmountResolution::factor(source, multiplier))
    .collect()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefeatedTargetPolicy {
    Exclude,
    AllowSingleTarget,
}

fn resolve_skill_targets(
    target: TargetSelector,
    actor_id: &str,
    selected_target_id: &str,
    encounter: &BattleEncounter,
    scene_positions: Option<&SceneCharacterPositions>,
    fallback_radius: Option<f32>,
    target_class: Option<&str>,
    defeated_target_policy: DefeatedTargetPolicy,
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
        let Some(selected_target) = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == selected_target_id)
        else {
            return Vec::new();
        };
        if !selected_target.alive && defeated_target_policy == DefeatedTargetPolicy::Exclude {
            return Vec::new();
        }
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
    if let Some(instance) = manager.unit_instances.get(&participant.target_id) {
        let name = instance.display_name.trim();
        if !name.is_empty() {
            return name.to_owned();
        }
    }
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

fn character_combat_source_notes(
    character: Option<&PlayerCharacter>,
    owner_label: &str,
) -> Vec<String> {
    let Some(character) = character else {
        return Vec::new();
    };
    let mut notes = Vec::new();
    let buff_names = character
        .active_buffs
        .iter()
        .filter(|buff| buff.turns_remaining != 0)
        .map(|buff| buff.name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    if !buff_names.is_empty() {
        notes.push(format!(
            "{owner_label}生效BUFF：{}",
            buff_names.join("、")
        ));
    }
    let equipment_names = character
        .inventory
        .equipment
        .values()
        .filter(|item| !item.stat_effects.is_empty())
        .map(|item| {
            let name = item.name.trim();
            if name.is_empty() {
                "未命名装备"
            } else {
                name
            }
        })
        .collect::<Vec<_>>();
    if !equipment_names.is_empty() {
        notes.push(format!(
            "{owner_label}属性装备：{}",
            equipment_names.join("、")
        ));
    }
    notes
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
            DefeatedTargetPolicy::Exclude,
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
            DefeatedTargetPolicy::Exclude,
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
            is_summon: false,
            summon_owner_id: None,
            summon_kind: SummonKind::Custom,
            level: 1,
            exp: 0,
            support_talent_experience_bonus_rate: 0.0,
            base_damage: 0.0,
            unit_rarity: UnitRarity::Normal,
            turn: 0,
            combat_turns_completed: 0,
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
            group_modifiers: TrpgGlobalCombatModifiers::default(),
            hidden_role: HiddenRoleBattleState::default(),
            protective_suit_kind: None,
            protective_suit_shield: 0.0,
            protective_suit_shield_max: 0.0,
            protective_suit_magic_only: false,
            protective_suit_passive_active: false,
            protective_suit_active_available: false,
            protective_suit_speed_bonus: 0.0,
            protective_suit_slow_rounds_remaining: 0,
            protective_suit_blind_rounds_remaining: 0,
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
            arcane_shield_rate: 0.0,
            overhealing_shield_cap_rate: 0.0,
            overhealing_shield: 0.0,
            overhealing_shield_turns_remaining: 0,
            revenge_soul_shield_rate: 0.0,
            revenge_soul_shield: 0.0,
            construct_shield: 0.0,
            construct_shield_max: 0.0,
            construct_shield_repair_rounds_remaining: 0,
            construct_repair_channel_rounds_remaining: 0,
            paralyzed_rounds_remaining: 0,
            commissar_proficiency: 0,
            next_attack_bonus_physical: 0.0,
            natural_hp_regen_suppressed: false,
            flying_needles_enabled: false,
            flying_needles_ready: 0,
            needle_case_enabled: false,
            needle_case_ready: 0,
            needle_case_progress_noncombat_rounds: 0,
            redeemed_invisibility_rounds_remaining: 0,
            undying_rage_enabled: false,
            undying_rage_used: false,
            undying_rage_active: false,
            hope_avatar_enabled: false,
            hope_avatar_used: false,
            hope_avatar_rounds_remaining: 0,
            mirror_coat_enabled: false,
            mirror_coat_layers: 0,
            mirror_coat_cooldown_remaining: 0,
            mirror_coat_cleanup_pending: false,
            sunset_enabled: false,
            sunset_death_time_reset_pending: false,
            goose_channeling_turns: 0,
            butterfly_enabled: false,
            butterfly_target_id: None,
            butterfly_effect_on_holder: false,
            butterfly_initialized: false,
            butterfly_locked: false,
            butterfly_effect: false,
            liquid_body_damage_delay_rate: 0.0,
            liquid_body_self_healing_rate: 0.0,
            calm_heart_healing_rate: 0.0,
            combat_damage_taken_total: 0.0,
            rest_then_fight_healing_rate: 0.0,
            rest_then_fight_turns: 0,
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
            damage_contribution_amounts: HashMap::new(),
            wound_healing_taken_turns: 0,
            delayed_damage_ticks: Vec::new(),
            delayed_healing_ticks: Vec::new(),
            corrosion_stacks: Vec::new(),
            damage_taken_this_turn: 0.0,
            healing_taken_this_turn: 0.0,
            skill_last_used_turns: HashMap::new(),
            skill_cooldown_ready_turns: HashMap::new(),
        }
    }

    #[test]
    fn normal_suit_shield_breaks_before_hp_and_disables_passive() {
        let mut participant = battle_participant("wearer");
        participant.protective_suit_kind = Some(ProtectiveSuitKind::Normal);
        participant.protective_suit_shield = 3.0;
        participant.protective_suit_shield_max = 3.0;
        participant.protective_suit_passive_active = true;
        participant.protective_suit_active_available = true;

        let resolution = apply_participant_typed_damage_for_battle(
            &mut participant,
            5.0,
            "enemy",
            true,
            DamageType::Physical,
        );

        assert_eq!(participant.protective_suit_shield, 0.0);
        assert_eq!(participant.hp, 8.0);
        assert!(!participant.protective_suit_passive_active);
        assert!(participant.protective_suit_active_available);
        assert_eq!(resolution.damage_absorbed, 3.0);
    }

    #[test]
    fn dark_matter_suit_only_absorbs_magical_damage() {
        let mut participant = battle_participant("wearer");
        participant.protective_suit_kind = Some(ProtectiveSuitKind::DarkMatter);
        participant.protective_suit_shield = 5.0;
        participant.protective_suit_shield_max = 5.0;
        participant.protective_suit_magic_only = true;
        participant.protective_suit_passive_active = true;

        apply_participant_typed_damage_for_battle(
            &mut participant,
            2.0,
            "enemy",
            true,
            DamageType::Physical,
        );
        assert_eq!(participant.hp, 8.0);
        assert_eq!(participant.protective_suit_shield, 5.0);

        apply_participant_typed_damage_for_battle(
            &mut participant,
            3.0,
            "enemy",
            true,
            DamageType::Magical,
        );
        assert_eq!(participant.hp, 8.0);
        assert_eq!(participant.protective_suit_shield, 2.0);
    }

    #[test]
    fn electronic_suit_loses_speed_bonus_when_its_shield_breaks() {
        let mut participant = battle_participant("wearer");
        participant.speed = 6.5;
        participant.low_survivor_speed = 6.5;
        participant.protective_suit_kind = Some(ProtectiveSuitKind::Electronic);
        participant.protective_suit_shield = 3.0;
        participant.protective_suit_shield_max = 3.0;
        participant.protective_suit_passive_active = true;
        participant.protective_suit_speed_bonus = 1.5;

        apply_participant_typed_damage_for_battle(
            &mut participant,
            3.0,
            "enemy",
            true,
            DamageType::Physical,
        );

        assert!(!participant.protective_suit_passive_active);
        assert_eq!(participant.speed, 5.0);
        assert_eq!(participant.low_survivor_speed, 5.0);
        assert_eq!(participant.protective_suit_speed_bonus, 0.0);
    }

    #[test]
    fn radiation_suit_ticks_nearby_hp_without_consuming_shields() {
        let mut emitter = battle_participant("emitter");
        emitter.protective_suit_kind = Some(ProtectiveSuitKind::Radiation);
        emitter.protective_suit_shield = 3.0;
        emitter.protective_suit_shield_max = 3.0;
        emitter.protective_suit_passive_active = false;
        let mut near = battle_participant("near");
        near.protective_suit_shield = 3.0;
        near.protective_suit_shield_max = 3.0;
        near.protective_suit_passive_active = true;
        let far = battle_participant("far");
        let mut encounter = BattleEncounter {
            active: true,
            participants: vec![emitter, near, far],
            ..default()
        };
        let positions = HashMap::from([
            ("emitter".to_owned(), Vec3::ZERO),
            ("near".to_owned(), Vec3::new(3.0, 0.0, 0.0)),
            ("far".to_owned(), Vec3::new(3.1, 0.0, 0.0)),
        ]);

        let advance = advance_radiation_protective_suits(&mut encounter, &positions);

        assert_eq!(encounter.participants[0].hp, 9.0);
        assert_eq!(encounter.participants[1].hp, 9.0);
        assert_eq!(encounter.participants[2].hp, 10.0);
        assert_eq!(encounter.participants[0].protective_suit_shield, 3.0);
        assert_eq!(encounter.participants[1].protective_suit_shield, 3.0);
        assert_eq!(advance.combat_log.len(), 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_target_moves_off_the_new_actor_when_turn_changes() {
        let target_options = vec![
            ("a".to_owned(), "A".to_owned()),
            ("b".to_owned(), "B".to_owned()),
        ];
        let living_target_ids = HashSet::from(["a".to_owned(), "b".to_owned()]);
        let mut target = "b".to_owned();

        update_action_target_for_actor(
            &mut target,
            "b",
            true,
            &target_options,
            &living_target_ids,
        );

        assert_eq!(target, "a");
    }

    #[test]
    fn action_target_keeps_a_non_self_selection_when_turn_changes() {
        let target_options = vec![
            ("a".to_owned(), "A".to_owned()),
            ("b".to_owned(), "B".to_owned()),
            ("c".to_owned(), "C".to_owned()),
        ];
        let living_target_ids = HashSet::from(["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        let mut target = "c".to_owned();

        update_action_target_for_actor(
            &mut target,
            "b",
            true,
            &target_options,
            &living_target_ids,
        );

        assert_eq!(target, "c");
    }

    #[test]
    fn action_target_allows_manual_self_selection_during_the_same_turn() {
        let target_options = vec![
            ("a".to_owned(), "A".to_owned()),
            ("b".to_owned(), "B".to_owned()),
        ];
        let living_target_ids = HashSet::from(["a".to_owned(), "b".to_owned()]);
        let mut target = "b".to_owned();

        update_action_target_for_actor(
            &mut target,
            "b",
            false,
            &target_options,
            &living_target_ids,
        );

        assert_eq!(target, "b");
    }

    #[test]
    fn summon_participant_uses_owner_level_and_charisma_damage_bonus() {
        let mut manager = empty_manager();
        let mut owner = PlayerCharacter::default();
        owner.inited = true;
        owner.level = 7;
        owner.status.cha = 10;
        let mut summon = Summon::default();
        summon.name = "小火灵".to_owned();
        summon.max_hp = 20.0;
        summon.hp = 12.0;
        manager
            .player_characters
            .insert("10001".to_owned(), owner.clone());

        let participant = participant_from_summon("10001", 0, &summon, &owner, &manager);
        assert!(participant.is_summon);
        assert_eq!(
            participant.summon_owner_id.as_deref(),
            Some("10001")
        );
        assert_eq!(participant.target_id, "summon:10001:0");
        assert_eq!(participant.level, 7);
        assert!((participant.hp - 12.0).abs() < f32::EPSILON);
        assert!((participant.max_hp - 20.0).abs() < f32::EPSILON);
        // 10点魅力 → +20%召唤物伤害。
        assert!((participant.damage_dealt_modifier - 1.2).abs() < f32::EPSILON);
    }

    #[test]
    fn redeemed_summon_profiles_expose_their_written_battle_skills() {
        let manager = empty_manager();
        let owner = PlayerCharacter {
            level: 3,
            ..Default::default()
        };
        let cases = [
            (
                Summon {
                    name: "武装无人机".to_owned(),
                    kind: SummonKind::ArmedDrone,
                    ..Default::default()
                },
                vec!["无人机枪械射击", "电能冲击"],
            ),
            (
                Summon {
                    name: "未命名机甲".to_owned(),
                    hp: 6.0,
                    max_hp: 6.0,
                    kind: SummonKind::Mech,
                    ..Default::default()
                },
                vec!["巨型金属刀横扫", "维修机甲"],
            ),
            (
                Summon {
                    name: "便携小炮塔".to_owned(),
                    kind: SummonKind::PortableTurret,
                    ..Default::default()
                },
                vec!["便携小炮塔射击"],
            ),
        ];

        for (summon, expected_names) in cases {
            let participant = participant_from_summon("owner", 0, &summon, &owner, &manager);
            let character = participant.unit_character.as_ref().unwrap();
            assert_eq!(
                character.skill_names,
                expected_names
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            );
        }
        let drone = Summon {
            kind: SummonKind::ArmedDrone,
            ..Default::default()
        };
        let participant = participant_from_summon("owner", 0, &drone, &owner, &manager);
        assert_eq!(participant.dex, 3);
        let skills = character_skills(participant.unit_character.as_ref().unwrap());
        assert_eq!(skills[0].range, Some(20));
        assert_eq!(skills[1].range, Some(3));

        let turret = Summon {
            kind: SummonKind::PortableTurret,
            ..Default::default()
        };
        let participant = participant_from_summon("owner", 0, &turret, &owner, &manager);
        let skills = character_skills(participant.unit_character.as_ref().unwrap());
        assert!(skills[0].note.contains("造成3点物理伤害"));
    }

    #[test]
    fn redeemed_mech_shield_absorbs_damage_and_rebuilds_after_two_rounds() {
        let manager = empty_manager();
        let owner = PlayerCharacter::default();
        let summon = Summon {
            hp: 6.0,
            max_hp: 6.0,
            kind: SummonKind::Mech,
            shield: 3.0,
            max_shield: 3.0,
            ..Default::default()
        };
        let mut mech = participant_from_summon("owner", 0, &summon, &owner, &manager);

        let resolution = apply_participant_damage_for_battle(&mut mech, 5.0, "enemy", true);
        assert_eq!(resolution.damage_absorbed, 3.0);
        assert_eq!(resolution.damage_applied, 2.0);
        assert_eq!(mech.hp, 4.0);
        assert_eq!(mech.construct_shield, 0.0);
        assert_eq!(
            mech.construct_shield_repair_rounds_remaining,
            2
        );

        assert!(advance_redeemed_construct_state(&mut mech, true).is_empty());
        assert_eq!(
            mech.construct_shield_repair_rounds_remaining,
            2
        );
        assert!(advance_redeemed_construct_state(&mut mech, false).is_empty());
        assert_eq!(mech.construct_shield, 0.0);
        let logs = advance_redeemed_construct_state(&mut mech, false);
        assert_eq!(mech.construct_shield, 3.0);
        assert_eq!(logs.len(), 1);
    }

    #[test]
    fn redeemed_mech_full_repair_channels_and_is_interrupted_by_hp_damage() {
        let mut mech = participant("mech", 0);
        mech.summon_kind = SummonKind::Mech;
        mech.hp = 2.0;
        mech.max_hp = 6.0;
        mech.construct_repair_channel_rounds_remaining = 2;

        assert!(advance_redeemed_construct_state(&mut mech, true).is_empty());
        assert!(!participant_can_act(&mech));
        let logs = advance_redeemed_construct_state(&mut mech, true);
        assert_eq!(mech.hp, 6.0);
        assert_eq!(logs.len(), 1);

        mech.hp = 2.0;
        mech.construct_repair_channel_rounds_remaining = 2;
        apply_participant_damage_for_battle(&mut mech, 1.0, "enemy", true);
        assert_eq!(
            mech.construct_repair_channel_rounds_remaining,
            0
        );
    }

    #[test]
    fn redeemed_drone_paralysis_note_parses_as_one_round_status() {
        let effects = static_skill_effects(
            "主动使用对周围3米内的目标施加1回合麻痹状态",
            &SkillRuleArgs::default(),
            Some("动作"),
            None,
        );
        assert!(effects.iter().any(|effect| matches!(
            effect,
            SkillEffect::GrantBuff { buff, .. }
                if buff.name.contains("麻痹") && buff.turns_remaining == 1
        )));
    }

    #[test]
    fn redeemed_drone_electric_shock_prevents_target_action() {
        let manager = empty_manager();
        let owner = PlayerCharacter::default();
        let drone = Summon {
            kind: SummonKind::ArmedDrone,
            ..Default::default()
        };
        let drone = participant_from_summon("owner", 0, &drone, &owner, &manager);
        let skill = character_skills(drone.unit_character.as_ref().unwrap())[1].clone();
        let target = participant("target", 0);
        let mut store = BattleRoundStore {
            encounters: HashMap::from([("battle".to_owned(), BattleEncounter {
                active: true,
                participants: vec![drone, target],
                ..Default::default()
            })]),
            ..Default::default()
        };
        let positions = SceneCharacterPositions {
            positions: HashMap::from([
                ("summon:owner:0".to_owned(), Vec3::ZERO),
                (
                    "target".to_owned(),
                    Vec3::new(2.0, 0.0, 0.0),
                ),
            ]),
        };

        assert!(store.record_skill_use(
            "battle",
            "summon:owner:0",
            "target",
            &skill,
            &manager,
            Some(&positions),
        ));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        assert_eq!(target.paralyzed_rounds_remaining, 1);
        assert!(!participant_can_act(target));
    }

    #[test]
    fn redeemed_commissar_war_cry_grants_and_consumes_proficiency_damage() {
        let manager = empty_manager();
        let war_cry = CharacterSkill {
            index: 0,
            name: "政委".to_owned(),
            note: "职业:政委（3分）\n政委是帝国卫队中最具标志性的角色。\n职业技能:\n战吼—为了帝皇:熟练度0/5，每次使用前熟练度+1，使目标下次攻击伤害附带【熟练度】点物理伤害。".to_owned(),
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
        let attack = CharacterSkill {
            index: 0,
            name: "测试攻击".to_owned(),
            note: "主动使用对目标造成2点物理伤害".to_owned(),
            skill_type: Some("近战".to_owned()),
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: Some(1),
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let mut store = BattleRoundStore {
            encounters: HashMap::from([("battle".to_owned(), BattleEncounter {
                active: true,
                participants: vec![
                    participant("commissar", 0),
                    participant("ally", 0),
                    participant("enemy", 0),
                ],
                ..Default::default()
            })]),
            ..Default::default()
        };

        assert!(!store.record_skill_use(
            "battle",
            "commissar",
            "missing",
            &war_cry,
            &manager,
            None,
        ));
        assert_eq!(
            store.encounters["battle"].participants[0].commissar_proficiency,
            0
        );
        assert!(
            store.encounters["battle"].participants[0]
                .skill_last_used_turns
                .is_empty()
        );

        assert!(store.record_skill_use(
            "battle",
            "commissar",
            "ally",
            &war_cry,
            &manager,
            None,
        ));
        assert_eq!(
            store.encounters["battle"].participants[0].commissar_proficiency,
            1
        );
        assert_eq!(
            store.encounters["battle"].participants[1].next_attack_bonus_physical,
            1.0
        );
        assert!(store.record_skill_use("battle", "ally", "enemy", &attack, &manager, None,));
        assert_eq!(
            store.encounters["battle"].participants[1].next_attack_bonus_physical,
            0.0
        );
        assert_eq!(
            store.encounters["battle"].participants[2].hp,
            7.0
        );
    }

    #[test]
    fn redeemed_commissar_proficiency_caps_and_syncs_to_character() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "commissar".to_owned(),
            PlayerCharacter {
                name: "膳鹰".to_owned(),
                ..Default::default()
            },
        );
        let war_cry = CharacterSkill {
            index: 0,
            name: "政委".to_owned(),
            note: "战吼-为了帝皇：熟练度3/5，每次使用前熟练度+1，使目标下次攻击伤害附带熟练度点物理伤害。".to_owned(),
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
        let mut commissar = participant("commissar", 0);
        commissar.player_character = true;
        let mut store = BattleRoundStore {
            encounters: HashMap::from([("battle".to_owned(), BattleEncounter {
                active: true,
                participants: vec![commissar, participant("ally", 0)],
                ..Default::default()
            })]),
            ..Default::default()
        };

        for _ in 0..6 {
            assert!(store.record_skill_use(
                "battle",
                "commissar",
                "ally",
                &war_cry,
                &manager,
                None,
            ));
        }

        let encounter = &store.encounters["battle"];
        assert_eq!(
            encounter.participants[0].commissar_proficiency,
            5
        );
        assert_eq!(
            encounter.participants[1].next_attack_bonus_physical,
            5.0
        );
        assert!(sync_encounter_to_manager(
            Some(encounter),
            &mut manager
        ));
        assert_eq!(
            manager.player_characters["commissar"].redeemed_commissar_proficiency,
            5
        );
    }

    #[test]
    fn redeemed_chainsword_tear_suppresses_only_natural_regeneration() {
        let manager = empty_manager();
        let chainsword = CharacterSkill {
            index: 0,
            name: "链锯剑".to_owned(),
            note: "装备\n链锯剑（6分）\n由机械教加持的加强版链锯剑，锯齿采用更坚硬的合金，近身攻击会造成5点物理伤害。\n特性:撕裂 近身命中目标造成流血效果，抑制目标的自然生命回复效果。".to_owned(),
            skill_type: Some("近战".to_owned()),
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: Some(1),
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let mut target = participant("enemy", 0);
        target.hp_regen = 2.0;
        let mut store = BattleRoundStore {
            encounters: HashMap::from([("battle".to_owned(), BattleEncounter {
                participants: vec![participant("actor", 0), target],
                ..Default::default()
            })]),
            ..Default::default()
        };

        assert!(store.record_skill_use(
            "battle",
            "actor",
            "enemy",
            &chainsword,
            &manager,
            None,
        ));
        assert_eq!(
            store.encounters["battle"].participants[1].hp,
            5.0
        );
        assert!(store.encounters["battle"].participants[1].natural_hp_regen_suppressed);
        assert!(store.next_round("battle"));
        assert_eq!(
            store.encounters["battle"].participants[1].hp,
            5.0
        );
    }

    #[test]
    fn redeemed_blow_needles_consumes_all_inventory_for_dynamic_damage() {
        let manager = empty_manager();
        let skill = CharacterSkill {
            index: 0,
            name: "吹针术".to_owned(),
            note:
                "一次性射出当前持有的所有飞针，造成命中的飞针数量+2的远程物理伤害，但命中率会降低"
                    .to_owned(),
            skill_type: Some("远程".to_owned()),
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: Some(1),
            target_class: Some("单目标".to_owned()),
            range: Some(6),
            arg_values: SkillRuleArgs::default(),
        };
        let mut actor = participant("actor", 0);
        actor.flying_needles_enabled = true;
        actor.flying_needles_ready = 3;
        actor.needle_case_enabled = true;
        actor.needle_case_ready = 1;
        let mut enemy = participant("enemy", 0);
        enemy.hp = 20.0;
        enemy.max_hp = 20.0;
        let mut store = BattleRoundStore {
            encounters: HashMap::from([("battle".to_owned(), BattleEncounter {
                active: true,
                participants: vec![actor, enemy],
                ..Default::default()
            })]),
            ..Default::default()
        };
        let positions = SceneCharacterPositions {
            positions: HashMap::from([
                ("actor".to_owned(), Vec3::ZERO),
                (
                    "enemy".to_owned(),
                    Vec3::new(2.0, 0.0, 0.0),
                ),
            ]),
        };

        assert!(store.record_skill_use(
            "battle",
            "actor",
            "enemy",
            &skill,
            &manager,
            Some(&positions),
        ));
        let encounter = &store.encounters["battle"];
        assert_eq!(
            encounter.participants[0].flying_needles_ready,
            0
        );
        assert_eq!(
            encounter.participants[0].needle_case_ready,
            0
        );
        assert_eq!(encounter.participants[1].hp, 14.0);
    }

    #[test]
    fn redeemed_needles_refill_only_on_noncombat_rounds() {
        let mut actor = participant("actor", 0);
        actor.flying_needles_enabled = true;
        actor.flying_needles_ready = 2;
        actor.needle_case_enabled = true;

        assert!(!advance_redeemed_needles_noncombat(
            &mut participant("other", 0)
        ));
        assert!(advance_redeemed_needles_noncombat(
            &mut actor
        ));
        assert_eq!(actor.flying_needles_ready, 3);
        assert_eq!(actor.needle_case_ready, 0);
        assert_eq!(
            actor.needle_case_progress_noncombat_rounds,
            1
        );
        assert!(advance_redeemed_needles_noncombat(
            &mut actor
        ));
        assert_eq!(actor.flying_needles_ready, 4);
        assert_eq!(actor.needle_case_ready, 1);
        assert_eq!(
            actor.needle_case_progress_noncombat_rounds,
            0
        );
    }

    #[test]
    fn redeemed_invisibility_hides_actor_for_three_rounds() {
        let manager = empty_manager();
        let skill = CharacterSkill {
            index: 0,
            name: "隐身术".to_owned(),
            note: "冷却2，持续3回合，无消耗。".to_owned(),
            skill_type: Some("动作".to_owned()),
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 2,
            cooldown_left: None,
            target_count: None,
            target_class: Some("无目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let mut store = BattleRoundStore {
            encounters: HashMap::from([("battle".to_owned(), BattleEncounter {
                active: true,
                participants: vec![participant("540716134", 0)],
                ..Default::default()
            })]),
            ..Default::default()
        };

        assert!(store.record_skill_use(
            "battle",
            "540716134",
            "540716134",
            &skill,
            &manager,
            None,
        ));
        assert_eq!(
            store.encounters["battle"].participants[0].redeemed_invisibility_rounds_remaining,
            3
        );
        assert!(store.next_round("battle"));
        assert_eq!(
            store.encounters["battle"].participants[0].redeemed_invisibility_rounds_remaining,
            2
        );
    }

    #[test]
    fn summon_participant_ignores_low_hp_damage_penalty() {
        let mut manager = empty_manager();
        let mut owner = PlayerCharacter::default();
        owner.level = 5;
        owner.status.cha = 5;
        let mut summon = Summon::default();
        summon.max_hp = 20.0;
        summon.hp = 2.0; // 仅10%生命，普通角色会吃到严重低血惩罚。
        manager
            .player_characters
            .insert("10001".to_owned(), owner.clone());
        let participant = participant_from_summon("10001", 0, &summon, &owner, &manager);
        let config = TrpgBasicConfig::default();
        let modifiers = participant_damage_modifiers(
            &participant,
            None,
            &config,
            0,
            DamageType::Magical,
            true,
        );
        assert!(!modifiers
            .iter()
            .any(|modifier| modifier.source == "低生命/疲惫行者"));
        // 相同生命下普通角色会吃到0.1倍低血惩罚，召唤物不受影响。
        let low_hp_multiplier = low_hp_damage_multiplier_with_fatigue(2.0, 20.0, false);
        assert!((low_hp_multiplier - 0.1).abs() < 0.0001);
        let summon_multiplier = participant_damage_multiplier(
            &participant,
            None,
            &config,
            0,
            DamageType::Magical,
            true,
        );
        assert!((summon_multiplier - 1.1).abs() < 0.0001);
    }

    #[test]
    fn battle_sync_writes_summon_hp_back_to_owner() {
        let mut manager = empty_manager();
        let mut owner = PlayerCharacter::default();
        owner.inited = true;
        owner.level = 5;
        let mut summon = Summon::default();
        summon.max_hp = 20.0;
        summon.hp = 20.0;
        owner.summons.push(summon);
        manager.player_characters.insert("10001".to_owned(), owner);

        let owner = manager.player_characters["10001"].clone();
        let summon = owner.summons[0].clone();
        let mut participant = participant_from_summon("10001", 0, &summon, &owner, &manager);
        participant.hp = 3.0;
        let encounter = BattleEncounter {
            participants: vec![participant],
            ..Default::default()
        };

        assert!(sync_encounter_to_manager(
            Some(&encounter),
            &mut manager
        ));
        assert_eq!(
            manager.player_characters["10001"].summons[0].hp,
            3.0
        );
    }

    fn empty_manager() -> NapcatMessageManager {
        NapcatMessageManager {
            messages: HashMap::default(),
            replay_snapshots: HashMap::default(),
            chat_targets: HashMap::default(),
            chat_target_kinds: HashMap::default(),
            player_characters: HashMap::default(),
            hidden_roles: HashMap::default(),
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
            item_pool: Vec::new(),
            unit_pool: HashMap::default(),
            unit_instances: HashMap::default(),
            next_unit_instance_index: 1,
            pending_talent_choices: HashMap::default(),
            used_talent_names: HashSet::default(),
        }
    }

    #[test]
    fn persistent_unit_instances_join_battle_independently_and_sync_back() {
        let mut manager = empty_manager();
        let mut template = UnitPoolEntry::default();
        template.label = "史莱姆".to_owned();
        template.character.hp = 12.0;
        template.character.max_hp = 12.0;
        manager.unit_pool.insert("slime".to_owned(), template);
        let first_id = manager.create_unit_instance("slime").unwrap();
        let second_id = manager.create_unit_instance("slime").unwrap();

        let mut store = BattleRoundStore::default();
        store.encounters.insert(
            "battle-1".to_owned(),
            BattleEncounter::default(),
        );
        assert!(store
            .add_unit_instance_to_encounter("battle-1", &first_id, &manager)
            .unwrap());
        assert!(store
            .add_unit_instance_to_encounter("battle-1", &second_id, &manager)
            .unwrap());
        assert!(!store
            .add_unit_instance_to_encounter("battle-1", &first_id, &manager)
            .unwrap());

        let encounter = store.encounters.get_mut("battle-1").unwrap();
        assert_eq!(encounter.participants.len(), 2);
        encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == first_id)
            .unwrap()
            .hp = 4.0;
        assert!(sync_encounter_to_manager(
            Some(encounter),
            &mut manager
        ));
        assert_eq!(
            manager.unit_instances[&first_id].character.hp,
            4.0
        );
        assert_eq!(
            manager.unit_instances[&second_id].character.hp,
            12.0
        );
    }

    fn participant(id: &str, turn: u32) -> BattleParticipantSnapshot {
        BattleParticipantSnapshot {
            target_id: id.to_owned(),
            display_name: id.to_owned(),
            unit_template_id: None,
            unit_character: None,
            player_character: false,
            is_summon: false,
            summon_owner_id: None,
            summon_kind: SummonKind::Custom,
            level: 1,
            exp: 0,
            support_talent_experience_bonus_rate: 0.0,
            base_damage: 0.0,
            unit_rarity: UnitRarity::Normal,
            turn,
            combat_turns_completed: 0,
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
            group_modifiers: TrpgGlobalCombatModifiers::default(),
            hidden_role: HiddenRoleBattleState::default(),
            protective_suit_kind: None,
            protective_suit_shield: 0.0,
            protective_suit_shield_max: 0.0,
            protective_suit_magic_only: false,
            protective_suit_passive_active: false,
            protective_suit_active_available: false,
            protective_suit_speed_bonus: 0.0,
            protective_suit_slow_rounds_remaining: 0,
            protective_suit_blind_rounds_remaining: 0,
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
            arcane_shield_rate: 0.0,
            overhealing_shield_cap_rate: 0.0,
            overhealing_shield: 0.0,
            overhealing_shield_turns_remaining: 0,
            revenge_soul_shield_rate: 0.0,
            revenge_soul_shield: 0.0,
            construct_shield: 0.0,
            construct_shield_max: 0.0,
            construct_shield_repair_rounds_remaining: 0,
            construct_repair_channel_rounds_remaining: 0,
            paralyzed_rounds_remaining: 0,
            commissar_proficiency: 0,
            next_attack_bonus_physical: 0.0,
            natural_hp_regen_suppressed: false,
            flying_needles_enabled: false,
            flying_needles_ready: 0,
            needle_case_enabled: false,
            needle_case_ready: 0,
            needle_case_progress_noncombat_rounds: 0,
            redeemed_invisibility_rounds_remaining: 0,
            undying_rage_enabled: false,
            undying_rage_used: false,
            undying_rage_active: false,
            hope_avatar_enabled: false,
            hope_avatar_used: false,
            hope_avatar_rounds_remaining: 0,
            mirror_coat_enabled: false,
            mirror_coat_layers: 0,
            mirror_coat_cooldown_remaining: 0,
            mirror_coat_cleanup_pending: false,
            sunset_enabled: false,
            sunset_death_time_reset_pending: false,
            goose_channeling_turns: 0,
            butterfly_enabled: false,
            butterfly_target_id: None,
            butterfly_effect_on_holder: false,
            butterfly_initialized: false,
            butterfly_locked: false,
            butterfly_effect: false,
            liquid_body_damage_delay_rate: 0.0,
            liquid_body_self_healing_rate: 0.0,
            calm_heart_healing_rate: 0.0,
            combat_damage_taken_total: 0.0,
            rest_then_fight_healing_rate: 0.0,
            rest_then_fight_turns: 0,
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
            damage_contribution_amounts: HashMap::new(),
            wound_healing_taken_turns: 0,
            delayed_damage_ticks: Vec::new(),
            delayed_healing_ticks: Vec::new(),
            corrosion_stacks: Vec::new(),
            damage_taken_this_turn: 0.0,
            healing_taken_this_turn: 0.0,
            skill_last_used_turns: HashMap::new(),
            skill_cooldown_ready_turns: HashMap::new(),
        }
    }

    #[test]
    fn battle_round_export_round_trips_active_combat_state() {
        let mut actor = participant("a", 7);
        actor.action_done = true;
        actor.hp = 6.0;
        actor.arcane_shield = 4.5;
        actor.support_talent_experience_bonus_rate = SUPPORT_TALENT_EXPERIENCE_BONUS_RATE;
        actor.damage_contributors = vec!["enemy".to_owned()];
        actor.skill_cooldown_ready_turns = HashMap::from([("0".to_owned(), 12)]);
        let store = BattleRoundStore {
            encounters: HashMap::from(
                [("battle-4".to_owned(), BattleEncounter {
                    name: "首领战".to_owned(),
                    trpg_group: Some("table".to_owned()),
                    trpg_campaign_id: Some("campaign-a".to_owned()),
                    manager_sync_quarantined: true,
                    active: true,
                    round: 7,
                    combat_completed_turns: 13,
                    participants: vec![actor],
                    action_log: vec!["a使用技能".to_owned()],
                    ..Default::default()
                })],
            ),
            active_encounter_id: Some("battle-4".to_owned()),
            next_encounter_index: 9,
            scene_positions: HashMap::new(),
        };
        let json = store.to_export_json().unwrap();
        let restored = BattleRoundStore::from_export_json(&json).unwrap();
        let encounter = &restored.encounters["battle-4"];
        let actor = &encounter.participants[0];

        assert_eq!(
            restored.active_encounter_id.as_deref(),
            Some("battle-4")
        );
        assert_eq!(restored.next_encounter_index, 9);
        assert_eq!(
            encounter.trpg_campaign_id.as_deref(),
            Some("campaign-a")
        );
        assert!(encounter.manager_sync_quarantined);
        assert_eq!(encounter.round, 7);
        assert_eq!(encounter.combat_completed_turns, 13);
        assert_eq!(encounter.action_log, vec![
            "a使用技能".to_owned()
        ]);
        assert_eq!(actor.hp, 6.0);
        assert_eq!(actor.arcane_shield, 4.5);
        assert_eq!(
            actor.support_talent_experience_bonus_rate,
            SUPPORT_TALENT_EXPERIENCE_BONUS_RATE
        );
        assert_eq!(actor.damage_contributors, vec![
            "enemy".to_owned()
        ]);
        assert_eq!(
            actor.skill_cooldown_ready_turns["0"],
            12
        );
        assert!(json.contains("\"export_type\": \"battle_rounds\""));
    }

    #[test]
    fn manager_sync_quarantine_marks_each_battle_once_and_survives_backup() {
        let mut store = BattleRoundStore {
            encounters: HashMap::from([
                (
                    "first".to_owned(),
                    BattleEncounter::default(),
                ),
                (
                    "second".to_owned(),
                    BattleEncounter::default(),
                ),
            ]),
            ..Default::default()
        };

        assert_eq!(store.quarantine_manager_sync(), 2);
        assert_eq!(store.quarantine_manager_sync(), 0);

        let restored =
            BattleRoundStore::from_export_json(&store.to_export_json().unwrap()).unwrap();
        assert!(restored
            .encounters
            .values()
            .all(|encounter| encounter.manager_sync_quarantined));
    }

    #[test]
    fn legacy_battle_backup_defaults_to_manager_sync_enabled() {
        let store = BattleRoundStore {
            encounters: HashMap::from([(
                "battle".to_owned(),
                BattleEncounter::default(),
            )]),
            ..Default::default()
        };
        let mut export: serde_json::Value =
            serde_json::from_str(&store.to_export_json().unwrap()).unwrap();
        export["store"]["encounters"]["battle"]
            .as_object_mut()
            .unwrap()
            .remove("manager_sync_quarantined");

        let restored = BattleRoundStore::from_export_json(&export.to_string()).unwrap();
        assert!(!restored.encounters["battle"].manager_sync_quarantined);
    }

    #[test]
    fn battle_round_import_repairs_duplicate_participants_and_stale_active_id() {
        let store = BattleRoundStore {
            encounters: HashMap::from([("battle".to_owned(), BattleEncounter {
                participants: vec![participant("a", 1), participant("a", 2)],
                ..Default::default()
            })]),
            active_encounter_id: Some("missing".to_owned()),
            next_encounter_index: 2,
            scene_positions: HashMap::new(),
        };

        let restored =
            BattleRoundStore::from_export_json(&store.to_export_json().unwrap()).unwrap();

        assert_eq!(
            restored.encounters["battle"].participants.len(),
            1
        );
        assert_eq!(
            restored.encounters["battle"].participants[0].turn,
            1
        );
        assert_eq!(restored.active_encounter_id, None);
    }

    #[test]
    fn battle_round_import_rejects_wrong_type_and_empty_participant_id() {
        let wrong_version = serde_json::json!({
            "version": BATTLE_ROUND_EXPORT_VERSION + 1,
            "export_type": "battle_rounds",
            "store": {},
        })
        .to_string();
        assert!(
            BattleRoundStore::from_export_json(&wrong_version)
                .err()
                .expect("wrong export version should fail")
                .contains("unsupported battle round export version")
        );

        let wrong_type = serde_json::json!({
            "version": BATTLE_ROUND_EXPORT_VERSION,
            "export_type": "voxel_scene",
            "store": {},
        })
        .to_string();
        assert!(
            BattleRoundStore::from_export_json(&wrong_type)
                .err()
                .expect("wrong export type should fail")
                .contains("unsupported battle round export type")
        );

        let invalid = BattleRoundStore {
            encounters: HashMap::from([("battle".to_owned(), BattleEncounter {
                participants: vec![participant(" ", 0)],
                ..Default::default()
            })]),
            ..Default::default()
        };
        assert!(
            BattleRoundStore::from_export_json(&invalid.to_export_json().unwrap())
                .err()
                .expect("empty participant id should fail")
                .contains("empty participant id")
        );
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
        );

        let encounter = &store.encounters[&encounter_id];
        assert_eq!(encounter.round, 7);
        assert_eq!(
            encounter.trpg_campaign_id.as_deref(),
            Some("default")
        );
        assert!(
            encounter.participants.is_empty(),
            "new battles must not auto-join all group players"
        );
        let encounter = store.encounters.get_mut(&encounter_id).unwrap();
        assert!(refresh_encounter_players(
            encounter, &manager
        ));
        assert_eq!(encounter.participants[0].turn, 6);
        assert_eq!(
            encounter.participants[0].skill_last_used_turns,
            HashMap::from([("0".to_owned(), 5)])
        );
    }

    #[test]
    fn duplicate_group_players_create_one_actionable_battle_participant() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "a".to_owned(),
            PlayerCharacter::default(),
        );
        let group = TrpgGroup {
            players: vec!["a".to_owned(), "a".to_owned()],
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
        );

        assert!(store.encounters[&encounter_id].participants.is_empty());
        {
            let encounter = store.encounters.get_mut(&encounter_id).unwrap();
            assert!(refresh_encounter_players(
                encounter, &manager
            ));
            assert_eq!(encounter.participants.len(), 1);
        }
        assert!(store.finish_actor_action(&encounter_id, "a"));
        assert_eq!(store.encounters[&encounter_id].round, 1);
    }

    #[test]
    fn new_battle_id_wraps_and_skips_existing_imported_encounters() {
        let group = TrpgGroup::default();
        let mut store = BattleRoundStore {
            next_encounter_index: u64::MAX,
            encounters: HashMap::from([
                (
                    format!("battle-{}", u64::MAX),
                    BattleEncounter {
                        name: "maximum".to_owned(),
                        ..Default::default()
                    },
                ),
                ("battle-1".to_owned(), BattleEncounter {
                    name: "first".to_owned(),
                    ..Default::default()
                }),
            ]),
            ..Default::default()
        };

        let encounter_id = store.create_encounter_from_group(
            "new".to_owned(),
            "party".to_owned(),
            &group,
        );

        assert_eq!(encounter_id, "battle-2");
        assert_eq!(store.next_encounter_index, 3);
        assert_eq!(
            store.encounters[&format!("battle-{}", u64::MAX)].name,
            "maximum"
        );
        assert_eq!(
            store.encounters["battle-1"].name,
            "first"
        );
        assert_eq!(store.encounters["battle-2"].name, "new");
    }

    #[test]
    fn new_battle_reuses_the_canonical_encounter_for_its_group() {
        let group = TrpgGroup::default();
        let mut store = BattleRoundStore {
            active_encounter_id: Some("battle-old".to_owned()),
            encounters: HashMap::from([
                (
                    "battle-old".to_owned(),
                    BattleEncounter {
                        name: "stale".to_owned(),
                        trpg_group: Some("party".to_owned()),
                        round: 2,
                        ..Default::default()
                    },
                ),
                (
                    "battle-current".to_owned(),
                    BattleEncounter {
                        name: "current".to_owned(),
                        trpg_group: Some("party".to_owned()),
                        round: 3,
                        ..Default::default()
                    },
                ),
            ]),
            ..Default::default()
        };

        assert_eq!(
            store.canonical_encounter_id_for_group("party", None),
            Some("battle-current")
        );
        let encounter_id = store.create_encounter_from_group(
            "replacement".to_owned(),
            "party".to_owned(),
            &group,
        );

        assert_eq!(encounter_id, "battle-current");
        assert_eq!(store.encounters.len(), 2);
        assert_eq!(
            store.encounters["battle-current"].name,
            "current"
        );
        assert_eq!(
            store.encounters["battle-old"].name,
            "stale"
        );
    }

    #[test]
    fn recreated_group_does_not_reuse_an_old_campaign_battle() {
        let group = TrpgGroup {
            campaign_id: "party-2".to_owned(),
            ..Default::default()
        };
        let mut store = BattleRoundStore {
            encounters: HashMap::from([(
                "battle-old".to_owned(),
                BattleEncounter {
                    name: "old campaign".to_owned(),
                    trpg_group: Some("party".to_owned()),
                    trpg_campaign_id: Some("party".to_owned()),
                    round: 9,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };

        let encounter_id = store.create_encounter_from_group(
            "new campaign".to_owned(),
            "party".to_owned(),
            &group,
        );

        assert_ne!(encounter_id, "battle-old");
        assert_eq!(store.encounters.len(), 2);
        assert_eq!(
            store.encounters[&encounter_id].trpg_campaign_id.as_deref(),
            Some("party-2")
        );
        assert_eq!(
            store.encounters["battle-old"].name,
            "old campaign"
        );
    }

    #[test]
    fn legacy_group_battle_binds_to_campaign_once() {
        let mut store = BattleRoundStore {
            encounters: HashMap::from([("battle".to_owned(), BattleEncounter {
                trpg_group: Some("party".to_owned()),
                ..Default::default()
            })]),
            ..Default::default()
        };

        assert!(store.bind_legacy_encounter_campaign("battle", "campaign-a"));
        assert!(!store.bind_legacy_encounter_campaign("battle", "campaign-b"));
        assert_eq!(
            store.encounters["battle"].trpg_campaign_id.as_deref(),
            Some("campaign-a")
        );
    }

    #[test]
    fn active_encounter_breaks_equal_round_group_ties() {
        let store = BattleRoundStore {
            active_encounter_id: Some("battle-selected".to_owned()),
            encounters: HashMap::from([
                (
                    "battle-selected".to_owned(),
                    BattleEncounter {
                        trpg_group: Some("party".to_owned()),
                        round: 3,
                        ..Default::default()
                    },
                ),
                ("battle-z".to_owned(), BattleEncounter {
                    trpg_group: Some("party".to_owned()),
                    round: 3,
                    ..Default::default()
                }),
            ]),
            ..Default::default()
        };

        assert_eq!(
            store.canonical_encounter_id_for_group("party", None),
            Some("battle-selected")
        );
    }

    #[test]
    fn duplicate_group_encounter_cannot_advance_or_apply_actions() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            world_turn: 4,
            ..Default::default()
        });
        let mut store = BattleRoundStore {
            encounters: HashMap::from([
                (
                    "battle-stale".to_owned(),
                    BattleEncounter {
                        trpg_group: Some("party".to_owned()),
                        round: 2,
                        participants: vec![participant("actor", 2), participant("target", 2)],
                        ..Default::default()
                    },
                ),
                (
                    "battle-current".to_owned(),
                    BattleEncounter {
                        trpg_group: Some("party".to_owned()),
                        round: 3,
                        ..Default::default()
                    },
                ),
            ]),
            ..Default::default()
        };

        assert!(!sync_encounter_from_group_clock(
            &mut store,
            "battle-stale",
            &manager
        ));
        assert!(!store.next_round("battle-stale"));
        assert!(!store.apply_action(
            "battle-stale",
            "actor",
            "target",
            "攻击",
            3.0
        ));

        let stale = &store.encounters["battle-stale"];
        assert_eq!(stale.round, 2);
        assert_eq!(stale.participants[1].hp, 10.0);
        assert!(stale.action_log.is_empty());
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
            round: 3,
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
        assert_eq!(group.world_turn, 3);
        assert_eq!(group.player_turns["a"].turns_passed, 3);
        assert!(group.player_turns["a"].acted);
        assert!(!group.player_turns["a"].skipped);
    }

    #[test]
    fn linked_battle_sync_ignores_player_characters_outside_its_group() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("member".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager
            .player_characters
            .insert("outsider".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["member".to_owned()],
            player_turns: HashMap::from([(
                "member".to_owned(),
                crate::napcat::TrpgPlayerTurnState::default(),
            )]),
            ..Default::default()
        });
        let mut member = participant("member", 0);
        member.player_character = true;
        member.hp = 6.0;
        let mut outsider = participant("outsider", 0);
        outsider.player_character = true;
        outsider.hp = 1.0;
        let encounter = BattleEncounter {
            trpg_group: Some("party".to_owned()),
            participants: vec![member, outsider],
            ..Default::default()
        };

        assert!(sync_encounter_to_manager(
            Some(&encounter),
            &mut manager
        ));
        assert_eq!(
            manager.player_characters["member"].hp,
            6.0
        );
        assert_eq!(
            manager.player_characters["outsider"].hp,
            10.0
        );
    }

    #[test]
    fn battle_level_up_grants_two_status_points_per_level() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "a".to_owned(),
            PlayerCharacter::default(),
        );
        let mut player = participant("a", 0);
        player.player_character = true;
        player.level = 3;
        player.exp = 0;
        let encounter = BattleEncounter {
            participants: vec![player],
            ..Default::default()
        };

        assert!(sync_encounter_to_manager(
            Some(&encounter),
            &mut manager
        ));

        let character = &manager.player_characters["a"];
        assert_eq!(character.level, 3);
        assert_eq!(character.status_points, 9);
        assert_eq!(character.max_hp, 25.0);
    }

    #[test]
    fn quarantined_battle_cannot_write_character_or_group_state() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("a".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 8.0,
                max_mp: 8.0,
                damage_taken_this_turn: 1.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            campaign_id: "campaign-a".to_owned(),
            players: vec!["a".to_owned()],
            world_turn: 2,
            player_turns: HashMap::from([(
                "a".to_owned(),
                crate::napcat::TrpgPlayerTurnState {
                    turns_passed: 2,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        let mut player = participant("a", 7);
        player.player_character = true;
        player.action_done = true;
        player.hp = 2.0;
        player.mp = 1.0;
        player.damage_taken_this_turn = 8.0;
        let encounter = BattleEncounter {
            trpg_group: Some("party".to_owned()),
            trpg_campaign_id: Some("campaign-a".to_owned()),
            manager_sync_quarantined: true,
            round: 7,
            participants: vec![player],
            ..Default::default()
        };

        assert!(!sync_encounter_to_manager(
            Some(&encounter),
            &mut manager
        ));

        let character = &manager.player_characters["a"];
        assert_eq!(character.hp, 10.0);
        assert_eq!(character.mp, 8.0);
        assert_eq!(character.damage_taken_this_turn, 1.0);
        let group = &manager.trpg_groups["party"];
        assert_eq!(group.world_turn, 2);
        assert_eq!(group.player_turns["a"].turns_passed, 2);
        assert!(!group.player_turns["a"].acted);
    }

    #[test]
    fn quarantined_battle_reconnect_requires_matching_group_and_campaign() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            campaign_id: "campaign-new".to_owned(),
            ..Default::default()
        });
        let standalone = BattleEncounter::default();
        let legacy = BattleEncounter {
            trpg_group: Some("party".to_owned()),
            ..Default::default()
        };
        let matching = BattleEncounter {
            trpg_group: Some("party".to_owned()),
            trpg_campaign_id: Some("campaign-new".to_owned()),
            ..Default::default()
        };
        let stale = BattleEncounter {
            trpg_group: Some("party".to_owned()),
            trpg_campaign_id: Some("campaign-old".to_owned()),
            ..Default::default()
        };
        let missing = BattleEncounter {
            trpg_group: Some("missing".to_owned()),
            ..Default::default()
        };

        assert!(encounter_can_reconnect_to_manager(
            &standalone,
            &manager
        ));
        assert!(encounter_can_reconnect_to_manager(
            &legacy, &manager
        ));
        assert!(encounter_can_reconnect_to_manager(
            &matching, &manager
        ));
        assert!(!encounter_can_reconnect_to_manager(
            &stale, &manager
        ));
        assert!(!encounter_can_reconnect_to_manager(
            &missing, &manager
        ));
    }

    #[test]
    fn deleted_group_battle_cannot_write_character_state() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("a".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 8.0,
                max_mp: 8.0,
                damage_taken_this_turn: 1.0,
                ..Default::default()
            });
        let mut player = participant("a", 4);
        player.player_character = true;
        player.hp = 2.0;
        player.mp = 1.0;
        player.damage_taken_this_turn = 8.0;
        let encounter = BattleEncounter {
            trpg_group: Some("deleted-party".to_owned()),
            round: 4,
            participants: vec![player],
            ..Default::default()
        };

        assert!(!sync_encounter_to_manager(
            Some(&encounter),
            &mut manager
        ));

        let character = &manager.player_characters["a"];
        assert_eq!(character.hp, 10.0);
        assert_eq!(character.mp, 8.0);
        assert_eq!(character.damage_taken_this_turn, 1.0);
    }

    #[test]
    fn old_campaign_battle_cannot_write_into_recreated_group() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("a".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            campaign_id: "party-2".to_owned(),
            ..Default::default()
        });
        let mut player = participant("a", 4);
        player.player_character = true;
        player.hp = 2.0;
        let encounter = BattleEncounter {
            trpg_group: Some("party".to_owned()),
            trpg_campaign_id: Some("party".to_owned()),
            round: 4,
            participants: vec![player],
            ..Default::default()
        };

        assert!(!sync_encounter_to_manager(
            Some(&encounter),
            &mut manager
        ));
        assert_eq!(manager.player_characters["a"].hp, 10.0);
    }

    #[test]
    fn battle_sync_preserves_a_group_skip_that_is_already_ahead() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "a".to_owned(),
            PlayerCharacter::default(),
        );
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["a".to_owned()],
            world_turn: 3,
            player_turns: HashMap::from([(
                "a".to_owned(),
                crate::napcat::TrpgPlayerTurnState {
                    turns_passed: 3,
                    skipped: true,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        let mut player = participant("a", 3);
        player.player_character = true;
        let encounter = BattleEncounter {
            trpg_group: Some("party".to_owned()),
            round: 3,
            participants: vec![player],
            ..Default::default()
        };

        sync_encounter_to_manager(Some(&encounter), &mut manager);

        let turn = &manager.trpg_groups["party"].player_turns["a"];
        assert_eq!(turn.turns_passed, 3);
        assert!(!turn.acted);
        assert!(turn.skipped);
    }

    #[test]
    fn newer_battle_round_clears_previous_group_completion_flags() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "a".to_owned(),
            PlayerCharacter::default(),
        );
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["a".to_owned()],
            world_turn: 3,
            player_turns: HashMap::from([(
                "a".to_owned(),
                crate::napcat::TrpgPlayerTurnState {
                    turns_passed: 3,
                    acted: true,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        let mut player = participant("a", 4);
        player.player_character = true;
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

        let group = &manager.trpg_groups["party"];
        assert_eq!(group.world_turn, 4);
        assert_eq!(group.player_turns["a"].turns_passed, 4);
        assert!(!group.player_turns["a"].acted);
        assert!(!group.player_turns["a"].skipped);
    }

    #[test]
    fn newer_group_round_is_not_rolled_back_by_a_stale_battle() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "a".to_owned(),
            PlayerCharacter::default(),
        );
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["a".to_owned()],
            world_turn: 4,
            player_turns: HashMap::from([(
                "a".to_owned(),
                crate::napcat::TrpgPlayerTurnState {
                    turns_passed: 4,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        let mut player = participant("a", 3);
        player.player_character = true;
        player.action_done = true;
        let encounter = BattleEncounter {
            trpg_group: Some("party".to_owned()),
            round: 3,
            participants: vec![player],
            ..Default::default()
        };

        sync_encounter_to_manager(Some(&encounter), &mut manager);

        let group = &manager.trpg_groups["party"];
        assert_eq!(group.world_turn, 4);
        assert_eq!(group.player_turns["a"].turns_passed, 4);
        assert!(!group.player_turns["a"].acted);
        assert!(!group.player_turns["a"].skipped);
    }

    #[test]
    fn newer_group_round_catches_battle_up_from_manager_vitals() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("a".to_owned(), PlayerCharacter {
                hp: 8.0,
                max_hp: 10.0,
                mp: 4.0,
                max_mp: 10.0,
                mp_regen: 1.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["a".to_owned()],
            world_turn: 2,
            player_turns: HashMap::from([(
                "a".to_owned(),
                crate::napcat::TrpgPlayerTurnState {
                    turns_passed: 2,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        let mut player = participant("a", 0);
        player.player_character = true;
        player.hp = 1.0;
        player.mp = 0.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                trpg_group: Some("party".to_owned()),
                active: true,
                participants: vec![player],
                ..Default::default()
            });

        assert!(sync_encounter_from_group_clock(
            &mut store, "battle", &manager
        ));

        let encounter = &store.encounters["battle"];
        let player = &encounter.participants[0];
        assert_eq!(encounter.round, 2);
        assert_eq!(player.turn, 2);
        assert_eq!(player.hp, 8.0);
        assert_eq!(player.mp, 6.0);
        assert_eq!(player.combat_turns_completed, 2);
        assert_eq!(encounter.combat_completed_turns, 2);
        assert!(!player.action_done);
    }

    #[test]
    fn extreme_group_round_gap_is_bounded_and_keeps_actions_closed() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("a".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 0.0,
                max_mp: 10.0,
                mp_regen: 1.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["a".to_owned()],
            world_turn: u32::MAX,
            player_turns: HashMap::from([(
                "a".to_owned(),
                crate::napcat::TrpgPlayerTurnState {
                    turns_passed: u32::MAX,
                    acted: true,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        let mut player = participant("a", 0);
        player.player_character = true;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                trpg_group: Some("party".to_owned()),
                active: true,
                participants: vec![player],
                ..Default::default()
            });

        assert!(sync_encounter_from_group_clock(
            &mut store, "battle", &manager
        ));

        let encounter = &store.encounters["battle"];
        let player = &encounter.participants[0];
        assert_eq!(
            encounter.round,
            MAX_GROUP_CLOCK_CATCH_UP_ROUNDS_PER_FRAME
        );
        assert_eq!(
            player.turn,
            MAX_GROUP_CLOCK_CATCH_UP_ROUNDS_PER_FRAME
        );
        assert_eq!(
            player.combat_turns_completed,
            MAX_GROUP_CLOCK_CATCH_UP_ROUNDS_PER_FRAME
        );
        assert!(!player.action_done);
        assert_eq!(
            group_rounds_ahead_of_encounter(&store, "battle", &manager),
            u32::MAX - MAX_GROUP_CLOCK_CATCH_UP_ROUNDS_PER_FRAME
        );
    }

    #[test]
    fn current_group_skip_finishes_the_matching_battle_action() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "a".to_owned(),
            PlayerCharacter::default(),
        );
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["a".to_owned()],
            world_turn: 3,
            player_turns: HashMap::from([(
                "a".to_owned(),
                crate::napcat::TrpgPlayerTurnState {
                    turns_passed: 3,
                    skipped: true,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        let mut player = participant("a", 3);
        player.player_character = true;
        player.pending_negative = true;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                trpg_group: Some("party".to_owned()),
                round: 3,
                active: true,
                participants: vec![player],
                ..Default::default()
            });

        assert!(sync_encounter_from_group_clock(
            &mut store, "battle", &manager
        ));

        let encounter = &store.encounters["battle"];
        let player = &encounter.participants[0];
        assert_eq!(player.turn, 4);
        assert!(player.action_done);
        assert!(!player.pending_negative);
        assert_eq!(player.combat_turns_completed, 1);
        assert_eq!(encounter.combat_completed_turns, 1);
    }

    #[test]
    fn stale_group_completion_does_not_finish_a_newer_battle_action() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "a".to_owned(),
            PlayerCharacter::default(),
        );
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["a".to_owned()],
            world_turn: 3,
            player_turns: HashMap::from([(
                "a".to_owned(),
                crate::napcat::TrpgPlayerTurnState {
                    turns_passed: 3,
                    acted: true,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        let mut player = participant("a", 5);
        player.player_character = true;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                trpg_group: Some("party".to_owned()),
                round: 3,
                participants: vec![player],
                ..Default::default()
            });

        assert!(!sync_encounter_from_group_clock(
            &mut store, "battle", &manager
        ));

        let player = &store.encounters["battle"].participants[0];
        assert_eq!(player.turn, 5);
        assert!(!player.action_done);
    }

    #[test]
    fn manual_alive_edits_keep_hp_and_refresh_state_consistent() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("a".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        let mut player = participant("a", 0);
        player.player_character = true;
        player.hope_avatar_rounds_remaining = 2;

        set_participant_alive_after_manual_edit(&mut player, false);
        assert_eq!(player.hp, 0.0);
        assert!(!player.alive);
        assert_eq!(player.hope_avatar_rounds_remaining, 0);

        let mut encounter = BattleEncounter {
            participants: vec![player],
            ..Default::default()
        };
        assert!(sync_encounter_to_manager(
            Some(&encounter),
            &mut manager
        ));
        sync_participant_from_manager_with_vitals(&mut encounter.participants[0], &manager);
        assert_eq!(encounter.participants[0].hp, 0.0);
        assert!(!encounter.participants[0].alive);

        encounter.participants[0].hp = 5.0;
        normalize_encounter_after_edit(&mut encounter);
        assert!(encounter.participants[0].alive);
        set_participant_alive_after_manual_edit(&mut encounter.participants[0], false);

        set_participant_alive_after_manual_edit(&mut encounter.participants[0], true);
        assert_eq!(encounter.participants[0].hp, 1.0);
        assert!(encounter.participants[0].alive);
        assert!(sync_encounter_to_manager(
            Some(&encounter),
            &mut manager
        ));
        sync_participant_from_manager_with_vitals(&mut encounter.participants[0], &manager);
        assert_eq!(encounter.participants[0].hp, 1.0);
        assert!(encounter.participants[0].alive);
    }

    #[test]
    fn battle_order_uses_agi_and_ignores_speed_modifiers() {
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
            participant_order_speed(
                gale_participant,
                living_player_participant_count(&encounter),
                encounter.active,
            ),
            12.0
        );
        assert_eq!(
            ordered_participant_indices(&encounter)
                .into_iter()
                .map(|index| encounter.participants[index].target_id.as_str())
                .collect::<Vec<_>>(),
            vec!["gale", "fast", "p3", "p4"]
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
            participant_order_speed(
                encounter
                    .participants
                    .iter()
                    .find(|participant| participant.target_id == "gale")
                    .unwrap(),
                living_player_participant_count(&encounter),
                encounter.active,
            ),
            13.5
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
            category: String::new(),
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            rarity: UnitRarity::Normal,
            base_damage: 0.0,
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
    fn trpg_group_global_modifiers_cover_all_combat_amount_channels_and_units() {
        let unit = UnitPoolEntry {
            category: String::new(),
            label: "测试NPC".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            rarity: UnitRarity::Normal,
            base_damage: 1.0,
            character: PlayerCharacter::default(),
        };
        let mut participant = participant_from_unit_template("npc:1", "test", &unit);
        participant.group_modifiers = TrpgGlobalCombatModifiers {
            damage_dealt: 1.5,
            damage_taken: 0.75,
            healing_dealt: 2.0,
            healing_taken: 0.5,
            ..Default::default()
        };
        let config = TrpgBasicConfig::default();

        let damage_dealt = participant_damage_modifiers(
            &participant,
            None,
            &config,
            0,
            DamageType::Physical,
            false,
        );
        assert!(damage_dealt.iter().any(|modifier| {
            modifier.source == "TRPG组全局造成伤害修正"
                && (modifier.multiplier - 1.5).abs() < f32::EPSILON
        }));
        let damage_taken = participant_damage_taken_modifiers(
            &participant,
            None,
            DamageType::Physical,
            false,
        );
        assert!(damage_taken.iter().any(|modifier| {
            modifier.source == "TRPG组全局承受伤害修正"
                && (modifier.multiplier - 0.75).abs() < f32::EPSILON
        }));
        let healing_dealt = participant_healing_modifiers(&participant, None, &config);
        assert!(healing_dealt.iter().any(|modifier| {
            modifier.source == "TRPG组全局造成治疗修正"
                && (modifier.multiplier - 2.0).abs() < f32::EPSILON
        }));
        assert!((participant_healing_taken_multiplier(&participant) - 0.5).abs() < f32::EPSILON);

        let expected = participant.group_modifiers;
        participant.unit_template_id = None;
        participant.player_character = true;
        assert_eq!(
            participant_group_modifiers(&participant),
            expected,
            "players must receive the global modifier"
        );
        participant.player_character = false;
        participant.is_summon = true;
        assert_eq!(
            participant_group_modifiers(&participant),
            expected,
            "player summons must receive the global modifier"
        );
    }

    #[test]
    fn group_global_modifier_changes_refresh_existing_players_and_npcs() {
        let mut manager = empty_manager();
        let unit = UnitPoolEntry {
            category: String::new(),
            label: "测试NPC".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            rarity: UnitRarity::Normal,
            base_damage: 1.0,
            character: PlayerCharacter::default(),
        };
        manager.unit_pool.insert("test".to_owned(), unit.clone());
        let summon = Summon {
            hp: 5.0,
            max_hp: 5.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("player".to_owned(), PlayerCharacter {
                summons: vec![summon.clone()],
                ..Default::default()
            });
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            global_combat_modifiers: TrpgGlobalCombatModifiers {
                damage_dealt: 1.0,
                damage_dealt_per_world_turn_percent: 0.1,
                damage_taken: 0.8,
                damage_taken_per_world_turn_percent: 0.2,
                healing_dealt: 1.5,
                healing_dealt_per_world_turn_percent: 0.3,
                healing_taken: 0.6,
                healing_taken_per_world_turn_percent: 0.4,
            },
            players: vec!["player".to_owned()],
            world_turn: 10,
            ..Default::default()
        });
        let mut encounter = BattleEncounter {
            trpg_group: Some("table".to_owned()),
            participants: vec![
                participant_from_unit_template("npc:1", "test", &unit),
                participant_from_character(
                    "player",
                    &manager.player_characters["player"],
                    &manager,
                ),
                participant_from_summon(
                    "player",
                    0,
                    &summon,
                    &manager.player_characters["player"],
                    &manager,
                ),
            ],
            ..Default::default()
        };

        assert!(refresh_encounter_players(
            &mut encounter,
            &manager
        ));
        assert_eq!(
            encounter.participants[0].group_modifiers,
            manager.trpg_groups["table"]
                .global_combat_modifiers
                .effective_at_world_turn(10)
        );
        assert_eq!(
            encounter.participants[1].group_modifiers, encounter.participants[0].group_modifiers,
            "players and NPCs must receive the same group modifier"
        );
        assert_eq!(
            encounter.participants[2].group_modifiers, encounter.participants[0].group_modifiers,
            "player summons must remain in the roster and receive the same group modifier"
        );
        let effective = encounter.participants[0].group_modifiers;
        assert!((effective.damage_dealt - 1.01).abs() < 0.0001);
        assert!((effective.damage_taken - 0.82).abs() < 0.0001);
        assert!((effective.healing_dealt - 1.53).abs() < 0.0001);
        assert!((effective.healing_taken - 0.64).abs() < 0.0001);

        let legacy_group: TrpgGroup = serde_json::from_str("{}").unwrap();
        assert_eq!(
            legacy_group.global_combat_modifiers,
            TrpgGlobalCombatModifiers::default()
        );
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
            category: String::new(),
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            rarity: UnitRarity::Normal,
            base_damage: 0.0,
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
            skill_names: vec!["不死者之怒".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("source".to_owned(), source.clone());
        let unit = UnitPoolEntry {
            category: String::new(),
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            rarity: UnitRarity::Normal,
            base_damage: 0.0,
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
                active: false,
                participants: vec![
                    participant_from_character("source", &source, &manager),
                    target,
                ],
                ..Default::default()
            });
        store.encounters.get_mut("battle").unwrap().participants[0].undying_rage_active = true;

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
            category: String::new(),
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            rarity: UnitRarity::Normal,
            base_damage: 0.0,
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
            category: String::new(),
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            rarity: UnitRarity::Normal,
            base_damage: 0.0,
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
    fn refresh_encounter_players_repairs_duplicate_persisted_participants() {
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
        let mut encounter = BattleEncounter {
            name: "battle".to_owned(),
            trpg_group: Some("table".to_owned()),
            participants: vec![participant("pc", 0), participant("pc", 0)],
            ..Default::default()
        };

        assert!(refresh_encounter_players(
            &mut encounter,
            &manager
        ));
        assert_eq!(encounter.participants.len(), 1);
        assert_eq!(
            encounter.participants[0].target_id,
            "pc"
        );
    }

    #[test]
    fn battle_store_repairs_duplicate_participants_in_every_loaded_encounter() {
        let duplicate_encounter = |target_id: &str| {
            let mut first = participant(target_id, 0);
            first.hp = 3.0;
            let mut duplicate = participant(target_id, 0);
            duplicate.hp = 9.0;
            BattleEncounter {
                name: target_id.to_owned(),
                participants: vec![first, duplicate],
                ..Default::default()
            }
        };
        let mut store = BattleRoundStore {
            encounters: HashMap::from([
                (
                    "battle-a".to_owned(),
                    duplicate_encounter("a"),
                ),
                (
                    "battle-b".to_owned(),
                    duplicate_encounter("b"),
                ),
            ]),
            ..Default::default()
        };

        assert!(store.repair_duplicate_participants());
        assert_eq!(
            store.encounters["battle-a"].participants.len(),
            1
        );
        assert_eq!(
            store.encounters["battle-b"].participants.len(),
            1
        );
        assert_eq!(
            store.encounters["battle-a"].participants[0].hp,
            3.0
        );
        assert!(!store.repair_duplicate_participants());
    }

    #[test]
    fn linked_battle_roster_prunes_outsiders_but_keeps_units() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["member".to_owned()],
            ..Default::default()
        });
        let mut unit = participant("unit:slime", 0);
        unit.player_character = false;
        unit.unit_template_id = Some("slime".to_owned());
        let mut encounter = BattleEncounter {
            trpg_group: Some("table".to_owned()),
            participants: vec![participant("member", 0), participant("outsider", 0), unit],
            ..Default::default()
        };

        assert!(prune_unbound_group_participants(
            &mut encounter,
            &manager
        ));
        assert_eq!(
            encounter
                .participants
                .iter()
                .map(|participant| participant.target_id.as_str())
                .collect::<Vec<_>>(),
            vec!["member", "unit:slime"]
        );
        assert!(!prune_unbound_group_participants(
            &mut encounter,
            &manager
        ));
    }

    #[test]
    fn removed_unit_participants_stay_out_of_linked_battle_roster() {
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
            category: String::new(),
            label: "史莱姆".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            rarity: UnitRarity::Normal,
            base_damage: 0.0,
            character: PlayerCharacter {
                hp: 6.0,
                max_hp: 6.0,
                ..Default::default()
            },
        });
        let unit = manager.unit_pool["slime"].clone();
        let mut encounter = BattleEncounter {
            name: "battle".to_owned(),
            trpg_group: Some("table".to_owned()),
            participants: vec![
                participant_from_unit_template("unit:slime", "slime", &unit),
                participant("pc", 0),
            ],
            ..Default::default()
        };

        encounter
            .participants
            .retain(|participant| participant.target_id != "unit:slime");

        assert!(!encounter
            .participants
            .iter()
            .any(|participant| participant.target_id == "unit:slime"));
        let roster_len_before_sync = encounter.participants.len();
        refresh_encounter_players(&mut encounter, &manager);
        assert_eq!(
            encounter.participants.len(),
            roster_len_before_sync
        );
        assert!(!encounter
            .participants
            .iter()
            .any(|participant| participant.target_id == "unit:slime"));
    }

    #[test]
    fn linked_battle_player_candidates_exclude_other_groups() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "member".to_owned(),
            PlayerCharacter::default(),
        );
        manager.player_characters.insert(
            "outsider".to_owned(),
            PlayerCharacter::default(),
        );
        manager
            .chat_targets
            .insert("unbound".to_owned(), Default::default());
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["member".to_owned()],
            ..Default::default()
        });
        let linked = BattleEncounter {
            trpg_group: Some("table".to_owned()),
            ..Default::default()
        };
        let missing_link = BattleEncounter {
            trpg_group: Some("missing".to_owned()),
            ..Default::default()
        };
        let standalone = BattleEncounter::default();

        assert_eq!(
            available_group_players(&linked, &manager)
                .into_iter()
                .map(|(target_id, _)| target_id)
                .collect::<Vec<_>>(),
            vec!["member"]
        );
        assert!(available_group_players(&missing_link, &manager).is_empty());
        let standalone_candidates = available_group_players(&standalone, &manager)
            .into_iter()
            .map(|(target_id, _)| target_id)
            .collect::<HashSet<_>>();
        assert_eq!(
            standalone_candidates,
            HashSet::from([
                "member".to_owned(),
                "outsider".to_owned(),
                "unbound".to_owned(),
            ])
        );
    }

    #[test]
    fn group_battle_defaults_apply_to_new_encounters() {
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
        );

        let encounter = &store.encounters[&encounter_id];
        assert!(!encounter.sort_by_turn);
        assert!(encounter.negative_enabled);
        assert!(
            encounter.participants.is_empty(),
            "new battles must not auto-join all group players"
        );
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
        assert_eq!(participant.combat_turns_completed, 1);
        assert_eq!(
            store.encounters["battle"].combat_completed_turns,
            1
        );
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
        assert!((target.hp - 15.0).abs() < 0.0001);
        assert!((manager.player_characters["b"].hp - 15.0).abs() < 0.0001);

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
        let weave_bonus =
            f32::from(crate::napcat::campaign_weave_state("default").magic_damage_bonus_percent)
                / 100.0;
        let expected_hp = 50.0 - 10.0 * (2.05 + weave_bonus);
        assert!((target.hp - expected_hp).abs() < 0.0001);
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

        let serialized = serde_json::to_string(&store).unwrap();
        let mut store = serde_json::from_str::<BattleRoundStore>(&serialized).unwrap();

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 6.5).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 3.5).abs() < 0.0001);
        assert!(target.delayed_damage_ticks.is_empty());

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!(target.delayed_damage_ticks.is_empty());
        assert!((target.hp - 6.5).abs() < 0.0001);

        let mut stale_target = participant("stale", 0);
        stale_target.hp = 20.0;
        stale_target.max_hp = 20.0;
        stale_target
            .delayed_damage_ticks
            .push(BattleDelayedDamageTick {
                name: "旧存档已结算伤害".to_owned(),
                source_id: "a".to_owned(),
                source_name: "a".to_owned(),
                amount: 5.0,
                damage_type: DamageType::Magical,
                turns_remaining: 1,
            });
        store
            .encounters
            .insert("stale".to_owned(), BattleEncounter {
                name: "stale".to_owned(),
                participants: vec![stale_target],
                ..Default::default()
            });
        assert!(store.next_round("stale"));
        let stale_target = &store.encounters["stale"].participants[0];
        assert!((stale_target.hp - 20.0).abs() < 0.0001);
        assert!(stale_target.delayed_damage_ticks.is_empty());
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
                target.turn = 99;
                target.combat_turns_completed = turn;
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

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            false
        ));
        assert_eq!(
            store.encounters["battle"].combat_completed_turns,
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
        assert_eq!(target.combat_turns_completed, 0);
        target.hp = 100.0;
        target.damage_taken_this_turn = 0.0;
        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 90.0).abs() < 0.0001);
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
        assert!((target.hp - 90.0).abs() < 0.0001);

        {
            let encounter = store.encounters.get_mut("battle").unwrap();
            encounter.combat_completed_turns = 5;
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
        assert!((target.hp - 89.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 11.0).abs() < 0.0001);

        {
            let encounter = store.encounters.get_mut("battle").unwrap();
            encounter.combat_completed_turns = 10;
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

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            false
        ));
        assert_eq!(
            store.encounters["battle"].combat_completed_turns,
            0
        );
        let encounter = store.encounters.get_mut("battle").unwrap();
        encounter.combat_completed_turns = 10;
        let target = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        target.hp = 100.0;
        target.damage_taken_this_turn = 0.0;
        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 90.0).abs() < 0.0001);

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            true
        ));
        assert_eq!(
            store.encounters["battle"].combat_completed_turns,
            0
        );
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

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            false
        ));
        let arrogant = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!(arrogant.arrogance_damage_source_ids.is_empty());

        assert!(store.apply_action("battle", "e", "a", "休整试探", 1.0));
        let encounter = store.encounters.get_mut("battle").unwrap();
        let arrogant = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!(arrogant.arrogance_damage_source_ids.is_empty());
        arrogant.arrogance_damage_source_ids = vec!["b".to_owned(), "c".to_owned(), "d".to_owned()];
        let target = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        target.hp = 100.0;
        target.damage_taken_this_turn = 0.0;

        assert!(store.record_skill_use("battle", "a", "target", &skill, &manager, None,));
        let encounter = &store.encounters["battle"];
        let arrogant = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        assert_eq!(
            arrogant.arrogance_damage_source_ids.len(),
            3
        );
        assert!((target.hp - 90.0).abs() < 0.0001);

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            true
        ));
        let arrogant = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!(arrogant.arrogance_damage_source_ids.is_empty());
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

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            false
        ));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert_eq!(actor.endless_pain_stacks, 0);

        assert!(store.apply_action("battle", "b", "a", "休整试探", 1.0));
        let encounter = store.encounters.get_mut("battle").unwrap();
        let actor = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert_eq!(actor.endless_pain_stacks, 0);
        actor.endless_pain_stacks = 2;
        let target = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        target.hp = 100.0;
        target.damage_taken_this_turn = 0.0;

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
        assert_eq!(actor.endless_pain_stacks, 2);
        assert!((target.hp - 90.0).abs() < 0.0001);

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            true
        ));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert_eq!(actor.endless_pain_stacks, 0);
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
        assert!(target.delayed_damage_ticks.is_empty());

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

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            false
        ));
        let target = store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        target.hp = 20.0;
        target.alive = true;
        reset_participant_turn_totals(target);

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 10.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 10.0).abs() < 0.0001);
        assert!(target.delayed_damage_ticks.is_empty());

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 10.0).abs() < 0.0001);
        assert_eq!(target.healing_taken_this_turn, 0.0);
        assert_eq!(target.damage_taken_this_turn, 0.0);
        assert!(target.delayed_damage_ticks.is_empty());

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            true
        ));
        let target = store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        target.hp = 20.0;
        target.alive = true;
        reset_participant_turn_totals(target);
        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 15.0).abs() < 0.0001);
        assert_eq!(target.delayed_damage_ticks.len(), 1);

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            false
        ));
        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 10.0).abs() < 0.0001);
        assert_eq!(target.healing_taken_this_turn, 0.0);
        assert!(target.delayed_damage_ticks.is_empty());

        assert!(store.advance_participant("battle", "b", false));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 10.0).abs() < 0.0001);
        assert_eq!(target.healing_taken_this_turn, 0.0);
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

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, false
        ));
        assert!(!encounter.participants[1].keen_evasion_available);
        encounter.participants[1].keen_evasion_available = true;
        let resting_area_skill = CharacterSkill {
            index: 2,
            name: "休整范围测试".to_owned(),
            note: "主动使用对范围内目标造成1点物理伤害".to_owned(),
            ..area_skill.clone()
        };
        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &resting_area_skill,
            &manager,
            None
        ));
        let target = &store.encounters["battle"].participants[1];
        assert!((target.hp - 5.0).abs() < 0.0001);
        assert!(target.keen_evasion_available);

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, true
        ));
        assert!(encounter.participants[1].keen_evasion_available);
        assert!(store.record_skill_use(
            "battle",
            "a",
            "b",
            &area_skill,
            &manager,
            None
        ));
        let target = &store.encounters["battle"].participants[1];
        assert!((target.hp - 5.0).abs() < 0.0001);
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
        assert!((restored.arcane_shield_rate - 0.10).abs() < 0.0001);

        let mut encounter = BattleEncounter {
            active: true,
            participants: vec![restored],
            ..Default::default()
        };
        encounter.participants[0].arcane_shield = 2.0;
        assert!(set_encounter_active_state(
            &mut encounter,
            false
        ));
        assert!((encounter.participants[0].arcane_shield - 0.0).abs() < 0.0001);
        assert!(!set_encounter_active_state(
            &mut encounter,
            false
        ));
        let participant = &mut encounter.participants[0];
        participant.hp = 20.0;
        participant.alive = true;
        participant.arcane_shield = 5.0;
        participant.damage_taken_this_turn = 0.0;
        participant.damage_contributors.clear();
        let resolution = apply_participant_damage_for_battle(participant, 3.0, "enemy", false);
        assert!((resolution.damage_applied - 3.0).abs() < 0.0001);
        assert!((resolution.damage_absorbed - 0.0).abs() < 0.0001);
        assert!((participant.hp - 17.0).abs() < 0.0001);
        assert!((participant.arcane_shield - 0.0).abs() < 0.0001);

        encounter.participants[0].max_mp = 80.0;
        assert!(set_encounter_active_state(
            &mut encounter,
            true
        ));
        assert!((encounter.participants[0].arcane_shield - 8.0).abs() < 0.0001);
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

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, false
        ));
        let actor = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        actor.hp = 20.0;
        actor.alive = true;
        actor.undying_rage_used = false;
        actor.undying_rage_active = true;
        let resolution = apply_participant_damage_for_battle(actor, 20.0, "enemy", false);
        assert!(resolution.defeat_outcome.is_some());
        assert!(!resolution.undying_rage_triggered);
        assert!(!actor.alive);
        assert!(!actor.undying_rage_used);

        actor.hp = 20.0;
        actor.alive = true;
        assert!(set_encounter_active_state(
            encounter, true
        ));
        let actor = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!(!actor.undying_rage_used);
        assert!(!actor.undying_rage_active);
        let resolution = apply_participant_damage_for_battle(actor, 20.0, "enemy", true);
        assert!(resolution.defeat_outcome.is_none());
        assert!(resolution.undying_rage_triggered);
        assert!(actor.alive);
        assert!(actor.undying_rage_used);
        assert!(actor.undying_rage_active);

        let mut stale_actor = participant_from_character("a", &actor_character, &manager);
        stale_actor.undying_rage_active = true;
        let mut resting_target = participant("rest-target", 0);
        resting_target.hp = 100.0;
        resting_target.max_hp = 100.0;
        let mut resting_store = BattleRoundStore::default();
        resting_store
            .encounters
            .insert("rest".to_owned(), BattleEncounter {
                name: "rest".to_owned(),
                active: false,
                participants: vec![stale_actor, resting_target],
                ..Default::default()
            });
        assert!(resting_store.record_skill_use(
            "rest",
            "a",
            "rest-target",
            &skill,
            &manager,
            None,
        ));
        let resting_target = resting_store.encounters["rest"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "rest-target")
            .unwrap();
        assert!((resting_target.hp - 90.0).abs() < 0.0001);

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
    fn rest_then_fight_heals_capped_resting_turns_once_on_combat_entry() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 100.0,
            skill_names: vec!["以逸待劳".to_owned()],
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
        assert!((actor.rest_then_fight_healing_rate - 0.05).abs() < 0.0001);

        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                active: false,
                participants: vec![actor],
                ..Default::default()
            });
        for _ in 0..12 {
            assert!(store.next_round("battle"));
        }
        let actor = &store.encounters["battle"].participants[0];
        assert_eq!(actor.rest_then_fight_turns, 10);
        assert!((actor.hp - 20.0).abs() < 0.0001);

        let serialized = serde_json::to_string(&store).unwrap();
        let mut restored = serde_json::from_str::<BattleRoundStore>(&serialized).unwrap();
        let encounter = restored.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, true
        ));
        assert_eq!(
            encounter.participants[0].rest_then_fight_turns,
            0
        );
        assert!((encounter.participants[0].hp - 70.0).abs() < 0.0001);
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("触发以逸待劳，回复50点生命值")));
        assert!(!set_encounter_active_state(
            encounter, true
        ));
        assert!((encounter.participants[0].hp - 70.0).abs() < 0.0001);

        assert!(restored.next_round("battle"));
        assert_eq!(
            restored.encounters["battle"].participants[0].rest_then_fight_turns,
            0
        );
        let encounter = restored.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, false
        ));
        assert!(restored.advance_participant("battle", "a", false));
        assert_eq!(
            restored.encounters["battle"].participants[0].rest_then_fight_turns,
            1
        );
        let encounter = restored.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, true
        ));
        assert!((encounter.participants[0].hp - 75.0).abs() < 0.0001);

        assert!(set_encounter_active_state(
            encounter, false
        ));
        encounter.participants[0].hp = 0.0;
        encounter.participants[0].alive = false;
        encounter.participants[0].rest_then_fight_turns = 10;
        assert!(set_encounter_active_state(
            encounter, true
        ));
        assert_eq!(
            encounter.participants[0].rest_then_fight_turns,
            0
        );
        assert!((encounter.participants[0].hp - 0.0).abs() < 0.0001);
        assert!(!encounter.participants[0].alive);
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

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, false
        ));
        let actor = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        actor.hp = 10.0;
        actor.alive = true;
        actor.hope_avatar_used = false;
        assert!(store.apply_action(
            "battle",
            "enemy",
            "a",
            "休整致命攻击",
            10.0
        ));
        let actor = &store.encounters["battle"].participants[0];
        assert!(!actor.alive);
        assert!(!actor.hope_avatar_used);
        assert_eq!(actor.hope_avatar_rounds_remaining, 0);

        let encounter = store.encounters.get_mut("battle").unwrap();
        let actor = &mut encounter.participants[0];
        actor.hp = 10.0;
        actor.alive = true;
        actor.hope_avatar_used = true;
        assert!(set_encounter_active_state(
            encounter, true
        ));
        assert!(!encounter.participants[0].hope_avatar_used);
        assert!(store.apply_action(
            "battle",
            "enemy",
            "a",
            "新战斗致命攻击",
            10.0
        ));
        let actor = &store.encounters["battle"].participants[0];
        assert!(actor.alive);
        assert!(actor.hope_avatar_used);
        assert_eq!(actor.hope_avatar_rounds_remaining, 2);

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, false
        ));
        let actor = &encounter.participants[0];
        assert!(!actor.alive);
        assert!((actor.hp - 0.0).abs() < 0.0001);
        assert_eq!(actor.hope_avatar_rounds_remaining, 0);
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("希望化身随战斗结束，角色死亡")));
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
        assert!((target.healing_taken_this_turn - 35.0).abs() < 0.0001);
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
    fn redeemed_revenge_soul_grants_healer_a_capped_battle_shield() {
        let mut manager = empty_manager();
        let healer_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            skill_names: vec!["复仇之魂".to_owned()],
            skill_notes: vec!["【被动】自身会获得治疗数值一半的护盾，最多5点。".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::default()],
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 0.0,
            max_hp: 20.0,
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), healer_character.clone());
        manager
            .player_characters
            .insert("b".to_owned(), target_character.clone());
        let healer = participant_from_character("a", &healer_character, &manager);
        assert!((healer.revenge_soul_shield_rate - 0.5).abs() < 0.0001);
        let target = participant_from_character("b", &target_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                active: true,
                participants: vec![healer, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "灵魂脉冲（治疗）".to_owned(),
            note: "主动使用对目标回复6点生命值".to_owned(),
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

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));
        let healer = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!(
            (healer.revenge_soul_shield - 5.0).abs() < 0.0001,
            "shield={}, log={:?}",
            healer.revenge_soul_shield,
            store.encounters["battle"].action_log,
        );

        let healer = store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        let resolution = apply_participant_damage_for_battle(healer, 7.0, "enemy", true);
        assert!((resolution.damage_applied - 2.0).abs() < 0.0001);
        assert!((healer.revenge_soul_shield - 0.0).abs() < 0.0001);
        assert!((healer.hp - 18.0).abs() < 0.0001);

        let encounter = store.encounters.get_mut("battle").unwrap();
        encounter.participants[0].revenge_soul_shield = 4.0;
        assert!(set_encounter_active_state(
            encounter, false
        ));
        assert!((encounter.participants[0].revenge_soul_shield - 0.0).abs() < 0.0001);
    }

    #[test]
    fn redeemed_void_break_damages_hp_to_one_before_consuming_shields() {
        let manager = empty_manager();
        let actor = participant("a", 0);
        let mut target = participant("b", 0);
        target.hp = 4.0;
        target.max_hp = 20.0;
        target.overhealing_shield = 1.0;
        target.overhealing_shield_turns_remaining = 2;
        target.revenge_soul_shield = 1.0;
        target.arcane_shield = 2.0;
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
            name: "玄空破".to_owned(),
            note: "从一个4米内的可视目标内部造成恐怖又玄妙的爆炸，会优先伤害生命值，在有护盾的情况下，最多优先将生命值降低为1，之后会优先对护盾造成伤害。爆炸造成7点物理伤害。".to_owned(),
            skill_type: Some("动作".to_owned()),
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 1,
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
        assert!((target.hp - 1.0).abs() < 0.0001);
        assert!((target.overhealing_shield - 0.0).abs() < 0.0001);
        assert!((target.revenge_soul_shield - 0.0).abs() < 0.0001);
        assert!((target.arcane_shield - 0.0).abs() < 0.0001);
        assert!(target.alive);
        assert!((target.damage_taken_this_turn - 3.0).abs() < 0.0001);
    }

    #[test]
    fn redeemed_void_break_leaves_shields_when_hp_can_take_all_damage() {
        let mut target = participant("b", 0);
        target.hp = 10.0;
        target.max_hp = 20.0;
        target.overhealing_shield = 3.0;
        target.arcane_shield = 2.0;

        let resolution = apply_void_break_damage_for_battle(&mut target, 7.0, "a", true);

        assert!((resolution.damage_applied - 7.0).abs() < 0.0001);
        assert!((target.hp - 3.0).abs() < 0.0001);
        assert!((target.overhealing_shield - 3.0).abs() < 0.0001);
        assert!((target.arcane_shield - 2.0).abs() < 0.0001);
    }

    #[test]
    fn battle_buff_healing_uses_source_talent_and_encounter_target_vitals() {
        let mut manager = empty_manager();
        let source_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            skill_names: vec!["生死时速".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            ..Default::default()
        };
        manager.player_characters.insert(
            "source".to_owned(),
            source_character.clone(),
        );
        manager.player_characters.insert(
            "target".to_owned(),
            target_character.clone(),
        );
        let source = participant_from_character("source", &source_character, &manager);
        let mut target = participant_from_character("target", &target_character, &manager);
        target.hp = 4.0;
        let mut encounter = BattleEncounter {
            name: "battle".to_owned(),
            active: true,
            participants: vec![source, target],
            ..Default::default()
        };

        apply_battle_buff_ticks(&mut encounter, &manager, &[
            BattleBuffTick {
                source_id: "source".to_owned(),
                target_id: "target".to_owned(),
                action_name: "持续治疗".to_owned(),
                action: BuffTickAction::Heal { amount: 4.0 },
            },
        ]);

        let target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        assert!((target.hp - 10.0).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 6.0).abs() < 0.0001);
    }

    #[test]
    fn battle_buff_ticks_use_shared_encounter_source_modifiers() {
        let mut manager = empty_manager();
        let source_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            healing_dealt_modifier: 1.25,
            ..Default::default()
        };
        manager.player_characters.insert(
            "source".to_owned(),
            source_character.clone(),
        );
        let mut source = participant_from_character("source", &source_character, &manager);
        source.inspiration_sources.insert("healer".to_owned(), 1);
        source.penance_healing_bonus_percent = 25.0;
        source.penance_kill_assist_count = 1;
        let mut damage_target = participant("damage-target", 0);
        damage_target.hp = 100.0;
        damage_target.max_hp = 100.0;
        let mut healing_target = participant("healing-target", 0);
        healing_target.hp = 0.0;
        healing_target.max_hp = 100.0;
        healing_target.alive = true;
        let mut encounter = BattleEncounter {
            name: "battle".to_owned(),
            active: true,
            participants: vec![source, damage_target, healing_target],
            ..Default::default()
        };

        apply_battle_buff_ticks(&mut encounter, &manager, &[
            BattleBuffTick {
                source_id: "source".to_owned(),
                target_id: "damage-target".to_owned(),
                action_name: "持续伤害".to_owned(),
                action: BuffTickAction::Damage {
                    amount: 10.0,
                    damage_type: DamageType::Physical,
                },
            },
            BattleBuffTick {
                source_id: "source".to_owned(),
                target_id: "healing-target".to_owned(),
                action_name: "持续治疗".to_owned(),
                action: BuffTickAction::Heal { amount: 10.0 },
            },
        ]);

        let damage_target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "damage-target")
            .unwrap();
        assert!((damage_target.hp - 89.0).abs() < 0.0001);
        assert!((damage_target.damage_taken_this_turn - 11.0).abs() < 0.0001);
        let healing_target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "healing-target")
            .unwrap();
        assert!((healing_target.hp - 11.5).abs() < 0.0001);
        assert!((healing_target.healing_taken_this_turn - 11.5).abs() < 0.0001);
    }

    #[test]
    fn battle_buff_damage_uses_shared_encounter_target_mitigation() {
        let mut manager = empty_manager();
        let source_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["斗志昂扬".to_owned(), "过度免疫".to_owned()],
            skill_metadata: vec![
                crate::napcat::CharacterSkillMetadata::talent("normal_talent", "天赋"),
                crate::napcat::CharacterSkillMetadata::talent("normal_talent", "天赋"),
            ],
            ..Default::default()
        };
        manager.player_characters.insert(
            "source".to_owned(),
            source_character.clone(),
        );
        manager.player_characters.insert(
            "target".to_owned(),
            target_character.clone(),
        );
        let source = participant_from_character("source", &source_character, &manager);
        let mut target = participant_from_character("target", &target_character, &manager);
        target.champion_damage_reduction_per_stack = 0.10;
        target.champion_stacks = 1;
        let mut encounter = BattleEncounter {
            name: "battle".to_owned(),
            active: true,
            participants: vec![source, target],
            ..Default::default()
        };

        apply_battle_buff_ticks(&mut encounter, &manager, &[
            BattleBuffTick {
                source_id: "source".to_owned(),
                target_id: "target".to_owned(),
                action_name: "持续伤害".to_owned(),
                action: BuffTickAction::Damage {
                    amount: 50.0,
                    damage_type: DamageType::Physical,
                },
            },
        ]);

        let target = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        assert!((target.hp - 82.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 18.0).abs() < 0.0001);
    }

    #[test]
    fn background_effects_do_not_modify_defeated_participants() {
        let manager = empty_manager();
        let source = participant("source", 0);
        let mut target = participant("target", 0);
        target.hp = 0.0;
        target.alive = false;
        schedule_participant_delayed_healing(
            &mut target,
            "source",
            "healer",
            "延迟治疗",
            10.0,
            0.0,
            1,
        );
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![source, target],
                ..Default::default()
            });

        assert!(store.next_round("battle"));
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        assert_eq!(target.hp, 0.0);
        assert!(!target.alive);
        assert!(target.delayed_healing_ticks.is_empty());
        assert!(!store.encounters["battle"]
            .action_log
            .iter()
            .any(|entry| entry.contains("延迟治疗")));

        store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "target")
            .unwrap()
            .overhealing_shield = 3.0;
        let log_count = store.encounters["battle"].action_log.len();
        apply_battle_buff_ticks(
            store.encounters.get_mut("battle").unwrap(),
            &manager,
            &[
                BattleBuffTick {
                    source_id: "source".to_owned(),
                    target_id: "target".to_owned(),
                    action_name: "持续治疗".to_owned(),
                    action: BuffTickAction::Heal { amount: 5.0 },
                },
                BattleBuffTick {
                    source_id: "source".to_owned(),
                    target_id: "target".to_owned(),
                    action_name: "持续伤害".to_owned(),
                    action: BuffTickAction::Damage {
                        amount: 2.0,
                        damage_type: DamageType::Physical,
                    },
                },
                BattleBuffTick {
                    source_id: "source".to_owned(),
                    target_id: "target".to_owned(),
                    action_name: "持续固定伤害".to_owned(),
                    action: BuffTickAction::FixedDamage {
                        amount: 2.0,
                        damage_type: DamageType::None,
                    },
                },
            ],
        );
        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "target")
            .unwrap();
        assert_eq!(target.hp, 0.0);
        assert!(!target.alive);
        assert_eq!(target.overhealing_shield, 3.0);
        assert_eq!(target.damage_taken_this_turn, 0.0);
        assert_eq!(
            store.encounters["battle"].action_log.len(),
            log_count
        );
    }

    #[test]
    fn parsed_battle_overkill_uses_actual_hp_loss_for_damage_rewards() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 90.0,
            max_hp: 100.0,
            damage_dealt_modifier: 1.0,
            damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            healing_taken_modifier: 1.0,
            skill_names: vec!["禅宗古训".to_owned(), "苏萨斯之爪".to_owned()],
            skill_metadata: (0..2)
                .map(|_| crate::napcat::CharacterSkillMetadata::talent("normal_talent", "天赋"))
                .collect(),
            ..Default::default()
        };
        manager
            .player_characters
            .insert("a".to_owned(), actor_character.clone());
        let actor = participant_from_character("a", &actor_character, &manager);
        let mut target = participant("b", 0);
        target.hp = 3.0;
        target.max_hp = 20.0;
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
            name: "过量斩击".to_owned(),
            note: "主动使用对目标造成10点物理伤害".to_owned(),
            skill_type: Some("物理".to_owned()),
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
        assert!((actor.hp - 90.45).abs() < 0.0001);
        assert!((actor.healing_taken_this_turn - 0.45).abs() < 0.0001);
        assert_eq!(target.hp, 0.0);
        assert!(!target.alive);
        assert_eq!(target.damage_taken_this_turn, 3.0);
        assert_eq!(target.combat_damage_taken_total, 3.0);
        assert_eq!(target.delayed_damage_ticks.len(), 1);
        assert!((target.delayed_damage_ticks[0].amount - 1.05).abs() < 0.0001);
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("过量斩击") && entry.contains("造成3点伤害")));
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

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            false
        ));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert_eq!(actor.infinite_focus_target_id, None);
        assert_eq!(actor.infinite_focus_stacks, 0);

        let actor = store
            .encounters
            .get_mut("battle")
            .unwrap()
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        actor.infinite_focus_target_id = Some("c".to_owned());
        actor.infinite_focus_stacks = 2;
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
        assert_eq!(actor.infinite_focus_stacks, 2);
        assert!((target_c.hp - 80.0).abs() < 0.0001);

        assert!(set_encounter_active_state(
            store.encounters.get_mut("battle").unwrap(),
            true
        ));
        let actor = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert_eq!(actor.infinite_focus_target_id, None);
        assert_eq!(actor.infinite_focus_stacks, 0);
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

        let resting_holder = participant_from_character("a", &dominion_character, &manager);
        let resting_attacker = participant_from_character("killer", &attacker_character, &manager);
        let resting_victim = participant_from_character("victim", &victim_character, &manager);
        store.encounters.insert("rest".to_owned(), BattleEncounter {
            name: "rest".to_owned(),
            active: false,
            participants: vec![resting_holder, resting_attacker, resting_victim],
            ..Default::default()
        });

        assert!(store.record_skill_use("rest", "killer", "victim", &skill, &manager, None,));

        let resting_encounter = &store.encounters["rest"];
        let resting_holder = resting_encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "a")
            .unwrap();
        assert!((resting_holder.dominion_max_hp_bonus - 0.0).abs() < 0.0001);
        assert!((resting_holder.max_hp - 100.0).abs() < 0.0001);
        assert!(!resting_encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("触发役于我手")));
    }

    #[test]
    fn dominion_bonus_persists_across_combat_boundaries() {
        let mut holder = participant("holder", 0);
        holder.hp = 115.0;
        holder.max_hp = 120.0;
        holder.dominion_max_hp_gain_rate = 0.05;
        holder.dominion_max_hp_bonus_cap = 20.0;
        holder.dominion_max_hp_bonus = 20.0;
        let mut avatar = participant("avatar", 0);
        avatar.hp = 0.0;
        avatar.max_hp = 50.0;
        avatar.hope_avatar_enabled = true;
        avatar.hope_avatar_used = true;
        avatar.hope_avatar_rounds_remaining = 1;
        let mut encounter = BattleEncounter {
            active: true,
            participants: vec![holder, avatar],
            ..Default::default()
        };

        assert!(set_encounter_active_state(
            &mut encounter,
            false
        ));
        let holder = &encounter.participants[0];
        assert_eq!(holder.hp, 115.0);
        assert_eq!(holder.max_hp, 120.0);
        assert_eq!(holder.dominion_max_hp_bonus, 20.0);
        assert_eq!(encounter.participants[1].hp, 0.0);
        assert!(!encounter.participants[1].alive);
        assert!(!encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("触发役于我手")));

        assert!(set_encounter_active_state(
            &mut encounter,
            true
        ));
        let holder = &encounter.participants[0];
        assert_eq!(holder.hp, 115.0);
        assert_eq!(holder.max_hp, 120.0);
        assert_eq!(holder.dominion_max_hp_bonus, 20.0);
    }

    #[test]
    fn dominion_bonus_persists_on_durable_character_until_world_close() {
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
        manager.player_characters.insert(
            "holder".to_owned(),
            dominion_character.clone(),
        );
        manager.trpg_groups.insert(
            "g".to_owned(),
            crate::napcat::TrpgGroup {
                players: vec!["holder".to_owned()],
                ..Default::default()
            },
        );

        let mut holder = participant_from_character("holder", &dominion_character, &manager);
        assert_eq!(holder.dominion_max_hp_bonus, 0.0);
        holder.hp = 120.0;
        holder.max_hp = 120.0;
        holder.dominion_max_hp_bonus = 20.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                trpg_group: Some("g".to_owned()),
                participants: vec![holder],
                ..Default::default()
            });

        assert!(sync_encounter_to_manager(
            store.encounters.get("battle"),
            &mut manager,
        ));
        assert!((manager.player_characters["holder"].dominion_max_hp_bonus - 20.0).abs() < 0.0001);
        assert!((manager.player_characters["holder"].hp - 120.0).abs() < 0.0001);

        // 退出战斗后重新入场，加成仍保留到本世界结束。
        let reentered = participant_from_character(
            "holder",
            &manager.player_characters["holder"],
            &manager,
        );
        assert!((reentered.dominion_max_hp_bonus - 20.0).abs() < 0.0001);
        assert!((reentered.max_hp - 120.0).abs() < 0.0001);

        // 结团清空持久加成并结束世界；开团开始新世界。
        assert_eq!(
            store.encounters["battle"].participants[0].dominion_max_hp_bonus,
            20.0
        );
        assert_eq!(
            clear_trpg_group_dominion_bonuses(&mut store, "g"),
            1
        );
        assert_eq!(
            store.encounters["battle"].participants[0].dominion_max_hp_bonus,
            0.0
        );
        assert!(crate::napcat::close_trpg_group_world(&mut manager, "g").is_some());
        assert!(!manager.trpg_groups["g"].campaign_active);
        assert_eq!(
            manager.player_characters["holder"].dominion_max_hp_bonus,
            0.0
        );
        assert!(crate::napcat::open_trpg_group_world(&mut manager, "g").is_some());
        assert!(manager.trpg_groups["g"].campaign_active);
    }

    #[test]
    fn character_in_active_encounter_matches_group_and_participant() {
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                trpg_group: Some("g".to_owned()),
                trpg_campaign_id: Some("campaign-a".to_owned()),
                active: true,
                participants: vec![participant("hero", 0)],
                ..Default::default()
            });
        assert!(character_in_active_encounter(
            &store,
            "g",
            "campaign-a",
            "hero"
        ));
        assert!(!character_in_active_encounter(
            &store,
            "g",
            "campaign-a",
            "other"
        ));
        assert!(!character_in_active_encounter(
            &store,
            "other",
            "campaign-b",
            "hero"
        ));

        // 非激活战斗轮不算进入战斗。
        store.encounters.get_mut("battle").unwrap().active = false;
        assert!(!character_in_active_encounter(
            &store,
            "g",
            "campaign-a",
            "hero"
        ));
    }

    #[test]
    fn mirror_coat_lethal_hit_sets_hp_one_consumes_mp_and_grants_layers() {
        let mut holder = participant("holder", 0);
        holder.hp = 10.0;
        holder.max_hp = 50.0;
        holder.mp = 12.0;
        holder.max_mp = 50.0;
        holder.mirror_coat_enabled = true;

        let resolution = apply_participant_damage_for_battle(&mut holder, 20.0, "enemy", true);

        assert!(resolution.mirror_coat_triggered);
        assert_eq!(holder.hp, 1.0);
        assert!(holder.alive);
        // 基础 1 层 + 12 MP 每 5 点叠 1 层（最多 3 层额外）＝ 3 层，消耗 10 MP。
        assert_eq!(holder.mirror_coat_layers, 3);
        assert!((holder.mp - 2.0).abs() < 0.0001);
        assert_eq!(holder.mirror_coat_cooldown_remaining, 6);
        assert!(holder.mirror_coat_cleanup_pending);
        assert!(resolution.defeat_outcome.is_none());
    }

    #[test]
    fn mirror_coat_does_not_trigger_while_on_cooldown() {
        let mut holder = participant("holder", 0);
        holder.hp = 10.0;
        holder.max_hp = 50.0;
        holder.mirror_coat_enabled = true;
        holder.mirror_coat_cooldown_remaining = 3;

        let resolution = apply_participant_damage_for_battle(&mut holder, 20.0, "enemy", true);

        assert!(!resolution.mirror_coat_triggered);
        assert!(!holder.alive);
        assert_eq!(holder.hp, 0.0);
        assert!(resolution.defeat_outcome.is_some());
    }

    #[test]
    fn mirror_coat_stealth_breaks_when_taking_damage() {
        let mut holder = participant("holder", 0);
        holder.hp = 20.0;
        holder.max_hp = 50.0;
        holder.mirror_coat_layers = 3;

        let resolution = apply_participant_damage_for_battle(&mut holder, 5.0, "enemy", true);

        assert_eq!(holder.mirror_coat_layers, 0);
        assert!((holder.hp - 15.0).abs() < 0.0001);
        assert!((resolution.damage_applied - 5.0).abs() < 0.0001);
    }

    #[test]
    fn mirror_coat_stealth_breaks_when_holder_deals_damage() {
        let mut manager = empty_manager();
        let mut attacker = participant("attacker", 0);
        attacker.hp = 50.0;
        attacker.max_hp = 50.0;
        attacker.mirror_coat_enabled = true;
        attacker.mirror_coat_layers = 2;
        let mut defender = participant("defender", 0);
        defender.hp = 50.0;
        defender.max_hp = 50.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![attacker, defender],
                ..Default::default()
            });

        assert!(store.apply_action(
            "battle",
            "attacker",
            "defender",
            "测试攻击",
            5.0
        ));
        let attacker = &store.encounters["battle"].participants[0];
        assert_eq!(attacker.mirror_coat_layers, 0);
    }

    #[test]
    fn mirror_coat_layers_and_cooldown_advance_per_round() {
        let mut holder = participant("holder", 0);
        holder.mirror_coat_layers = 2;
        holder.mirror_coat_cooldown_remaining = 6;

        advance_participant_mirror_coat(&mut holder);
        assert_eq!(holder.mirror_coat_layers, 1);
        assert_eq!(holder.mirror_coat_cooldown_remaining, 5);
        advance_participant_mirror_coat(&mut holder);
        assert_eq!(holder.mirror_coat_layers, 0);
        assert_eq!(holder.mirror_coat_cooldown_remaining, 4);
    }

    #[test]
    fn mirror_coat_cleanup_removes_damage_dealing_harmful_buffs() {
        let mut manager = empty_manager();
        let character = PlayerCharacter {
            hp: 10.0,
            max_hp: 50.0,
            mp: 12.0,
            max_mp: 50.0,
            skill_names: vec!["镜像外衣".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            active_buffs: vec![
                BuffSpec {
                    name: "流血".to_owned(),
                    kind: BuffKind::Magic,
                    priority: 0,
                    turns_remaining: 3,
                    source_id: "enemy".to_owned(),
                    beneficial: false,
                    effects: Vec::new(),
                    tick_actions: vec![BuffTickAction::Damage {
                        amount: 3.0,
                        damage_type: DamageType::Physical,
                    }],
                },
                BuffSpec {
                    name: "护佑".to_owned(),
                    kind: BuffKind::Magic,
                    priority: 0,
                    turns_remaining: 3,
                    source_id: "ally".to_owned(),
                    beneficial: true,
                    effects: Vec::new(),
                    tick_actions: vec![BuffTickAction::Heal { amount: 2.0 }],
                },
            ],
            ..Default::default()
        };
        manager
            .player_characters
            .insert("holder".to_owned(), character.clone());
        let mut holder = participant_from_character("holder", &character, &manager);
        let resolution = apply_participant_damage_for_battle(&mut holder, 20.0, "enemy", true);
        assert!(resolution.mirror_coat_triggered);

        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![holder],
                ..Default::default()
            });
        assert!(sync_encounter_to_manager(
            store.encounters.get("battle"),
            &mut manager,
        ));
        let buffs = &manager.player_characters["holder"].active_buffs;
        assert_eq!(buffs.len(), 1);
        assert_eq!(buffs[0].name, "护佑");
    }

    #[test]
    fn sunset_distance_reduction_scales_with_distance_beyond_ten_yards() {
        let mut manager = empty_manager();
        let attacker_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            ..Default::default()
        };
        let victim_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["日薄崦嵫".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager.player_characters.insert(
            "killer".to_owned(),
            attacker_character.clone(),
        );
        manager.player_characters.insert(
            "victim".to_owned(),
            victim_character.clone(),
        );
        let attacker = participant_from_character("killer", &attacker_character, &manager);
        let victim = participant_from_character("victim", &victim_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![attacker, victim],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "远程射击".to_owned(),
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

        // 11 码：超出 10 码 1 码，减免 1 点（100 - 9 = 91）。
        let positions = crate::scene::SceneCharacterPositions {
            positions: HashMap::from([
                ("killer".to_owned(), Vec3::ZERO),
                (
                    "victim".to_owned(),
                    Vec3::new(11.0, 0.0, 0.0),
                ),
            ]),
        };
        assert!(store.record_skill_use(
            "battle",
            "killer",
            "victim",
            &skill,
            &manager,
            Some(&positions),
        ));
        assert!((store.encounters["battle"].participants[1].hp - 91.0).abs() < 0.0001);
    }

    #[test]
    fn sunset_distance_reduction_caps_at_twenty_percent() {
        let mut manager = empty_manager();
        let attacker_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            ..Default::default()
        };
        let victim_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["日薄崦嵫".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager.player_characters.insert(
            "killer".to_owned(),
            attacker_character.clone(),
        );
        manager.player_characters.insert(
            "victim".to_owned(),
            victim_character.clone(),
        );
        let attacker = participant_from_character("killer", &attacker_character, &manager);
        let victim = participant_from_character("victim", &victim_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![attacker, victim],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "远程射击".to_owned(),
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

        // 100 码：理论减免 90 点，上限为伤害的 20%（2 点）→ 实际造成 8 点。
        let positions = crate::scene::SceneCharacterPositions {
            positions: HashMap::from([
                ("killer".to_owned(), Vec3::ZERO),
                (
                    "victim".to_owned(),
                    Vec3::new(100.0, 0.0, 0.0),
                ),
            ]),
        };
        assert!(store.record_skill_use(
            "battle",
            "killer",
            "victim",
            &skill,
            &manager,
            Some(&positions),
        ));
        assert!((store.encounters["battle"].participants[1].hp - 92.0).abs() < 0.0001);
    }

    #[test]
    fn sunset_does_not_reduce_within_ten_yards() {
        let mut manager = empty_manager();
        let attacker_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            ..Default::default()
        };
        let victim_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["日薄崦嵫".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            ..Default::default()
        };
        manager.player_characters.insert(
            "killer".to_owned(),
            attacker_character.clone(),
        );
        manager.player_characters.insert(
            "victim".to_owned(),
            victim_character.clone(),
        );
        let attacker = participant_from_character("killer", &attacker_character, &manager);
        let victim = participant_from_character("victim", &victim_character, &manager);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![attacker, victim],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "近战攻击".to_owned(),
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

        // 10 码内不减免，正常造成 10 点。
        let positions = crate::scene::SceneCharacterPositions {
            positions: HashMap::from([
                ("killer".to_owned(), Vec3::ZERO),
                (
                    "victim".to_owned(),
                    Vec3::new(10.0, 0.0, 0.0),
                ),
            ]),
        };
        assert!(store.record_skill_use(
            "battle",
            "killer",
            "victim",
            &skill,
            &manager,
            Some(&positions),
        ));
        assert!((store.encounters["battle"].participants[1].hp - 90.0).abs() < 0.0001);
    }

    #[test]
    fn sunset_death_resets_group_world_time_to_six_pm() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert(
            "g".to_owned(),
            crate::napcat::TrpgGroup {
                world_time_minutes: 3 * crate::napcat::WORLD_DAY_MINUTES + 5 * 60,
                ..Default::default()
            },
        );
        let mut holder = participant("holder", 0);
        holder.hp = 0.0;
        holder.alive = false;
        holder.sunset_enabled = true;
        let mut encounter = BattleEncounter {
            trpg_group: Some("g".to_owned()),
            participants: vec![holder],
            ..Default::default()
        };
        let outcome = participant_defeat_outcome(
            &mut encounter.participants[0],
            true,
            None,
        )
        .unwrap();
        apply_battle_defeat_outcome(&mut encounter, outcome);
        assert!(encounter.participants[0].sunset_death_time_reset_pending);

        apply_sunset_world_time_reset(&mut encounter, &mut manager);
        assert!(!encounter.participants[0].sunset_death_time_reset_pending);
        assert_eq!(
            manager.trpg_groups["g"].world_time_minutes,
            3 * crate::napcat::WORLD_DAY_MINUTES + 18 * 60
        );
    }

    #[test]
    fn sunset_death_without_talent_does_not_reset_world_time() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert(
            "g".to_owned(),
            crate::napcat::TrpgGroup {
                world_time_minutes: 5 * 60,
                ..Default::default()
            },
        );
        let mut holder = participant("holder", 0);
        holder.hp = 0.0;
        holder.alive = false;
        let mut encounter = BattleEncounter {
            trpg_group: Some("g".to_owned()),
            participants: vec![holder],
            ..Default::default()
        };
        let outcome = participant_defeat_outcome(
            &mut encounter.participants[0],
            true,
            None,
        )
        .unwrap();
        apply_battle_defeat_outcome(&mut encounter, outcome);
        assert!(!encounter.participants[0].sunset_death_time_reset_pending);
        apply_sunset_world_time_reset(&mut encounter, &mut manager);
        assert_eq!(
            manager.trpg_groups["g"].world_time_minutes,
            5 * 60
        );
    }

    #[test]
    fn goose_skill_starts_channel_and_blocks_other_actions() {
        let mut manager = empty_manager();
        let mut actor = participant("actor", 0);
        actor.hp = 100.0;
        actor.max_hp = 100.0;
        let mut defender = participant("defender", 0);
        defender.hp = 100.0;
        defender.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![actor, defender],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "食用精美烧鹅".to_owned(),
            note: "开始食用烧鹅，引导5回合，每回合回复20%最大生命和魔法值".to_owned(),
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

        assert!(store.record_skill_use("battle", "actor", "actor", &skill, &manager, None,));
        let actor = &store.encounters["battle"].participants[0];
        assert_eq!(actor.goose_channeling_turns, 5);
        assert!(!participant_can_act(actor));
    }

    #[test]
    fn goose_channel_heals_per_round_for_five_turns() {
        let mut manager = empty_manager();
        let mut holder = participant("holder", 0);
        holder.hp = 50.0;
        holder.max_hp = 100.0;
        holder.mp = 20.0;
        holder.max_mp = 100.0;
        holder.hp_regen = 0.0;
        holder.mp_regen = 0.0;
        holder.goose_channeling_turns = 5;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![holder],
                ..Default::default()
            });

        for _ in 0..5 {
            assert!(store.next_round("battle"));
        }
        let holder = &store.encounters["battle"].participants[0];
        assert!((holder.hp - 100.0).abs() < 0.0001);
        assert!((holder.mp - 100.0).abs() < 0.0001);
        assert_eq!(holder.goose_channeling_turns, 0);
    }

    #[test]
    fn goose_channel_interrupted_by_damage() {
        let mut holder = participant("holder", 0);
        holder.hp = 100.0;
        holder.max_hp = 100.0;
        holder.goose_channeling_turns = 5;

        apply_participant_damage_for_battle(&mut holder, 10.0, "enemy", true);

        assert_eq!(holder.goose_channeling_turns, 0);
        assert!((holder.hp - 90.0).abs() < 0.0001);
    }

    #[test]
    fn butterfly_effect_alternates_between_holder_and_target_each_round() {
        let mut holder = participant("holder", 0);
        holder.butterfly_enabled = true;
        holder.butterfly_target_id = Some("target".to_owned());
        let target = participant("target", 0);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![holder, target],
                ..Default::default()
            });

        // 第1轮：指定目标先持有效果。
        assert!(store.next_round("battle"));
        assert!(!store.encounters["battle"].participants[0].butterfly_effect);
        assert!(store.encounters["battle"].participants[1].butterfly_effect);
        // 第2轮：转移给持有者。
        assert!(store.next_round("battle"));
        assert!(store.encounters["battle"].participants[0].butterfly_effect);
        assert!(!store.encounters["battle"].participants[1].butterfly_effect);
        // 第3轮：回到目标。
        assert!(store.next_round("battle"));
        assert!(!store.encounters["battle"].participants[0].butterfly_effect);
        assert!(store.encounters["battle"].participants[1].butterfly_effect);
    }

    #[test]
    fn butterfly_effect_stays_on_survivor_when_target_dies() {
        let mut holder = participant("holder", 0);
        holder.hp = 50.0;
        holder.max_hp = 50.0;
        holder.butterfly_enabled = true;
        holder.butterfly_target_id = Some("target".to_owned());
        let mut target = participant("target", 0);
        target.hp = 10.0;
        target.max_hp = 10.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![holder, target],
                ..Default::default()
            });
        assert!(store.next_round("battle"));
        let target = &mut store.encounters.get_mut("battle").unwrap().participants[1];
        target.hp = 0.0;
        target.alive = false;

        assert!(store.next_round("battle"));
        let holder = &store.encounters["battle"].participants[0];
        let target = &store.encounters["battle"].participants[1];
        assert!(holder.butterfly_locked);
        assert!(holder.butterfly_effect);
        assert!(!target.butterfly_effect);
        // 后续轮次不再互换。
        assert!(store.next_round("battle"));
        let holder = &store.encounters["battle"].participants[0];
        assert!(holder.butterfly_effect);
    }

    #[test]
    fn butterfly_modifiers_apply_damage_bonus_and_reduction() {
        let mut holder = participant("holder", 0);
        holder.butterfly_effect = true;
        let config = TrpgBasicConfig::default();
        let dealt = participant_damage_modifiers(
            &holder,
            None,
            &config,
            0,
            DamageType::Physical,
            true,
        );
        assert!(dealt.iter().any(|modifier| {
            modifier.source == "蝴蝶效应" && (modifier.multiplier - 1.05).abs() < 0.0001
        }));
        let taken = participant_damage_taken_modifiers(
            &holder,
            None,
            DamageType::Physical,
            true,
        );
        assert!(taken.iter().any(|modifier| {
            modifier.source == "蝴蝶效应" && (modifier.multiplier - 0.95).abs() < 0.0001
        }));

        holder.butterfly_effect = false;
        let dealt = participant_damage_modifiers(
            &holder,
            None,
            &config,
            0,
            DamageType::Physical,
            true,
        );
        assert!(!dealt.iter().any(|modifier| modifier.source == "蝴蝶效应"));
    }

    #[test]
    fn dominion_overflow_hp_uses_battle_cap_during_round_buff_healing() {
        let mut manager = empty_manager();
        let dominion_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            skill_names: vec!["役于我手".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            )],
            active_buffs: vec![BuffSpec {
                name: "再生".to_owned(),
                kind: BuffKind::Magic,
                priority: 0,
                turns_remaining: 2,
                source_id: "holder".to_owned(),
                beneficial: true,
                effects: Vec::new(),
                tick_actions: vec![BuffTickAction::Heal { amount: 20.0 }],
            }],
            ..Default::default()
        };
        manager.player_characters.insert(
            "holder".to_owned(),
            dominion_character.clone(),
        );
        let mut holder = participant_from_character("holder", &dominion_character, &manager);
        holder.hp = 105.0;
        holder.max_hp = 120.0;
        holder.dominion_max_hp_bonus = 20.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![holder],
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

        let holder = &store.encounters["battle"].participants[0];
        assert_eq!(holder.hp, 120.0);
        assert_eq!(holder.max_hp, 120.0);
        assert_eq!(holder.dominion_max_hp_bonus, 20.0);
        let character = &manager.player_characters["holder"];
        assert_eq!(character.hp, 120.0);
        assert_eq!(character.max_hp, 100.0);
        assert_eq!(character.dominion_max_hp_bonus, 20.0);
        assert_eq!(
            character.active_buffs[0].turns_remaining,
            1
        );
    }

    #[test]
    fn dominion_overflow_hp_survives_grant_buff_refresh() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            ..Default::default()
        };
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
        manager.player_characters.insert(
            "actor".to_owned(),
            actor_character.clone(),
        );
        manager.player_characters.insert(
            "holder".to_owned(),
            dominion_character.clone(),
        );
        let mut holder = participant_from_character("holder", &dominion_character, &manager);
        holder.hp = 115.0;
        holder.max_hp = 120.0;
        holder.dominion_max_hp_bonus = 20.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![
                    participant_from_character("actor", &actor_character, &manager),
                    holder,
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

        assert!(store.record_skill_use_with_buffs(
            "battle",
            "actor",
            "holder",
            &guard,
            &mut manager,
            None,
        ));

        let holder = &store.encounters["battle"].participants[1];
        assert_eq!(holder.hp, 115.0);
        assert_eq!(holder.max_hp, 120.0);
        assert_eq!(holder.dominion_max_hp_bonus, 20.0);
        assert!((holder.damage_taken_modifier - 0.5).abs() < 0.0001);
        let character = &manager.player_characters["holder"];
        assert_eq!(character.hp, 115.0);
        assert_eq!(character.max_hp, 100.0);
        assert_eq!(character.dominion_max_hp_bonus, 20.0);
        assert_eq!(character.active_buffs.len(), 1);
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
    fn battle_exit_prevents_cross_combat_kill_assist_credit() {
        let mut old_attacker = participant("old", 0);
        old_attacker.sin_on_sin_exp_bonus_per_stack = 0.025;
        old_attacker.sin_on_sin_recovery_rate = 0.10;
        let mut new_attacker = participant("new", 0);
        new_attacker.sin_on_sin_exp_bonus_per_stack = 0.025;
        new_attacker.sin_on_sin_recovery_rate = 0.10;
        let mut victim = participant("victim", 0);
        victim.hp = 10.0;
        victim.max_hp = 10.0;

        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![old_attacker, new_attacker, victim],
                ..Default::default()
            });

        assert!(store.apply_action(
            "battle",
            "old",
            "victim",
            "旧战斗攻击",
            4.0
        ));
        assert_eq!(
            store.encounters["battle"].participants[2].damage_contributors,
            vec!["old".to_owned()]
        );

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, false
        ));
        assert!(encounter.participants[2].damage_contributors.is_empty());
        encounter.participants[2]
            .damage_contributors
            .push("old".to_owned());
        assert!(set_encounter_active_state(
            encounter, true
        ));
        assert!(encounter.participants[2].damage_contributors.is_empty());

        assert!(store.apply_action(
            "battle",
            "new",
            "victim",
            "新战斗终击",
            6.0
        ));
        let encounter = &store.encounters["battle"];
        assert_eq!(
            encounter.participants[0].sin_on_sin_stacks,
            0
        );
        assert_eq!(
            encounter.participants[1].sin_on_sin_stacks,
            1
        );
        assert!(!encounter.participants[2].alive);
        assert!(encounter.participants[2].damage_contributors.is_empty());
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
            skill_names: vec!["生死时速".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 4.0,
            max_hp: 20.0,
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
    fn parsed_battle_wasted_healing_does_not_trigger_follow_up_talents() {
        let mut manager = empty_manager();
        let healer_character = PlayerCharacter {
            hp: 10.0,
            max_hp: 20.0,
            skill_names: vec![
                "互帮互助".to_owned(),
                "一心".to_owned(),
                "振奋".to_owned(),
                "千万回忆".to_owned(),
            ],
            skill_metadata: (0..4)
                .map(|_| {
                    crate::napcat::CharacterSkillMetadata::talent("support_talent", "辅助天赋")
                })
                .collect(),
            ..Default::default()
        };
        let target_character = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
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
                active: true,
                participants: vec![healer, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "无效治疗".to_owned(),
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
        let healer = &encounter.participants[0];
        let target = &encounter.participants[1];
        assert!((healer.hp - 10.0).abs() < 0.0001);
        assert!(healer.one_heart_target_id.is_none());
        assert!(healer.inspiration_target_id.is_none());
        assert!((target.healing_taken_this_turn - 0.0).abs() < 0.0001);
        assert!(target.inspiration_sources.is_empty());
        assert!(target.delayed_healing_ticks.is_empty());
        assert!(encounter
            .action_log
            .iter()
            .any(|entry| entry.contains("无效治疗，回复0点生命值")));
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

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, false
        ));
        assert!(encounter.participants[0].one_heart_target_id.is_none());
        assert_eq!(
            encounter.participants[0].one_heart_stacks,
            0
        );
        encounter.participants[0].one_heart_target_id = Some("c".to_owned());
        encounter.participants[0].one_heart_stacks = 5;

        assert!(store.record_skill_use("battle", "a", "c", &skill, &manager, None));
        let encounter = &store.encounters["battle"];
        assert!((encounter.participants[2].hp - 20.0).abs() < 0.0001);
        assert_eq!(
            encounter.participants[0].one_heart_target_id.as_deref(),
            Some("c")
        );
        assert_eq!(
            encounter.participants[0].one_heart_stacks,
            5
        );

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, true
        ));
        assert!(encounter.participants[0].one_heart_target_id.is_none());
        assert_eq!(
            encounter.participants[0].one_heart_stacks,
            0
        );
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

        store.encounters.get_mut("battle").unwrap().participants[1].hp = 10.0;
        assert!(store.record_skill_use("battle", "a", "b", &heal, &manager, None));
        assert_eq!(
            store.encounters["battle"].participants[1]
                .inspiration_sources
                .get("a"),
            Some(&1)
        );
        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, false
        ));
        assert!(encounter.participants[0].inspiration_target_id.is_none());
        assert!(encounter.participants[1].inspiration_sources.is_empty());
        encounter.participants[2]
            .inspiration_sources
            .insert("stale".to_owned(), 1);

        assert!(store.record_skill_use("battle", "c", "d", &damage, &manager, None));
        assert!((store.encounters["battle"].participants[3].hp - 48.0).abs() < 0.0001);
        assert!(store.record_skill_use("battle", "a", "b", &heal, &manager, None));
        let encounter = &store.encounters["battle"];
        assert!(encounter.participants[0].inspiration_target_id.is_none());
        assert!(encounter.participants[1].inspiration_sources.is_empty());
        assert_eq!(
            encounter.participants[2].inspiration_sources.get("stale"),
            Some(&1)
        );

        let encounter = store.encounters.get_mut("battle").unwrap();
        assert!(set_encounter_active_state(
            encounter, true
        ));
        assert!(
            encounter.participants.iter().all(|participant| participant
                .inspiration_target_id
                .is_none()
                && participant.inspiration_sources.is_empty())
        );
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
    fn parsed_battle_fatigue_walker_mitigates_low_hp_damage_penalty() {
        let mut manager = empty_manager();
        let actor_character = PlayerCharacter {
            hp: 5.0,
            max_hp: 10.0,
            skill_names: vec!["疲惫行者".to_owned()],
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
            name: "疲惫攻击".to_owned(),
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

        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None));

        let target = store.encounters["battle"]
            .participants
            .iter()
            .find(|participant| participant.target_id == "b")
            .unwrap();
        assert!((target.hp - 16.8).abs() < 0.0001);
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
    fn battle_skills_expand_soul_pulse_into_shared_cost_modes() {
        let character = PlayerCharacter {
            skill_names: vec!["灵魂脉冲".to_owned()],
            skill_notes: vec!["向周围释放灵魂波动，对敌方单位造成6点法术伤害或者治疗一个目标6点生命值。消耗9法力值，无冷却, 范围4米内".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::default()],
            ..Default::default()
        };

        let skills = character_skills(&character);

        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "灵魂脉冲（伤害）");
        assert_eq!(skills[1].name, "灵魂脉冲（治疗）");
        assert!(skills.iter().all(|skill| skill.index == 0));
        assert!(skills.iter().all(|skill| skill.mp_cost == 9.0));
        assert_eq!(
            skills[0].target_class.as_deref(),
            Some("范围")
        );
        assert_eq!(
            skills[1].target_class.as_deref(),
            Some("单目标")
        );
        assert!(skills.iter().all(|skill| skill.range == Some(4)));
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
        let mut actor = participant("a", 0);
        actor.skill_cooldown_ready_turns.insert("0".to_owned(), 2);
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![actor, participant("b", 0)],
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

        store.encounters.get_mut("battle").unwrap().participants[0].turn = 2;
        assert!(store.record_skill_use("battle", "a", "b", &skill, &manager, None,));
        assert!(
            !store.encounters["battle"].participants[0]
                .skill_cooldown_ready_turns
                .contains_key("0")
        );
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
        assert_eq!(actor.combat_turns_completed, 1);
        assert_eq!(
            store.encounters["battle"].combat_completed_turns,
            1
        );
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

    #[test]
    fn roster_action_completion_advances_turn_clocks_and_round() {
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                participants: vec![participant("a", 0), participant("b", 0)],
                ..Default::default()
            });

        assert!(!set_roster_action_done(
            &mut store, "battle", "a", false,
        ));
        assert!(set_roster_action_done(
            &mut store, "battle", "a", true,
        ));
        let first = &store.encounters["battle"].participants[0];
        assert!(first.action_done);
        assert_eq!(first.turn, 1);
        assert_eq!(first.combat_turns_completed, 1);
        assert_eq!(
            store.encounters["battle"].combat_completed_turns,
            1
        );

        assert!(set_roster_action_done(
            &mut store, "battle", "b", true,
        ));
        let encounter = &store.encounters["battle"];
        assert_eq!(encounter.round, 1);
        assert_eq!(encounter.combat_completed_turns, 2);
        assert!(encounter
            .participants
            .iter()
            .all(|participant| participant.turn == 1 && !participant.action_done));
    }

    #[test]
    fn forced_next_round_advances_only_unfinished_living_actor_clocks() {
        let mut finished = participant("finished", 4);
        finished.action_done = true;
        finished.combat_turns_completed = 2;
        let mut unfinished = participant("unfinished", 3);
        unfinished.combat_turns_completed = 1;
        unfinished.skill_last_used_turns.insert("0".to_owned(), 3);
        let mut defeated = participant("defeated", 2);
        defeated.alive = false;
        defeated.combat_turns_completed = 1;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                round: 3,
                combat_completed_turns: 3,
                participants: vec![finished, unfinished, defeated],
                ..Default::default()
            });

        assert!(store.next_round("battle"));

        let encounter = &store.encounters["battle"];
        let finished = &encounter.participants[0];
        let unfinished = &encounter.participants[1];
        let defeated = &encounter.participants[2];
        assert_eq!(encounter.round, 4);
        assert_eq!(encounter.combat_completed_turns, 4);
        assert_eq!(
            (
                finished.turn,
                finished.combat_turns_completed
            ),
            (4, 2)
        );
        assert_eq!(
            (
                unfinished.turn,
                unfinished.combat_turns_completed
            ),
            (4, 2)
        );
        assert_eq!(
            skill_cooldown_remaining(unfinished, 0, 2, None),
            1
        );
        assert_eq!(
            (
                defeated.turn,
                defeated.combat_turns_completed
            ),
            (2, 1)
        );
        assert!(encounter
            .participants
            .iter()
            .all(|participant| !participant.action_done));
    }

    #[test]
    fn maximum_battle_round_rejects_partial_round_effects() {
        let mut actor = participant("a", u32::MAX);
        actor.mp = 0.0;
        actor.max_mp = 10.0;
        actor.mp_regen = 1.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                round: u32::MAX,
                participants: vec![actor],
                action_log: vec!["before".to_owned()],
                ..Default::default()
            });

        assert!(!store.next_round("battle"));

        let encounter = &store.encounters["battle"];
        assert_eq!(encounter.round, u32::MAX);
        assert_eq!(encounter.participants[0].turn, u32::MAX);
        assert_eq!(encounter.participants[0].mp, 0.0);
        assert!(!encounter.participants[0].action_done);
        assert_eq!(encounter.action_log, vec!["before"]);
    }

    #[test]
    fn maximum_participant_counters_saturate_when_action_finishes() {
        let mut actor = participant("a", u32::MAX);
        actor.negative_layers = u32::MAX;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                name: "battle".to_owned(),
                round: u32::MAX,
                participants: vec![actor],
                ..Default::default()
            });

        assert!(store.skip_negative_participant("battle", "a"));

        let encounter = &store.encounters["battle"];
        let actor = &encounter.participants[0];
        assert_eq!(encounter.round, u32::MAX);
        assert_eq!(actor.turn, u32::MAX);
        assert_eq!(actor.negative_layers, u32::MAX);
        assert!(actor.action_done);
    }

    #[test]
    fn ineligible_battle_actors_cannot_mutate_targets_resources_or_clocks() {
        let manager = empty_manager();
        let mut defeated = participant("defeated", 4);
        defeated.hp = 0.0;
        defeated.mp = 8.0;
        defeated.alive = false;
        defeated.pending_negative = true;
        let mut finished = participant("finished", 7);
        finished.action_done = true;
        finished.pending_negative = true;
        let mut target = participant("target", 0);
        target.hp = 10.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                round: 3,
                participants: vec![defeated, finished, target],
                ..Default::default()
            });
        let skill = CharacterSkill {
            index: 0,
            name: "违规攻击".to_owned(),
            note: "主动使用对目标造成4点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 3.0,
            cooldown_turns: 2,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(!store.apply_action(
            "battle",
            "missing",
            "target",
            "幽灵攻击",
            4.0
        ));
        assert!(!store.record_skill_use("battle", "missing", "target", &skill, &manager, None));
        assert!(!store.apply_action(
            "battle",
            "defeated",
            "target",
            "倒地攻击",
            4.0
        ));
        assert!(!store.record_skill_use("battle", "defeated", "target", &skill, &manager, None,));
        assert!(!store.finish_actor_action("battle", "defeated"));
        assert!(!store.skip_negative_participant("battle", "defeated"));
        assert!(!store.apply_action(
            "battle",
            "finished",
            "target",
            "重复攻击",
            4.0
        ));
        assert!(!store.record_skill_use("battle", "finished", "target", &skill, &manager, None,));
        assert!(!store.finish_actor_action("battle", "finished"));
        assert!(!store.skip_negative_participant("battle", "finished"));

        let encounter = &store.encounters["battle"];
        let defeated = &encounter.participants[0];
        let finished = &encounter.participants[1];
        let target = &encounter.participants[2];
        assert_eq!(encounter.round, 3);
        assert_eq!(encounter.combat_completed_turns, 0);
        assert_eq!(target.hp, 10.0);
        assert_eq!(defeated.mp, 8.0);
        assert_eq!(defeated.turn, 4);
        assert_eq!(defeated.negative_layers, 0);
        assert!(defeated.skill_last_used_turns.is_empty());
        assert_eq!(finished.turn, 7);
        assert_eq!(finished.negative_layers, 0);
        assert!(finished.skill_last_used_turns.is_empty());
        assert_eq!(encounter.action_log.len(), 4);
    }

    #[test]
    fn battle_resolution_and_action_completion_are_one_transaction() {
        let mut manager = empty_manager();
        let mut actor = participant("actor", 0);
        actor.mp = 0.0;
        let target = participant("target", 0);
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("failed".to_owned(), BattleEncounter {
                participants: vec![actor, target],
                ..Default::default()
            });
        let unaffordable_skill = CharacterSkill {
            index: 0,
            name: "昂贵法术".to_owned(),
            note: "主动使用对目标造成4点魔法伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 5.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(!store.apply_action_and_finish(
            "failed",
            "actor",
            "missing",
            "无效攻击",
            4.0,
        ));
        assert!(
            !store.record_skill_use_with_buffs_and_finish(
                "failed",
                "actor",
                "target",
                &unaffordable_skill,
                &mut manager,
                None,
            )
        );
        let failed_actor = &store.encounters["failed"].participants[0];
        assert_eq!(failed_actor.turn, 0);
        assert_eq!(failed_actor.mp, 0.0);
        assert!(!failed_actor.action_done);
        assert_eq!(store.encounters["failed"].round, 0);
        assert_eq!(
            store.encounters["failed"].participants[1].hp,
            10.0
        );

        let mut self_defeating_actor = participant("self", 0);
        self_defeating_actor.hp = 5.0;
        store.encounters.insert("self".to_owned(), BattleEncounter {
            participants: vec![self_defeating_actor],
            ..Default::default()
        });

        assert!(store.apply_action_and_finish("self", "self", "self", "自我牺牲", 10.0,));
        let encounter = &store.encounters["self"];
        let actor = &encounter.participants[0];
        assert_eq!(actor.hp, 0.0);
        assert!(!actor.alive);
        assert_eq!(actor.turn, 1);
        assert_eq!(actor.combat_turns_completed, 1);
        assert!(!actor.action_done);
        assert_eq!(encounter.round, 1);
        assert_eq!(encounter.combat_completed_turns, 1);
    }

    #[test]
    fn direct_healing_can_revive_defeated_targets_but_attacks_and_buffs_cannot() {
        let mut manager = empty_manager();
        let mut actor = participant("actor", 0);
        actor.mp = 10.0;
        let mut defeated = participant("defeated", 0);
        defeated.hp = 0.0;
        defeated.alive = false;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![actor, defeated],
                ..Default::default()
            });
        let damage = CharacterSkill {
            index: 0,
            name: "补刀".to_owned(),
            note: "主动使用对目标造成4点物理伤害".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 2.0,
            cooldown_turns: 2,
            cooldown_left: None,
            target_count: None,
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let guard = CharacterSkill {
            index: 1,
            name: "守护术".to_owned(),
            note: "主动使用给予目标2回合守护状态使承伤设为0.5".to_owned(),
            ..damage.clone()
        };
        let healing = CharacterSkill {
            index: 2,
            name: "急救".to_owned(),
            note: "主动使用对目标回复4点生命值".to_owned(),
            ..damage.clone()
        };

        assert!(!store.apply_action_and_finish(
            "battle",
            "actor",
            "defeated",
            "普通攻击",
            4.0,
        ));
        assert!(
            !store.record_skill_use_with_buffs_and_finish(
                "battle",
                "actor",
                "defeated",
                &damage,
                &mut manager,
                None,
            )
        );
        assert!(
            !store.record_skill_use_with_buffs_and_finish(
                "battle",
                "actor",
                "defeated",
                &guard,
                &mut manager,
                None,
            )
        );
        let actor_before_healing = &store.encounters["battle"].participants[0];
        assert_eq!(actor_before_healing.mp, 10.0);
        assert_eq!(actor_before_healing.turn, 0);
        assert!(!actor_before_healing.action_done);
        assert!(actor_before_healing.skill_last_used_turns.is_empty());
        assert_eq!(
            store.encounters["battle"].participants[1].hp,
            0.0
        );

        assert!(
            store.record_skill_use_with_buffs_and_finish(
                "battle",
                "actor",
                "defeated",
                &healing,
                &mut manager,
                None,
            )
        );
        let encounter = &store.encounters["battle"];
        let actor = &encounter.participants[0];
        let revived = &encounter.participants[1];
        assert_eq!(actor.mp, 8.0);
        assert_eq!(actor.turn, 1);
        assert!(actor.action_done);
        assert_eq!(revived.hp, 4.0);
        assert!(revived.alive);
        assert_eq!(encounter.round, 0);
    }

    #[test]
    fn common_attack_applies_stats_and_records_modifier_sources() {
        let mut manager = empty_manager();
        let mut actor = participant("actor", 0);
        actor.str_ = 10;
        actor.hp = 100.0;
        actor.max_hp = 100.0;
        let mut target = participant("target", 0);
        target.hp = 100.0;
        target.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![actor, target],
                ..Default::default()
            });

        assert!(store.apply_common_attack_and_finish(
            "battle",
            "actor",
            "target",
            "普通攻击",
            10.0,
            &mut manager,
        ));
        let encounter = &store.encounters["battle"];
        assert!((encounter.participants[1].hp - 87.5).abs() < 0.0001);
        assert_eq!(encounter.combat_log.len(), 1);
        assert!(encounter.combat_log[0]
            .modifiers
            .iter()
            .any(|modifier| modifier.source == "属性伤害加成"));
    }

    #[test]
    fn corrosion_wave_stacks_poison_and_rust_on_enemy_turns_only() {
        let mut manager = empty_manager();
        let mut actor_character = PlayerCharacter {
            hp: 100.0,
            max_hp: 100.0,
            ..Default::default()
        };
        actor_character.skill_names = vec![String::new()];
        actor_character.skill_notes = vec!["腐蚀波（10分）：自己每次攻击后，给与所有敌人一层可叠加的中毒（3点物理伤害，持续4回合），如果是机械单位，则将中毒改为锈蚀（6点物理伤害，持续2回合）。对同一个目标第一层之后的层数会减少25%伤害。".to_owned()];
        actor_character.skill_metadata = vec![CharacterSkillMetadata::default()];
        manager.player_characters.insert(
            "actor".to_owned(),
            actor_character.clone(),
        );

        let actor = participant_from_character("actor", &actor_character, &manager);
        let mut living_enemy = participant("living", 0);
        living_enemy.hp = 100.0;
        living_enemy.max_hp = 100.0;
        let mut mechanical_enemy = participant("mech", 0);
        mechanical_enemy.hp = 100.0;
        mechanical_enemy.max_hp = 100.0;
        mechanical_enemy.is_summon = true;
        mechanical_enemy.summon_owner_id = Some("npc-owner".to_owned());
        mechanical_enemy.summon_kind = SummonKind::Mech;
        let mut player_ally = participant("ally", 0);
        player_ally.player_character = true;

        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                active: true,
                participants: vec![actor, living_enemy, mechanical_enemy, player_ally],
                ..Default::default()
            });
        let attack = CharacterSkill {
            index: 1,
            name: "打击".to_owned(),
            note: "主动使用对目标造成1点物理伤害".to_owned(),
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

        assert!(store.record_skill_use("battle", "actor", "living", &attack, &manager, None));
        assert!(store.record_skill_use("battle", "actor", "living", &attack, &manager, None));
        assert!(store.record_skill_use("battle", "actor", "living", &attack, &manager, None));

        let encounter = store.encounters.get_mut("battle").unwrap();
        let living = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "living")
            .unwrap();
        assert_eq!(living.corrosion_stacks.len(), 3);
        assert_eq!(
            living.corrosion_stacks[0].kind,
            BattleCorrosionKind::Poison
        );
        assert!((living.corrosion_stacks[0].damage_per_turn - 3.0).abs() < 0.0001);
        assert!((living.corrosion_stacks[1].damage_per_turn - 2.25).abs() < 0.0001);
        assert!((living.corrosion_stacks[2].damage_per_turn - 2.25).abs() < 0.0001);
        living.hp = 100.0;
        for _ in 0..4 {
            advance_participant_corrosion(living, true, 1);
        }
        assert!((living.hp - 70.0).abs() < 0.0001);
        assert!(living.corrosion_stacks.is_empty());

        let mechanical = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == "mech")
            .unwrap();
        assert_eq!(mechanical.corrosion_stacks.len(), 3);
        assert_eq!(
            mechanical.corrosion_stacks[0].kind,
            BattleCorrosionKind::Rust
        );
        assert!((mechanical.corrosion_stacks[0].damage_per_turn - 6.0).abs() < 0.0001);
        assert!((mechanical.corrosion_stacks[1].damage_per_turn - 4.5).abs() < 0.0001);
        assert!((mechanical.corrosion_stacks[2].damage_per_turn - 4.5).abs() < 0.0001);
        advance_participant_corrosion(mechanical, true, 1);
        advance_participant_corrosion(mechanical, true, 2);
        assert!((mechanical.hp - 70.0).abs() < 0.0001);
        assert!(mechanical.corrosion_stacks.is_empty());

        let ally = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == "ally")
            .unwrap();
        assert!(ally.corrosion_stacks.is_empty());
    }

    #[test]
    fn owned_item_can_hold_multiple_skills_and_consume_on_cast() {
        let mut manager = empty_manager();
        let mut character = PlayerCharacter::default();
        character.hp = 100.0;
        character.max_hp = 100.0;
        character
            .inventory
            .items
            .push(crate::napcat::InventoryItem {
                name: "法术卷轴".to_owned(),
                stack: 2,
                max_stack: 10,
                skills: vec![
                    crate::napcat::InventoryItemSkill {
                        name: "火花".to_owned(),
                        note: "主动使用对目标造成4点魔法伤害".to_owned(),
                        consume_item: true,
                        ..Default::default()
                    },
                    crate::napcat::InventoryItemSkill {
                        name: "微光".to_owned(),
                        note: "主动使用对目标回复2点生命值".to_owned(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("actor".to_owned(), character.clone());
        let mut actor = participant("actor", 0);
        actor.player_character = true;
        actor.hp = 100.0;
        actor.max_hp = 100.0;
        let mut target = participant("target", 0);
        target.hp = 100.0;
        target.max_hp = 100.0;
        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![actor, target],
                ..Default::default()
            });
        let groups = item_skill_groups(&character);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1.len(), 2);

        assert!(
            store.record_skill_use_with_buffs_and_finish(
                "battle",
                "actor",
                "target",
                &groups[0].1[0],
                &mut manager,
                None,
            )
        );
        assert_eq!(
            manager.player_characters["actor"].inventory.items[0].stack,
            1
        );
        assert!((store.encounters["battle"].participants[1].hp - 96.0).abs() < 0.0001);
    }

    #[test]
    fn pve_level_scaling_is_personalized_and_pvp_is_unscaled() {
        let mut player = participant("player", 0);
        player.player_character = true;
        player.level = 10;
        let mut other_player = participant("other-player", 0);
        other_player.player_character = true;
        other_player.level = 3;
        let mut mob = participant("unit:wolf", 0);
        mob.unit_template_id = Some("wolf".to_owned());
        mob.level = 5;

        assert!((personalized_pve_level_multiplier(&player, &mob) - 0.5).abs() < f32::EPSILON);
        assert!((personalized_pve_level_multiplier(&mob, &player) - 2.0).abs() < f32::EPSILON);
        assert_eq!(
            personalized_pve_level_multiplier(&player, &other_player),
            1.0
        );
    }

    #[test]
    fn kill_experience_is_weighted_across_damage_healing_and_buffs_without_exceeding_total() {
        let mut killer = participant("killer", 0);
        killer.player_character = true;
        killer.level = 1;
        let mut damage_assist = participant("damage-assist", 0);
        damage_assist.player_character = true;
        damage_assist.level = 1;
        let mut healer = participant("healer", 0);
        healer.player_character = true;
        healer.level = 1;
        let mut buffer = participant("buffer", 0);
        buffer.player_character = true;
        buffer.level = 1;
        let mut victim = participant("unit:wolf", 0);
        victim.unit_template_id = Some("wolf".to_owned());
        victim.level = 1;
        victim.max_hp = 20.0;
        victim.base_damage = 2.0;

        let mut encounter = BattleEncounter {
            participants: vec![killer, damage_assist, healer, buffer, victim],
            combat_log: vec![
                CombatLogEntry {
                    round: 1,
                    kind: CombatLogKind::Healing,
                    source_id: "healer".to_owned(),
                    source_name: "healer".to_owned(),
                    target_id: "killer".to_owned(),
                    target_name: "killer".to_owned(),
                    action_name: "heal".to_owned(),
                    base_amount: 4.0,
                    effective_amount: 4.0,
                    modifiers: Vec::new(),
                    benefits: Vec::new(),
                },
                CombatLogEntry {
                    round: 1,
                    kind: CombatLogKind::Buff,
                    source_id: "buffer".to_owned(),
                    source_name: "buffer".to_owned(),
                    target_id: "killer".to_owned(),
                    target_name: "killer".to_owned(),
                    action_name: "buff".to_owned(),
                    base_amount: 0.0,
                    effective_amount: 0.0,
                    modifiers: Vec::new(),
                    benefits: vec!["有益".to_owned()],
                },
            ],
            ..Default::default()
        };
        let outcome = BattleDefeatOutcome {
            contributors: vec!["killer".to_owned(), "damage-assist".to_owned()],
            contribution_amounts: HashMap::from([
                ("killer".to_owned(), 6.0),
                ("damage-assist".to_owned(), 10.0),
            ]),
            killer_id: Some("killer".to_owned()),
            defeated_id: "unit:wolf".to_owned(),
            defeated_player_character: false,
            defeated_level: 1,
            defeated_max_hp: 20.0,
            defeated_base_damage: 2.0,
            defeated_rarity: UnitRarity::Normal,
        };
        let total = battle_defeat_total_experience(&encounter, &outcome);

        apply_battle_experience_reward(&mut encounter, &outcome);

        let awarded = encounter
            .participants
            .iter()
            .filter(|participant| participant.player_character)
            .map(|participant| participant.exp)
            .sum::<i32>();
        assert_eq!(awarded, total);
        for id in ["killer", "damage-assist", "healer", "buffer"] {
            assert!(encounter
                .participants
                .iter()
                .find(|participant| participant.target_id == id)
                .is_some_and(|participant| participant.exp > 0));
        }
    }

    #[test]
    fn approved_support_talent_draw_grants_fifteen_percent_extra_battle_experience() {
        let manager = empty_manager();
        let mut support_character = PlayerCharacter {
            level: 2,
            skill_names: vec!["互帮互助".to_owned()],
            skill_metadata: vec![crate::napcat::CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            )],
            ..Default::default()
        };
        let support = participant_from_character("support", &support_character, &manager);
        assert_eq!(
            support.support_talent_experience_bonus_rate,
            SUPPORT_TALENT_EXPERIENCE_BONUS_RATE
        );

        support_character.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_support_talent_experience_bonus_rate(&support_character),
            0.0
        );

        let mut victim = participant("unit:wolf", 0);
        victim.unit_template_id = Some("wolf".to_owned());
        victim.level = 2;
        victim.max_hp = 79.0;
        victim.base_damage = 3.0;
        let mut encounter = BattleEncounter {
            participants: vec![support, victim],
            ..Default::default()
        };
        let outcome = BattleDefeatOutcome {
            contributors: vec!["support".to_owned()],
            contribution_amounts: HashMap::from([("support".to_owned(), 10.0)]),
            killer_id: Some("support".to_owned()),
            defeated_id: "unit:wolf".to_owned(),
            defeated_player_character: false,
            defeated_level: 2,
            defeated_max_hp: 79.0,
            defeated_base_damage: 3.0,
            defeated_rarity: UnitRarity::Normal,
        };
        let base_exp = battle_defeat_total_experience(&encounter, &outcome);
        assert_eq!(base_exp, 100);

        apply_battle_experience_reward(&mut encounter, &outcome);

        assert_eq!(encounter.participants[0].level, 2);
        assert_eq!(encounter.participants[0].exp, 115);
        assert!(
            encounter.action_log.iter().any(|entry| {
                entry.contains("获得115经验")
                    && entry.contains("辅助天赋经验加成15%")
                    && entry.contains("基础100")
            })
        );
    }
}
