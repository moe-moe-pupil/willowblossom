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
        NapcatMessageManager,
        PlayerCharacter,
        TrpgGroup,
    },
    rule_engine::{
        parse_rule,
        Action,
        ActorRef,
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
    #[serde(default)]
    pub turn: u32,
    #[serde(default)]
    pub agi: i32,
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
    pub skill_last_used_turns: HashMap<String, u32>,
}

#[derive(Debug, Clone)]
struct CharacterSkill {
    index: usize,
    name: String,
    note: String,
    mp_cost: f32,
    cooldown_turns: u32,
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
            participant.turn.hash(&mut hasher);
            participant.agi.hash(&mut hasher);
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
                        ui.label("No battle rounds yet.");
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
                    if ui.button("Close").clicked() {
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
        ui.label("TRPG group");
        egui::ComboBox::from_id_salt("battle_round_group_select")
            .selected_text(if ui_state.selected_group.is_empty() {
                "No group"
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
        ui.label("Name");
        ui.text_edit_singleline(&mut ui_state.new_encounter_name);
        if ui.button("Create").clicked() {
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
                ui.small(format!("Round {}", encounter.round));
                ui.small(if encounter_entity.active { "Active" } else { "Downtime" });
                if encounter_entity.negative_enabled {
                    ui.small("消极 on");
                }
                changed |= ui.checkbox(&mut encounter.active, "Active").changed();
                changed |= ui
                    .checkbox(&mut encounter.negative_enabled, "消极")
                    .changed();
                changed |= ui
                    .checkbox(&mut encounter.sort_by_turn, "Sort")
                    .on_hover_text("Sort action order by AGI.")
                    .changed();
                if ui.button("Refresh players").clicked() {
                    changed |= refresh_encounter_players(encounter, manager);
                }
                if ui.button("Prev round").clicked() {
                    prev_round_requested = true;
                }
                if ui.button("Next round").clicked() {
                    next_round_requested = true;
                }
                if ui.button("Delete").clicked() {
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
                        if ui.button("Confirm next round").clicked() {
                            changed |= store.next_round(encounter_id);
                            ui_state.confirm_next_round.remove(encounter_id);
                        }
                        if ui.button("Cancel").clicked() {
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

    ui.label("Action order");
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
            changed |= ui.checkbox(&mut participant.alive, "Alive").changed();
            if participant.action_done {
                ui.small("done");
            }
            if ui.button("Remove").clicked() {
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
            ui.label("Add player");
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
            if ui.button("Add").clicked() {
                encounter.participants.push(participant_from_target(
                    selected, manager,
                ));
                changed = true;
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
        ui.label("All actions are done.");
        if ui.button("Start next round").clicked() {
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
    let skills = manager
        .player_characters
        .get(&actor.target_id)
        .map(character_skills)
        .unwrap_or_default();

    ui.label(format!(
        "Current actor: {}",
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
        ui.label("Target");
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
        ui.label("Damage");
        ui.add(egui::DragValue::new(amount).speed(1.0).range(0.0..=9999.0));
        if ui.button("Normal attack").clicked() {
            changed |= store.apply_action(
                encounter_id,
                &actor.target_id,
                target,
                "普通攻击",
                *amount,
            );
            changed |= store.finish_actor_action(encounter_id, &actor.target_id);
        }
        if ui.button("Mark done").clicked() {
            changed |= store.finish_actor_action(encounter_id, &actor.target_id);
        }
        if ui.button("Skip + 消极").clicked() {
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
            ui.label("Skill");
            egui::ComboBox::from_id_salt(format!("battle_skill_{encounter_id}"))
                .selected_text(skills[*selected_skill].name.as_str())
                .show_ui(ui, |ui| {
                    for (index, skill) in skills.iter().enumerate() {
                        let remaining = skill_cooldown_remaining(
                            &actor,
                            skill.index,
                            skill.cooldown_turns,
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
            );
            let can_pay = actor.mp + f32::EPSILON >= skill.mp_cost.max(0.0);
            let can_use = cooldown_remaining == 0 && can_pay;
            let response = ui.add_enabled(can_use, egui::Button::new("Use skill"));
            if response.clicked() {
                changed |= store.record_skill_use(
                    encounter_id,
                    &actor.target_id,
                    target,
                    skill,
                    scene_positions,
                );
                changed |= store.finish_actor_action(encounter_id, &actor.target_id);
            }
            if !can_pay {
                ui.small(format!(
                    "Need {} MP",
                    format_number(skill.mp_cost.max(0.0))
                ));
            } else if cooldown_remaining > 0 {
                ui.small(format!(
                    "Cooldown {cooldown_remaining} turns"
                ));
            }
        });
    } else {
        ui.small("No skills on this character.");
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
    ui.label("Log");
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
                sort_by_turn: true,
                negative_enabled: false,
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
        encounter.action_log.push(format!(
            "GM moved back to round {}",
            encounter.round
        ));
        true
    }

    fn next_round(&mut self, encounter_id: &str) -> bool {
        let Some(encounter) = self.encounters.get_mut(encounter_id) else {
            return false;
        };
        encounter.round += 1;
        for participant in &mut encounter.participants {
            participant.action_done = false;
            if participant.alive {
                if !encounter.active {
                    participant.hp =
                        (participant.hp + participant.hp_regen).min(participant.max_hp);
                }
                participant.mp = (participant.mp + participant.mp_regen).min(participant.max_mp);
            }
        }
        encounter.action_log.push(format!(
            "Round {} started",
            encounter.round
        ));
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
                let display_name = participant_display_name(&participant.target_id, manager);
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
        target.hp = (target.hp - final_damage).max(0.0);
        target.alive = target.hp > 0.0;
        encounter.action_log.push(format!(
            "{} used {} on {} for {} damage",
            actor_name,
            action_name,
            target.display_name,
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
        scene_positions: Option<&SceneCharacterPositions>,
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
        let cooldown_remaining = skill_cooldown_remaining(actor, skill.index, skill.cooldown_turns);
        if cooldown_remaining > 0 {
            encounter.action_log.push(format!(
                "{} cannot use {}; cooldown {} turns",
                actor_name, skill.name, cooldown_remaining
            ));
            return false;
        }
        if actor.mp + f32::EPSILON < mp_cost {
            encounter.action_log.push(format!(
                "{} cannot use {}; needs {} MP",
                actor_name,
                skill.name,
                format_number(mp_cost)
            ));
            return false;
        }
        actor.mp = (actor.mp - mp_cost).max(0.0);
        actor
            .skill_last_used_turns
            .insert(skill.index.to_string(), actor.turn);

        match static_skill_effect(&skill.note) {
            Some(SkillEffect::Damage { amount, target }) => {
                let target_ids = resolve_skill_targets(
                    target,
                    actor_id,
                    target_id,
                    encounter,
                    scene_positions,
                );
                if target_ids.is_empty() {
                    encounter.action_log.push(format!(
                        "{} used {} but no targets were in range",
                        actor_name, skill.name
                    ));
                }
                for resolved_target_id in target_ids {
                    let Some(target) = encounter
                        .participants
                        .iter_mut()
                        .find(|participant| participant.target_id == resolved_target_id)
                    else {
                        continue;
                    };
                    target.hp = (target.hp - amount).max(0.0);
                    target.alive = target.hp > 0.0;
                    encounter.action_log.push(format!(
                        "{} used {} on {} for {} damage",
                        actor_name,
                        skill.name,
                        target.display_name,
                        format_number(amount)
                    ));
                }
            },
            Some(SkillEffect::Heal { amount, target }) => {
                let target_ids = resolve_skill_targets(
                    target,
                    actor_id,
                    target_id,
                    encounter,
                    scene_positions,
                );
                if target_ids.is_empty() {
                    encounter.action_log.push(format!(
                        "{} used {} but no targets were in range",
                        actor_name, skill.name
                    ));
                }
                for resolved_target_id in target_ids {
                    let Some(target) = encounter
                        .participants
                        .iter_mut()
                        .find(|participant| participant.target_id == resolved_target_id)
                    else {
                        continue;
                    };
                    target.hp = (target.hp + amount).min(target.max_hp);
                    target.alive = target.hp > 0.0;
                    encounter.action_log.push(format!(
                        "{} used {} on {} for {} healing",
                        actor_name,
                        skill.name,
                        target.display_name,
                        format_number(amount)
                    ));
                }
            },
            None => {
                let note = skill.note.trim();
                if note.is_empty() {
                    encounter.action_log.push(format!(
                        "{} used {} on {}",
                        actor_name, skill.name, target_name
                    ));
                } else {
                    encounter.action_log.push(format!(
                        "{} used {} on {} ({})",
                        actor_name, skill.name, target_name, note
                    ));
                }
            },
        }
        if mp_cost > 0.0 {
            encounter.action_log.push(format!(
                "{} spent {} MP",
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
    encounter
        .participants
        .retain(|participant| group.players.contains(&participant.target_id));
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
        participant.agi.hash(&mut hasher);
        participant.action_done.hash(&mut hasher);
        participant.alive.hash(&mut hasher);
        participant.hp.to_bits().hash(&mut hasher);
        participant.max_hp.to_bits().hash(&mut hasher);
        participant.mp.to_bits().hash(&mut hasher);
        participant.max_mp.to_bits().hash(&mut hasher);
        participant.hp_regen.to_bits().hash(&mut hasher);
        participant.mp_regen.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

fn participant_from_character(
    target_id: &str,
    character: &PlayerCharacter,
    manager: &NapcatMessageManager,
) -> BattleParticipantSnapshot {
    BattleParticipantSnapshot {
        target_id: target_id.to_owned(),
        display_name: character_display_name(target_id, character, manager),
        turn: 0,
        agi: character.status.agi + character.extra_status.agi,
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
        turn: 0,
        agi: 0,
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
        skill_last_used_turns: HashMap::new(),
    }
}

fn sync_participant_from_manager(
    participant: &mut BattleParticipantSnapshot,
    manager: &NapcatMessageManager,
) {
    if let Some(character) = manager.player_characters.get(&participant.target_id) {
        participant.display_name = character_display_name(
            &participant.target_id,
            character,
            manager,
        );
        participant.max_hp = character.max_hp;
        participant.max_mp = character.max_mp;
        participant.hp_regen = character.hp_regen;
        participant.mp_regen = character.mp_regen;
        participant.agi = character.status.agi + character.extra_status.agi;
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

fn character_skills(character: &PlayerCharacter) -> Vec<CharacterSkill> {
    character
        .skill_names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let display_name = if name.trim().is_empty() {
                format!("Skill {}", index + 1)
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
            }
        })
        .collect()
}

fn skill_cooldown_remaining(
    participant: &BattleParticipantSnapshot,
    skill_index: usize,
    cooldown_turns: u32,
) -> u32 {
    if cooldown_turns == 0 {
        return 0;
    }
    participant
        .skill_last_used_turns
        .get(&skill_index.to_string())
        .map(|last_used_turn| {
            cooldown_turns.saturating_sub(participant.turn.saturating_sub(*last_used_turn))
        })
        .unwrap_or(0)
}

fn display_name_for_target(options: &[(String, String)], target_id: &str) -> String {
    options
        .iter()
        .find(|(id, _)| id == target_id)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| target_id.to_owned())
}

enum SkillEffect {
    Damage { amount: f32, target: TargetSelector },
    Heal { amount: f32, target: TargetSelector },
}

fn static_skill_effect(note: &str) -> Option<SkillEffect> {
    let ast = parse_rule(note).ok()?;
    ast.actions.into_iter().find_map(|action| match action {
        Action::Damage {
            target,
            amount: ValueExpr::Number(amount),
            ..
        } => Some(SkillEffect::Damage {
            amount: amount.max(0.0),
            target,
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
) -> Vec<String> {
    if let Some(area) = target.area {
        let Some(radius) = area.radius_meters else {
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

    match target.actor {
        ActorRef::SelfActor => vec![actor_id.to_owned()],
        ActorRef::Source | ActorRef::Target => vec![selected_target_id.to_owned()],
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
        );

        assert_eq!(targets, vec!["near".to_owned()]);
    }

    fn battle_participant(target_id: &str) -> BattleParticipantSnapshot {
        BattleParticipantSnapshot {
            target_id: target_id.to_owned(),
            display_name: target_id.to_owned(),
            turn: 0,
            agi: 0,
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
            skill_last_used_turns: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn participant(id: &str, turn: u32) -> BattleParticipantSnapshot {
        BattleParticipantSnapshot {
            target_id: id.to_owned(),
            display_name: id.to_owned(),
            turn,
            agi: 0,
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
            skill_last_used_turns: HashMap::new(),
        }
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
}
