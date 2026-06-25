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

use crate::{
    napcat::{
        character_damage_dealt_talent_buffs,
        character_damage_taken_attribute_multiplier,
        character_dying_healing_taken_modifier,
        character_large_hit_damage_taken_modifier,
        character_minimum_damage_floor,
        character_minimum_range_meters,
        character_moonberry_talent_damage_attribute_bonus,
        character_mutual_aid_healing_rate,
        character_physical_damage_lifesteal,
        character_range_magic_converter_damage_bonus,
        character_wounded_healing_dealt_modifier,
        dying_healing_taken_multiplier,
        large_hit_damage_taken_multiplier,
        low_hp_damage_multiplier,
        moonberry_effective_skill_range_radius,
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
        DamageType,
        TargetSelector,
        ValueExpr,
    },
    scene::SceneCharacterPositions,
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
    #[serde(default = "default_combat_modifier")]
    pub damage_dealt_modifier: f32,
    #[serde(default = "default_combat_modifier")]
    pub damage_taken_modifier: f32,
    #[serde(default = "default_combat_modifier")]
    pub healing_dealt_modifier: f32,
    #[serde(default = "default_combat_modifier")]
    pub healing_taken_modifier: f32,
    #[serde(default)]
    pub wound_healing_taken_turns: i32,
    #[serde(default)]
    pub damage_taken_this_turn: f32,
    #[serde(default)]
    pub healing_taken_this_turn: f32,
    #[serde(default)]
    pub skill_last_used_turns: HashMap<String, u32>,
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

fn reset_participant_turn_totals(participant: &mut BattleParticipantSnapshot) -> bool {
    let changed = participant.damage_taken_this_turn.abs() > f32::EPSILON
        || participant.healing_taken_this_turn.abs() > f32::EPSILON;
    participant.damage_taken_this_turn = 0.0;
    participant.healing_taken_this_turn = 0.0;
    changed
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
            participant.wound_healing_taken_turns.hash(&mut hasher);
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
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
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
    let Some(manager) = manager.as_deref() else {
        return;
    };

    let mut panel_open = ui_state.panel_open;
    let mut changed = false;
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
                        changed |= encounter_ui(
                            ui,
                            &mut ui_state,
                            store,
                            manager,
                            scene_positions.as_deref(),
                            encounter_entity,
                        );
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
    manager: &NapcatMessageManager,
    scene_positions: Option<&SceneCharacterPositions>,
    encounter_entity: &BattleEncounterEntity,
) -> bool {
    let mut changed = false;
    let encounter_id = encounter_entity.id.as_str();
    let mut remove = false;
    if !store.encounters.contains_key(encounter_id) {
        return false;
    }

    ui.group(|ui| {
        ui.set_width(ui.available_width());
        let mut prev_round_requested = false;
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
                changed |= ui.checkbox(&mut encounter.active, "进行中").changed();
                changed |= ui
                    .checkbox(&mut encounter.negative_enabled, "消极")
                    .changed();
                changed |= ui
                    .checkbox(&mut encounter.sort_by_turn, "排序")
                    .on_hover_text("按AGI排序行动顺序。")
                    .changed();
                if ui.button("刷新玩家").clicked() {
                    changed |= refresh_encounter_players(encounter, manager);
                }
                if ui.button("上一轮").clicked() {
                    prev_round_requested = true;
                }
                if ui.button("下一轮").clicked() {
                    next_round_requested = true;
                }
                if ui.button("删除").clicked() {
                    remove = true;
                }
            });
        }
        if prev_round_requested {
            changed |= store.previous_round(encounter_id);
            ui_state.confirm_next_round.remove(encounter_id);
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
                encounter.participants.push(participant_from_target(
                    selected, manager,
                ));
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
    manager: &NapcatMessageManager,
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
        .map(character_skills)
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
        if ui.button("普通攻击").clicked() {
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
            let can_use = cooldown_remaining == 0 && can_pay;
            let response = ui.add_enabled(can_use, egui::Button::new("使用技能"));
            if response.clicked() {
                changed |= store.record_skill_use(
                    encounter_id,
                    &actor.target_id,
                    target,
                    skill,
                    manager,
                    scene_positions,
                );
                changed |= store.finish_actor_action(encounter_id, &actor.target_id);
            }
            if !can_pay {
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
            .map(|target_id| participant_from_target(target_id, manager))
            .collect::<Vec<_>>();

        self.encounters
            .insert(encounter_id.clone(), BattleEncounter {
                name,
                trpg_group: Some(group_name),
                active: true,
                sort_by_turn: group.battle_sort_by_turn,
                negative_enabled: group.battle_negative_enabled,
                round: 0,
                participants,
                action_log: Vec::new(),
            });
        encounter_id
    }

    fn previous_round(&mut self, encounter_id: &str) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        if encounter.round > 0 {
            encounter.round -= 1;
        }
        for participant in &mut encounter.participants {
            participant.action_done = false;
        }
        encounter
            .action_log
            .push(format!("GM回到第{}轮", encounter.round));
        true
    }

    fn next_round(&mut self, encounter_id: &str) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        encounter.round += 1;
        for participant in &mut encounter.participants {
            participant.action_done = false;
            reset_participant_turn_totals(participant);
            if participant.alive {
                if !encounter.active {
                    participant.hp =
                        (participant.hp + participant.hp_regen).min(participant.max_hp);
                }
                participant.mp = (participant.mp + participant.mp_regen).min(participant.max_mp);
            }
        }
        encounter
            .action_log
            .push(format!("第{}轮开始", encounter.round));
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
        let actor_name = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == actor_id)
            .map(|participant| participant.display_name.clone())
            .unwrap_or_else(|| actor_id.to_owned());
        let Some(target) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == target_id)
        else {
            return false;
        };
        let final_damage = damage.max(0.0);
        record_participant_damage_taken(target, final_damage);
        target.hp = (target.hp - final_damage).max(0.0);
        target.alive = target.hp > 0.0;
        encounter.action_log.push(format!(
            "{}对{}使用{}，造成{}点伤害",
            actor_name,
            target.display_name,
            action_name,
            format_number(final_damage)
        ));
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
        let Some(actor_snapshot) = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == actor_id)
            .cloned()
        else {
            return false;
        };
        let actor_character = character_for_participant(&actor_snapshot, manager);
        let actor_damage_dealt_buffs = actor_character
            .map(|character| character_damage_dealt_talent_buffs(character, actor_id))
            .unwrap_or_default();
        let actor_physical_damage_lifesteal = actor_character
            .map(character_physical_damage_lifesteal)
            .unwrap_or(0.0);
        let actor_minimum_damage_floor = actor_character
            .map(character_minimum_damage_floor)
            .unwrap_or(0.0);
        let actor_name = actor_snapshot.display_name.clone();
        let target_name = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == target_id)
            .map(|participant| participant.display_name.clone())
            .unwrap_or_else(|| target_id.to_owned());
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

        match static_skill_effect(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            skill.legacy_buff_machine_json.as_deref(),
        ) {
            Some(SkillEffect::Damage {
                amount,
                target,
                damage_type,
            }) => {
                let actor_damage_multiplier = participant_damage_multiplier(
                    &actor_snapshot,
                    actor_character,
                    &basic_config,
                    damage_type,
                );
                let fallback_radius = battle_skill_damage_range_radius(
                    skill.range,
                    actor_character,
                    damage_type,
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
                if target_ids.is_empty() {
                    encounter.action_log.push(format!(
                        "{}使用{}，但范围内没有目标",
                        actor_name, skill.name
                    ));
                }
                let mut pending_actor_lifesteal = 0.0;
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
                        * target_character
                            .map(|character| {
                                character_damage_taken_attribute_multiplier(
                                    character,
                                    trpg_damage_taken_kind(damage_type),
                                )
                            })
                            .unwrap_or(1.0);
                    let incoming_amount =
                        (amount * actor_damage_multiplier * target_damage_multiplier).max(0.0);
                    let target_large_hit_modifier = target_character
                        .map(character_large_hit_damage_taken_modifier)
                        .unwrap_or(1.0);
                    let typed_final_amount = (incoming_amount
                        * large_hit_damage_taken_multiplier(
                            target.max_hp,
                            incoming_amount,
                            target_large_hit_modifier,
                        ))
                    .max(0.0);
                    let final_amount =
                        if amount > f32::EPSILON && actor_minimum_damage_floor > f32::EPSILON {
                            typed_final_amount.max(actor_minimum_damage_floor)
                        } else {
                            typed_final_amount
                        };
                    record_participant_damage_taken(target, final_amount);
                    target.hp = (target.hp - final_amount).max(0.0);
                    target.alive = target.hp > 0.0;
                    if final_amount > f32::EPSILON
                        && actor_damage_dealt_buffs
                            .iter()
                            .any(|buff| buff.name == "溃伤")
                    {
                        target.wound_healing_taken_turns = 1;
                    }
                    if final_amount > f32::EPSILON && damage_type == DamageType::Physical {
                        pending_actor_lifesteal +=
                            typed_final_amount * actor_physical_damage_lifesteal;
                    }
                    encounter.action_log.push(format!(
                        "{}对{}使用{}，造成{}点伤害",
                        actor_name,
                        target.display_name,
                        skill.name,
                        format_number(final_amount)
                    ));
                }
                if pending_actor_lifesteal > f32::EPSILON {
                    if let Some(actor) = encounter
                        .participants
                        .iter_mut()
                        .find(|participant| participant.target_id == actor_id)
                    {
                        record_participant_healing_taken(actor, pending_actor_lifesteal);
                        actor.hp = (actor.hp + pending_actor_lifesteal).min(actor.max_hp);
                        actor.alive = actor.hp > 0.0;
                        encounter.action_log.push(format!(
                            "{}触发禅宗古训，回复{}点生命值",
                            actor_name,
                            format_number(pending_actor_lifesteal)
                        ));
                    }
                }
            },
            Some(SkillEffect::Heal { amount, target }) => {
                let actor_healing_multiplier = participant_healing_multiplier(
                    &actor_snapshot,
                    actor_character,
                    &basic_config,
                );
                let actor_mutual_aid_healing_rate = actor_character
                    .map(character_mutual_aid_healing_rate)
                    .unwrap_or(0.0);
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
                let mut pending_actor_mutual_aid_healing = 0.0;
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
                        .map(character_dying_healing_taken_modifier)
                        .unwrap_or(1.0);
                    let target_mutual_aid_healing_rate = target_character
                        .map(character_mutual_aid_healing_rate)
                        .unwrap_or(0.0);
                    let target_healing_multiplier = target.healing_taken_modifier
                        * participant_wound_healing_multiplier(target)
                        * dying_healing_taken_multiplier(
                            target.hp,
                            target.max_hp,
                            target_dying_healing_modifier,
                        );
                    let final_amount =
                        (amount * actor_healing_multiplier * target_healing_multiplier).max(0.0);
                    record_participant_healing_taken(target, final_amount);
                    target.hp = (target.hp + final_amount).min(target.max_hp);
                    target.alive = target.hp > 0.0;
                    if resolved_target_id != actor_id && final_amount > f32::EPSILON {
                        pending_actor_mutual_aid_healing += final_amount
                            * (actor_mutual_aid_healing_rate + target_mutual_aid_healing_rate);
                    }
                    encounter.action_log.push(format!(
                        "{}对{}使用{}，回复{}点生命值",
                        actor_name,
                        target.display_name,
                        skill.name,
                        format_number(final_amount)
                    ));
                }
                if pending_actor_mutual_aid_healing > f32::EPSILON {
                    if let Some(actor) = encounter
                        .participants
                        .iter_mut()
                        .find(|participant| participant.target_id == actor_id)
                    {
                        record_participant_healing_taken(actor, pending_actor_mutual_aid_healing);
                        actor.hp = (actor.hp + pending_actor_mutual_aid_healing).min(actor.max_hp);
                        actor.alive = actor.hp > 0.0;
                        encounter.action_log.push(format!(
                            "{}触发互帮互助，回复{}点生命值",
                            actor_name,
                            format_number(pending_actor_mutual_aid_healing)
                        ));
                    }
                }
            },
            None => {
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
            },
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
        } else if participant.alive {
            if !encounter.active {
                participant.hp = (participant.hp + participant.hp_regen).min(participant.max_hp);
            }
            participant.mp = (participant.mp + participant.mp_regen).min(participant.max_mp);
        }
        reset_participant_turn_totals(participant);
        if participant.wound_healing_taken_turns > 0 {
            participant.wound_healing_taken_turns -= 1;
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
        sync_participant_from_manager(participant, manager);
    }
    for target_id in &group.players {
        if let Some(participant) = encounter
            .participants
            .iter_mut()
            .find(|participant| participant.target_id == *target_id)
        {
            sync_participant_from_manager(participant, manager);
        } else {
            encounter.participants.push(participant_from_target(
                target_id, manager,
            ));
        }
    }
    before_signature != encounter_participants_signature(&encounter.participants)
}

fn encounter_participants_signature(participants: &[BattleParticipantSnapshot]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for participant in participants {
        participant.target_id.hash(&mut hasher);
        participant.display_name.hash(&mut hasher);
        participant.unit_template_id.hash(&mut hasher);
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

fn participant_from_character(
    target_id: &str,
    character: &PlayerCharacter,
    manager: &NapcatMessageManager,
) -> BattleParticipantSnapshot {
    let status = character.status.combined(&character.extra_status);
    BattleParticipantSnapshot {
        target_id: target_id.to_owned(),
        display_name: character_display_name(target_id, character, manager),
        unit_template_id: None,
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
        damage_dealt_modifier: character.damage_dealt_modifier,
        damage_taken_modifier: character.damage_taken_modifier,
        healing_dealt_modifier: character.healing_dealt_modifier,
        healing_taken_modifier: character.healing_taken_modifier,
        wound_healing_taken_turns: 0,
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
    BattleParticipantSnapshot {
        target_id: target_id.to_owned(),
        display_name: unit_participant_display_name(target_id, unit_id, unit),
        unit_template_id: Some(unit_id.to_owned()),
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
        damage_dealt_modifier: character.damage_dealt_modifier,
        damage_taken_modifier: character.damage_taken_modifier,
        healing_dealt_modifier: character.healing_dealt_modifier,
        healing_taken_modifier: character.healing_taken_modifier,
        wound_healing_taken_turns: 0,
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
        damage_dealt_modifier: 1.0,
        damage_taken_modifier: 1.0,
        healing_dealt_modifier: 1.0,
        healing_taken_modifier: 1.0,
        wound_healing_taken_turns: 0,
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
            let character = &unit.character;
            let status = character.status.combined(&character.extra_status);
            participant.display_name =
                unit_participant_display_name(&participant.target_id, unit_id, unit);
            participant.max_hp = character.max_hp;
            participant.max_mp = character.max_mp;
            participant.hp_regen = character.hp_regen;
            participant.mp_regen = character.mp_regen;
            participant.str_ = status.str_;
            participant.agi = status.agi;
            participant.dex = status.dex;
            participant.int_ = status.int_;
            participant.wis = status.wis;
            participant.damage_dealt_modifier = character.damage_dealt_modifier;
            participant.damage_taken_modifier = character.damage_taken_modifier;
            participant.healing_dealt_modifier = character.healing_dealt_modifier;
            participant.healing_taken_modifier = character.healing_taken_modifier;
            participant.hp = participant.hp.min(participant.max_hp);
            participant.mp = participant.mp.min(participant.max_mp);
            participant.alive = participant.hp > 0.0;
        }
        return;
    }

    if let Some(character) = manager.player_characters.get(&participant.target_id) {
        let status = character.status.combined(&character.extra_status);
        participant.display_name = character_display_name(
            &participant.target_id,
            character,
            manager,
        );
        participant.max_hp = character.max_hp;
        participant.max_mp = character.max_mp;
        participant.hp_regen = character.hp_regen;
        participant.mp_regen = character.mp_regen;
        participant.str_ = status.str_;
        participant.agi = status.agi;
        participant.dex = status.dex;
        participant.int_ = status.int_;
        participant.wis = status.wis;
        participant.damage_dealt_modifier = character.damage_dealt_modifier;
        participant.damage_taken_modifier = character.damage_taken_modifier;
        participant.healing_dealt_modifier = character.healing_dealt_modifier;
        participant.healing_taken_modifier = character.healing_taken_modifier;
        participant.hp = participant.hp.min(participant.max_hp);
        participant.mp = participant.mp.min(participant.max_mp);
        participant.alive = participant.hp > 0.0;
    } else {
        participant.display_name = participant_display_name(&participant.target_id, manager);
    }
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
        if participant.hp <= 0.0 {
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

fn character_for_participant<'a>(
    participant: &BattleParticipantSnapshot,
    manager: &'a NapcatMessageManager,
) -> Option<&'a PlayerCharacter> {
    if let Some(unit_id) = participant.unit_template_id.as_deref() {
        return manager.unit_pool.get(unit_id).map(|unit| &unit.character);
    }

    manager.player_characters.get(&participant.target_id)
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
        * low_hp_damage_multiplier(participant.hp, participant.max_hp)
        * (status_damage_attribute_multiplier(&status, config, bonus_kind) + talent_bonus)
}

fn participant_healing_multiplier(
    participant: &BattleParticipantSnapshot,
    character: Option<&PlayerCharacter>,
    config: &TrpgBasicConfig,
) -> f32 {
    let wounded_modifier = character
        .map(character_wounded_healing_dealt_modifier)
        .unwrap_or(1.0);
    participant.healing_dealt_modifier
        * status_healing_attribute_multiplier(&participant_status(participant), config)
        * wounded_healing_dealt_multiplier(
            participant.hp,
            participant.max_hp,
            wounded_modifier,
        )
}

fn participant_wound_healing_multiplier(participant: &BattleParticipantSnapshot) -> f32 {
    if participant.wound_healing_taken_turns > 0 {
        0.75
    } else {
        1.0
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
}

fn static_skill_effect(
    note: &str,
    arg_values: &SkillRuleArgs,
    skill_type: Option<&str>,
    legacy_buff_machine_json: Option<&str>,
) -> Option<SkillEffect> {
    let ast = parse_rule_with_named_args(
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
    })?;
    ast.actions.into_iter().find_map(|action| match action {
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
        _ => None,
    })
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
) -> Option<f32> {
    let minimum_range = if damage_type == DamageType::Range {
        actor_character
            .map(character_minimum_range_meters)
            .unwrap_or(0.0)
    } else {
        0.0
    };
    moonberry_effective_skill_range_radius(skill_range, minimum_range)
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
            damage_dealt_modifier: 1.0,
            damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            healing_taken_modifier: 1.0,
            wound_healing_taken_turns: 0,
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
            damage_dealt_modifier: 1.0,
            damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            healing_taken_modifier: 1.0,
            wound_healing_taken_turns: 0,
            damage_taken_this_turn: 0.0,
            healing_taken_this_turn: 0.0,
            skill_last_used_turns: HashMap::new(),
        }
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
            .map(character_skills)
            .unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "黏液喷吐");
        assert_eq!(skills[0].mp_cost, 1.0);
        assert_eq!(skills[0].cooldown_turns, 2);
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

        assert!(store.advance_participant("battle", "b", false));
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
