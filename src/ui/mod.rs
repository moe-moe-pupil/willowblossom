mod ime;
use std::{
    collections::{
        BTreeSet,
        HashMap,
        HashSet,
    },
    fs,
    hash::{
        Hash,
        Hasher,
    },
    path::Path,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};
mod components;

use std::collections::hash_map::DefaultHasher;

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
};
use bevy_egui::{
    egui::{
        self,
        epaint::CircleShape,
        menu,
        Context,
        Id,
        Memory,
        Modal,
        Modifiers,
        Painter,
        Pos2,
        Rect,
        Response,
        Sense,
        Stroke,
        TextureHandle,
        Ui,
        Vec2,
        Widget,
    },
    EguiContexts,
    EguiGlobalSettings,
    EguiPlugin,
    EguiPrimaryContextPass,
};
use bevy_persistent::{
    Persistent,
    StorageFormat,
};
use ime::*;
use rand::RngExt;
use serde::{
    Deserialize,
    Serialize,
};
use tokio_tungstenite::tungstenite::protocol::Message;

const CHAT_WINDOW_SIZE: Vec2 = Vec2::new(360.0, 520.0);
const CHAT_WINDOW_MIN_SIZE: Vec2 = Vec2::new(260.0, 260.0);
const CHAT_WINDOW_MAX_SIZE: Vec2 = Vec2::new(720.0, 720.0);
const GROUP_CHAT_MAX_WIDTH: f32 = 520.0;
const GROUP_CHAT_MAX_HEIGHT: f32 = 720.0;
const GROUP_CHAT_MIN_HEIGHT: f32 = 140.0;
const GROUP_CHAT_SEPARATOR_HEIGHT: f32 = 10.0;
const GROUP_MEMBER_CHAT_SIZE: Vec2 = Vec2::new(320.0, 420.0);
const GROUP_BROADCAST_INPUT_HEIGHT: f32 = 96.0;
const GROUP_BROADCAST_INPUT_ROWS: usize = 3;
const GROUP_MEMBER_WINDOW_SIDE_GAP: f32 = 14.0;
const GROUP_MEMBER_WINDOW_TOP_GAP: f32 = 58.0;
const GROUP_MEMBER_WINDOW_BOTTOM_GAP: f32 = GROUP_BROADCAST_INPUT_HEIGHT + 7.0;
const GROUP_MEMBER_WINDOW_MAX_SIZE: Vec2 = Vec2::new(520.0, 620.0);
const CHAT_AUTO_SCROLL_THRESHOLD: f32 = 48.0;
const CHAT_IMAGE_MAX_SIZE: Vec2 = Vec2::new(220.0, 220.0);
const CHARACTER_WINDOW_DEFAULT_WIDTH: f32 = 360.0;
const CHARACTER_WINDOW_MIN_WIDTH: f32 = 320.0;
const CHARACTER_WINDOW_MAX_WIDTH: f32 = 720.0;
const CHARACTER_FIELD_MAX_WIDTH: f32 = 560.0;
const NAPCAT_EXPORT_DEFAULT_PATH: &str = ".data/willowblossom/exports/messages_export.json";
const NAPCAT_PC_EXPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/exports/player_characters_export.json";
const NAPCAT_CHAT_LIST_EXPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/exports/chat_list_export.json";
const NAPCAT_UNIT_POOL_EXPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/exports/unit_pool_export.json";
const NAPCAT_MOONBERRY_LEGACY_IMPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/imports/moonberry_legacy.json";
const DEEPSEEK_SUMMARY_EXPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/exports/deepseek_summaries_export.json";
const VOXEL_SCENE_EXPORT_DEFAULT_PATH: &str = ".data/willowblossom/exports/voxel_scene_export.json";

use crate::{
    battle_round::BattleRoundUiState,
    deepseek::{
        DeepseekIOSender,
        DeepseekManager,
        DeepseekPlugin,
        DeepseekRequest,
        DeepseekSummaryBlock,
        DEEPSEEK_SUMMARY_EXPORT_VERSION,
    },
    napcat::{
        normalized_random_pool_counts,
        update_character_from_status,
        update_character_from_status_with_config,
        CampaignMessage,
        CharacterBuffBaseStats,
        CharacterCreationStep,
        CharacterInventory,
        CharacterSkillMetadata,
        CharacterSkillSourceKind,
        CharacterStatus,
        ChatGroup,
        EquipmentSlot,
        ImageData,
        InventoryItem,
        InventoryQuality,
        NapcatIOSender,
        NapcatMessage,
        NapcatMessageChain,
        NapcatMessageChainType,
        NapcatMessageData,
        NapcatMessageManager,
        NapcatMessageType,
        NapcatSendManager,
        NapcatSender,
        PlayerCharacter,
        RandomPool,
        RandomPoolEntry,
        RandomPoolTextResult,
        SkillPoolEntry,
        TextData,
        TrpgBasicConfig,
        TrpgGroup,
        UnitPoolEntry,
        Visibility,
        NAPCAT_MANAGER_EXPORT_VERSION,
    },
    rule_engine::{
        parse_rule,
        Action,
        ActorRef,
        BuffEffect,
        BuffField,
        BuffKind,
        BuffSpec,
        BuffValue,
        Character as RuleCharacter,
        RuleAst,
        RuleEngineState,
        StatusKey,
        TargetSelector,
        ValueExpr,
    },
    scene::{
        SceneCharacterPositions,
        ScenePlayerCameraPositions,
        ScenePlayerViewRequest,
        VoxelMapRuntimeState,
        VoxelSceneStore,
        VOXEL_SCENE_EXPORT_VERSION,
    },
};
pub struct UIPlugin;
#[derive(Resource)]
pub struct GIFImages {
    images: HashMap<String, Vec<(TextureHandle, u32)>>,
}

#[derive(Default)]
pub(crate) struct ChatScrollState {
    message_count: usize,
    near_bottom: bool,
}

#[derive(Default)]
pub(crate) struct TrpgGroupSettingsState {
    open: bool,
    new_group_name: String,
    new_random_pool_name: String,
    random_pool_award_target: String,
    new_unit_id: String,
    unit_pool_source_target: String,
    focused_group_name: Option<String>,
    pending_character_delete: Option<String>,
    random_pool_entry_drafts: HashMap<String, RandomPoolEntry>,
    unit_pool_draft: UnitPoolEntry,
    skill_pool_draft: SkillPoolEntry,
    party_name_drafts: HashMap<String, String>,
    export_path: String,
    pc_export_path: String,
    chat_list_export_path: String,
    unit_pool_export_path: String,
    moonberry_legacy_import_path: String,
    deepseek_summary_export_path: String,
    voxel_scene_export_path: String,
    import_path: String,
    import_export_status: String,
}

#[derive(Default)]
pub(crate) struct CharacterEditState {
    unlocked_status_targets: HashSet<String>,
    gm_status_drafts: HashMap<String, CharacterStatus>,
    buff_drafts: HashMap<String, BuffDraft>,
    pending_character_reset: Option<String>,
    quick_cast_skill_index: HashMap<String, usize>,
    pending_force_cast: Option<(String, usize)>,
    skill_pool_selected_index: HashMap<String, usize>,
}

#[derive(Clone)]
pub(crate) struct BuffDraft {
    name: String,
    kind: BuffKind,
    priority: i32,
    turns_remaining: i32,
    beneficial: bool,
    field: BuffField,
    value: BuffValue,
}

impl Default for BuffDraft {
    fn default() -> Self {
        Self {
            name: "新buff".to_owned(),
            kind: BuffKind::Magic,
            priority: 0,
            turns_remaining: 1,
            beneficial: true,
            field: BuffField::DamageTakenModifier,
            value: BuffValue::Set(0.5),
        }
    }
}

#[derive(SystemParam)]
pub struct UiSystemLocals<'s> {
    has_run_once: Local<'s, bool>,
    new_chat_group_modal_string_open: Local<'s, (String, bool)>,
    chat_input_msgs: Local<'s, HashMap<String, String>>,
    chat_scroll_states: Local<'s, HashMap<String, ChatScrollState>>,
    previous_group_rects: Local<'s, HashMap<String, Rect>>,
    chat_list_edit_target: Local<'s, Option<String>>,
    chat_list_edit_name: Local<'s, String>,
    trpg_group_settings: Local<'s, TrpgGroupSettingsState>,
    character_edit_state: Local<'s, CharacterEditState>,
    quick_character_targets: Local<'s, HashSet<String>>,
    chat_image_textures: Local<'s, HashMap<String, TextureHandle>>,
    chat_turn_count_drafts: Local<'s, HashMap<(String, String), u32>>,
    group_broadcast_scopes: Local<'s, HashMap<String, String>>,
}

pub struct CircleImageButton {
    image: egui::TextureId,
    size: f32,
}

#[derive(Resource, Serialize, Deserialize)]
pub struct CachedMemory {
    ui_memory: Memory,
}

impl CircleImageButton {
    pub fn new(image: egui::TextureId, size: f32) -> Self { Self { image, size } }
}

fn file_menu_button(
    ui: &mut Ui,
    new_chat_group_modal_open: &mut bool,
    trpg_group_settings_open: &mut bool,
) {
    let new_chat_group_shortcup = egui::KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::G);

    // NOTE: we must check the shortcuts OUTSIDE of the actual "File" menu,
    // or else they would only be checked if the "File" menu was actually open!

    if ui.input_mut(|i| i.consume_shortcut(&new_chat_group_shortcup)) {
        *new_chat_group_modal_open = true
    }

    ui.menu_button("编辑", |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

        if ui
            .add(
                egui::Button::new("新建讨论组")
                    .shortcut_text(ui.ctx().format_shortcut(&new_chat_group_shortcup)),
            )
            .clicked()
        {
            *new_chat_group_modal_open = true
        }

        if ui.button("TRPG设置").clicked() {
            *trpg_group_settings_open = true;
            ui.close();
        }
    });
}

fn tools_menu_button(
    ui: &mut Ui,
    rule_engine_state: &mut RuleEngineState,
    battle_round_state: &mut BattleRoundUiState,
) {
    ui.menu_button("工具", |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

        if ui.button("战斗轮").clicked() {
            battle_round_state.open_panel();
            ui.close();
        }
        if ui.button("规则引擎").clicked() {
            rule_engine_state.open_panel();
            ui.close();
        }
    });
}

fn pool_menu_button(
    ui: &mut Ui,
    manager: &mut NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
) -> bool {
    let mut changed = false;
    let player_targets = sorted_pool_targets(manager, false);
    let group_chat_targets = sorted_pool_targets(manager, true);

    ui.menu_button("池", |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

        ui.menu_button(
            format!("玩家池 ({})", player_targets.len()),
            |ui| {
                if player_targets.is_empty() {
                    ui.label("还没有玩家私聊。");
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("top_player_pool_menu")
                        .max_height(220.0)
                        .show(ui, |ui| {
                            for target_id in &player_targets {
                                ui.horizontal(|ui| {
                                    ui.label(target_display_name(manager, target_id));
                                    ui.small(target_id);
                                });
                            }
                        });
                }
            },
        );

        ui.menu_button(
            format!("群聊池 ({})", group_chat_targets.len()),
            |ui| {
                if group_chat_targets.is_empty() {
                    ui.label("还没有QQ群聊。");
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("top_group_chat_pool_menu")
                        .max_height(220.0)
                        .show(ui, |ui| {
                            for target_id in &group_chat_targets {
                                ui.horizontal(|ui| {
                                    ui.label(target_display_name(manager, target_id));
                                    ui.small(target_id);
                                });
                            }
                        });
                }
            },
        );

        ui.menu_button(
            format!(
                "随机池 ({})",
                manager.random_pools.len()
            ),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("top_random_pool_menu")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        changed |= random_pool_settings_ui(ui, manager, state, &player_targets);
                    });
            },
        );

        ui.menu_button(
            format!("单位池 ({})", manager.unit_pool.len()),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("top_unit_pool_menu")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        changed |= unit_pool_settings_ui(ui, manager, state, &player_targets);
                    });
            },
        );

        ui.menu_button(
            format!("技能池 ({})", manager.skill_pool.len()),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("top_skill_pool_menu")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        changed |= skill_pool_settings_ui(ui, manager, state);
                    });
            },
        );
    });

    changed
}

impl Widget for CircleImageButton {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(self.size), Sense::click());
        let painter = Painter::new(ui.ctx().clone(), ui.layer_id(), rect);
        painter.add(egui::Shape::Circle(CircleShape {
            center: rect.center(),
            radius: self.size / 2.0,
            fill: Default::default(),
            stroke: Stroke::NONE,
        }));
        painter.add(egui::Shape::image(
            self.image,
            bevy_egui::egui::Rect::from_center_size(rect.center(), Vec2::splat(self.size)),
            bevy_egui::egui::Rect::from_center_size(rect.center(), Vec2::splat(self.size)),
            egui::Color32::WHITE,
        ));
        response
    }
}

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .insert_resource(EguiGlobalSettings {
                auto_create_primary_context: false,
                ..default()
            })
            .add_plugins(ImePlugin)
            .add_plugins(DeepseekPlugin)
            .add_systems(Startup, setup_system)
            .add_systems(
                EguiPrimaryContextPass,
                load_ui_memory.run_if(resource_added::<Persistent<CachedMemory>>),
            )
            .add_systems(
                EguiPrimaryContextPass,
                configure_ui_fonts.after(load_ui_memory),
            )
            .add_systems(
                EguiPrimaryContextPass,
                ui_system
                    .run_if(resource_exists::<Persistent<CachedMemory>>)
                    .after(load_ui_memory),
            );
    }
}

pub fn setup_system(mut command: Commands) {
    let config_dir = Path::new(".data").join("willowblossom");
    let cached_memory = Persistent::<CachedMemory>::builder()
        .name("ui_memory")
        .format(StorageFormat::Ron)
        .path(config_dir.join("ui_memory.ron"))
        .default(CachedMemory {
            ui_memory: Memory::default(),
        })
        .revertible(true)
        .revert_to_default_on_deserialization_errors(true)
        .build()
        .expect("failed to init ui memory");
    command.insert_resource(cached_memory);
    command.insert_resource(GIFImages {
        images: HashMap::default(),
    });
}

pub fn configure_ui_fonts(mut egui_context: EguiContexts, mut fonts_configured: Local<bool>) {
    if *fonts_configured {
        return;
    }

    let Ok(ctx) = egui_context.ctx_mut() else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/AlibabaHealthFont.ttf"
        ))
        .into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "cjk".to_owned());

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (
            egui::TextStyle::Heading,
            egui::FontId::new(18.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Monospace,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Button,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Small,
            egui::FontId::new(10.0, egui::FontFamily::Proportional),
        ),
    ]
    .into();

    ctx.set_fonts(fonts);
    ctx.set_style(style);
    egui_extras::install_image_loaders(ctx);
    *fonts_configured = true;
}

pub fn load_ui_memory(
    mut egui_context: EguiContexts,
    cached_memory: Res<Persistent<CachedMemory>>,
) {
    let Ok(ctx) = egui_context.ctx_mut() else {
        return;
    };
    let mut memory = cached_memory.ui_memory.clone();
    memory.reset_areas();
    ctx.memory_mut(|m| *m = memory);
}

fn chat_window(
    nickname: &str,
    id: Id,
    rect: Rect,
    ctx: &Context,
    _lens: Vec<usize>,
    messages: &Vec<NapcatMessage>,
    napcat_sender: Option<&NapcatIOSender>,
    target_id: &str,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    targets: Vec<NapcatSendTarget>,
    ime: &mut ResMut<ImeManager>,
    chat_scroll_states: &mut Local<HashMap<String, ChatScrollState>>,
    group_rects: &HashMap<String, Rect>,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    current_group: Option<&str>,
    group_delta: Option<Vec2>,
    unread_count: usize,
    quick_character_targets: &mut Local<HashSet<String>>,
    image_textures: &mut Local<HashMap<String, TextureHandle>>,
    focused_trpg_group_name: Option<&str>,
    turn_count_drafts: &mut Local<HashMap<(String, String), u32>>,
    rule_engine_state: &mut RuleEngineState,
    mut player_view_request: Option<&mut ScenePlayerViewRequest>,
) {
    let mut window_open = true;
    let mut leave_group = false;
    let constraint_rect =
        if current_group.is_some() { group_member_constraint_rect(rect) } else { rect };
    let window_min_size = egui::vec2(
        CHAT_WINDOW_MIN_SIZE.x.min(constraint_rect.width().max(1.0)),
        CHAT_WINDOW_MIN_SIZE
            .y
            .min(constraint_rect.height().max(1.0)),
    );
    let max_window_size = if current_group.is_some() {
        egui::vec2(
            GROUP_MEMBER_WINDOW_MAX_SIZE
                .x
                .min(constraint_rect.width())
                .max(window_min_size.x),
            GROUP_MEMBER_WINDOW_MAX_SIZE
                .y
                .min(constraint_rect.height())
                .max(window_min_size.y),
        )
    } else {
        egui::vec2(
            CHAT_WINDOW_MAX_SIZE
                .x
                .min(constraint_rect.width())
                .max(window_min_size.x),
            CHAT_WINDOW_MAX_SIZE
                .y
                .min(constraint_rect.height())
                .max(window_min_size.y),
        )
    };
    let window_id = current_group
        .map(|group_name| {
            Id::new((
                group_name,
                target_id,
                "group_member_chat_window_v2",
            ))
        })
        .unwrap_or_else(|| standalone_chat_window_id(id, target_id));
    let mut window = egui::Window::new(nickname)
        .open(&mut window_open)
        .id(window_id)
        .constrain_to(constraint_rect)
        .default_size(CHAT_WINDOW_SIZE)
        .min_size(window_min_size)
        .max_size(max_window_size)
        .max_height(GROUP_CHAT_MAX_HEIGHT);
    if current_group.is_some() {
        if let Some(delta) = group_delta {
            if let Some(member_rect) = ctx.memory(|memory| memory.area_rect(window_id)) {
                window = window.current_pos(member_rect.min + delta);
            }
        }
        window = window.default_pos(group_member_default_pos(
            constraint_rect,
            target_id,
        ));
    }
    let show_character_button = !is_group_chat_target(manager, target_id);
    let trpg_membership_group = focused_trpg_group_name
        .filter(|group_name| manager.trpg_groups.contains_key(*group_name))
        .map(str::to_owned);
    let target_is_group_chat = is_group_chat_target(manager, target_id);
    let mut trpg_membership_selected = trpg_membership_group
        .as_deref()
        .and_then(|group_name| manager.trpg_groups.get(group_name))
        .map(|group| {
            if target_is_group_chat {
                group.group_chats.contains(&target_id.to_owned())
            } else {
                group.players.contains(&target_id.to_owned())
            }
        })
        .unwrap_or_default();
    let mut trpg_membership_changed = false;
    let trpg_turn_snapshot = trpg_membership_group.as_deref().and_then(|group_name| {
        manager
            .trpg_groups
            .get(group_name)
            .filter(|group| {
                !target_is_group_chat
                    && group.players.iter().any(|player_id| player_id == target_id)
            })
            .map(|group| {
                let turn = group.player_turns.get(target_id);
                (
                    group_name.to_owned(),
                    group.world_turn,
                    turn.map(|turn| turn.turns_passed).unwrap_or_default(),
                    turn.map(|turn| turn.acted).unwrap_or_default(),
                    turn.map(|turn| turn.skipped).unwrap_or_default(),
                )
            })
    });
    let mut player_turn_count_set: Option<(String, String, u32)> = None;
    let mut player_acted_toggle: Option<(String, String, bool)> = None;
    let response = window.show(ctx, |ui| {
        if current_group.is_some() || show_character_button || trpg_membership_group.is_some() {
            ui.horizontal(|ui| {
                if let Some(group_name) = trpg_membership_group.as_deref() {
                    if ui
                        .checkbox(
                            &mut trpg_membership_selected,
                            target_display_name(manager, target_id),
                        )
                        .on_hover_text(format!(
                            "切换在{group_name}中的成员状态"
                        ))
                        .changed()
                    {
                        trpg_membership_changed = true;
                    }
                }
                if show_character_button {
                    if ui.button("角色").clicked() {
                        quick_character_targets.insert(target_id.to_owned());
                    }
                    let can_view_player =
                        target_id.parse::<u64>().is_ok() && player_view_request.is_some();
                    if ui
                        .add_enabled(
                            can_view_player,
                            egui::Button::new("查看玩家视角"),
                        )
                        .on_hover_text("切换到这个玩家的场景捕捉相机，并按其可见性过滤场景")
                        .clicked()
                    {
                        if let (Ok(user_id), Some(request)) = (
                            target_id.parse::<u64>(),
                            player_view_request.as_deref_mut(),
                        ) {
                            request.user_id = Some(user_id);
                        }
                    }
                }
                if let Some((group_name, _, _, acted, _)) = trpg_turn_snapshot.as_ref() {
                    let button_text = if *acted { "已行动" } else { "行动" };
                    if ui.button(button_text).clicked() {
                        player_acted_toggle = Some((
                            group_name.clone(),
                            target_id.to_owned(),
                            !*acted,
                        ));
                    }
                }
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if current_group.is_some()
                            && ui.button("离开").on_hover_text("离开讨论组").clicked()
                        {
                            leave_group = true;
                        }
                    },
                );
            });
            if let Some((group_name, world_turn, turns_passed, acted, skipped)) =
                trpg_turn_snapshot.as_ref()
            {
                ui.horizontal_wrapped(|ui| {
                    let status = if *acted {
                        "已行动"
                    } else if *skipped {
                        "已跳过"
                    } else {
                        "等待中"
                    };
                    ui.small(format!("世界轮次 {world_turn}"));
                    ui.small(format!("玩家轮次 {turns_passed}"));
                    ui.small(status);

                    let draft_key = (group_name.clone(), target_id.to_owned());
                    let draft = turn_count_drafts
                        .entry(draft_key.clone())
                        .or_insert(*turns_passed);
                    ui.add(
                        egui::DragValue::new(draft)
                            .range(0..=9999)
                            .speed(1)
                            .prefix("设为 "),
                    );
                    if ui.button("设置轮次").clicked() {
                        player_turn_count_set = Some((
                            group_name.clone(),
                            target_id.to_owned(),
                            *draft,
                        ));
                    }
                    if ui.small_button("当前").clicked() {
                        *draft = *turns_passed;
                    }
                });
            }
        }
        chat_body_ui(
            ui,
            ctx,
            messages,
            napcat_sender,
            target_id,
            chat_input_msgs,
            targets,
            ime,
            chat_scroll_states,
            image_textures,
            None,
        );
    });

    if let Some((group_name, target_id, turns_passed)) = player_turn_count_set {
        if manager
            .trpg_groups
            .get_mut(&group_name)
            .is_some_and(|group| group.set_player_turns_passed(&target_id, turns_passed))
        {
            manager.persist().ok();
        }
    }

    if let Some((group_name, target_id, acted)) = player_acted_toggle {
        let changed = if acted {
            mark_group_player_turn(
                manager.as_mut(),
                &group_name,
                &target_id,
                true,
                rule_engine_state,
            )
        } else {
            set_group_player_waiting(
                manager.as_mut(),
                &group_name,
                &target_id,
            )
        };
        if changed {
            manager.persist().ok();
        }
    }

    if trpg_membership_changed {
        if let Some(group_name) = trpg_membership_group.as_deref() {
            if let Some(group) = manager.trpg_groups.get_mut(group_name) {
                if target_is_group_chat {
                    set_target_membership(
                        &mut group.group_chats,
                        target_id,
                        trpg_membership_selected,
                    );
                } else {
                    set_target_membership(
                        &mut group.players,
                        target_id,
                        trpg_membership_selected,
                    );
                    group.sync_turn_players();
                }
                manager.persist().ok();
            }
        }
    }

    if let Some(group_name) = current_group {
        ctx.set_sublayer(
            egui::LayerId::new(egui::Order::Middle, Id::new(group_name)),
            egui::LayerId::new(egui::Order::Middle, window_id),
        );
    }

    if current_group.is_some() && !window_open {
        leave_group = true;
    }
    if current_group.is_none() && !window_open {
        manager.open_chat_targets.remove(target_id);
        manager.persist().ok();
        return;
    }
    if let Some(group_name) = current_group {
        if leave_group {
            if let Some(group) = manager.groups.get_mut(group_name) {
                group.members.retain(|member_id| member_id != target_id);
                manager.persist().ok();
            }
            return;
        }
    }

    if let Some(response) = response {
        if current_group.is_none() {
            paint_unread_badge(
                ctx,
                response.response.rect,
                unread_count,
            );
        }

        if window_received_focus(ctx, &response.response) {
            mark_target_read(manager, target_id, messages.len());
        }

        if current_group.is_some() {
            return;
        }

        if let Some(drop_pos) = ctx.input(|input| input.pointer.latest_pos()) {
            if response.response.dragged() {
                if let Some((_, preview_rect)) =
                    group_rects.iter().find(|(_, rect)| rect.contains(drop_pos))
                {
                    draw_drop_preview(ctx, *preview_rect);
                }
            }
        }

        if !response.response.drag_stopped() {
            return;
        }

        let Some(drop_pos) = ctx.input(|input| input.pointer.latest_pos()) else {
            return;
        };

        for (k, rect) in group_rects {
            if rect.contains(drop_pos) {
                let Some(members) = manager.groups.get_mut(k).map(|group| &mut group.members)
                else {
                    continue;
                };
                if !members.contains(&target_id.to_owned()) {
                    members.push(target_id.to_string());
                    manager.persist().ok();
                }
            }
        }
    }
}

fn window_received_focus(ctx: &Context, response: &Response) -> bool {
    response.contains_pointer() && ctx.input(|input| input.pointer.any_pressed())
}

fn standalone_chat_window_id(id: Id, target_id: &str) -> Id {
    Id::new((
        id,
        target_id,
        "standalone_chat_window_v2",
    ))
}

fn focus_standalone_chat_window(ctx: &Context, target_id: &str) {
    let window_id = standalone_chat_window_id(Id::new(target_id), target_id);
    let mut collapsing = egui::collapsing_header::CollapsingState::load_with_default_open(
        ctx,
        window_id.with("collapsing"),
        true,
    );
    collapsing.set_open(true);
    collapsing.store(ctx);
    ctx.move_to_top(egui::LayerId::new(
        egui::Order::Middle,
        window_id,
    ));
    ctx.request_repaint();
}

fn mark_target_read(
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    target_id: &str,
    message_count: usize,
) {
    let current_read_count = manager
        .read_message_counts
        .get(target_id)
        .copied()
        .unwrap_or_default();
    if current_read_count >= message_count {
        return;
    }

    manager
        .read_message_counts
        .insert(target_id.to_owned(), message_count);
    manager.persist().ok();
}

fn group_member_constraint_rect(rect: Rect) -> Rect {
    let min = egui::pos2(
        rect.left() + GROUP_MEMBER_WINDOW_SIDE_GAP,
        rect.top() + GROUP_MEMBER_WINDOW_TOP_GAP,
    );
    let max = egui::pos2(
        rect.right() - GROUP_MEMBER_WINDOW_SIDE_GAP,
        rect.bottom() - GROUP_MEMBER_WINDOW_BOTTOM_GAP,
    );

    if max.x > min.x + CHAT_WINDOW_MIN_SIZE.x && max.y > min.y + CHAT_WINDOW_MIN_SIZE.y {
        Rect::from_min_max(min, max)
    } else {
        rect.shrink2(egui::vec2(
            GROUP_MEMBER_WINDOW_SIDE_GAP.min(rect.width() * 0.25),
            GROUP_MEMBER_WINDOW_SIDE_GAP.min(rect.height() * 0.25),
        ))
    }
}

fn group_member_default_pos(rect: Rect, target_id: &str) -> Pos2 {
    let mut hasher = DefaultHasher::new();
    target_id.hash(&mut hasher);
    let hash = hasher.finish();
    let x_slots = ((rect.width() - CHAT_WINDOW_SIZE.x).max(0.0) / 36.0).floor() as u64 + 1;
    let y_slots = ((rect.height() - CHAT_WINDOW_SIZE.y).max(0.0) / 36.0).floor() as u64 + 1;
    let x = rect.left() + 12.0 + (hash % x_slots) as f32 * 36.0;
    let y = rect.top() + 12.0 + ((hash / 17) % y_slots) as f32 * 36.0;
    egui::pos2(x, y)
}

fn chat_body_ui(
    ui: &mut Ui,
    ctx: &Context,
    messages: &Vec<NapcatMessage>,
    napcat_sender: Option<&NapcatIOSender>,
    target_id: &str,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    targets: Vec<NapcatSendTarget>,
    ime: &mut ResMut<ImeManager>,
    chat_scroll_states: &mut Local<HashMap<String, ChatScrollState>>,
    image_textures: &mut Local<HashMap<String, TextureHandle>>,
    desired_height: Option<f32>,
) {
    ui.vertical(|ui| {
        if !chat_input_msgs.contains_key(target_id) {
            chat_input_msgs.insert(target_id.to_owned(), String::new());
        }

        let input_height = ui.spacing().interact_size.y * 3.0 + ui.spacing().item_spacing.y * 2.0;
        let available_height = desired_height.unwrap_or_else(|| ui.available_height());
        let message_height =
            (available_height - input_height - ui.spacing().item_spacing.y).max(0.0);

        let message_width = ui.available_width();
        ui.allocate_ui(
            egui::vec2(message_width, message_height),
            |ui| {
                let scroll_state = chat_scroll_states
                    .entry(target_id.to_owned())
                    .or_insert_with(|| ChatScrollState {
                        message_count: messages.len(),
                        near_bottom: true,
                    });
                let should_stick_to_bottom =
                    messages.len() > scroll_state.message_count && scroll_state.near_bottom;
                let mut scroll_area = egui::ScrollArea::vertical()
                    .id_salt((target_id, "messages"))
                    .max_height(message_height)
                    .min_scrolled_height(message_height)
                    .auto_shrink([false, false]);
                if should_stick_to_bottom {
                    scroll_area = scroll_area.stick_to_bottom(true);
                }

                let output = scroll_area.show(ui, |ui| {
                    ui.with_layout(
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            for message in messages {
                                message_row_ui(
                                    ui,
                                    message,
                                    message_width,
                                    image_textures,
                                );
                                ui.add_space(ui.spacing().item_spacing.y);
                            }
                        },
                    );
                });

                let max_scroll_y = (output.content_size.y - output.inner_rect.height()).max(0.0);
                let distance_to_bottom = (max_scroll_y - output.state.offset.y).max(0.0);
                scroll_state.message_count = messages.len();
                scroll_state.near_bottom =
                    should_stick_to_bottom || distance_to_bottom <= CHAT_AUTO_SCROLL_THRESHOLD;
            },
        );

        ui.add_space(ui.spacing().item_spacing.y);
        let text = chat_input_msgs.get_mut(target_id).unwrap();
        if let Some(napcat_sender) = napcat_sender {
            let _ = ime.chat_input_multiline(
                target_id,
                text,
                ui.available_width(),
                3,
                ui,
                ctx,
                napcat_sender,
                targets,
            );
        } else {
            ui.add_enabled(
                false,
                egui::TextEdit::multiline(text)
                    .desired_width(ui.available_width())
                    .desired_rows(3),
            );
        }
    });
}

fn group_broadcast_input_ui(
    ui: &mut Ui,
    ctx: &Context,
    group_name: &str,
    members: &[String],
    manager: &NapcatMessageManager,
    napcat_sender: Option<&NapcatIOSender>,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    broadcast_scopes: &mut Local<HashMap<String, String>>,
    ime: &mut ResMut<ImeManager>,
) {
    let input_id = format!("group:{group_name}:broadcast");
    chat_input_msgs
        .entry(input_id.clone())
        .or_insert_with(String::new);

    ui.separator();
    let text = chat_input_msgs.get_mut(&input_id).unwrap();
    let current_group = manager.current_group();
    let scope = broadcast_scopes
        .entry(group_name.to_owned())
        .or_insert_with(|| BROADCAST_SCOPE_ALL.to_owned());
    group_broadcast_scope_ui(
        ui,
        group_name,
        members,
        current_group,
        scope,
    );
    let targets = group_broadcast_targets(current_group, members, manager, scope);

    let enabled = napcat_sender.is_some() && !targets.is_empty();
    if let Some(napcat_sender) = napcat_sender.filter(|_| enabled) {
        let _ = ime.chat_input_multiline(
            &input_id,
            text,
            ui.available_width(),
            GROUP_BROADCAST_INPUT_ROWS,
            ui,
            ctx,
            napcat_sender,
            targets,
        );
    } else {
        ui.add_enabled(
            false,
            egui::TextEdit::multiline(text)
                .desired_width(ui.available_width())
                .desired_rows(GROUP_BROADCAST_INPUT_ROWS),
        );
    }
}

const BROADCAST_SCOPE_ALL: &str = "all";
const BROADCAST_SCOPE_PARTY_PREFIX: &str = "party:";

fn group_broadcast_scope_ui(
    ui: &mut Ui,
    group_name: &str,
    members: &[String],
    current_group: Option<&TrpgGroup>,
    scope: &mut String,
) {
    let mut party_names = current_group
        .map(|group| {
            group
                .parties
                .keys()
                .filter(|party_id| {
                    members.iter().any(|member_id| {
                        group.party_id_for_player(member_id) == Some(party_id.as_str())
                    })
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    party_names.sort();
    if scope != BROADCAST_SCOPE_ALL
        && !party_names
            .iter()
            .any(|party_id| scope == &broadcast_party_scope(party_id))
    {
        *scope = BROADCAST_SCOPE_ALL.to_owned();
    }

    ui.horizontal_wrapped(|ui| {
        ui.label("发送范围");
        egui::ComboBox::from_id_salt((group_name, "broadcast_scope"))
            .selected_text(broadcast_scope_label(scope))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    scope,
                    BROADCAST_SCOPE_ALL.to_owned(),
                    "全部成员",
                );
                for party_name in party_names {
                    ui.selectable_value(
                        scope,
                        broadcast_party_scope(&party_name),
                        format!("小队：{party_name}"),
                    );
                }
            });
    });
}

fn group_broadcast_targets(
    current_group: Option<&TrpgGroup>,
    members: &[String],
    manager: &NapcatMessageManager,
    scope: &str,
) -> Vec<NapcatSendTarget> {
    let requested_party = scope.strip_prefix(BROADCAST_SCOPE_PARTY_PREFIX);
    let mut seen = HashSet::new();
    let mut targets = members
        .iter()
        .filter(|member_id| match requested_party {
            Some(party_id) => {
                current_group.and_then(|group| group.party_id_for_player(member_id))
                    == Some(party_id)
            },
            None => true,
        })
        .filter_map(|member_id| private_broadcast_target(manager, member_id))
        .filter(|target| match target {
            NapcatSendTarget::Private(user_id) => seen.insert(*user_id),
            NapcatSendTarget::Group(_) => false,
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|target| match target {
        NapcatSendTarget::Private(user_id) => *user_id,
        NapcatSendTarget::Group(group_id) => *group_id,
    });
    targets
}

fn private_broadcast_target(
    manager: &NapcatMessageManager,
    member_id: &str,
) -> Option<NapcatSendTarget> {
    if !matches!(
        manager
            .messages
            .get(member_id)
            .and_then(|messages| messages.first())
            .map(|message| &message.data.message_type),
        Some(NapcatMessageType::Private)
    ) {
        return None;
    }

    match member_id.parse::<u64>() {
        Ok(user_id) => Some(NapcatSendTarget::Private(user_id)),
        Err(_) => {
            eprintln!("invalid NapCat group member id: {member_id}");
            None
        },
    }
}

fn broadcast_party_scope(party_id: &str) -> String {
    format!("{BROADCAST_SCOPE_PARTY_PREFIX}{party_id}")
}

fn broadcast_scope_label(scope: &str) -> String {
    scope
        .strip_prefix(BROADCAST_SCOPE_PARTY_PREFIX)
        .map(|party_id| format!("小队：{party_id}"))
        .unwrap_or_else(|| "全部成员".to_owned())
}

fn group_drop_area_ui(ui: &mut Ui, group_name: &str, members: &[String]) {
    let body_height =
        (ui.available_height() - GROUP_BROADCAST_INPUT_HEIGHT - ui.spacing().item_spacing.y)
            .max(GROUP_CHAT_MIN_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), body_height),
        Sense::hover(),
    );

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(
                1.0,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ),
            egui::epaint::StrokeKind::Inside,
        );
        if members.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("拖入聊天到讨论组 {group_name}"),
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
        }
    }
}

fn message_row_ui(
    ui: &mut Ui,
    message: &NapcatMessage,
    row_width: f32,
    image_textures: &mut Local<HashMap<String, TextureHandle>>,
) {
    let is_self = message.data.self_id == message.data.user_id;
    let max_message_width = if row_width < 120.0 {
        row_width
    } else {
        (row_width * 0.72).clamp(120.0, row_width)
    };
    let margin_width = (row_width - max_message_width).max(0.0);

    ui.horizontal_top(|ui| {
        ui.set_width(row_width);
        if is_self {
            ui.add_space(margin_width);
            ui.vertical(|ui| {
                ui.set_width(max_message_width);
                ui.set_max_width(max_message_width);
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::RIGHT),
                    |ui| {
                        message_text_ui(ui, message, image_textures);
                    },
                );
            });
        } else {
            ui.vertical(|ui| {
                ui.set_width(max_message_width);
                ui.set_max_width(max_message_width);
                message_text_ui(ui, message, image_textures);
            });
            ui.add_space(margin_width);
        }
    });
}

fn message_text_ui(
    ui: &mut Ui,
    message: &NapcatMessage,
    image_textures: &mut Local<HashMap<String, TextureHandle>>,
) {
    ui.label(&message.data.sender.nickname);
    for chain in &message.data.message {
        match &chain.variant {
            NapcatMessageChainType::Text { data: text_data } => {
                ui.add(
                    egui::Label::new(text_data.text.trim())
                        .wrap()
                        .selectable(false),
                );
            },
            NapcatMessageChainType::Image { data } => {
                message_image_ui(ui, data, image_textures);
            },
            NapcatMessageChainType::Source(_) => {},
            NapcatMessageChainType::Unsupported => {},
        }
    }
}

fn message_image_ui(
    ui: &mut Ui,
    data: &ImageData,
    image_textures: &mut Local<HashMap<String, TextureHandle>>,
) {
    let Some(path) = cached_image_path(data.local_path.trim()) else {
        ui.label("[图片]");
        if !data.url.trim().is_empty() {
            ui.small("图片URL不可用");
        }
        return;
    };

    let texture = if let Some(texture) = image_textures.get(&path) {
        texture.clone()
    } else {
        let Some(color_image) = load_cached_color_image(&path) else {
            ui.label("[图片]");
            ui.small("缓存图片解码失败");
            return;
        };
        let texture = ui.ctx().load_texture(
            format!("chat-image:{path}"),
            color_image,
            egui::TextureOptions::LINEAR,
        );
        image_textures.insert(path.clone(), texture.clone());
        texture
    };

    let max_width = ui.available_width().min(CHAT_IMAGE_MAX_SIZE.x).max(1.0);
    let size = fit_image_size(
        texture.size_vec2(),
        Vec2::new(max_width, CHAT_IMAGE_MAX_SIZE.y),
    );
    ui.add(egui::Image::from_texture((texture.id(), size)).corner_radius(4))
        .on_hover_text(data.url.trim());
}

fn cached_image_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    let path = Path::new(path);
    if !path.exists() {
        return None;
    }

    Some(
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string(),
    )
}

fn load_cached_color_image(path: &str) -> Option<egui::ColorImage> {
    let bytes = fs::read(path).ok()?;
    let image = ::image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()))
}

fn fit_image_size(original: Vec2, max_size: Vec2) -> Vec2 {
    if original.x <= 0.0 || original.y <= 0.0 {
        return max_size;
    }

    let scale = (max_size.x / original.x)
        .min(max_size.y / original.y)
        .min(1.0);
    original * scale
}

pub fn get_nickname_lens(target_id: String, messages: &Vec<NapcatMessage>) -> (&str, Vec<usize>) {
    let mut nickname = "";
    let mut lens: Vec<usize> = vec![];
    for message in messages {
        let mut len: usize = 0;
        for chain in &message.data.message {
            match &chain.variant {
                NapcatMessageChainType::Source(_) => {},
                NapcatMessageChainType::Text { data } => {
                    len += data.text.len();
                },
                NapcatMessageChainType::Image { .. } => {
                    len += 12;
                },
                NapcatMessageChainType::Unsupported => {},
            };
        }

        if message.data.sender.user_id.to_string() == *target_id {
            nickname = &message.data.sender.nickname;
        }
        lens.push(len)
    }

    (nickname, lens)
}

fn target_default_display_name(target_id: &str, messages: Option<&Vec<NapcatMessage>>) -> String {
    messages
        .map(|messages| get_nickname_lens(target_id.to_owned(), messages).0)
        .filter(|nickname| !nickname.is_empty())
        .unwrap_or(target_id)
        .to_owned()
}

fn target_display_name(manager: &NapcatMessageManager, target_id: &str) -> String {
    if let Some(display_name) = manager
        .chat_targets
        .get(target_id)
        .map(|metadata| metadata.display_name.trim())
        .filter(|display_name| !display_name.is_empty() && *display_name != target_id)
        .map(str::to_owned)
    {
        return display_name;
    }

    if let Some(automatic_name) = manager
        .chat_targets
        .get(target_id)
        .map(|metadata| metadata.automatic_name.trim())
        .filter(|automatic_name| !automatic_name.is_empty())
        .map(str::to_owned)
    {
        return automatic_name;
    }

    target_default_display_name(
        target_id,
        manager.messages.get(target_id),
    )
}

fn chat_group_title(group_name: &str, group: &ChatGroup, manager: &NapcatMessageManager) -> String {
    let member_names = group
        .members
        .iter()
        .map(|member_id| target_display_name(manager, member_id))
        .collect::<Vec<_>>();

    if member_names.is_empty() {
        format!("讨论组: {group_name}")
    } else {
        format!(
            "讨论组: {}: {}",
            group_name,
            member_names.join(", ")
        )
    }
}

fn target_unread_count(manager: &NapcatMessageManager, target_id: &str) -> usize {
    let read_count = manager
        .read_message_counts
        .get(target_id)
        .copied()
        .unwrap_or_default();

    manager
        .messages
        .get(target_id)
        .map(|messages| {
            messages
                .iter()
                .skip(read_count)
                .filter(|message| message.data.user_id != message.data.self_id)
                .count()
        })
        .unwrap_or_default()
}

fn chat_group_unread_count(manager: &NapcatMessageManager, group: &ChatGroup) -> usize {
    group
        .members
        .iter()
        .map(|member_id| target_unread_count(manager, member_id))
        .sum()
}

fn group_chat_inner_size(member_count: usize, max_rect: Rect) -> Vec2 {
    let desired_height = if member_count == 0 {
        GROUP_CHAT_MIN_HEIGHT + GROUP_BROADCAST_INPUT_HEIGHT
    } else {
        member_count as f32 * GROUP_MEMBER_CHAT_SIZE.y
            + member_count.saturating_sub(1) as f32 * GROUP_CHAT_SEPARATOR_HEIGHT
            + GROUP_BROADCAST_INPUT_HEIGHT
    };

    egui::vec2(
        (GROUP_MEMBER_CHAT_SIZE.x + 48.0)
            .min(max_rect.width())
            .max(CHAT_WINDOW_MIN_SIZE.x),
        desired_height
            .min(GROUP_CHAT_MAX_HEIGHT)
            .min(max_rect.height())
            .max(GROUP_CHAT_MIN_HEIGHT),
    )
}

fn group_chat_max_size(max_rect: Rect) -> Vec2 {
    egui::vec2(
        GROUP_CHAT_MAX_WIDTH
            .min(max_rect.width())
            .max(CHAT_WINDOW_MIN_SIZE.x),
        GROUP_CHAT_MAX_HEIGHT
            .min(max_rect.height())
            .max(CHAT_WINDOW_MIN_SIZE.y),
    )
}

fn draw_drop_preview(ctx: &Context, rect: Rect) {
    let rect = rect.shrink(4.0);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        Id::new("chat_drop_preview"),
    ));
    let fill = egui::Color32::from_rgba_unmultiplied(60, 210, 120, 28);
    let stroke = Stroke::new(
        2.0,
        egui::Color32::from_rgb(70, 220, 130),
    );

    painter.rect_filled(rect, 4.0, fill);
    paint_dotted_line(
        &painter,
        rect.left_top(),
        rect.right_top(),
        stroke,
    );
    paint_dotted_line(
        &painter,
        rect.right_top(),
        rect.right_bottom(),
        stroke,
    );
    paint_dotted_line(
        &painter,
        rect.right_bottom(),
        rect.left_bottom(),
        stroke,
    );
    paint_dotted_line(
        &painter,
        rect.left_bottom(),
        rect.left_top(),
        stroke,
    );
}

fn paint_unread_badge(ctx: &Context, window_rect: Rect, unread_count: usize) {
    if unread_count == 0 {
        return;
    }

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        Id::new((
            "unread_badge",
            window_rect.min.x.to_bits(),
            window_rect.min.y.to_bits(),
        )),
    ));
    let badge_text = if unread_count > 99 { "99+".to_owned() } else { unread_count.to_string() };
    let radius = if unread_count > 99 { 11.0 } else { 9.0 };
    let center = Pos2::new(
        window_rect.right() - 18.0,
        window_rect.top() + 16.0,
    );

    painter.circle_filled(
        center,
        radius,
        egui::Color32::from_rgb(235, 55, 55),
    );
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        badge_text,
        egui::FontId::proportional(10.0),
        egui::Color32::WHITE,
    );
}

fn paint_dotted_line(painter: &Painter, start: Pos2, end: Pos2, stroke: Stroke) {
    let line = end - start;
    let length = line.length();
    if length <= 0.0 {
        return;
    }

    let direction = line / length;
    let dash = 7.0;
    let gap = 5.0;
    let mut offset = 0.0;

    while offset < length {
        let segment_end = (offset + dash).min(length);
        painter.line_segment(
            [start + direction * offset, start + direction * segment_end],
            stroke,
        );
        offset += dash + gap;
    }
}

struct PlayerTextLine {
    player_message_count: usize,
    text: String,
    summary_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SummaryScope {
    Private,
    GroupPublic,
    GroupParty(String),
}

impl SummaryScope {
    fn summary_key(&self, target_id: &str) -> String {
        match self {
            SummaryScope::Private => target_id.to_owned(),
            SummaryScope::GroupPublic => format!("group:{target_id}:public"),
            SummaryScope::GroupParty(party_id) => format!("group:{target_id}:party:{party_id}"),
        }
    }

    fn label(&self) -> String {
        match self {
            SummaryScope::Private => "私聊".to_owned(),
            SummaryScope::GroupPublic => "公开".to_owned(),
            SummaryScope::GroupParty(party_id) => format!("小队：{party_id}"),
        }
    }
}

fn player_text_lines(messages: &[CampaignMessage]) -> Vec<PlayerTextLine> {
    let mut player_message_count = 0;
    let mut lines = Vec::new();

    for message in messages {
        let text = message.text.trim();

        if text.is_empty() {
            continue;
        }

        player_message_count += 1;
        lines.push(PlayerTextLine {
            player_message_count,
            summary_eligible: !matches!(
                text.trim(),
                "#观察" | "#gc" | ".观察" | ".gc"
            ),
            text: format!("{}: {}", message.sender_name, text),
        });
    }

    lines
}

fn queue_summaries_if_needed(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
    summarized_message_counts: &HashMap<String, usize>,
    deepseek_sender: Option<&DeepseekIOSender>,
    deepseek_manager: &mut DeepseekManager,
) -> bool {
    let mut changed = false;
    for scope in summary_scopes_for_target(manager, target_id, messages) {
        let summary_key = scope.summary_key(target_id);
        let summarized_message_count = summarized_message_counts
            .get(&summary_key)
            .copied()
            .unwrap_or_default();
        changed |= queue_summary_if_needed_for_scope(
            manager,
            target_id,
            messages,
            &scope,
            &summary_key,
            summarized_message_count,
            deepseek_sender,
            deepseek_manager,
        );
    }
    changed
}

fn queue_summary_if_needed_for_scope(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
    scope: &SummaryScope,
    summary_key: &str,
    summarized_message_count: usize,
    deepseek_sender: Option<&DeepseekIOSender>,
    deepseek_manager: &mut DeepseekManager,
) -> bool {
    let campaign_messages =
        campaign_messages_for_summary_scope(manager, target_id, messages, scope);
    let lines = player_text_lines(&campaign_messages);
    let message_count = lines
        .last()
        .map(|line| line.player_message_count)
        .unwrap_or_default();
    if message_count == 0 || message_count < summarized_message_count + 5 {
        return false;
    }
    if summarized_message_count >= message_count {
        return false;
    }

    if let Some(summary) = deepseek_manager.summaries.get(summary_key) {
        if summary
            .blocks
            .iter()
            .any(|block| block.message_count == message_count)
        {
            return false;
        }
    }

    let Some(deepseek_sender) = deepseek_sender else {
        return false;
    };

    let text = lines
        .iter()
        .filter(|line| {
            line.player_message_count > summarized_message_count && line.summary_eligible
        })
        .map(|line| line.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return false;
    }

    let request = DeepseekRequest::Summary {
        target_id: summary_key.to_owned(),
        message_count,
        text,
    };

    let send_result = serde_json::to_string(&request)
        .map(|request| Message::Text(request.into()))
        .map_err(|err| err.to_string())
        .and_then(|request| {
            deepseek_sender
                .0
                .try_send(request)
                .map_err(|err| err.to_string())
        });

    match send_result {
        Ok(()) => {
            deepseek_manager
                .summaries
                .entry(summary_key.to_owned())
                .or_default()
                .upsert_block(DeepseekSummaryBlock {
                    latest: String::new(),
                    message_count,
                    pending: true,
                    error: None,
                });
            true
        },
        Err(error) => {
            deepseek_manager
                .summaries
                .entry(summary_key.to_owned())
                .or_default()
                .upsert_block(DeepseekSummaryBlock {
                    latest: String::new(),
                    message_count,
                    pending: false,
                    error: Some(error),
                });
            true
        },
    }
}

fn summary_scopes_for_target(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
) -> Vec<SummaryScope> {
    if !matches!(
        messages.first().map(|message| &message.data.message_type),
        Some(NapcatMessageType::Group)
    ) {
        return vec![SummaryScope::Private];
    }

    let mut party_ids = BTreeSet::new();
    if let Some(group) = manager.current_group().filter(|group| {
        group
            .group_chats
            .iter()
            .any(|group_id| group_id == target_id)
    }) {
        party_ids.extend(group.parties.keys().cloned());
    }

    for message in messages {
        if message.data.user_id == message.data.self_id {
            continue;
        }
        if let Visibility::Party(party_id) = manager
            .campaign_message_for_target(target_id, message)
            .visibility
        {
            party_ids.insert(party_id);
        }
    }

    let mut scopes = vec![SummaryScope::GroupPublic];
    scopes.extend(party_ids.into_iter().map(SummaryScope::GroupParty));
    scopes
}

fn campaign_messages_for_summary_scope(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
    scope: &SummaryScope,
) -> Vec<CampaignMessage> {
    match scope {
        SummaryScope::Private => manager.visible_campaign_messages_for_summary(target_id, messages),
        SummaryScope::GroupPublic => messages
            .iter()
            .filter(|message| message.data.user_id != message.data.self_id)
            .map(|message| manager.campaign_message_for_target(target_id, message))
            .filter(|message| matches!(message.visibility, Visibility::Public))
            .collect(),
        SummaryScope::GroupParty(party_id) => messages
            .iter()
            .filter(|message| message.data.user_id != message.data.self_id)
            .map(|message| manager.campaign_message_for_target(target_id, message))
            .filter(|message| {
                matches!(message.visibility, Visibility::Public)
                    || matches!(&message.visibility, Visibility::Party(message_party) if message_party == party_id)
            })
            .collect(),
    }
}

fn sync_summarized_message_counts(
    manager: &mut NapcatMessageManager,
    deepseek_manager: &DeepseekManager,
) -> bool {
    let mut changed = false;

    for (target_id, summary) in &deepseek_manager.summaries {
        let latest_summarized_count = summary
            .blocks
            .iter()
            .filter(|block| !block.pending && block.error.is_none())
            .map(|block| block.message_count)
            .max()
            .unwrap_or_default();

        if latest_summarized_count == 0 {
            continue;
        }

        let entry = manager
            .summarized_message_counts
            .entry(target_id.clone())
            .or_default();
        if *entry < latest_summarized_count {
            *entry = latest_summarized_count;
            changed = true;
        }
    }

    changed
}

fn summary_display_parts<'a>(
    manager: &NapcatMessageManager,
    summary_key: &'a str,
) -> (String, String) {
    let (target_id, scope) = parse_group_summary_key(summary_key)
        .map(|(target_id, scope)| (target_id, scope.label()))
        .unwrap_or_else(|| {
            let scope = if manager.messages.get(summary_key).is_some_and(|messages| {
                matches!(
                    messages.first().map(|message| &message.data.message_type),
                    Some(NapcatMessageType::Group)
                )
            }) {
                "全部（旧）".to_owned()
            } else {
                "私聊".to_owned()
            };
            (summary_key, scope)
        });
    let display_name = manager
        .messages
        .get(target_id)
        .map(|messages| {
            get_nickname_lens(target_id.to_string(), messages)
                .0
                .to_owned()
        })
        .filter(|nickname| !nickname.is_empty())
        .unwrap_or_else(|| target_id.to_string());

    (display_name, scope)
}

fn parse_group_summary_key(summary_key: &str) -> Option<(&str, SummaryScope)> {
    let rest = summary_key.strip_prefix("group:")?;
    let mut parts = rest.splitn(3, ':');
    let target_id = parts.next()?;
    let scope_kind = parts.next()?;
    match scope_kind {
        "public" => Some((target_id, SummaryScope::GroupPublic)),
        "party" => Some((
            target_id,
            SummaryScope::GroupParty(parts.next()?.to_owned()),
        )),
        _ => None,
    }
}

fn summary_panel(ui: &mut Ui, manager: &NapcatMessageManager, deepseek_manager: &DeepseekManager) {
    ui.heading("DeepSeek 总结");
    ui.separator();

    if deepseek_manager.summaries.is_empty() {
        ui.label("暂无总结");
        ui.allocate_rect(
            ui.available_rect_before_wrap(),
            egui::Sense::hover(),
        );
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut summaries = deepseek_manager.summaries.iter().collect::<Vec<_>>();
        summaries.sort_by_key(|(target_id, _)| target_id.as_str());

        for (target_id, summary) in summaries {
            let (nickname, scope) = summary_display_parts(manager, target_id);

            ui.group(|ui| {
                ui.label(format!(
                    "{} / {} / {} 个总结",
                    nickname,
                    scope,
                    summary.blocks.len()
                ));
                for block in &summary.blocks {
                    let start = block.message_count.saturating_sub(4);
                    ui.separator();
                    ui.label(format!(
                        "{}-{} 条",
                        start, block.message_count
                    ));
                    if block.pending {
                        ui.label("总结中...");
                    } else if let Some(error) = &block.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    } else {
                        ui.label(block.latest.trim());
                    }
                }
            });
        }
    });
}

fn pending_chat_requests_window(
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    napcat_sender: Option<&NapcatIOSender>,
    ime: &mut ResMut<ImeManager>,
) {
    if manager.pending_chat_targets.is_empty() {
        return;
    }

    let mut pending_targets = manager
        .pending_chat_targets
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    pending_targets.sort();

    let mut changed = false;
    egui::Window::new("新的聊天请求")
        .id(Id::new("pending_chat_requests_window"))
        .default_pos(Pos2::new(16.0, 48.0))
        .default_size(Vec2::new(300.0, 120.0))
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label("NapCat收到了还没有打开窗口的聊天消息。");
            ui.separator();

            for target_id in pending_targets {
                let display_name = target_display_name(manager, &target_id);
                ui.horizontal(|ui| {
                    ui.label(display_name);
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.button("创建聊天").clicked() {
                                if manager.approve_chat_target(&target_id) {
                                    changed = true;
                                    if let (Some(sender), Some(text), Some(target)) = (
                                        napcat_sender,
                                        approval_onboarding_text(manager, &target_id),
                                        private_broadcast_target(manager, &target_id),
                                    ) {
                                        if let Err(err) =
                                            ime.queue_text_send(&target_id, text, sender, vec![
                                                target,
                                            ])
                                        {
                                            eprintln!(
                                                "failed to queue NapCat onboarding message: {err}"
                                            );
                                        }
                                    }
                                }
                            }
                            if ui.button("拒绝").clicked() {
                                changed |= manager.reject_chat_target(&target_id);
                            }
                        },
                    );
                });
            }
        });

    if changed {
        manager.persist().ok();
    }
}

fn approval_onboarding_text(manager: &NapcatMessageManager, target_id: &str) -> Option<String> {
    if !matches!(
        manager
            .messages
            .get(target_id)
            .and_then(|messages| messages.first())
            .map(|message| &message.data.message_type),
        Some(NapcatMessageType::Private)
    ) {
        return None;
    }

    let group = manager.current_group()?;
    if !group.players.iter().any(|player_id| player_id == target_id) {
        return None;
    }

    let guide = group.guide.trim();
    if guide.is_empty() {
        None
    } else {
        Some(format!("团内引导：\n{guide}"))
    }
}

fn waiting_turn_manager_window(
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
) {
    let Some(group_name) = manager.current_trpg_group.clone() else {
        return;
    };
    let Some(group) = manager.trpg_groups.get(&group_name) else {
        return;
    };

    let waiting_players = group
        .players
        .iter()
        .filter(|target_id| {
            group
                .player_turns
                .get(*target_id)
                .map(|turn| !turn.acted && !turn.skipped)
                .unwrap_or(true)
        })
        .map(|target_id| {
            (
                target_id.clone(),
                target_display_name(manager, target_id),
            )
        })
        .collect::<Vec<_>>();

    let mut target_to_focus = None;
    egui::Window::new("轮次管理")
        .id(Id::new("waiting_turn_manager_window"))
        .default_pos(Pos2::new(240.0, 48.0))
        .default_size(Vec2::new(240.0, 220.0))
        .min_size(Vec2::new(180.0, 120.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(group_name.as_str());
                ui.small(format!(
                    "{}人等待中",
                    waiting_players.len()
                ));
            });
            ui.separator();

            if waiting_players.is_empty() {
                ui.label("所有玩家都已行动。");
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("waiting_turn_manager_players")
                .show(ui, |ui| {
                    for (target_id, display_name) in &waiting_players {
                        if ui.button(display_name).on_hover_text(target_id).clicked() {
                            target_to_focus = Some(target_id.clone());
                        }
                    }
                });
        });

    if let Some(target_id) = target_to_focus {
        let opened = manager.open_chat_targets.insert(target_id.clone());
        manager.pending_chat_targets.remove(&target_id);
        focus_standalone_chat_window(ctx, &target_id);
        if opened {
            manager.persist().ok();
        }
    }
}

fn chat_target_kind(messages: Option<&Vec<NapcatMessage>>) -> &'static str {
    match messages.and_then(|messages| messages.first()) {
        Some(message)
            if matches!(
                message.data.message_type,
                NapcatMessageType::Group
            ) =>
        {
            "群"
        },
        Some(_) => "私聊",
        None => "聊天",
    }
}

fn is_group_chat_target(manager: &NapcatMessageManager, target_id: &str) -> bool {
    matches!(
        manager
            .messages
            .get(target_id)
            .and_then(|messages| messages.first())
            .map(|message| &message.data.message_type),
        Some(NapcatMessageType::Group)
    )
}

fn sorted_pool_targets(manager: &NapcatMessageManager, group_chats: bool) -> Vec<String> {
    let mut targets = manager
        .messages
        .keys()
        .filter(|target_id| is_group_chat_target(manager, target_id) == group_chats)
        .cloned()
        .collect::<Vec<_>>();
    targets.sort_by(|a, b| target_display_name(manager, a).cmp(&target_display_name(manager, b)));
    targets
}

fn trpg_group_member_count(group: &TrpgGroup) -> usize {
    group.players.len() + group.group_chats.len()
}

fn chat_list_panel(
    ui: &mut Ui,
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    edit_target: &mut Option<String>,
    edit_name: &mut String,
    trpg_group_settings: &mut TrpgGroupSettingsState,
) {
    ui.heading("TRPG组");
    ui.add_space(4.0);

    let mut trpg_group_names = manager.trpg_groups.keys().cloned().collect::<Vec<_>>();
    trpg_group_names.sort();
    if trpg_group_names.is_empty() {
        ui.label("还没有TRPG组。");
    } else {
        for group_name in trpg_group_names {
            let Some(group) = manager.trpg_groups.get(&group_name).cloned() else {
                continue;
            };
            let unread_count = group
                .players
                .iter()
                .chain(group.group_chats.iter())
                .map(|target_id| target_unread_count(manager, target_id))
                .sum::<usize>();

            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(&group_name);
                    if unread_count > 0 {
                        ui.label(format!("({unread_count})"));
                    }
                });
                ui.small(format!(
                    "{}名玩家，{}个群聊，第{}轮",
                    group.players.len(),
                    group.group_chats.len(),
                    group.world_turn
                ));
                if ui.button("打开工作区").clicked() {
                    trpg_group_settings.open = true;
                    trpg_group_settings.focused_group_name = Some(group_name.clone());
                }
            });
            ui.add_space(4.0);
        }
    }

    ui.separator();
    ui.heading("聊天");
    ui.add_space(4.0);

    if manager.messages.is_empty() {
        ui.label("还没有保存的聊天。");
        return;
    }

    let mut targets = manager.messages.keys().cloned().collect::<Vec<_>>();
    targets.sort_by(|a, b| {
        let a_time = manager
            .messages
            .get(a)
            .and_then(|messages| messages.last())
            .map(|message| message.data.time)
            .unwrap_or_default();
        let b_time = manager
            .messages
            .get(b)
            .and_then(|messages| messages.last())
            .map(|message| message.data.time)
            .unwrap_or_default();
        b_time.cmp(&a_time).then_with(|| a.cmp(b))
    });

    let mut changed = false;
    egui::ScrollArea::vertical()
        .id_salt("chat_list_panel_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for target_id in targets {
                let display_name = target_display_name(manager, &target_id);
                let unread_count = target_unread_count(manager, &target_id);
                let is_open = manager.open_chat_targets.contains(&target_id);
                let is_editing = edit_target.as_deref() == Some(target_id.as_str());

                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(is_open, display_name)
                            .on_hover_text(&target_id)
                            .clicked()
                        {
                            manager.open_chat_targets.insert(target_id.clone());
                            manager.pending_chat_targets.remove(&target_id);
                            focus_standalone_chat_window(ctx, &target_id);
                            changed = true;
                        }

                        if unread_count > 0 {
                            ui.label(format!("({unread_count})"));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.small(chat_target_kind(
                            manager.messages.get(&target_id),
                        ));
                        ui.small(&target_id);
                    });

                    if is_editing {
                        ui.text_edit_singleline(edit_name);
                        ui.horizontal(|ui| {
                            if ui.button("保存").clicked() {
                                manager
                                    .chat_targets
                                    .entry(target_id.clone())
                                    .or_default()
                                    .display_name = edit_name.trim().to_owned();
                                *edit_target = None;
                                edit_name.clear();
                                changed = true;
                            }
                            if ui.button("取消").clicked() {
                                *edit_target = None;
                                edit_name.clear();
                            }
                            if ui.button("清除").clicked() {
                                if let Some(metadata) = manager.chat_targets.get_mut(&target_id) {
                                    metadata.display_name.clear();
                                }
                                *edit_target = None;
                                edit_name.clear();
                                changed = true;
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            if ui.button("编辑").clicked() {
                                *edit_target = Some(target_id.clone());
                                *edit_name = manager
                                    .chat_targets
                                    .get(&target_id)
                                    .map(|metadata| metadata.display_name.clone())
                                    .filter(|name| !name.trim().is_empty())
                                    .unwrap_or_else(|| target_display_name(manager, &target_id));
                            }
                            let close_label = if is_open { "关闭" } else { "打开" };
                            if ui.button(close_label).clicked() {
                                if is_open {
                                    manager.open_chat_targets.remove(&target_id);
                                } else {
                                    manager.open_chat_targets.insert(target_id.clone());
                                    manager.pending_chat_targets.remove(&target_id);
                                    focus_standalone_chat_window(ctx, &target_id);
                                }
                                changed = true;
                            }
                        });
                    }
                });
                ui.add_space(4.0);
            }
        });

    if changed {
        manager.persist().ok();
    }
}

fn set_target_membership(targets: &mut Vec<String>, target_id: &str, selected: bool) {
    if selected {
        if !targets.iter().any(|existing| existing == target_id) {
            targets.push(target_id.to_owned());
        }
    } else {
        targets.retain(|existing| existing != target_id);
    }
}

fn character_status_summary_ui(ui: &mut Ui, character: &PlayerCharacter) {
    let display_name = if character.nickname.trim().is_empty() {
        character.name.trim()
    } else {
        character.nickname.trim()
    };
    let display_name = if display_name.is_empty() { "未命名角色" } else { display_name };
    let state_label = if character.inited {
        "已完成"
    } else {
        character_creation_step_label(character.creation_step)
    };

    ui.horizontal_wrapped(|ui| {
        ui.strong(display_name);
        ui.small(state_label);
        ui.small(format!(
            "HP {}/{} [{}]",
            format_character_number(character.hp),
            format_character_number(character.max_hp),
            character_hp_status(character.hp, character.max_hp)
        ));
        ui.small(format!(
            "MP {}/{}",
            format_character_number(character.mp),
            format_character_number(character.max_mp)
        ));
        ui.small(format!("Lv {}", character.level));
        ui.small(format!(
            "速度 {}",
            format_character_number(character.speed)
        ));
    });

    egui::Grid::new(ui.next_auto_id())
        .num_columns(4)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            status_summary_value_ui(
                ui,
                "STR",
                character.status.str_,
                character.extra_status.str_,
            );
            status_summary_value_ui(
                ui,
                "AGI",
                character.status.agi,
                character.extra_status.agi,
            );
            status_summary_value_ui(
                ui,
                "DEX",
                character.status.dex,
                character.extra_status.dex,
            );
            status_summary_value_ui(
                ui,
                "VIT",
                character.status.vit,
                character.extra_status.vit,
            );
            ui.end_row();
            status_summary_value_ui(
                ui,
                "INT",
                character.status.int_,
                character.extra_status.int_,
            );
            status_summary_value_ui(
                ui,
                "WIS",
                character.status.wis,
                character.extra_status.wis,
            );
            status_summary_value_ui(
                ui,
                "K",
                character.status.k,
                character.extra_status.k,
            );
            status_summary_value_ui(
                ui,
                "CHA",
                character.status.cha,
                character.extra_status.cha,
            );
            ui.end_row();
        });
}

#[derive(Clone)]
struct QuickCastSkill {
    index: usize,
    name: String,
    note: String,
    mp_cost: f32,
    cooldown_turns: u32,
}

#[derive(Clone, Copy)]
enum QuickCastEffect {
    Damage { amount: f32, target: TargetSelector },
    Heal { amount: f32, target: TargetSelector },
}

struct QuickCastAction {
    caster_id: String,
    skill: QuickCastSkill,
    targets: Vec<String>,
    effect: Option<QuickCastEffect>,
    cast_turn: u32,
    force: bool,
}

fn quick_character_windows(
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    quick_character_targets: &mut Local<HashSet<String>>,
    character_edit_state: &mut CharacterEditState,
    rule_engine_state: &mut RuleEngineState,
    scene_positions: Option<&SceneCharacterPositions>,
    player_camera_positions: Option<&ScenePlayerCameraPositions>,
) {
    let mut target_ids = quick_character_targets.iter().cloned().collect::<Vec<_>>();
    target_ids.sort();

    let mut closed_targets = Vec::new();
    for target_id in target_ids {
        if is_group_chat_target(manager, &target_id) {
            closed_targets.push(target_id);
            continue;
        }

        let display_name = target_display_name(manager, &target_id);
        let character_targets = quick_cast_character_targets(manager);
        let cast_turn = quick_cast_cooldown_turn(manager, &target_id);
        let mut open = true;
        let mut changed = false;
        let mut cast_action = None;
        let window_max_width = ctx
            .content_rect()
            .width()
            .min(CHARACTER_WINDOW_MAX_WIDTH)
            .max(CHARACTER_WINDOW_MIN_WIDTH);
        egui::Window::new(format!("角色：{display_name}"))
            .id(Id::new((
                "quick_character_window",
                target_id.as_str(),
            )))
            .open(&mut open)
            .default_width(CHARACTER_WINDOW_DEFAULT_WIDTH)
            .min_width(CHARACTER_WINDOW_MIN_WIDTH)
            .max_width(window_max_width)
            .resizable(true)
            .show(ctx, |ui| {
                ui.set_max_width(window_max_width);
                ui.horizontal(|ui| {
                    ui.small("玩家");
                    ui.monospace(&target_id);
                });
                let skill_pool_snapshot = manager.skill_pool.clone();
                let stat_config = manager.character_stat_config_for_target(&target_id);
                let character = manager
                    .player_characters
                    .entry(target_id.clone())
                    .or_default();
                character_status_summary_ui(ui, character);
                ui.separator();
                cast_action = quick_cast_ui(
                    ui,
                    &target_id,
                    character,
                    character_edit_state,
                    &character_targets,
                    cast_turn,
                    scene_positions,
                    player_camera_positions,
                );
                ui.separator();
                ui.collapsing("编辑角色", |ui| {
                    changed |= character_editor_ui(
                        ui,
                        &target_id,
                        character,
                        &display_name,
                        character_edit_state,
                        rule_engine_state,
                        &skill_pool_snapshot,
                        stat_config,
                    );
                });
            });

        if !open {
            closed_targets.push(target_id);
        }
        if let Some(action) = cast_action {
            changed |= apply_quick_cast_action(manager, action);
        }
        if changed {
            manager.persist().ok();
        }
    }

    for target_id in closed_targets {
        quick_character_targets.remove(&target_id);
    }
}

fn quick_cast_ui(
    ui: &mut Ui,
    caster_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    character_targets: &[(String, String)],
    cast_turn: u32,
    scene_positions: Option<&SceneCharacterPositions>,
    player_camera_positions: Option<&ScenePlayerCameraPositions>,
) -> Option<QuickCastAction> {
    let skills = quick_cast_skills(character);
    if skills.is_empty() {
        ui.small("没有可释放技能。");
        return None;
    }

    let selected = edit_state
        .quick_cast_skill_index
        .entry(caster_id.to_owned())
        .or_insert(0);
    if *selected >= skills.len() {
        *selected = 0;
    }
    let skill = skills[*selected].clone();
    let effect = quick_cast_effect(&skill.note);
    let cooldown_remaining = quick_skill_cooldown_remaining(
        character,
        skill.index,
        skill.cooldown_turns,
        cast_turn,
    );
    let targets = effect
        .as_ref()
        .map(|effect| {
            quick_cast_targets(
                caster_id,
                effect,
                character_targets,
                scene_positions,
                player_camera_positions,
            )
        })
        .unwrap_or_default();
    let can_pay = character.mp + f32::EPSILON >= skill.mp_cost;
    let can_cast = can_pay && cooldown_remaining == 0 && effect.is_some();
    let force_pending = edit_state
        .pending_force_cast
        .as_ref()
        .is_some_and(|(target_id, index)| target_id == caster_id && *index == *selected);

    let mut action = None;
    egui::CollapsingHeader::new("释放技能")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("技能");
                egui::ComboBox::from_id_salt(format!("quick_cast_skill_{caster_id}"))
                    .selected_text(skill.name.as_str())
                    .show_ui(ui, |ui| {
                        for (index, skill) in skills.iter().enumerate() {
                            let remaining = quick_skill_cooldown_remaining(
                                character,
                                skill.index,
                                skill.cooldown_turns,
                                cast_turn,
                            );
                            let mut details = Vec::new();
                            if skill.mp_cost > 0.0 {
                                details.push(format!(
                                    "MP {}",
                                    format_character_number(skill.mp_cost)
                                ));
                            }
                            if remaining > 0 {
                                details.push(format!("CD {remaining}"));
                            } else if skill.cooldown_turns > 0 {
                                details.push(format!("CD {}", skill.cooldown_turns));
                            }
                            let label = if details.is_empty() {
                                skill.name.clone()
                            } else {
                                format!(
                                    "{} ({})",
                                    skill.name,
                                    details.join(", ")
                                )
                            };
                            ui.selectable_value(selected, index, label);
                        }
                    });
                if let Some(radius) = effect.as_ref().and_then(quick_cast_radius) {
                    ui.small(format!(
                        "以玩家镜头为中心 {}米",
                        format_character_number(radius)
                    ));
                }
            });

            if let Some(effect) = effect.as_ref() {
                let target_label = match effect {
                    QuickCastEffect::Damage { .. } => "范围内目标",
                    QuickCastEffect::Heal { .. } => "可影响角色",
                };
                if targets.is_empty() {
                    ui.small("范围内没有可影响角色。");
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.small(target_label);
                        for target_id in &targets {
                            let name = character_targets
                                .iter()
                                .find(|(candidate_id, _)| candidate_id == target_id)
                                .map(|(_, name)| name.as_str())
                                .unwrap_or(target_id.as_str());
                            ui.small(name);
                        }
                    });
                }
            } else {
                ui.small("技能描述需要是可解析的固定伤害或治疗规则。");
            }

            let response = ui.add_enabled(can_cast, egui::Button::new("释放"));
            if response.clicked() {
                action = Some(QuickCastAction {
                    caster_id: caster_id.to_owned(),
                    skill: skill.clone(),
                    targets: targets.clone(),
                    effect: effect.clone(),
                    cast_turn,
                    force: false,
                });
                edit_state.pending_force_cast = None;
            }

            ui.horizontal_wrapped(|ui| {
                let force_response = ui
                    .add(egui::Button::new("强制释放"))
                    .on_hover_text("GM强制释放：忽略MP、目标和规则解析条件。");
                if force_response.clicked() {
                    if force_pending {
                        action = Some(QuickCastAction {
                            caster_id: caster_id.to_owned(),
                            skill: skill.clone(),
                            targets: targets.clone(),
                            effect: effect.clone(),
                            cast_turn,
                            force: true,
                        });
                        edit_state.pending_force_cast = None;
                    } else {
                        edit_state.pending_force_cast = Some((caster_id.to_owned(), *selected));
                    }
                }
                if force_pending {
                    ui.small("再次点击强制释放确认。");
                    if ui.small_button("取消").clicked() {
                        edit_state.pending_force_cast = None;
                    }
                }
            });

            if cooldown_remaining > 0 {
                ui.small(format!(
                    "冷却还剩{cooldown_remaining}轮"
                ));
            }
            if !can_cast && can_pay && cooldown_remaining == 0 {
                ui.small("普通释放需要可解析的固定伤害或治疗规则；强制释放可忽略。");
            }
            if !can_pay {
                ui.small(format!(
                    "需要{} MP",
                    format_character_number(skill.mp_cost)
                ));
            }
        });
    action
}

fn quick_cast_skills(character: &mut PlayerCharacter) -> Vec<QuickCastSkill> {
    normalize_character_skill_fields(character);
    character
        .skill_names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            if !character.skill_metadata[index].is_approved() {
                return None;
            }
            let name = if name.trim().is_empty() {
                format!("技能{}", index + 1)
            } else {
                name.trim().to_owned()
            };
            Some(QuickCastSkill {
                index,
                name,
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
            })
        })
        .collect()
}

fn quick_cast_cooldown_turn(manager: &NapcatMessageManager, caster_id: &str) -> u32 {
    manager
        .trpg_groups
        .values()
        .filter(|group| group.players.iter().any(|player_id| player_id == caster_id))
        .map(|group| {
            group
                .player_turns
                .get(caster_id)
                .map(|turn| turn.turns_passed)
                .unwrap_or(group.world_turn)
        })
        .max()
        .unwrap_or_default()
}

fn quick_skill_cooldown_remaining(
    character: &PlayerCharacter,
    skill_index: usize,
    cooldown_turns: u32,
    cast_turn: u32,
) -> u32 {
    if cooldown_turns == 0 {
        return 0;
    }
    character
        .skill_last_cast_turns
        .get(&skill_index.to_string())
        .map(|last_cast_turn| {
            cooldown_turns.saturating_sub(cast_turn.saturating_sub(*last_cast_turn))
        })
        .unwrap_or(0)
}

fn quick_cast_effect(note: &str) -> Option<QuickCastEffect> {
    let ast = parse_rule(note).ok()?;
    ast.actions.into_iter().find_map(|action| match action {
        Action::Damage {
            target,
            amount: ValueExpr::Number(amount),
            ..
        } => Some(QuickCastEffect::Damage {
            amount: amount.max(0.0),
            target,
        }),
        Action::Heal {
            target,
            amount: ValueExpr::Number(amount),
        } => Some(QuickCastEffect::Heal {
            amount: amount.max(0.0),
            target,
        }),
        _ => None,
    })
}

fn quick_cast_radius(effect: &QuickCastEffect) -> Option<f32> {
    let target = match effect {
        QuickCastEffect::Damage { target, .. } | QuickCastEffect::Heal { target, .. } => target,
    };
    target.area.and_then(|area| area.radius_meters)
}

fn quick_cast_targets(
    caster_id: &str,
    effect: &QuickCastEffect,
    character_targets: &[(String, String)],
    scene_positions: Option<&SceneCharacterPositions>,
    player_camera_positions: Option<&ScenePlayerCameraPositions>,
) -> Vec<String> {
    let target = match effect {
        QuickCastEffect::Damage { target, .. } | QuickCastEffect::Heal { target, .. } => target,
    };
    if let Some(area) = target.area {
        let Some(radius) = area.radius_meters else {
            return character_targets
                .iter()
                .filter(|(target_id, _)| target_id != caster_id)
                .map(|(target_id, _)| target_id.clone())
                .collect();
        };
        let Some(user_id) = caster_id.parse::<u64>().ok() else {
            return Vec::new();
        };
        let Some(camera_position) =
            player_camera_positions.and_then(|positions| positions.positions.get(&user_id))
        else {
            return Vec::new();
        };
        let Some(scene_positions) = scene_positions else {
            return Vec::new();
        };
        return character_targets
            .iter()
            .filter(|(target_id, _)| target_id != caster_id)
            .filter(|(target_id, _)| {
                scene_positions
                    .positions
                    .get(target_id)
                    .map(|position| camera_position.distance(*position) <= radius)
                    .unwrap_or(false)
            })
            .map(|(target_id, _)| target_id.clone())
            .collect();
    }

    match target.actor {
        ActorRef::SelfActor => vec![caster_id.to_owned()],
        ActorRef::Source | ActorRef::Target => character_targets
            .iter()
            .find(|(target_id, _)| target_id != caster_id)
            .map(|(target_id, _)| vec![target_id.clone()])
            .unwrap_or_default(),
    }
}

fn quick_cast_character_targets(manager: &NapcatMessageManager) -> Vec<(String, String)> {
    let mut targets = manager
        .player_characters
        .iter()
        .filter(|(_, character)| character.inited && character.hp > 0.0)
        .map(|(target_id, character)| {
            let display_name = if !character.nickname.trim().is_empty() {
                character.nickname.trim().to_owned()
            } else if !character.name.trim().is_empty() {
                character.name.trim().to_owned()
            } else {
                target_display_name(manager, target_id)
            };
            (target_id.clone(), display_name)
        })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    targets
}

fn apply_quick_cast_action(
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    action: QuickCastAction,
) -> bool {
    apply_quick_cast_action_to_manager(manager.as_mut(), action)
}

fn apply_quick_cast_action_to_manager(
    manager: &mut NapcatMessageManager,
    action: QuickCastAction,
) -> bool {
    let Some(caster) = manager.player_characters.get_mut(&action.caster_id) else {
        return false;
    };
    if !action.force && caster.mp + f32::EPSILON < action.skill.mp_cost {
        return false;
    }
    let cooldown_remaining = quick_skill_cooldown_remaining(
        caster,
        action.skill.index,
        action.skill.cooldown_turns,
        action.cast_turn,
    );
    if !action.force && cooldown_remaining > 0 {
        return false;
    }
    if !action.force {
        caster.mp = (caster.mp - action.skill.mp_cost).max(0.0);
    }
    caster.skill_last_cast_turns.insert(
        action.skill.index.to_string(),
        action.cast_turn,
    );

    let mut changed = true;
    let Some(effect) = action.effect else {
        return changed;
    };
    for target_id in action.targets {
        let Some(target) = manager.player_characters.get_mut(&target_id) else {
            continue;
        };
        match effect {
            QuickCastEffect::Damage { amount, .. } => {
                let next_hp = (target.hp - amount).max(0.0);
                if (target.hp - next_hp).abs() > f32::EPSILON {
                    target.hp = next_hp;
                    changed = true;
                }
            },
            QuickCastEffect::Heal { amount, .. } => {
                let next_hp = (target.hp + amount).min(target.max_hp);
                if (target.hp - next_hp).abs() > f32::EPSILON {
                    target.hp = next_hp;
                    changed = true;
                }
            },
        }
    }
    changed
}

fn status_summary_value_ui(ui: &mut Ui, label: &str, base: i32, extra: i32) {
    let total = base + extra;
    if extra == 0 {
        ui.label(format!("{label} {base}"));
    } else {
        ui.label(format!(
            "{label} {base}+{extra}={total}"
        ));
    }
}

fn format_character_number(value: f32) -> String {
    if value.fract().abs() < f32::EPSILON {
        format!("{}", value as i32)
    } else {
        format!("{value:.1}")
    }
}

fn character_editor_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    chat_display_name: &str,
    edit_state: &mut CharacterEditState,
    rule_engine_state: &mut RuleEngineState,
    skill_pool: &[SkillPoolEntry],
    stat_config: TrpgBasicConfig,
) -> bool {
    let mut changed = false;
    let mut derived_stats_changed = false;
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut character.inited, "已完成").changed();
        egui::ComboBox::from_label("流程")
            .selected_text(character_creation_step_label(
                character.creation_step,
            ))
            .show_ui(ui, |ui| {
                for (step, label) in character_creation_step_options() {
                    changed |= ui
                        .selectable_value(
                            &mut character.creation_step,
                            step,
                            label,
                        )
                        .changed();
                }
            });
        if ui.button("使用聊天名").clicked() {
            character.nickname = chat_display_name.to_owned();
            changed = true;
        }
        if ui.button("重置").clicked() {
            edit_state.pending_character_reset = Some(target_id.to_owned());
        }
    });

    if edit_state.pending_character_reset.as_deref() == Some(target_id) {
        let character_label = if character.name.trim().is_empty() {
            chat_display_name.to_owned()
        } else {
            character.name.trim().to_owned()
        };
        let modal = Modal::new(Id::new((
            "character_reset_confirm",
            target_id,
        )))
        .show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.heading("重置角色？");
            ui.label(format!(
                "这会清空{character_label}的所有角色数据。"
            ));
            ui.label("此操作无法撤销。");

            egui::Sides::new().show(
                ui,
                |ui| {
                    if ui.button("取消").clicked() {
                        ui.close();
                    }
                },
                |ui| {
                    if ui.button("重置").clicked() {
                        *character = PlayerCharacter::default();
                        edit_state.unlocked_status_targets.remove(target_id);
                        edit_state.gm_status_drafts.remove(target_id);
                        edit_state.buff_drafts.remove(target_id);
                        edit_state.pending_character_reset = None;
                        changed = true;
                    }
                },
            );
        });
        if modal.should_close() {
            edit_state.pending_character_reset = None;
        }
    }

    ui.columns(2, |columns| {
        columns[0].label("角色名");
        changed |= columns[0]
            .text_edit_singleline(&mut character.name)
            .changed();
        columns[1].label("昵称");
        changed |= columns[1]
            .text_edit_singleline(&mut character.nickname)
            .changed();
    });
    ui.label("图片URL");
    changed |= ui.text_edit_singleline(&mut character.image).changed();

    ui.separator();
    let status_unlocked = edit_state.unlocked_status_targets.contains(target_id);
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "创建点数剩余 {}",
            character.status_points
        ));
        ui.label(format!(
            "兑换点数 {}",
            character.exchange_points
        ));
        ui.label(format!(
            "HP状态：{}",
            character_hp_status(character.hp, character.max_hp)
        ));
        if status_unlocked {
            if ui.button("锁定").clicked() {
                edit_state.unlocked_status_targets.remove(target_id);
                edit_state.gm_status_drafts.remove(target_id);
            }
        } else if ui.button("解锁").clicked() {
            edit_state
                .unlocked_status_targets
                .insert(target_id.to_owned());
            edit_state.gm_status_drafts.insert(
                target_id.to_owned(),
                character.extra_status.clone(),
            );
        }
        let level_response = ui
            .add(
                egui::DragValue::new(&mut character.level)
                    .range(1..=999)
                    .prefix("等级 "),
            )
            .changed();
        changed |= level_response;
        derived_stats_changed |= level_response;
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.exp)
                    .range(0..=999_999)
                    .prefix("经验 "),
            )
            .changed();
    });
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.hp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("HP "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.max_hp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("/ "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.hp_regen)
                    .range(-9999.0..=9999.0)
                    .speed(0.1)
                    .prefix("回复 "),
            )
            .changed();
    });
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.mp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("MP "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.max_mp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("/ "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.mp_regen)
                    .range(-9999.0..=9999.0)
                    .speed(0.1)
                    .prefix("回复 "),
            )
            .changed();
    });
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.speed)
                    .range(0.0..=9999.0)
                    .speed(0.1)
                    .prefix("速度 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.damage_dealt_modifier)
                    .range(0.0..=99.0)
                    .speed(0.01)
                    .prefix("伤害 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.damage_taken_modifier)
                    .range(0.0..=99.0)
                    .speed(0.01)
                    .prefix("承伤 "),
            )
            .changed();
    });
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.healing_dealt_modifier)
                    .range(0.0..=99.0)
                    .speed(0.01)
                    .prefix("治疗 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.healing_taken_modifier)
                    .range(0.0..=99.0)
                    .speed(0.01)
                    .prefix("受疗 "),
            )
            .changed();
    });

    ui.separator();
    let status_changed = character_status_source_ui(
        ui,
        target_id,
        character,
        edit_state,
        status_unlocked,
    );
    changed |= status_changed;
    derived_stats_changed |= status_changed;
    ui.separator();
    changed |= character_buff_editor_ui(
        ui,
        target_id,
        character,
        edit_state,
        rule_engine_state,
    );
    ui.separator();
    changed |= character_inventory_editor_ui(ui, character);
    ui.separator();
    changed |= character_skill_editor_ui(
        ui,
        target_id,
        character,
        edit_state,
        rule_engine_state,
        skill_pool,
    );

    if derived_stats_changed {
        update_character_from_status_with_config(character, &stat_config);
        if !character.active_buffs.is_empty() {
            character.buff_base_stats = Some(CharacterBuffBaseStats::from_character(
                character,
            ));
            sync_character_buffs(target_id, character, rule_engine_state);
        } else {
            sync_character_skill_rules(target_id, character, rule_engine_state);
        }
        changed = true;
    }

    if character.max_hp < 0.0 {
        character.max_hp = 0.0;
        changed = true;
    }
    if character.hp > character.max_hp {
        character.hp = character.max_hp;
        changed = true;
    }
    if character.mp > character.max_mp {
        character.mp = character.max_mp;
        changed = true;
    }

    changed
}

fn character_skill_pool_picker_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    skill_pool: &[SkillPoolEntry],
) -> bool {
    if skill_pool.is_empty() {
        return false;
    }

    let selected = edit_state
        .skill_pool_selected_index
        .entry(target_id.to_owned())
        .or_insert(0);
    if *selected >= skill_pool.len() {
        *selected = 0;
    }

    let mut changed = false;
    ui.collapsing("技能池", |ui| {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt(format!("skill_pool_picker_{target_id}"))
                .selected_text(skill_pool_entry_label(
                    &skill_pool[*selected],
                ))
                .show_ui(ui, |ui| {
                    for (index, entry) in skill_pool.iter().enumerate() {
                        ui.selectable_value(
                            selected,
                            index,
                            skill_pool_entry_label(entry),
                        );
                    }
                });
            if ui.button("复制到角色").clicked() {
                add_skill_pool_entry_to_character(character, &skill_pool[*selected]);
                changed = true;
            }
        });
        let entry = &skill_pool[*selected];
        ui.horizontal_wrapped(|ui| {
            ui.small(format!(
                "MP {}",
                format_character_number(entry.mp_cost)
            ));
            ui.small(format!(
                "冷却 {}轮",
                entry.cooldown_turns
            ));
            if let Some(source) = entry.source_character_name.as_deref() {
                ui.small(format!("来源 {source}"));
            } else {
                ui.small("来源 手动");
            }
            if let Some(category) = entry
                .category
                .as_deref()
                .filter(|category| !category.trim().is_empty())
            {
                ui.small(format!("类型 {category}"));
            }
            if !entry.tags.is_empty() {
                ui.small(format!("标签 {}", entry.tags.join(" ")));
            }
        });
        if let Some(legacy_label) = skill_pool_entry_legacy_label(entry) {
            ui.small(legacy_label);
        }
        if !entry.note.trim().is_empty() {
            ui.monospace(entry.note.trim());
        }
    });
    changed
}

fn add_skill_pool_entry_to_character(character: &mut PlayerCharacter, entry: &SkillPoolEntry) {
    normalize_character_skill_fields(character);
    character.skill_names.push(entry.name.clone());
    character.skill_notes.push(entry.note.clone());
    character.skill_mp_costs.push(entry.mp_cost.max(0.0));
    character.skill_cooldown_turns.push(entry.cooldown_turns);
    character
        .skill_metadata
        .push(CharacterSkillMetadata::skill_pool(
            entry,
        ));
}

fn skill_pool_entry_label(entry: &SkillPoolEntry) -> String {
    match entry.source_character_name.as_deref() {
        Some(source) if !source.trim().is_empty() => {
            format!(
                "{} - {}",
                skill_pool_entry_name(entry),
                source
            )
        },
        _ => skill_pool_entry_name(entry),
    }
}

fn character_skill_editor_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    rule_engine_state: &mut RuleEngineState,
    skill_pool: &[SkillPoolEntry],
) -> bool {
    let mut changed = false;
    let mut remove_index = None;

    changed |= normalize_character_skill_fields(character);

    changed |= character_skill_pool_picker_ui(
        ui, target_id, character, edit_state, skill_pool,
    );

    ui.horizontal(|ui| {
        ui.label(format!(
            "技能描述：{}",
            character.skill_notes.len()
        ));
        if ui.button("+").on_hover_text("添加技能描述").clicked() {
            character.skill_names.push(String::new());
            character.skill_notes.push(String::new());
            character.skill_mp_costs.push(0.0);
            character.skill_cooldown_turns.push(0);
            character
                .skill_metadata
                .push(CharacterSkillMetadata::default());
            changed = true;
        }
    });

    for index in 0..character.skill_names.len() {
        let validation = parse_skill_note(&character.skill_notes[index]);
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 28.0).clamp(160.0, CHARACTER_FIELD_MAX_WIDTH);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("技能名");
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut character.skill_names[index])
                                .desired_width((width - 78.0).max(82.0)),
                        )
                        .changed();
                });
                ui.horizontal_wrapped(|ui| {
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut character.skill_mp_costs[index])
                                .range(0.0..=9999.0)
                                .speed(1.0)
                                .prefix("MP "),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut character.skill_cooldown_turns[index])
                                .range(0..=999)
                                .speed(1)
                                .prefix("冷却 "),
                        )
                        .changed();
                });
                ui.horizontal_wrapped(|ui| {
                    let metadata = &mut character.skill_metadata[index];
                    changed |= ui.checkbox(&mut metadata.pc_approved, "PC确认").changed();
                    changed |= ui.checkbox(&mut metadata.st_approved, "GM确认").changed();
                    if let Some(source) = character_skill_metadata_source_label(metadata) {
                        ui.small(source);
                    }
                });
                let metadata = &mut character.skill_metadata[index];
                ui.collapsing("技能结构", |ui| {
                    changed |= character_skill_shape_metadata_ui(ui, metadata);
                });
                let response = ui.add(
                    egui::TextEdit::multiline(&mut character.skill_notes[index])
                        .desired_rows(2)
                        .desired_width(width),
                );
                if response.changed() {
                    changed = true;
                }
                if validation.is_err() {
                    let y = response.rect.bottom() - 2.0;
                    ui.painter().line_segment(
                        [
                            egui::pos2(response.rect.left(), y),
                            egui::pos2(response.rect.right(), y),
                        ],
                        Stroke::new(1.5, egui::Color32::RED),
                    );
                }
            });
            if ui.button("-").on_hover_text("移除技能描述").clicked() {
                remove_index = Some(index);
            }
        });
        if let Err(err) = validation {
            ui.colored_label(egui::Color32::RED, err);
        }
    }

    if let Some(index) = remove_index {
        character.skill_names.remove(index);
        character.skill_notes.remove(index);
        character.skill_mp_costs.remove(index);
        character.skill_cooldown_turns.remove(index);
        character.skill_metadata.remove(index);
        shift_skill_last_cast_turns_after_remove(
            &mut character.skill_last_cast_turns,
            index,
        );
        changed = true;
    }

    sync_character_skill_rules(target_id, character, rule_engine_state);

    changed
}

fn character_skill_shape_metadata_ui(ui: &mut Ui, metadata: &mut CharacterSkillMetadata) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= optional_string_field(
            ui,
            "类型",
            &mut metadata.skill_type,
            86.0,
        );
        changed |= optional_string_field(
            ui,
            "目标",
            &mut metadata.target_class,
            86.0,
        );
        changed |= optional_u32_drag(
            ui,
            "数量",
            &mut metadata.target_count,
            0..=999,
        );
        changed |= optional_i32_drag(
            ui,
            "范围",
            &mut metadata.range,
            0..=9999,
        );
        changed |= optional_i32_drag(
            ui,
            "兑换点",
            &mut metadata.exchange_point,
            0..=9999,
        );
        changed |= optional_u32_drag(
            ui,
            "剩余冷却",
            &mut metadata.cooldown_left,
            0..=999,
        );
        changed |= optional_string_field(
            ui,
            "释放者",
            &mut metadata.legacy_caster,
            86.0,
        );
    });
    if !metadata.args.is_empty() {
        ui.small(format!(
            "旧变量：{}",
            metadata
                .args
                .iter()
                .map(skill_arg_label)
                .collect::<Vec<_>>()
                .join("，")
        ));
    }
    if metadata.legacy_has_buff_machine {
        ui.small("含旧buff机，尚未转换为规则。");
    }
    changed
}

fn optional_string_field(ui: &mut Ui, label: &str, value: &mut Option<String>, width: f32) -> bool {
    ui.label(label);
    let mut text = value.clone().unwrap_or_default();
    let changed = ui
        .add(egui::TextEdit::singleline(&mut text).desired_width(width))
        .changed();
    if changed {
        let text = text.trim();
        *value = (!text.is_empty()).then(|| text.to_owned());
    }
    changed
}

fn optional_u32_drag(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<u32>,
    range: std::ops::RangeInclusive<u32>,
) -> bool {
    let mut changed = false;
    let mut enabled = value.is_some();
    changed |= ui.checkbox(&mut enabled, label).changed();
    if enabled {
        let value_ref = value.get_or_insert(0);
        changed |= ui
            .add(egui::DragValue::new(value_ref).range(range).speed(1))
            .changed();
    } else if value.take().is_some() {
        changed = true;
    }
    changed
}

fn optional_i32_drag(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<i32>,
    range: std::ops::RangeInclusive<i32>,
) -> bool {
    let mut changed = false;
    let mut enabled = value.is_some();
    changed |= ui.checkbox(&mut enabled, label).changed();
    if enabled {
        let value_ref = value.get_or_insert(0);
        changed |= ui
            .add(egui::DragValue::new(value_ref).range(range).speed(1))
            .changed();
    } else if value.take().is_some() {
        changed = true;
    }
    changed
}

fn skill_arg_label(arg: &crate::napcat::SkillPoolArg) -> String {
    let name = if arg.name.trim().is_empty() { "未命名变量" } else { arg.name.trim() };
    if arg.kind.trim().is_empty() && arg.value.trim().is_empty() {
        name.to_owned()
    } else if arg.value.trim().is_empty() {
        format!("{name}:{}", arg.kind.trim())
    } else if arg.kind.trim().is_empty() {
        format!("{name}={}", arg.value.trim())
    } else {
        format!(
            "{name}:{}={}",
            arg.kind.trim(),
            arg.value.trim()
        )
    }
}

fn normalize_character_skill_fields(character: &mut PlayerCharacter) -> bool {
    let mut changed = false;
    let skill_count = character
        .skill_names
        .len()
        .max(character.skill_notes.len())
        .max(character.skill_mp_costs.len())
        .max(character.skill_cooldown_turns.len())
        .max(character.skill_metadata.len());
    if character.skill_names.len() != skill_count {
        character.skill_names.resize(skill_count, String::new());
        changed = true;
    }
    if character.skill_notes.len() != skill_count {
        character.skill_notes.resize(skill_count, String::new());
        changed = true;
    }
    if character.skill_mp_costs.len() != skill_count {
        character.skill_mp_costs.resize(skill_count, 0.0);
        changed = true;
    }
    if character.skill_cooldown_turns.len() != skill_count {
        character.skill_cooldown_turns.resize(skill_count, 0);
        changed = true;
    }
    if character.skill_metadata.len() != skill_count {
        character.skill_metadata.resize(
            skill_count,
            CharacterSkillMetadata::default(),
        );
        changed = true;
    }
    for cost in &mut character.skill_mp_costs {
        if *cost < 0.0 {
            *cost = 0.0;
            changed = true;
        }
    }
    if retain_valid_skill_last_cast_turns(
        &mut character.skill_last_cast_turns,
        skill_count,
    ) {
        changed = true;
    }
    changed
}

fn character_skill_metadata_source_label(metadata: &CharacterSkillMetadata) -> Option<String> {
    match metadata.source {
        CharacterSkillSourceKind::Manual => None,
        CharacterSkillSourceKind::Talent => {
            let label = metadata
                .source_pool_label
                .as_deref()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or("天赋");
            Some(format!("来源：{label}"))
        },
        CharacterSkillSourceKind::SkillPool => {
            let label = metadata
                .source_pool_label
                .as_deref()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or("技能池");
            Some(format!("来源：{label}"))
        },
    }
}

fn retain_valid_skill_last_cast_turns(
    last_cast_turns: &mut HashMap<String, u32>,
    skill_count: usize,
) -> bool {
    let before_len = last_cast_turns.len();
    last_cast_turns.retain(|key, _| {
        key.parse::<usize>()
            .ok()
            .is_some_and(|index| index < skill_count)
    });
    before_len != last_cast_turns.len()
}

fn shift_skill_last_cast_turns_after_remove(
    last_cast_turns: &mut HashMap<String, u32>,
    removed_index: usize,
) {
    let shifted = last_cast_turns
        .iter()
        .filter_map(|(key, turn)| {
            let index = key.parse::<usize>().ok()?;
            if index == removed_index {
                None
            } else if index > removed_index {
                Some(((index - 1).to_string(), *turn))
            } else {
                Some((key.clone(), *turn))
            }
        })
        .collect();
    *last_cast_turns = shifted;
}

fn parse_skill_note(note: &str) -> Result<Option<RuleAst>, String> {
    if note.trim().is_empty() {
        return Ok(None);
    }
    parse_rule(note).map(Some)
}

fn sync_character_skill_rules(
    target_id: &str,
    character: &PlayerCharacter,
    rule_engine_state: &mut RuleEngineState,
) {
    let stats = CharacterBuffBaseStats::from_character(character);
    sync_character_skill_rules_with_stats(
        target_id,
        character,
        &stats,
        rule_engine_state,
    );
}

fn sync_character_skill_rules_with_stats(
    target_id: &str,
    character: &PlayerCharacter,
    stats: &CharacterBuffBaseStats,
    rule_engine_state: &mut RuleEngineState,
) {
    let rules = character
        .skill_notes
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            character
                .skill_metadata
                .get(*index)
                .cloned()
                .unwrap_or_default()
                .is_approved()
        })
        .filter_map(|(_, note)| parse_skill_note(note).ok().flatten())
        .collect::<Vec<_>>();
    let display_name =
        if character.name.trim().is_empty() { target_id } else { character.name.trim() };
    rule_engine_state.sync_character(
        target_id,
        display_name,
        stats.hp,
        stats.max_hp,
        stats.damage_dealt_modifier,
        stats.damage_taken_modifier,
        stats.healing_dealt_modifier,
        stats.healing_taken_modifier,
        rules,
    );
}

fn character_creation_step_options() -> [(CharacterCreationStep, &'static str); 14] {
    [
        (CharacterCreationStep::Normal, "普通"),
        (CharacterCreationStep::Str, "STR"),
        (CharacterCreationStep::Agi, "AGI"),
        (CharacterCreationStep::Dex, "DEX"),
        (CharacterCreationStep::Vit, "VIT"),
        (CharacterCreationStep::Int, "INT"),
        (CharacterCreationStep::Wis, "WIS"),
        (CharacterCreationStep::K, "K"),
        (CharacterCreationStep::Cha, "CHA"),
        (
            CharacterCreationStep::ConfirmStatus,
            "确认属性",
        ),
        (CharacterCreationStep::Skill, "技能"),
        (
            CharacterCreationStep::ConfirmSkill,
            "确认技能",
        ),
        (CharacterCreationStep::Image, "图片"),
        (CharacterCreationStep::Nickname, "昵称"),
    ]
}

fn character_creation_step_label(step: CharacterCreationStep) -> &'static str {
    character_creation_step_options()
        .iter()
        .find_map(|(candidate, label)| (*candidate == step).then_some(*label))
        .unwrap_or("未知")
}

fn character_status_source_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    unlocked: bool,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label("属性来源");
        if unlocked {
            ui.small("GM修正草稿已解锁");
        } else {
            ui.small("已锁定");
        }
    });
    ui.small("创建值来自玩家建卡流程。GM修正值单独记录，并叠加到总值上。");

    if unlocked && !edit_state.gm_status_drafts.contains_key(target_id) {
        edit_state.gm_status_drafts.insert(
            target_id.to_owned(),
            character.extra_status.clone(),
        );
    }

    if unlocked {
        let draft_for_apply = {
            let draft = edit_state
                .gm_status_drafts
                .entry(target_id.to_owned())
                .or_insert_with(|| character.extra_status.clone());
            egui::Grid::new(ui.next_auto_id())
                .num_columns(5)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.strong("属性");
                    ui.strong("创建");
                    ui.strong("当前GM");
                    ui.strong("草稿GM");
                    ui.strong("总值");
                    ui.end_row();
                    status_source_value_ui(
                        ui,
                        "STR",
                        character.status.str_,
                        character.extra_status.str_,
                        &mut draft.str_,
                    );
                    status_source_value_ui(
                        ui,
                        "AGI",
                        character.status.agi,
                        character.extra_status.agi,
                        &mut draft.agi,
                    );
                    status_source_value_ui(
                        ui,
                        "DEX",
                        character.status.dex,
                        character.extra_status.dex,
                        &mut draft.dex,
                    );
                    status_source_value_ui(
                        ui,
                        "VIT",
                        character.status.vit,
                        character.extra_status.vit,
                        &mut draft.vit,
                    );
                    status_source_value_ui(
                        ui,
                        "INT",
                        character.status.int_,
                        character.extra_status.int_,
                        &mut draft.int_,
                    );
                    status_source_value_ui(
                        ui,
                        "WIS",
                        character.status.wis,
                        character.extra_status.wis,
                        &mut draft.wis,
                    );
                    status_source_value_ui(
                        ui,
                        "K",
                        character.status.k,
                        character.extra_status.k,
                        &mut draft.k,
                    );
                    status_source_value_ui(
                        ui,
                        "CHA",
                        character.status.cha,
                        character.extra_status.cha,
                        &mut draft.cha,
                    );
                });
            draft.clone()
        };
        ui.horizontal(|ui| {
            if ui.button("应用GM修正").clicked() {
                character.extra_status = draft_for_apply.clone();
                edit_state.unlocked_status_targets.remove(target_id);
                edit_state.gm_status_drafts.remove(target_id);
                changed = true;
            }
            if ui.button("取消").clicked() {
                edit_state.unlocked_status_targets.remove(target_id);
                edit_state.gm_status_drafts.remove(target_id);
            }
        });
    } else {
        egui::Grid::new(ui.next_auto_id())
            .num_columns(4)
            .spacing([12.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                ui.strong("属性");
                ui.strong("创建");
                ui.strong("GM");
                ui.strong("总值");
                ui.end_row();
                readonly_status_source_row(
                    ui,
                    "STR",
                    character.status.str_,
                    character.extra_status.str_,
                );
                readonly_status_source_row(
                    ui,
                    "AGI",
                    character.status.agi,
                    character.extra_status.agi,
                );
                readonly_status_source_row(
                    ui,
                    "DEX",
                    character.status.dex,
                    character.extra_status.dex,
                );
                readonly_status_source_row(
                    ui,
                    "VIT",
                    character.status.vit,
                    character.extra_status.vit,
                );
                readonly_status_source_row(
                    ui,
                    "INT",
                    character.status.int_,
                    character.extra_status.int_,
                );
                readonly_status_source_row(
                    ui,
                    "WIS",
                    character.status.wis,
                    character.extra_status.wis,
                );
                readonly_status_source_row(
                    ui,
                    "K",
                    character.status.k,
                    character.extra_status.k,
                );
                readonly_status_source_row(
                    ui,
                    "CHA",
                    character.status.cha,
                    character.extra_status.cha,
                );
            });
    }

    changed
}

fn readonly_status_source_row(ui: &mut Ui, label: &str, creation: i32, gm: i32) {
    ui.label(label);
    ui.label(creation.to_string());
    ui.label(format_signed_status(gm));
    ui.label((creation + gm).to_string());
    ui.end_row();
}

fn status_source_value_ui(
    ui: &mut Ui,
    label: &str,
    creation: i32,
    current_gm: i32,
    draft_gm: &mut i32,
) -> bool {
    ui.label(label);
    ui.label(creation.to_string());
    ui.label(format_signed_status(current_gm));
    let response = ui.add(
        egui::DragValue::new(draft_gm)
            .range(-999..=999)
            .speed(1)
            .prefix("GM "),
    );
    ui.label((creation + *draft_gm).to_string());
    ui.end_row();
    response.changed()
}

fn format_signed_status(value: i32) -> String {
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

fn character_hp_status(hp: f32, max_hp: f32) -> &'static str {
    if max_hp <= 0.0 {
        return "濒死";
    }
    if hp > max_hp * 0.8 {
        "无伤"
    } else if hp > max_hp * 0.6 {
        "轻伤"
    } else if hp > max_hp * 0.4 {
        "中伤"
    } else if hp > max_hp * 0.05 {
        "重伤"
    } else {
        "濒死"
    }
}

fn character_buff_editor_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    rule_engine_state: &mut RuleEngineState,
) -> bool {
    let mut changed = false;
    let mut remove_index = None;

    sync_character_buffs(target_id, character, rule_engine_state);
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "生效buff：{}",
            character.active_buffs.len()
        ));
        let active_names = rule_engine_state.active_buff_names(target_id);
        if !active_names.is_empty() {
            ui.small(active_names.join(", "));
        }
    });

    for (index, buff) in character.active_buffs.iter_mut().enumerate() {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                changed |= ui.text_edit_singleline(&mut buff.name).changed();
                changed |= buff_kind_combo(ui, &mut buff.kind);
                let turns_response = ui.add(
                    egui::DragValue::new(&mut buff.turns_remaining)
                        .range(0..=999)
                        .prefix("轮数 "),
                );
                changed |= turns_response.on_hover_text("输入0为永久buff").changed();
                if buff.turns_remaining == 0 {
                    ui.small("永久");
                }
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut buff.priority)
                            .range(-999..=999)
                            .prefix("优先级 "),
                    )
                    .changed();
                changed |= ui.checkbox(&mut buff.beneficial, "增益").changed();
                if ui.button("移除").clicked() {
                    remove_index = Some(index);
                }
            });
            for effect in &buff.effects {
                ui.small(format_buff_effect(effect));
            }
        });
    }

    if let Some(index) = remove_index {
        character.active_buffs.remove(index);
        changed = true;
    }

    let draft = edit_state
        .buff_drafts
        .entry(target_id.to_owned())
        .or_default();
    ui.collapsing("给予buff", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("名称");
            ui.text_edit_singleline(&mut draft.name);
            buff_kind_combo(ui, &mut draft.kind);
            ui.add(
                egui::DragValue::new(&mut draft.turns_remaining)
                    .range(0..=999)
                    .prefix("轮数 "),
            )
            .on_hover_text("输入0为永久buff");
            ui.small("输入0为永久buff");
            ui.add(
                egui::DragValue::new(&mut draft.priority)
                    .range(-999..=999)
                    .prefix("优先级 "),
            );
            ui.checkbox(&mut draft.beneficial, "增益");
        });
        ui.horizontal_wrapped(|ui| {
            buff_field_combo(ui, &mut draft.field);
            buff_value_ui(ui, &mut draft.value);
        });
        if ui.button("应用buff").clicked() {
            let name = draft.name.trim();
            character.active_buffs.push(BuffSpec {
                name: if name.is_empty() { "未命名buff".to_owned() } else { name.to_owned() },
                kind: draft.kind,
                priority: draft.priority,
                turns_remaining: draft.turns_remaining.max(0),
                source_id: "gm".to_owned(),
                beneficial: draft.beneficial,
                effects: vec![BuffEffect {
                    field: draft.field,
                    value: draft.value,
                }],
            });
            changed = true;
        }
    });

    if changed {
        sync_character_buffs(target_id, character, rule_engine_state);
    }
    changed
}

fn character_inventory_editor_ui(ui: &mut Ui, character: &mut PlayerCharacter) -> bool {
    let mut changed = false;
    changed |= normalize_inventory(&mut character.inventory);

    egui::CollapsingHeader::new("背包 / 装备")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut character.inventory.gold)
                            .range(0..=9_999_999)
                            .prefix("金币 "),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut character.inventory.bag_slots)
                            .range(1..=160)
                            .prefix("格子 "),
                    )
                    .changed();
                ui.small(format!(
                    "已用 {}/{}",
                    character.inventory.items.len(),
                    character.inventory.bag_slots
                ));
            });

            ui.collapsing("已装备", |ui| {
                for slot in equipment_slot_options() {
                    if slot == EquipmentSlot::None {
                        continue;
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.label(equipment_slot_label(slot));
                        if let Some(item) = character.inventory.equipment.get(&slot) {
                            ui.colored_label(
                                item_quality_color(item.quality),
                                item_display_name(item),
                            );
                            ui.small(format!("物品等级 {}", item.item_level));
                            if ui.button("卸下").clicked() {
                                if let Some(item) = character.inventory.equipment.remove(&slot) {
                                    add_item_to_inventory(&mut character.inventory, item);
                                    changed = true;
                                }
                            }
                        } else {
                            ui.small("空");
                        }
                    });
                }
            });

            let mut remove_index = None;
            let mut equip_index = None;
            ui.horizontal(|ui| {
                ui.label("背包");
                if ui.button("+").on_hover_text("添加空物品").clicked() {
                    add_item_to_inventory(
                        &mut character.inventory,
                        InventoryItem::default(),
                    );
                    changed = true;
                }
            });
            egui::Grid::new(ui.next_auto_id())
                .num_columns(7)
                .spacing([8.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("物品");
                    ui.strong("品质");
                    ui.strong("数量");
                    ui.strong("装备位");
                    ui.strong("等级");
                    ui.strong("绑定");
                    ui.strong("操作");
                    ui.end_row();

                    for (index, item) in character.inventory.items.iter_mut().enumerate() {
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut item.name).desired_width(120.0))
                            .changed();
                        changed |= item_quality_combo(ui, &mut item.quality);
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut item.stack)
                                    .range(1..=9999)
                                    .speed(1),
                            )
                            .changed();
                        changed |= equipment_slot_combo(ui, &mut item.equipment_slot);
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut item.item_level)
                                    .range(0..=9999)
                                    .speed(1),
                            )
                            .changed();
                        changed |= ui.checkbox(&mut item.soulbound, "").changed();
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    item.equipment_slot != EquipmentSlot::None,
                                    egui::Button::new("装备"),
                                )
                                .clicked()
                            {
                                equip_index = Some(index);
                            }
                            if ui.button("-").on_hover_text("移除物品").clicked() {
                                remove_index = Some(index);
                            }
                        });
                        ui.end_row();
                    }
                });

            if let Some(index) = equip_index {
                equip_inventory_item(&mut character.inventory, index);
                changed = true;
            }
            if let Some(index) = remove_index {
                character.inventory.items.remove(index);
                changed = true;
            }
        });

    changed |= normalize_inventory(&mut character.inventory);
    changed
}

fn buff_kind_combo(ui: &mut Ui, kind: &mut BuffKind) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label("类型")
        .selected_text(buff_kind_label(*kind))
        .show_ui(ui, |ui| {
            for candidate in buff_kind_options() {
                changed |= ui
                    .selectable_value(
                        kind,
                        candidate,
                        buff_kind_label(candidate),
                    )
                    .changed();
            }
        });
    changed
}

fn normalize_inventory(inventory: &mut CharacterInventory) -> bool {
    let mut changed = false;
    if inventory.bag_slots == 0 {
        inventory.bag_slots = 1;
        changed = true;
    }
    for item in &mut inventory.items {
        changed |= normalize_item(item);
    }
    for item in inventory.equipment.values_mut() {
        changed |= normalize_item(item);
    }
    let before_equipment = inventory.equipment.len();
    inventory
        .equipment
        .retain(|slot, item| *slot != EquipmentSlot::None && item.equipment_slot == *slot);
    changed |= inventory.equipment.len() != before_equipment;
    changed
}

fn normalize_item(item: &mut InventoryItem) -> bool {
    let mut changed = false;
    if item.max_stack == 0 {
        item.max_stack = 1;
        changed = true;
    }
    if item.stack == 0 {
        item.stack = 1;
        changed = true;
    }
    if item.stack > item.max_stack {
        item.stack = item.max_stack;
        changed = true;
    }
    changed
}

fn add_item_to_inventory(inventory: &mut CharacterInventory, mut item: InventoryItem) {
    normalize_item(&mut item);
    if !item.name.trim().is_empty() && item.max_stack > 1 {
        let mut remaining = item.stack;
        for existing in &mut inventory.items {
            if same_stackable_item(existing, &item) && existing.stack < existing.max_stack {
                let free = existing.max_stack - existing.stack;
                let moved = free.min(remaining);
                existing.stack += moved;
                remaining -= moved;
                if remaining == 0 {
                    return;
                }
            }
        }
        item.stack = remaining;
    }
    inventory.items.push(item);
}

fn same_stackable_item(left: &InventoryItem, right: &InventoryItem) -> bool {
    left.name == right.name
        && left.description == right.description
        && left.icon == right.icon
        && left.quality == right.quality
        && left.equipment_slot == right.equipment_slot
        && left.max_stack == right.max_stack
        && left.item_level == right.item_level
        && left.soulbound == right.soulbound
        && left.max_stack > 1
}

fn equip_inventory_item(inventory: &mut CharacterInventory, index: usize) {
    if index >= inventory.items.len() {
        return;
    }
    let item = inventory.items.remove(index);
    let slot = item.equipment_slot;
    if slot == EquipmentSlot::None {
        inventory.items.insert(index, item);
        return;
    }
    if let Some(previous) = inventory.equipment.insert(slot, item) {
        add_item_to_inventory(inventory, previous);
    }
}

fn item_display_name(item: &InventoryItem) -> String {
    if item.name.trim().is_empty() {
        "未命名物品".to_owned()
    } else if item.stack > 1 {
        format!("{} x{}", item.name.trim(), item.stack)
    } else {
        item.name.trim().to_owned()
    }
}

fn item_quality_combo(ui: &mut Ui, quality: &mut InventoryQuality) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(ui.next_auto_id())
        .selected_text(inventory_quality_label(*quality))
        .show_ui(ui, |ui| {
            for candidate in inventory_quality_options() {
                changed |= ui
                    .selectable_value(
                        quality,
                        candidate,
                        inventory_quality_label(candidate),
                    )
                    .changed();
            }
        });
    changed
}

fn equipment_slot_combo(ui: &mut Ui, slot: &mut EquipmentSlot) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(ui.next_auto_id())
        .selected_text(equipment_slot_label(*slot))
        .show_ui(ui, |ui| {
            for candidate in equipment_slot_options() {
                changed |= ui
                    .selectable_value(
                        slot,
                        candidate,
                        equipment_slot_label(candidate),
                    )
                    .changed();
            }
        });
    changed
}

fn inventory_quality_options() -> [InventoryQuality; 6] {
    [
        InventoryQuality::Poor,
        InventoryQuality::Common,
        InventoryQuality::Uncommon,
        InventoryQuality::Rare,
        InventoryQuality::Epic,
        InventoryQuality::Legendary,
    ]
}

fn inventory_quality_label(quality: InventoryQuality) -> &'static str {
    match quality {
        InventoryQuality::Poor => "粗糙",
        InventoryQuality::Common => "普通",
        InventoryQuality::Uncommon => "优秀",
        InventoryQuality::Rare => "精良",
        InventoryQuality::Epic => "史诗",
        InventoryQuality::Legendary => "传说",
    }
}

fn item_quality_color(quality: InventoryQuality) -> egui::Color32 {
    match quality {
        InventoryQuality::Poor => egui::Color32::from_gray(150),
        InventoryQuality::Common => egui::Color32::WHITE,
        InventoryQuality::Uncommon => egui::Color32::from_rgb(30, 255, 0),
        InventoryQuality::Rare => egui::Color32::from_rgb(0, 112, 221),
        InventoryQuality::Epic => egui::Color32::from_rgb(163, 53, 238),
        InventoryQuality::Legendary => egui::Color32::from_rgb(255, 128, 0),
    }
}

fn equipment_slot_options() -> [EquipmentSlot; 16] {
    [
        EquipmentSlot::None,
        EquipmentSlot::Head,
        EquipmentSlot::Neck,
        EquipmentSlot::Shoulder,
        EquipmentSlot::Back,
        EquipmentSlot::Chest,
        EquipmentSlot::Wrist,
        EquipmentSlot::Hands,
        EquipmentSlot::Waist,
        EquipmentSlot::Legs,
        EquipmentSlot::Feet,
        EquipmentSlot::Finger,
        EquipmentSlot::Trinket,
        EquipmentSlot::MainHand,
        EquipmentSlot::OffHand,
        EquipmentSlot::Ranged,
    ]
}

fn equipment_slot_label(slot: EquipmentSlot) -> &'static str {
    match slot {
        EquipmentSlot::Head => "头部",
        EquipmentSlot::Neck => "颈部",
        EquipmentSlot::Shoulder => "肩部",
        EquipmentSlot::Back => "背部",
        EquipmentSlot::Chest => "胸部",
        EquipmentSlot::Wrist => "手腕",
        EquipmentSlot::Hands => "手",
        EquipmentSlot::Waist => "腰部",
        EquipmentSlot::Legs => "腿部",
        EquipmentSlot::Feet => "脚",
        EquipmentSlot::Finger => "戒指",
        EquipmentSlot::Trinket => "饰品",
        EquipmentSlot::MainHand => "主手",
        EquipmentSlot::OffHand => "副手",
        EquipmentSlot::Ranged => "远程",
        EquipmentSlot::None => "非装备",
    }
}

fn sync_character_buffs(
    target_id: &str,
    character: &mut PlayerCharacter,
    rule_engine_state: &mut RuleEngineState,
) {
    if character.active_buffs.is_empty() {
        if let Some(base_stats) = character.buff_base_stats.take() {
            restore_character_base_stats(character, base_stats);
        }
        sync_character_skill_rules(target_id, character, rule_engine_state);
        rule_engine_state.replace_character_buffs(target_id, Vec::new());
        return;
    }

    if character.buff_base_stats.is_none() {
        character.buff_base_stats = Some(CharacterBuffBaseStats::from_character(
            character,
        ));
    }
    let base_stats = character
        .buff_base_stats
        .expect("buff base stats are initialized for active buffs");
    sync_character_skill_rules_with_stats(
        target_id,
        character,
        &base_stats,
        rule_engine_state,
    );
    rule_engine_state.replace_character_buffs(
        target_id,
        character.active_buffs.clone(),
    );
    if let Some(effective) = rule_engine_state.character(target_id).cloned() {
        apply_effective_character_stats(character, &effective);
    }
}

fn advance_group_world_turn(
    manager: &mut NapcatMessageManager,
    group_name: &str,
    rule_engine_state: &mut RuleEngineState,
) -> bool {
    let Some(group) = manager.trpg_groups.get_mut(group_name) else {
        return false;
    };
    let players = group.players.clone();
    let changed = group.advance_world_turn();
    if changed {
        advance_buffs_for_players(
            &mut manager.player_characters,
            &players,
            rule_engine_state,
        );
    }
    changed
}

fn mark_group_player_turn(
    manager: &mut NapcatMessageManager,
    group_name: &str,
    target_id: &str,
    acted: bool,
    rule_engine_state: &mut RuleEngineState,
) -> bool {
    let Some(group) = manager.trpg_groups.get_mut(group_name) else {
        return false;
    };
    let previous_world_turn = group.world_turn;
    let players = group.players.clone();
    let changed = if acted {
        group.mark_player_acted(target_id)
    } else {
        group.mark_player_skipped(target_id)
    };
    if changed && group.world_turn > previous_world_turn {
        advance_buffs_for_players(
            &mut manager.player_characters,
            &players,
            rule_engine_state,
        );
    }
    changed
}

fn set_group_player_waiting(
    manager: &mut NapcatMessageManager,
    group_name: &str,
    target_id: &str,
) -> bool {
    let Some(group) = manager.trpg_groups.get_mut(group_name) else {
        return false;
    };
    if !group.players.iter().any(|player_id| player_id == target_id) {
        return false;
    }
    group.sync_turn_players();
    let Some(turn) = group.player_turns.get_mut(target_id) else {
        return false;
    };
    if !turn.acted && !turn.skipped {
        return false;
    }

    turn.acted = false;
    turn.skipped = false;
    true
}

fn advance_buffs_for_players(
    player_characters: &mut HashMap<String, PlayerCharacter>,
    players: &[String],
    rule_engine_state: &mut RuleEngineState,
) -> bool {
    let mut changed = false;
    for target_id in players {
        let Some(character) = player_characters.get_mut(target_id) else {
            continue;
        };
        if advance_character_buffs(character) {
            sync_character_buffs(target_id, character, rule_engine_state);
            changed = true;
        }
    }
    changed
}

fn advance_character_buffs(character: &mut PlayerCharacter) -> bool {
    if character.active_buffs.is_empty() {
        return false;
    }

    let mut changed = false;
    character.active_buffs.retain_mut(|buff| {
        if buff.turns_remaining == 0 {
            return true;
        }
        if buff.turns_remaining < 0 {
            changed = true;
            return false;
        }

        buff.turns_remaining -= 1;
        changed = true;
        buff.turns_remaining > 0
    });
    changed
}

fn restore_character_base_stats(character: &mut PlayerCharacter, stats: CharacterBuffBaseStats) {
    character.hp = stats.hp;
    character.max_hp = stats.max_hp;
    character.damage_dealt_modifier = stats.damage_dealt_modifier;
    character.damage_taken_modifier = stats.damage_taken_modifier;
    character.healing_dealt_modifier = stats.healing_dealt_modifier;
    character.healing_taken_modifier = stats.healing_taken_modifier;
}

fn apply_effective_character_stats(character: &mut PlayerCharacter, effective: &RuleCharacter) {
    character.hp = effective.hp;
    character.max_hp = effective.max_hp;
    character.damage_dealt_modifier = effective.damage_dealt_modifier;
    character.damage_taken_modifier = effective.damage_taken_modifier;
    character.healing_dealt_modifier = effective.healing_dealt_modifier;
    character.healing_taken_modifier = effective.healing_taken_modifier;
}

fn buff_kind_options() -> [BuffKind; 6] {
    [
        BuffKind::Magic,
        BuffKind::Curse,
        BuffKind::Disease,
        BuffKind::Bleed,
        BuffKind::Poison,
        BuffKind::None,
    ]
}

fn buff_kind_label(kind: BuffKind) -> &'static str {
    match kind {
        BuffKind::None => "无",
        BuffKind::Magic => "魔法",
        BuffKind::Physical => "无",
        BuffKind::Curse => "诅咒",
        BuffKind::Disease => "疾病",
        BuffKind::Bleed => "流血",
        BuffKind::Range => "无",
        BuffKind::Poison => "中毒",
    }
}

fn buff_field_combo(ui: &mut Ui, field: &mut BuffField) {
    egui::ComboBox::from_label("字段")
        .selected_text(buff_field_label(*field))
        .show_ui(ui, |ui| {
            for candidate in buff_field_options() {
                ui.selectable_value(
                    field,
                    candidate,
                    buff_field_label(candidate),
                );
            }
        });
}

fn buff_value_ui(ui: &mut Ui, value: &mut BuffValue) {
    let mut mode = match value {
        BuffValue::Add(_) => 0,
        BuffValue::AddPercent(_) => 1,
        BuffValue::Set(_) => 2,
        BuffValue::SetPercentOfBase(_) => 3,
    };
    egui::ComboBox::from_label("数值")
        .selected_text(buff_value_mode_label(mode))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut mode, 0, "增加");
            ui.selectable_value(&mut mode, 1, "增加%");
            ui.selectable_value(&mut mode, 2, "设为");
            ui.selectable_value(&mut mode, 3, "设为基础%");
        });

    let mut amount = match *value {
        BuffValue::Add(amount)
        | BuffValue::AddPercent(amount)
        | BuffValue::Set(amount)
        | BuffValue::SetPercentOfBase(amount) => amount,
    };
    ui.add(
        egui::DragValue::new(&mut amount)
            .speed(0.1)
            .range(-9999.0..=9999.0),
    );
    *value = match mode {
        0 => BuffValue::Add(amount),
        1 => BuffValue::AddPercent(amount),
        2 => BuffValue::Set(amount),
        _ => BuffValue::SetPercentOfBase(amount),
    };
}

fn buff_field_options() -> [BuffField; 18] {
    [
        BuffField::Hp,
        BuffField::Mp,
        BuffField::MaxHp,
        BuffField::MaxMp,
        BuffField::HpRegen,
        BuffField::MpRegen,
        BuffField::Status(StatusKey::Str),
        BuffField::Status(StatusKey::Agi),
        BuffField::Status(StatusKey::Dex),
        BuffField::Status(StatusKey::Vit),
        BuffField::Status(StatusKey::Int),
        BuffField::Status(StatusKey::Wis),
        BuffField::Status(StatusKey::K),
        BuffField::Status(StatusKey::Cha),
        BuffField::DamageDealtModifier,
        BuffField::DamageTakenModifier,
        BuffField::HealingDealtModifier,
        BuffField::HealingTakenModifier,
    ]
}

fn buff_field_label(field: BuffField) -> &'static str {
    match field {
        BuffField::Hp => "HP",
        BuffField::Mp => "MP",
        BuffField::MaxHp => "最大HP",
        BuffField::MaxMp => "最大MP",
        BuffField::HpRegen => "HP回复",
        BuffField::MpRegen => "MP回复",
        BuffField::Status(StatusKey::Str) => "STR",
        BuffField::Status(StatusKey::Agi) => "AGI",
        BuffField::Status(StatusKey::Dex) => "DEX",
        BuffField::Status(StatusKey::Vit) => "VIT",
        BuffField::Status(StatusKey::Int) => "INT",
        BuffField::Status(StatusKey::Wis) => "WIS",
        BuffField::Status(StatusKey::K) => "K",
        BuffField::Status(StatusKey::Cha) => "CHA",
        BuffField::DamageDealtModifier => "造成伤害",
        BuffField::DamageTakenModifier => "受到伤害",
        BuffField::HealingDealtModifier => "造成治疗",
        BuffField::HealingTakenModifier => "受到治疗",
    }
}

fn buff_value_mode_label(mode: i32) -> &'static str {
    match mode {
        0 => "增加",
        1 => "增加%",
        2 => "设为",
        _ => "设为基础%",
    }
}

fn format_buff_effect(effect: &BuffEffect) -> String {
    let value = match effect.value {
        BuffValue::Add(amount) => format!("+{}", format_character_number(amount)),
        BuffValue::AddPercent(amount) => format!("+{}%", format_character_number(amount)),
        BuffValue::Set(amount) => format!("={}", format_character_number(amount)),
        BuffValue::SetPercentOfBase(amount) => {
            format!(
                "{}%基础",
                format_character_number(amount)
            )
        },
    };
    format!(
        "{} {}",
        buff_field_label(effect.field),
        value
    )
}

fn random_pool_settings_ui(
    ui: &mut Ui,
    manager: &mut NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
    player_targets: &[String],
) -> bool {
    let mut changed = false;

    ui.heading("随机池");
    ui.horizontal_wrapped(|ui| {
        ui.label("池名");
        ui.text_edit_singleline(&mut state.new_random_pool_name);
        if ui.button("创建随机池").clicked() {
            let name = state.new_random_pool_name.trim();
            if !name.is_empty() {
                manager
                    .random_pools
                    .entry(name.to_owned())
                    .or_insert_with(RandomPool::default);
                state.new_random_pool_name.clear();
                changed = true;
            }
        }

        if !player_targets.is_empty() {
            if state.random_pool_award_target.is_empty()
                || !player_targets
                    .iter()
                    .any(|target_id| target_id == &state.random_pool_award_target)
            {
                state.random_pool_award_target = player_targets[0].clone();
            }
            egui::ComboBox::from_label("发给角色")
                .selected_text(target_display_name(
                    manager,
                    &state.random_pool_award_target,
                ))
                .show_ui(ui, |ui| {
                    for target_id in player_targets {
                        ui.selectable_value(
                            &mut state.random_pool_award_target,
                            target_id.clone(),
                            target_display_name(manager, target_id),
                        );
                    }
                });
        }
    });

    let mut pool_names = manager.random_pools.keys().cloned().collect::<Vec<_>>();
    pool_names.sort();
    if pool_names.is_empty() {
        ui.label("还没有随机池。");
        return changed;
    }

    let mut pool_to_delete = None;
    for pool_name in pool_names {
        let Some(pool) = manager.random_pools.get_mut(&pool_name) else {
            continue;
        };
        let total_weight = random_pool_total_weight(pool);
        ui.collapsing(
            format!("{pool_name} ({})", pool.entries.len()),
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("随机抽取").clicked() {
                        if let Some(entry) = pick_random_pool_entry(pool) {
                            let mut item = entry.item.clone();
                            normalize_item(&mut item);
                            pool.last_pick = Some(item.clone());
                            pool.last_text_result = random_pool_entry_text_result(&entry);
                            if let Some(character) = manager
                                .player_characters
                                .get_mut(&state.random_pool_award_target)
                            {
                                add_item_to_inventory(&mut character.inventory, item);
                            }
                            changed = true;
                        }
                    }
                    if let Some(item) = pool.last_pick.as_ref() {
                        ui.label("上次抽取");
                        ui.colored_label(
                            item_quality_color(item.quality),
                            item_display_name(item),
                        );
                    }
                    if let Some(result) = pool.last_text_result.as_ref() {
                        ui.label("上次文本");
                        ui.label(random_pool_text_result_label(result));
                    }
                    if ui.button("删除池").clicked() {
                        pool_to_delete = Some(pool_name.clone());
                    }
                });

                let mut remove_index = None;
                egui::Grid::new(ui.next_auto_id())
                    .num_columns(11)
                    .spacing([8.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("启用");
                        ui.strong("物品");
                        ui.strong("文本结果");
                        ui.strong("最小");
                        ui.strong("最大");
                        ui.strong("品质");
                        ui.strong("权重");
                        ui.strong("概率");
                        ui.strong("数量");
                        ui.strong("装备位");
                        ui.strong("操作");
                        ui.end_row();

                        for (index, entry) in pool.entries.iter_mut().enumerate() {
                            changed |= ui.checkbox(&mut entry.enabled, "").changed();
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut entry.item.name)
                                        .desired_width(120.0),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut entry.result_text)
                                        .desired_width(140.0),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut entry.min_count)
                                        .range(0..=9999)
                                        .speed(1),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut entry.max_count)
                                        .range(0..=9999)
                                        .speed(1),
                                )
                                .changed();
                            changed |= normalize_random_pool_entry(entry);
                            changed |= item_quality_combo(ui, &mut entry.item.quality);
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut entry.weight)
                                        .range(0.0..=999_999.0)
                                        .speed(0.1),
                                )
                                .changed();
                            let probability = if entry.enabled && total_weight > 0.0 {
                                entry.weight.max(0.0) / total_weight * 100.0
                            } else {
                                0.0
                            };
                            ui.label(format!("{probability:.1}%"));
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut entry.item.stack)
                                        .range(1..=9999)
                                        .speed(1),
                                )
                                .changed();
                            changed |= equipment_slot_combo(ui, &mut entry.item.equipment_slot);
                            if ui.button("-").on_hover_text("移除池项目").clicked() {
                                remove_index = Some(index);
                            }
                            ui.end_row();
                        }
                    });

                if let Some(index) = remove_index {
                    pool.entries.remove(index);
                    changed = true;
                }

                let draft = state
                    .random_pool_entry_drafts
                    .entry(pool_name.clone())
                    .or_default();
                ui.collapsing("添加池项目", |ui| {
                    random_pool_entry_draft_ui(ui, draft);
                    if ui.button("添加到随机池").clicked() {
                        normalize_random_pool_entry(draft);
                        pool.entries.push(draft.clone());
                        changed = true;
                    }
                });
            },
        );
    }

    if let Some(pool_name) = pool_to_delete {
        manager.random_pools.remove(&pool_name);
        state.random_pool_entry_drafts.remove(&pool_name);
        changed = true;
    }

    changed
}

fn random_pool_entry_draft_ui(ui: &mut Ui, draft: &mut RandomPoolEntry) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= ui.checkbox(&mut draft.enabled, "启用").changed();
        ui.label("名称");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut draft.item.name).desired_width(120.0))
            .changed();
        changed |= item_quality_combo(ui, &mut draft.item.quality);
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.weight)
                    .range(0.0..=999_999.0)
                    .speed(0.1)
                    .prefix("权重 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.item.stack)
                    .range(1..=9999)
                    .speed(1)
                    .prefix("物品数量 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.min_count)
                    .range(0..=9999)
                    .speed(1)
                    .prefix("最少出现 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.max_count)
                    .range(0..=9999)
                    .speed(1)
                    .prefix("最多出现 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.item.max_stack)
                    .range(1..=9999)
                    .speed(1)
                    .prefix("最大堆叠 "),
            )
            .changed();
        changed |= equipment_slot_combo(ui, &mut draft.item.equipment_slot);
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.item.item_level)
                    .range(0..=9999)
                    .speed(1)
                    .prefix("等级 "),
            )
            .changed();
        changed |= ui.checkbox(&mut draft.item.soulbound, "绑定").changed();
    });
    ui.label("文本结果");
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut draft.result_text)
                .desired_rows(2)
                .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
        )
        .changed();
    ui.label("说明");
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut draft.item.description)
                .desired_rows(2)
                .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
        )
        .changed();
    changed |= normalize_random_pool_entry(draft);
    changed
}

fn normalize_random_pool_entry(entry: &mut RandomPoolEntry) -> bool {
    let mut changed = normalize_item(&mut entry.item);
    let weight = entry.weight.max(0.0);
    if (entry.weight - weight).abs() > f32::EPSILON {
        entry.weight = weight;
        changed = true;
    }
    let (min_count, max_count) = normalized_random_pool_counts(entry.min_count, entry.max_count);
    if entry.min_count != min_count {
        entry.min_count = min_count;
        changed = true;
    }
    if entry.max_count != max_count {
        entry.max_count = max_count;
        changed = true;
    }
    changed
}

fn random_pool_total_weight(pool: &RandomPool) -> f32 {
    pool.entries
        .iter()
        .filter(|entry| entry.enabled)
        .map(|entry| entry.weight.max(0.0))
        .sum()
}

fn pick_random_pool_entry(pool: &RandomPool) -> Option<RandomPoolEntry> {
    let total = random_pool_total_weight(pool);
    if total <= 0.0 {
        return None;
    }

    let mut roll = rand::rng().random_range(0.0..total);
    for entry in pool.entries.iter().filter(|entry| entry.enabled) {
        let weight = entry.weight.max(0.0);
        if roll < weight {
            return Some(entry.clone());
        }
        roll -= weight;
    }
    None
}

#[cfg(test)]
fn pick_random_pool_item(pool: &RandomPool) -> Option<InventoryItem> {
    pick_random_pool_entry(pool).map(|entry| {
        let mut item = entry.item;
        normalize_item(&mut item);
        item
    })
}

fn random_pool_entry_text_result(entry: &RandomPoolEntry) -> Option<RandomPoolTextResult> {
    let text = entry.result_text.trim();
    if text.is_empty() {
        return None;
    }
    let (min_count, max_count) = normalized_random_pool_counts(entry.min_count, entry.max_count);
    let count = if min_count == max_count {
        min_count
    } else {
        rand::rng().random_range(min_count..=max_count)
    };
    Some(RandomPoolTextResult {
        entry_name: entry.item.name.clone(),
        text: text.to_owned(),
        count,
    })
}

fn random_pool_text_result_label(result: &RandomPoolTextResult) -> String {
    if result.count == 1 {
        result.text.clone()
    } else {
        format!("{} x{}", result.text, result.count)
    }
}

fn unit_pool_settings_ui(
    ui: &mut Ui,
    manager: &mut NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
    player_targets: &[String],
) -> bool {
    let mut changed = false;

    ui.heading("单位池");
    ui.horizontal_wrapped(|ui| {
        ui.label("单位ID");
        ui.add(egui::TextEdit::singleline(&mut state.new_unit_id).desired_width(140.0));
        if ui.button("新建单位").clicked() {
            let unit_id = state.new_unit_id.trim().to_owned();
            if !unit_id.is_empty() {
                let mut unit = state.unit_pool_draft.clone();
                prepare_unit_pool_entry(&unit_id, &mut unit);
                manager.unit_pool.insert(unit_id, unit);
                state.new_unit_id.clear();
                state.unit_pool_draft = UnitPoolEntry::default();
                changed = true;
            }
        }
    });

    ui.collapsing("新单位模板", |ui| {
        changed |= unit_pool_entry_editor_ui(ui, "draft", &mut state.unit_pool_draft);
    });

    if !player_targets.is_empty() {
        if state.unit_pool_source_target.is_empty()
            || !player_targets
                .iter()
                .any(|target_id| target_id == &state.unit_pool_source_target)
        {
            state.unit_pool_source_target = player_targets[0].clone();
        }

        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_label("来源角色")
                .selected_text(target_display_name(
                    manager,
                    &state.unit_pool_source_target,
                ))
                .show_ui(ui, |ui| {
                    for target_id in player_targets {
                        ui.selectable_value(
                            &mut state.unit_pool_source_target,
                            target_id.clone(),
                            target_display_name(manager, target_id),
                        );
                    }
                });

            if ui.button("从角色复制").clicked() {
                let source_id = state.unit_pool_source_target.clone();
                let unit_id = if state.new_unit_id.trim().is_empty() {
                    format!("unit-{source_id}")
                } else {
                    state.new_unit_id.trim().to_owned()
                };
                if let Some(character) = manager.player_characters.get(&source_id).cloned() {
                    let mut unit = UnitPoolEntry {
                        label: target_display_name(manager, &source_id),
                        note: "从玩家角色复制".to_owned(),
                        character,
                    };
                    prepare_unit_pool_entry(&unit_id, &mut unit);
                    manager.unit_pool.insert(unit_id, unit);
                    state.new_unit_id.clear();
                    changed = true;
                }
            }
        });
    } else {
        ui.small("还没有可复制的玩家角色。");
    }

    let mut unit_ids = manager.unit_pool.keys().cloned().collect::<Vec<_>>();
    unit_ids.sort();
    if unit_ids.is_empty() {
        ui.label("还没有单位模板。");
        return changed;
    }

    let mut unit_to_delete = None;
    for unit_id in unit_ids {
        let title = manager
            .unit_pool
            .get(&unit_id)
            .map(|unit| unit_pool_entry_title(&unit_id, unit))
            .unwrap_or_else(|| unit_id.clone());
        ui.collapsing(title, |ui| {
            if let Some(unit) = manager.unit_pool.get_mut(&unit_id) {
                ui.horizontal_wrapped(|ui| {
                    ui.small(format!("ID {unit_id}"));
                    if ui.button("删除单位").clicked() {
                        unit_to_delete = Some(unit_id.clone());
                    }
                });
                changed |= unit_pool_entry_editor_ui(ui, &unit_id, unit);
            }
        });
    }

    if let Some(unit_id) = unit_to_delete {
        manager.unit_pool.remove(&unit_id);
        changed = true;
    }

    changed
}

fn prepare_unit_pool_entry(unit_id: &str, unit: &mut UnitPoolEntry) {
    let label = unit.label.trim();
    if label.is_empty() {
        unit.label = unit_id.to_owned();
    }

    let label = unit.label.trim();
    if unit.character.nickname.trim().is_empty() {
        unit.character.nickname = label.to_owned();
    }
    if unit.character.name.trim().is_empty() {
        unit.character.name = label.to_owned();
    }
    unit.character.inited = true;
    update_character_from_status(&mut unit.character);
}

fn unit_pool_entry_title(unit_id: &str, unit: &UnitPoolEntry) -> String {
    let label = unit.label.trim();
    if label.is_empty() {
        unit_id.to_owned()
    } else {
        format!("{label} ({unit_id})")
    }
}

fn unit_pool_entry_editor_ui(ui: &mut Ui, unit_id: &str, unit: &mut UnitPoolEntry) -> bool {
    let mut changed = false;

    ui.horizontal_wrapped(|ui| {
        ui.label("显示名");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut unit.label).desired_width(160.0))
            .changed();
    });
    ui.label("备注");
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut unit.note)
                .desired_rows(2)
                .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
        )
        .changed();

    changed |= unit_character_template_editor_ui(ui, unit_id, &mut unit.character);
    changed
}

fn unit_character_template_editor_ui(
    ui: &mut Ui,
    unit_id: &str,
    character: &mut PlayerCharacter,
) -> bool {
    let mut changed = false;
    let mut derived_stats_changed = false;

    ui.horizontal_wrapped(|ui| {
        changed |= ui.checkbox(&mut character.inited, "已完成").changed();
        ui.label("角色名");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut character.name).desired_width(120.0))
            .changed();
        ui.label("昵称");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut character.nickname).desired_width(120.0))
            .changed();
    });

    ui.horizontal_wrapped(|ui| {
        let level_response = ui
            .add(
                egui::DragValue::new(&mut character.level)
                    .range(1..=999)
                    .prefix("等级 "),
            )
            .changed();
        changed |= level_response;
        derived_stats_changed |= level_response;
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.exp)
                    .range(0..=999_999)
                    .prefix("经验 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.speed)
                    .range(0.0..=9999.0)
                    .speed(0.1)
                    .prefix("速度 "),
            )
            .changed();
    });

    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.hp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("HP "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.max_hp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("/ "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.mp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("MP "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.max_mp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("/ "),
            )
            .changed();
    });

    let status_changed = unit_character_status_editor_ui(ui, &mut character.status);
    changed |= status_changed;
    derived_stats_changed |= status_changed;

    if derived_stats_changed {
        update_character_from_status(character);
        changed = true;
    }
    if character.hp > character.max_hp {
        character.hp = character.max_hp;
        changed = true;
    }
    if character.mp > character.max_mp {
        character.mp = character.max_mp;
        changed = true;
    }

    character_status_summary_ui(ui, character);
    ui.horizontal_wrapped(|ui| {
        ui.small(format!(
            "技能 {}",
            character.skill_names.len()
        ));
        ui.small(format!(
            "背包 {}",
            character.inventory.items.len()
        ));
        ui.small(format!("模板 {unit_id}"));
    });

    changed
}

fn unit_character_status_editor_ui(ui: &mut Ui, status: &mut CharacterStatus) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.str_)
                    .range(0..=999)
                    .prefix("STR "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.agi)
                    .range(0..=999)
                    .prefix("AGI "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.dex)
                    .range(0..=999)
                    .prefix("DEX "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.vit)
                    .range(0..=999)
                    .prefix("VIT "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.int_)
                    .range(0..=999)
                    .prefix("INT "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.wis)
                    .range(0..=999)
                    .prefix("WIS "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.k)
                    .range(0..=999)
                    .prefix("K "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.cha)
                    .range(0..=999)
                    .prefix("CHA "),
            )
            .changed();
    });
    changed
}

fn skill_pool_settings_ui(
    ui: &mut Ui,
    manager: &mut NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
) -> bool {
    let mut changed = false;
    changed |= manager.sync_skill_pool_from_completed_characters();

    let auto_count = manager
        .skill_pool
        .iter()
        .filter(|entry| entry.source_key().is_some())
        .count();
    let manual_count = manager.skill_pool.len().saturating_sub(auto_count);
    ui.heading("技能池");
    ui.horizontal_wrapped(|ui| {
        ui.small(format!("已兑换技能 {auto_count}"));
        ui.small(format!("手动技能 {manual_count}"));
        if ui.button("刷新已兑换技能").clicked() {
            changed |= manager.sync_skill_pool_from_completed_characters();
        }
    });

    if manager.skill_pool.is_empty() {
        ui.label("还没有技能。完成角色兑换后，技能会自动进入这里。");
    } else {
        let mut remove_manual_index = None;
        egui::ScrollArea::vertical()
            .id_salt("skill_pool_settings")
            .max_height(180.0)
            .show(ui, |ui| {
                egui::Grid::new(ui.next_auto_id())
                    .num_columns(7)
                    .spacing([8.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("技能");
                        ui.strong("类型");
                        ui.strong("标签");
                        ui.strong("来源");
                        ui.strong("MP");
                        ui.strong("冷却");
                        ui.strong("操作");
                        ui.end_row();

                        for (index, entry) in manager.skill_pool.iter().enumerate() {
                            ui.label(skill_pool_entry_name(entry));
                            ui.small(skill_pool_entry_category_label(entry));
                            ui.small(skill_pool_entry_tags_label(entry));
                            ui.small(entry.source_character_name.as_deref().unwrap_or("手动"));
                            ui.label(format_character_number(entry.mp_cost));
                            ui.label(entry.cooldown_turns.to_string());
                            if entry.source_key().is_none() {
                                if ui.button("-").on_hover_text("移除手动技能").clicked() {
                                    remove_manual_index = Some(index);
                                }
                            } else {
                                ui.small("自动");
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(index) = remove_manual_index {
            manager.skill_pool.remove(index);
            changed = true;
        }
    }

    ui.collapsing("添加手动技能", |ui| {
        let draft = &mut state.skill_pool_draft;
        ui.horizontal_wrapped(|ui| {
            ui.label("技能名");
            ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(140.0));
            ui.add(
                egui::DragValue::new(&mut draft.mp_cost)
                    .range(0.0..=9999.0)
                    .speed(1.0)
                    .prefix("MP "),
            );
            ui.add(
                egui::DragValue::new(&mut draft.cooldown_turns)
                    .range(0..=999)
                    .speed(1)
                    .prefix("冷却 "),
            );
            ui.label("类型");
            ui.add(
                egui::TextEdit::singleline(draft.category.get_or_insert_with(String::new))
                    .desired_width(100.0),
            );
            ui.label("标签");
            let mut tags = draft.tags.join(" ");
            if ui
                .add(egui::TextEdit::singleline(&mut tags).desired_width(140.0))
                .changed()
            {
                draft.tags = tags.split_whitespace().map(str::to_owned).collect();
            }
        });
        ui.label("规则描述");
        ui.add(
            egui::TextEdit::multiline(&mut draft.note)
                .desired_rows(2)
                .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
        );
        if ui.button("加入技能池").clicked() {
            if let Some(mut entry) = normalized_manual_skill_pool_entry(draft) {
                entry.source_character_id = None;
                entry.source_character_name = None;
                entry.source_skill_index = None;
                manager.skill_pool.push(entry);
                state.skill_pool_draft = SkillPoolEntry::default();
                changed = true;
            }
        }
    });

    changed
}

fn normalized_manual_skill_pool_entry(draft: &SkillPoolEntry) -> Option<SkillPoolEntry> {
    let note = draft.note.trim();
    if note.is_empty() {
        return None;
    }
    let name = if draft.name.trim().is_empty() {
        "未命名技能".to_owned()
    } else {
        draft.name.trim().to_owned()
    };
    Some(SkillPoolEntry {
        name,
        note: note.to_owned(),
        mp_cost: draft.mp_cost.max(0.0),
        cooldown_turns: draft.cooldown_turns,
        source_character_id: None,
        source_character_name: None,
        source_skill_index: None,
        tags: draft.tags.clone(),
        category: draft.category.clone(),
        args: draft.args.clone(),
        ..Default::default()
    })
}

fn skill_pool_entry_name(entry: &SkillPoolEntry) -> String {
    if entry.name.trim().is_empty() {
        "未命名技能".to_owned()
    } else {
        entry.name.trim().to_owned()
    }
}

fn skill_pool_entry_category_label(entry: &SkillPoolEntry) -> String {
    entry
        .category
        .as_deref()
        .filter(|category| !category.trim().is_empty())
        .unwrap_or("-")
        .to_owned()
}

fn skill_pool_entry_tags_label(entry: &SkillPoolEntry) -> String {
    if entry.tags.is_empty() {
        "-".to_owned()
    } else {
        entry.tags.join(" ")
    }
}

fn skill_pool_entry_legacy_label(entry: &SkillPoolEntry) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(id) = entry
        .legacy_pool_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        parts.push(format!("旧ID {id}"));
    }
    if !entry.args.is_empty() {
        let args = entry
            .args
            .iter()
            .map(|arg| {
                let name =
                    if arg.name.trim().is_empty() { "未命名变量" } else { arg.name.trim() };
                if arg.kind.trim().is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}:{}", arg.kind.trim())
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("变量 {args}"));
    }
    if entry.legacy_buff_count > 0 {
        parts.push(format!(
            "旧BUFF {}",
            entry.legacy_buff_count
        ));
    }
    if entry.legacy_event_buff_count > 0 {
        parts.push(format!(
            "旧事件BUFF {}",
            entry.legacy_event_buff_count
        ));
    }
    if entry.legacy_has_graph {
        parts.push("含旧蓝图".to_owned());
    }
    (!parts.is_empty()).then(|| parts.join("；"))
}

fn ensure_import_export_paths(state: &mut TrpgGroupSettingsState) {
    if state.export_path.trim().is_empty() {
        state.export_path = NAPCAT_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.pc_export_path.trim().is_empty() {
        state.pc_export_path = NAPCAT_PC_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.chat_list_export_path.trim().is_empty() {
        state.chat_list_export_path = NAPCAT_CHAT_LIST_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.unit_pool_export_path.trim().is_empty() {
        state.unit_pool_export_path = NAPCAT_UNIT_POOL_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.moonberry_legacy_import_path.trim().is_empty() {
        state.moonberry_legacy_import_path = NAPCAT_MOONBERRY_LEGACY_IMPORT_DEFAULT_PATH.to_owned();
    }
    if state.deepseek_summary_export_path.trim().is_empty() {
        state.deepseek_summary_export_path = DEEPSEEK_SUMMARY_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.voxel_scene_export_path.trim().is_empty() {
        state.voxel_scene_export_path = VOXEL_SCENE_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.import_path.trim().is_empty() {
        state.import_path = state.export_path.clone();
    }
}

fn napcat_import_export_ui(
    ui: &mut Ui,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    deepseek_manager: &mut ResMut<Persistent<DeepseekManager>>,
    mut scene_store: Option<&mut Persistent<VoxelSceneStore>>,
    mut scene_runtime: Option<&mut VoxelMapRuntimeState>,
    state: &mut TrpgGroupSettingsState,
) {
    ensure_import_export_paths(state);

    ui.collapsing("导入/导出", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "NapCat格式版本 {} / DeepSeek总结版本 {} / 场景版本 {}",
                NAPCAT_MANAGER_EXPORT_VERSION,
                DEEPSEEK_SUMMARY_EXPORT_VERSION,
                VOXEL_SCENE_EXPORT_VERSION
            ));
            if !state.import_export_status.trim().is_empty() {
                ui.small(state.import_export_status.as_str());
            }
        });

        ui.horizontal(|ui| {
            ui.label("导出路径");
            ui.text_edit_singleline(&mut state.export_path);
            if ui.button("导出").clicked() {
                match write_napcat_manager_export(manager, &state.export_path) {
                    Ok(()) => {
                        state.import_export_status = format!("已导出到 {}", state.export_path);
                    },
                    Err(err) => {
                        state.import_export_status = format!("导出失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("PC文件");
            ui.text_edit_singleline(&mut state.pc_export_path);
            if ui.button("导出PC").clicked() {
                match write_text_export(
                    &state.pc_export_path,
                    manager.to_player_characters_export_json(),
                ) {
                    Ok(()) => {
                        state.import_export_status = format!("已导出PC到 {}", state.pc_export_path);
                    },
                    Err(err) => {
                        state.import_export_status = format!("PC导出失败：{err}");
                    },
                }
            }
            if ui.button("导入PC").clicked() {
                match read_text_import(&state.pc_export_path)
                    .and_then(|text| manager.merge_player_characters_export_json(&text))
                {
                    Ok(count) => match manager.persist() {
                        Ok(()) => {
                            state.import_export_status = format!("已导入{}个PC", count);
                        },
                        Err(err) => {
                            state.import_export_status = format!("PC导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("PC导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("聊天列表文件");
            ui.text_edit_singleline(&mut state.chat_list_export_path);
            if ui.button("导出聊天列表").clicked() {
                match write_text_export(
                    &state.chat_list_export_path,
                    manager.to_chat_list_export_json(),
                ) {
                    Ok(()) => {
                        state.import_export_status = format!(
                            "已导出聊天列表到 {}",
                            state.chat_list_export_path
                        );
                    },
                    Err(err) => {
                        state.import_export_status = format!("聊天列表导出失败：{err}");
                    },
                }
            }
            if ui.button("导入聊天列表").clicked() {
                match read_text_import(&state.chat_list_export_path)
                    .and_then(|text| manager.merge_chat_list_export_json(&text))
                {
                    Ok(count) => match manager.persist() {
                        Ok(()) => {
                            state.import_export_status = format!("已导入{}个聊天目标", count);
                        },
                        Err(err) => {
                            state.import_export_status = format!("聊天列表导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("聊天列表导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("单位池文件");
            ui.text_edit_singleline(&mut state.unit_pool_export_path);
            if ui.button("导出单位池").clicked() {
                match write_text_export(
                    &state.unit_pool_export_path,
                    manager.to_unit_pool_export_json(),
                ) {
                    Ok(()) => {
                        state.import_export_status = format!(
                            "已导出单位池到 {}",
                            state.unit_pool_export_path
                        );
                    },
                    Err(err) => {
                        state.import_export_status = format!("单位池导出失败：{err}");
                    },
                }
            }
            if ui.button("导入单位池").clicked() {
                match read_text_import(&state.unit_pool_export_path)
                    .and_then(|text| manager.merge_unit_pool_export_json(&text))
                {
                    Ok(count) => match manager.persist() {
                        Ok(()) => {
                            state.import_export_status = format!("已导入{}个单位模板", count);
                        },
                        Err(err) => {
                            state.import_export_status = format!("单位池导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("单位池导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("月莓旧JSON");
            ui.text_edit_singleline(&mut state.moonberry_legacy_import_path);
            if ui.button("导入月莓旧JSON").clicked() {
                match read_text_import(&state.moonberry_legacy_import_path)
                    .and_then(|text| manager.merge_moonberry_legacy_json(&text))
                {
                    Ok(summary) => match manager.persist() {
                        Ok(()) => {
                            state.import_export_status = format!(
                                "已导入月莓旧JSON：{}个团，{}个PC，{}个聊天目标，{}条消息，{}个技能池，{}个单位模板，{}个随机池",
                                summary.groups,
                                summary.players,
                                summary.chat_targets,
                                summary.messages,
                                summary.skill_pools,
                                summary.unit_templates,
                                summary.random_pools
                            );
                        },
                        Err(err) => {
                            state.import_export_status = format!("月莓旧JSON导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("月莓旧JSON导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("DeepSeek总结文件");
            ui.text_edit_singleline(&mut state.deepseek_summary_export_path);
            if ui.button("导出总结").clicked() {
                match write_text_export(
                    &state.deepseek_summary_export_path,
                    deepseek_manager.to_summary_export_json(),
                ) {
                    Ok(()) => {
                        state.import_export_status = format!(
                            "已导出DeepSeek总结到 {}",
                            state.deepseek_summary_export_path
                        );
                    },
                    Err(err) => {
                        state.import_export_status = format!("DeepSeek总结导出失败：{err}");
                    },
                }
            }
            if ui.button("导入总结").clicked() {
                match read_text_import(&state.deepseek_summary_export_path)
                    .and_then(|text| deepseek_manager.merge_summary_export_json(&text))
                {
                    Ok(count) => match deepseek_manager.persist() {
                        Ok(()) => {
                            state.import_export_status = format!("已导入{}个DeepSeek总结", count);
                        },
                        Err(err) => {
                            state.import_export_status = format!("DeepSeek总结导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("DeepSeek总结导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("体素场景文件");
            ui.text_edit_singleline(&mut state.voxel_scene_export_path);
            if ui.button("导出场景").clicked() {
                let result = scene_store
                    .as_deref()
                    .ok_or_else(|| "场景存储未就绪".to_owned())
                    .and_then(|store| {
                        write_text_export(
                            &state.voxel_scene_export_path,
                            store.to_export_json(),
                        )
                    });
                match result {
                    Ok(()) => {
                        state.import_export_status = format!(
                            "已导出体素场景到 {}",
                            state.voxel_scene_export_path
                        );
                    },
                    Err(err) => {
                        state.import_export_status = format!("体素场景导出失败：{err}");
                    },
                }
            }
            if ui.button("导入场景").clicked() {
                let result = scene_store
                    .as_deref_mut()
                    .ok_or_else(|| "场景存储未就绪".to_owned())
                    .and_then(|store| {
                        let text = read_text_import(&state.voxel_scene_export_path)?;
                        let count = store.merge_export_json(&text)?;
                        store.persist().map_err(|err| err.to_string())?;
                        if let Some(runtime) = scene_runtime.as_deref_mut() {
                            runtime.request_reload();
                        }
                        Ok(count)
                    });
                match result {
                    Ok(count) => {
                        state.import_export_status = format!("已导入{}张体素地图", count);
                    },
                    Err(err) => {
                        state.import_export_status = format!("体素场景导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("导入路径");
            ui.text_edit_singleline(&mut state.import_path);
            if ui.button("导入").clicked() {
                match read_napcat_manager_export(&state.import_path) {
                    Ok(imported) => match manager.set(imported) {
                        Ok(()) => {
                            state.import_export_status = format!("已从 {} 导入", state.import_path);
                        },
                        Err(err) => {
                            state.import_export_status = format!("导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("导入失败：{err}");
                    },
                }
            }
        });
    });
}

fn write_napcat_manager_export(manager: &NapcatMessageManager, path: &str) -> Result<(), String> {
    write_text_export(path, manager.to_export_json())
}

fn write_text_export(path: &str, text: Result<String, String>) -> Result<(), String> {
    let path = Path::new(path.trim());
    if path.as_os_str().is_empty() {
        return Err("路径不能为空".to_owned());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(path, text?).map_err(|err| err.to_string())
}

fn read_napcat_manager_export(path: &str) -> Result<NapcatMessageManager, String> {
    let text = read_text_import(path)?;
    NapcatMessageManager::from_export_json(&text)
}

fn read_text_import(path: &str) -> Result<String, String> {
    let path = Path::new(path.trim());
    if path.as_os_str().is_empty() {
        return Err("路径不能为空".to_owned());
    }
    fs::read_to_string(path).map_err(|err| err.to_string())
}

fn trpg_basic_config_ui(ui: &mut Ui, config: &mut TrpgBasicConfig) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= f32_config_drag(
            ui,
            "基础HP",
            &mut config.base_max_hp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "等级HP",
            &mut config.lv_max_hp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "力量HP",
            &mut config.str_max_hp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "体质HP",
            &mut config.vit_max_hp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "体质回复",
            &mut config.vit_hp_reg,
            -999.0..=999.0,
            0.1,
        );
    });
    ui.horizontal_wrapped(|ui| {
        changed |= f32_config_drag(
            ui,
            "智力MP",
            &mut config.int_max_mp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "智慧MP",
            &mut config.wis_max_mp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "智慧回蓝",
            &mut config.wis_mp_reg,
            -999.0..=999.0,
            0.1,
        );
        changed |= f32_config_drag(
            ui,
            "基础速度",
            &mut config.basic_speed,
            0.0..=999.0,
            0.1,
        );
        changed |= f32_config_drag(
            ui,
            "力量速度",
            &mut config.str_speed,
            -999.0..=999.0,
            0.1,
        );
        changed |= f32_config_drag(
            ui,
            "敏捷速度",
            &mut config.agi_speed,
            -999.0..=999.0,
            0.1,
        );
        changed |= f32_config_drag(
            ui,
            "灵巧速度",
            &mut config.dex_speed,
            -999.0..=999.0,
            0.1,
        );
    });
    ui.collapsing("伤害与治疗系数", |ui| {
        ui.horizontal_wrapped(|ui| {
            changed |= f32_config_drag(
                ui,
                "力量伤害",
                &mut config.str_damage_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "智力伤害",
                &mut config.int_damage_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "敏捷伤害",
                &mut config.agi_damage_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "灵巧伤害",
                &mut config.dex_damage_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "灵巧远程",
                &mut config.dex_range_damage_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "智力治疗",
                &mut config.int_heal_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "智慧治疗",
                &mut config.wis_heal_bonus,
                -99.0..=99.0,
                0.001,
            );
        });
    });
    ui.horizontal_wrapped(|ui| {
        changed |= f32_config_drag(
            ui,
            "升级经验",
            &mut config.exp_gain_per_level,
            0.0..=9999.0,
            0.1,
        );
        changed |= f32_config_drag(
            ui,
            "PVP经验",
            &mut config.exp_gain_per_level_pvp,
            0.0..=9999.0,
            0.01,
        );
        if ui.button("恢复默认公式").clicked() {
            *config = TrpgBasicConfig::default();
            changed = true;
        }
    });
    changed
}

fn f32_config_drag(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    speed: f64,
) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .range(range)
            .speed(speed)
            .prefix(format!("{label} ")),
    )
    .changed()
}

fn trpg_group_settings_window(
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    deepseek_manager: &mut ResMut<Persistent<DeepseekManager>>,
    scene_store: Option<&mut Persistent<VoxelSceneStore>>,
    scene_runtime: Option<&mut VoxelMapRuntimeState>,
    state: &mut TrpgGroupSettingsState,
    character_edit_state: &mut CharacterEditState,
    rule_engine_state: &mut RuleEngineState,
) {
    if !state.open {
        return;
    }

    let player_targets = sorted_pool_targets(manager, false);
    let group_chat_targets = sorted_pool_targets(manager, true);
    let mut changed = false;
    let mut group_to_delete = None;
    let mut character_to_delete = None;
    let mut turn_action: Option<(String, String, bool)> = None;
    let mut turn_reset: Option<String> = None;
    let mut turn_advance: Option<String> = None;
    let mut settings_open = state.open;

    egui::Window::new("TRPG设置")
        .id(Id::new("trpg_group_settings_window"))
        .open(&mut settings_open)
        .default_size(Vec2::new(620.0, 520.0))
        .min_width(420.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("TRPG组");
                ui.text_edit_singleline(&mut state.new_group_name);
                if ui.button("创建").clicked() {
                    let name = state.new_group_name.trim();
                    if !name.is_empty() {
                        manager.trpg_groups.entry(name.to_owned()).or_default();
                        if manager.current_trpg_group.is_none() {
                            manager.current_trpg_group = Some(name.to_owned());
                        }
                        state.new_group_name.clear();
                        changed = true;
                    }
                }
            });

            napcat_import_export_ui(
                ui,
                manager,
                deepseek_manager,
                scene_store,
                scene_runtime,
                state,
            );
            ui.separator();

            ui.heading("玩家角色");
            if player_targets.is_empty() {
                ui.label("还没有玩家私聊。");
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("player_character_settings")
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for target_id in &player_targets {
                            let display_name = target_display_name(manager, target_id);
                            let skill_pool_snapshot = manager.skill_pool.clone();
                            let stat_config = manager.character_stat_config_for_target(target_id);
                            let character = manager
                                .player_characters
                                .entry(target_id.clone())
                                .or_default();
                            ui.collapsing(
                                format!("{display_name} ({target_id})"),
                                |ui| {
                                    character_status_summary_ui(ui, character);
                                    ui.horizontal(|ui| {
                                        let pending_delete =
                                            state.pending_character_delete.as_deref()
                                                == Some(target_id.as_str());
                                        if pending_delete {
                                            ui.label("确认删除？");
                                            if ui.button("删除角色").clicked() {
                                                character_to_delete = Some(target_id.clone());
                                            }
                                            if ui.button("取消").clicked() {
                                                state.pending_character_delete = None;
                                            }
                                        } else if ui.button("删除角色").clicked() {
                                            state.pending_character_delete =
                                                Some(target_id.clone());
                                        }
                                    });
                                    ui.separator();
                                    ui.collapsing("编辑角色", |ui| {
                                        changed |= character_editor_ui(
                                            ui,
                                            target_id,
                                            character,
                                            &display_name,
                                            character_edit_state,
                                            rule_engine_state,
                                            &skill_pool_snapshot,
                                            stat_config,
                                        );
                                    });
                                },
                            );
                        }
                    });
            }

            ui.separator();
            ui.heading("TRPG组成员");

            let mut group_names = manager.trpg_groups.keys().cloned().collect::<Vec<_>>();
            group_names.sort();
            if group_names.is_empty() {
                ui.label("先创建TRPG组，再分配玩家和群聊。");
                return;
            }

            let mut current_group = manager.current_trpg_group.clone().unwrap_or_default();
            egui::ComboBox::from_label("当前TRPG组")
                .selected_text(if current_group.is_empty() {
                    "无"
                } else {
                    current_group.as_str()
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut current_group, String::new(), "无");
                    for group_name in &group_names {
                        ui.selectable_value(
                            &mut current_group,
                            group_name.clone(),
                            group_name,
                        );
                    }
                });
            let next_current_group = (!current_group.is_empty()).then_some(current_group);
            if manager.current_trpg_group != next_current_group {
                manager.current_trpg_group = next_current_group;
                changed = true;
            }
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .id_salt("trpg_group_membership_settings")
                .show(ui, |ui| {
                    for group_name in group_names {
                        let Some(snapshot) = manager.trpg_groups.get(&group_name).cloned() else {
                            continue;
                        };
                        let group_response = ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.heading(&group_name);
                                ui.small(format!(
                                    "{}个目标，世界轮次{}",
                                    trpg_group_member_count(&snapshot),
                                    snapshot.world_turn
                                ));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("删除").clicked() {
                                            group_to_delete = Some(group_name.clone());
                                        }
                                    },
                                );
                            });

                            ui.horizontal_wrapped(|ui| {
                                if ui.button("推进轮次").clicked() {
                                    turn_advance = Some(group_name.clone());
                                }
                                if ui.button("重置行动").clicked() {
                                    turn_reset = Some(group_name.clone());
                                }
                            });

                            ui.collapsing("团设与建卡规则", |ui| {
                                if let Some(group) = manager.trpg_groups.get_mut(&group_name) {
                                    ui.horizontal(|ui| {
                                        ui.label("活动ID");
                                        changed |= ui
                                            .add(
                                                egui::TextEdit::singleline(&mut group.campaign_id)
                                                    .desired_width(180.0),
                                            )
                                            .changed();
                                        ui.label("初始属性点");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(
                                                    &mut group.initial_status_points,
                                                )
                                                .range(0..=999),
                                            )
                                            .changed();
                                        ui.label("初始技能点");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(
                                                    &mut group.initial_exchange_points,
                                                )
                                                .range(0..=999),
                                            )
                                            .changed();
                                        changed |= ui
                                            .checkbox(
                                                &mut group.allow_join_requests,
                                                "允许入团请求",
                                            )
                                            .changed();
                                    });

                                    ui.label("公开说明");
                                    changed |= ui
                                        .add(
                                            egui::TextEdit::multiline(&mut group.description)
                                                .desired_width(ui.available_width())
                                                .desired_rows(2),
                                        )
                                        .changed();
                                    ui.label("GM说明");
                                    changed |= ui
                                        .add(
                                            egui::TextEdit::multiline(&mut group.st_description)
                                                .desired_width(ui.available_width())
                                                .desired_rows(2),
                                        )
                                        .changed();
                                    ui.label("玩家引导");
                                    changed |= ui
                                        .add(
                                            egui::TextEdit::multiline(&mut group.guide)
                                                .desired_width(ui.available_width())
                                                .desired_rows(3),
                                        )
                                        .changed();
                                    ui.collapsing("属性公式", |ui| {
                                        changed |=
                                            trpg_basic_config_ui(ui, &mut group.basic_config);
                                    });
                                }
                            });

                            if snapshot.players.is_empty() {
                                ui.small("这个TRPG轮次组里没有玩家。");
                            } else {
                                ui.label("轮次状态");
                                for target_id in &snapshot.players {
                                    let turn = snapshot.player_turns.get(target_id);
                                    let turns_passed =
                                        turn.map(|turn| turn.turns_passed).unwrap_or_default();
                                    let acted = turn.map(|turn| turn.acted).unwrap_or_default();
                                    let skipped = turn.map(|turn| turn.skipped).unwrap_or_default();
                                    let status = if acted {
                                        "已行动"
                                    } else if skipped {
                                        "已跳过"
                                    } else {
                                        "等待中"
                                    };
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(target_display_name(manager, target_id));
                                        ui.small(format!("{}轮", turns_passed));
                                        ui.small(status);
                                        if ui.button("行动").clicked() {
                                            turn_action = Some((
                                                group_name.clone(),
                                                target_id.clone(),
                                                true,
                                            ));
                                        }
                                        if ui.button("跳过").clicked() {
                                            turn_action = Some((
                                                group_name.clone(),
                                                target_id.clone(),
                                                false,
                                            ));
                                        }
                                    });
                                }
                                ui.separator();
                            }

                            ui.collapsing("小队与可见性", |ui| {
                                let draft = state
                                    .party_name_drafts
                                    .entry(group_name.clone())
                                    .or_default();
                                ui.horizontal(|ui| {
                                    ui.label("新小队");
                                    ui.text_edit_singleline(draft);
                                    if ui.button("创建小队").clicked() {
                                        let party_name = draft.trim().to_owned();
                                        if !party_name.is_empty() {
                                            if let Some(group) =
                                                manager.trpg_groups.get_mut(&group_name)
                                            {
                                                changed |= group.ensure_party(&party_name);
                                            }
                                            draft.clear();
                                        }
                                    }
                                });

                                let mut party_names =
                                    snapshot.parties.keys().cloned().collect::<Vec<_>>();
                                party_names.sort();

                                if snapshot.players.is_empty() {
                                    ui.small("这个TRPG组里没有可分配的小队玩家。");
                                } else {
                                    for target_id in &snapshot.players {
                                        let display_name = target_display_name(manager, target_id);
                                        let mut selected_party = snapshot
                                            .party_id_for_player(target_id)
                                            .unwrap_or_default()
                                            .to_owned();
                                        let before_party = selected_party.clone();
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(display_name);
                                            egui::ComboBox::from_id_salt((
                                                "party_assignment",
                                                &group_name,
                                                target_id,
                                            ))
                                            .selected_text(if selected_party.is_empty() {
                                                "无小队"
                                            } else {
                                                selected_party.as_str()
                                            })
                                            .show_ui(
                                                ui,
                                                |ui| {
                                                    ui.selectable_value(
                                                        &mut selected_party,
                                                        String::new(),
                                                        "无小队",
                                                    );
                                                    for party_name in &party_names {
                                                        ui.selectable_value(
                                                            &mut selected_party,
                                                            party_name.clone(),
                                                            party_name,
                                                        );
                                                    }
                                                },
                                            );

                                            if let Ok(user_id) = target_id.parse::<u64>() {
                                                let mut is_gm =
                                                    snapshot.gm_users.contains(&user_id);
                                                if ui.checkbox(&mut is_gm, "GM").changed() {
                                                    if let Some(group) =
                                                        manager.trpg_groups.get_mut(&group_name)
                                                    {
                                                        if is_gm {
                                                            changed |=
                                                                group.gm_users.insert(user_id);
                                                        } else {
                                                            changed |=
                                                                group.gm_users.remove(&user_id);
                                                        }
                                                    }
                                                }
                                            }
                                        });

                                        if selected_party != before_party {
                                            if let Some(group) =
                                                manager.trpg_groups.get_mut(&group_name)
                                            {
                                                changed |= group.set_player_party(
                                                    target_id,
                                                    (!selected_party.is_empty())
                                                        .then_some(selected_party.as_str()),
                                                );
                                            }
                                        }
                                    }
                                }
                            });

                            ui.columns(2, |columns| {
                                columns[0].label("玩家");
                                for target_id in &player_targets {
                                    let mut selected = snapshot.players.contains(target_id);
                                    if columns[0]
                                        .checkbox(
                                            &mut selected,
                                            target_display_name(manager, target_id),
                                        )
                                        .on_hover_text(target_id)
                                        .changed()
                                    {
                                        if let Some(group) =
                                            manager.trpg_groups.get_mut(&group_name)
                                        {
                                            set_target_membership(
                                                &mut group.players,
                                                target_id,
                                                selected,
                                            );
                                            group.sync_turn_players();
                                            group.sync_parties();
                                            changed = true;
                                        }
                                    }
                                }

                                columns[1].label("群聊");
                                for target_id in &group_chat_targets {
                                    let mut selected = snapshot.group_chats.contains(target_id);
                                    if columns[1]
                                        .checkbox(
                                            &mut selected,
                                            target_display_name(manager, target_id),
                                        )
                                        .on_hover_text(target_id)
                                        .changed()
                                    {
                                        if let Some(group) =
                                            manager.trpg_groups.get_mut(&group_name)
                                        {
                                            set_target_membership(
                                                &mut group.group_chats,
                                                target_id,
                                                selected,
                                            );
                                            changed = true;
                                        }
                                    }
                                }
                            });
                        });
                        if state.focused_group_name.as_deref() == Some(group_name.as_str()) {
                            group_response
                                .response
                                .scroll_to_me(Some(egui::Align::Center));
                            state.focused_group_name = None;
                        }
                        ui.add_space(6.0);
                    }
                });
        });
    state.open = settings_open;

    if let Some((group_name, target_id, acted)) = turn_action {
        changed |= mark_group_player_turn(
            manager.as_mut(),
            &group_name,
            &target_id,
            acted,
            rule_engine_state,
        );
    }
    if let Some(group_name) = turn_reset {
        if let Some(group) = manager.trpg_groups.get_mut(&group_name) {
            changed |= group.reset_current_turn();
        }
    }
    if let Some(group_name) = turn_advance {
        changed |= advance_group_world_turn(
            manager.as_mut(),
            &group_name,
            rule_engine_state,
        );
    }

    if let Some(group_name) = group_to_delete {
        manager.trpg_groups.remove(&group_name);
        if manager.current_trpg_group.as_deref() == Some(group_name.as_str()) {
            manager.current_trpg_group = None;
        }
        changed = true;
    }
    if let Some(target_id) = character_to_delete {
        manager
            .player_characters
            .insert(target_id, PlayerCharacter::default());
        state.pending_character_delete = None;
        changed = true;
    }

    if changed {
        manager.persist().ok();
    }
}

pub fn ui_system(
    mut contexts: EguiContexts,
    mut ime: ResMut<ImeManager>,
    napcat_sender: Option<Res<NapcatIOSender>>,
    deepseek_sender: Option<Res<DeepseekIOSender>>,
    mut deepseek_manager: ResMut<Persistent<DeepseekManager>>,
    mut send_manager: ResMut<NapcatSendManager>,
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
    mut cached_memory: ResMut<Persistent<CachedMemory>>,
    mut locals: UiSystemLocals,
    mut rule_engine_state: ResMut<RuleEngineState>,
    mut battle_round_state: ResMut<BattleRoundUiState>,
    scene_positions: Option<Res<SceneCharacterPositions>>,
    player_camera_positions: Option<Res<ScenePlayerCameraPositions>>,
    mut scene_store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    mut scene_runtime: Option<ResMut<VoxelMapRuntimeState>>,
    mut player_view_request: Option<ResMut<ScenePlayerViewRequest>>,
) {
    let has_run_once: &mut Local<bool> = &mut locals.has_run_once;
    let new_chat_group_modal_string_open: &mut Local<(String, bool)> =
        &mut locals.new_chat_group_modal_string_open;
    let chat_input_msgs: &mut Local<HashMap<String, String>> = &mut locals.chat_input_msgs;
    let chat_scroll_states: &mut Local<HashMap<String, ChatScrollState>> =
        &mut locals.chat_scroll_states;
    let previous_group_rects: &mut Local<HashMap<String, Rect>> = &mut locals.previous_group_rects;
    let chat_list_edit_target: &mut Local<Option<String>> = &mut locals.chat_list_edit_target;
    let chat_list_edit_name: &mut Local<String> = &mut locals.chat_list_edit_name;
    let trpg_group_settings: &mut Local<TrpgGroupSettingsState> = &mut locals.trpg_group_settings;
    let character_edit_state: &mut Local<CharacterEditState> = &mut locals.character_edit_state;
    let quick_character_targets: &mut Local<HashSet<String>> = &mut locals.quick_character_targets;
    let image_textures: &mut Local<HashMap<String, TextureHandle>> =
        &mut locals.chat_image_textures;
    let turn_count_drafts: &mut Local<HashMap<(String, String), u32>> =
        &mut locals.chat_turn_count_drafts;
    let group_broadcast_scopes: &mut Local<HashMap<String, String>> =
        &mut locals.group_broadcast_scopes;

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if manager.migrate_chat_window_state()
        || manager.sync_chat_targets()
        || manager.sync_skill_pool_from_completed_characters()
    {
        manager.persist().ok();
    }
    let napcat_sender = napcat_sender.as_deref();
    let deepseek_sender = deepseek_sender.as_deref();
    let mut sent_message_added = false;
    for (target_id, pending_text, sent_targets) in
        ime.apply_send_results(send_manager.results.drain(..))
    {
        if let Some(text) = chat_input_msgs.get_mut(&target_id) {
            let should_clear = match pending_text.as_deref() {
                Some(pending_text) => text.trim() == pending_text,
                None => true,
            };
            if should_clear {
                text.clear();
            }
        }
        if let Some(pending_text) = pending_text {
            for target in sent_targets {
                if append_local_sent_message(&mut manager, target, &pending_text) {
                    sent_message_added = true;
                }
            }
        }
    }
    if sent_message_added {
        if let Err(err) = manager.persist() {
            eprintln!("failed to persist local sent NapCat message: {err}");
        }
    }

    let mut group_rects: HashMap<String, Rect> = HashMap::default();
    let mut group_deltas: HashMap<String, Vec2> = HashMap::default();
    let mut latest_group_rects: HashMap<String, Rect> = HashMap::default();
    let reset_data = |new_chat_group_modal_string_bool: &mut Local<'_, (String, bool)>| {
        new_chat_group_modal_string_bool.0 = "".to_owned();
        new_chat_group_modal_string_bool.1 = false;
    };

    if new_chat_group_modal_string_open.1 {
        let modal = Modal::new(Id::new("New Chat Group")).show(ctx, |ui| {
            ui.set_width(250.0);

            ui.heading("新建讨论组");
            ui.label("名称：");
            ui.text_edit_singleline(&mut new_chat_group_modal_string_open.0);

            egui::Sides::new().show(
                ui,
                |_ui| {},
                |ui| {
                    if ui.button("保存").clicked() {
                        manager.groups.insert(
                            new_chat_group_modal_string_open.0.to_owned(),
                            ChatGroup { members: vec![] },
                        );
                        manager.persist().ok();
                        reset_data(new_chat_group_modal_string_open);
                    }
                    if ui.button("取消").clicked() {
                        reset_data(new_chat_group_modal_string_open);
                    }
                },
            );
        });

        if modal.should_close() {
            reset_data(new_chat_group_modal_string_open);
        }
    }

    trpg_group_settings_window(
        ctx,
        &mut manager,
        &mut deepseek_manager,
        scene_store.as_deref_mut(),
        scene_runtime.as_deref_mut(),
        trpg_group_settings,
        character_edit_state,
        &mut rule_engine_state,
    );
    quick_character_windows(
        ctx,
        &mut manager,
        quick_character_targets,
        character_edit_state,
        &mut rule_engine_state,
        scene_positions.as_deref(),
        player_camera_positions.as_deref(),
    );

    egui::TopBottomPanel::top("top_panel")
        .resizable(false)
        .show(ctx, |ui| {
            menu::bar(ui, |ui| {
                file_menu_button(
                    ui,
                    &mut new_chat_group_modal_string_open.1,
                    &mut trpg_group_settings.open,
                );
                tools_menu_button(
                    ui,
                    &mut rule_engine_state,
                    &mut battle_round_state,
                );
                let pools_changed = pool_menu_button(ui, &mut manager, trpg_group_settings);
                if pools_changed {
                    manager.persist().ok();
                }
            });
        });

    egui::SidePanel::right("right_panel")
        .resizable(true)
        .show(ctx, |ui| {
            if napcat_sender.is_none() {
                ui.label("NapCat websocket未连接");
            }
            if deepseek_sender.is_none() {
                ui.label("DeepSeek后台未就绪");
            }
            let summary_markers_changed =
                sync_summarized_message_counts(&mut manager, &deepseek_manager);
            if summary_markers_changed {
                if let Err(err) = manager.persist() {
                    eprintln!("failed to persist summarized message markers: {err}");
                }
            }

            summary_panel(ui, &manager, &deepseek_manager);
        });

    egui::SidePanel::left("chat_list_panel")
        .resizable(true)
        .default_width(220.0)
        .width_range(160.0..=340.0)
        .show(ctx, |ui| {
            chat_list_panel(
                ui,
                ctx,
                &mut manager,
                chat_list_edit_target,
                chat_list_edit_name,
                trpg_group_settings,
            );
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            pending_chat_requests_window(
                ctx,
                &mut manager,
                napcat_sender,
                &mut ime,
            );
            waiting_turn_manager_window(ctx, &mut manager);

            let mut closed_group_names = Vec::new();
            for (k, v) in &manager.groups.clone() {
                let group_title = chat_group_title(&k, v, &manager);
                let unread_count = chat_group_unread_count(&manager, v);
                let group_size = group_chat_inner_size(v.members.len(), ui.max_rect());
                let group_max_size = group_chat_max_size(ui.max_rect());
                let mut group_open = true;
                let response = egui::Window::new(group_title)
                    .open(&mut group_open)
                    .constrain_to(ui.max_rect())
                    .id(Id::new((
                        k.as_str(),
                        "chat_group_window_v2",
                    )))
                    .default_pos(ui.max_rect().left_top() + egui::vec2(12.0, 12.0))
                    .default_size(group_size)
                    .min_size(CHAT_WINDOW_MIN_SIZE)
                    .max_size(group_max_size)
                    .show(ctx, |ui| {
                        group_drop_area_ui(ui, &k, &v.members);
                        group_broadcast_input_ui(
                            ui,
                            ctx,
                            &k,
                            &v.members,
                            &manager,
                            napcat_sender,
                            chat_input_msgs,
                            group_broadcast_scopes,
                            &mut ime,
                        );
                    });

                if !group_open {
                    closed_group_names.push(k.clone());
                    continue;
                }

                if let Some(response) = response {
                    paint_unread_badge(
                        ctx,
                        response.response.rect,
                        unread_count,
                    );
                    if response.inner.is_some() {
                        if let Some(previous_rect) = previous_group_rects.get(k) {
                            let delta = response.response.rect.min - previous_rect.min;
                            if delta.length_sq() > 0.0 {
                                group_deltas.insert(k.clone(), delta);
                            }
                        }
                        latest_group_rects.insert(k.clone(), response.response.rect);
                        group_rects.insert(k.clone(), response.response.rect);
                    }
                }
            }
            if !closed_group_names.is_empty() {
                for group_name in &closed_group_names {
                    manager.groups.remove(group_name);
                    previous_group_rects.remove(group_name);
                }
                if let Err(err) = manager.persist() {
                    eprintln!("failed to persist closed chat groups: {err}");
                }
            }
            **previous_group_rects = latest_group_rects;

            let mut visible_targets: HashSet<String> = manager.open_chat_targets.clone();
            for group in manager.groups.values() {
                visible_targets.extend(group.members.iter().cloned());
            }

            for target_id in visible_targets {
                let Some(messages) = manager.messages.get(&target_id).cloned() else {
                    continue;
                };
                let id = egui::Id::new(&target_id);
                let mut default_rect: Rect = Rect::from_pos(Pos2::new(0.0, 0.0));
                if !**has_run_once {
                    ctx.memory(|m| {
                        if let Some(rect) = m.area_rect(id) {
                            default_rect = rect;
                        }
                    });
                    **has_run_once = true
                }

                let current_group = if manager.open_chat_targets.contains(&target_id) {
                    None
                } else {
                    manager.groups.iter().find_map(|(group_name, group)| {
                        group
                            .members
                            .contains(&target_id)
                            .then_some(group_name.clone())
                    })
                };
                let rect = if let Some(group_name) = current_group.as_deref() {
                    let Some(rect) = group_rects.get(group_name).copied() else {
                        continue;
                    };
                    rect
                } else {
                    ui.max_rect()
                };
                let (_nickname, heights) = get_nickname_lens(target_id.clone(), &messages);
                let window_title = target_display_name(&manager, &target_id);
                let targets = targets_for_messages(&target_id, &messages);
                let unread_count = target_unread_count(&manager, &target_id);
                let summary_request_changed = queue_summaries_if_needed(
                    &manager,
                    &target_id,
                    &messages,
                    &manager.summarized_message_counts,
                    deepseek_sender,
                    &mut deepseek_manager,
                );
                if summary_request_changed {
                    if let Err(err) = deepseek_manager.persist() {
                        eprintln!("failed to persist DeepSeek summary request: {err}");
                    }
                }

                let active_trpg_group = manager.current_trpg_group.clone();
                chat_window(
                    &window_title,
                    id,
                    rect,
                    ctx,
                    heights,
                    &messages,
                    napcat_sender,
                    &target_id,
                    chat_input_msgs,
                    targets,
                    &mut ime,
                    chat_scroll_states,
                    &group_rects,
                    &mut manager,
                    current_group.as_deref(),
                    current_group
                        .as_deref()
                        .and_then(|group_name| group_deltas.get(group_name).copied()),
                    unread_count,
                    quick_character_targets,
                    image_textures,
                    active_trpg_group.as_deref(),
                    turn_count_drafts,
                    &mut rule_engine_state,
                    player_view_request.as_deref_mut(),
                );
            }
        });

    let should_persist_ui_memory = ctx.input(|input| {
        input.pointer.any_released()
            || input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Copy
                        | egui::Event::Cut
                        | egui::Event::Paste(_)
                        | egui::Event::Text(_)
                        | egui::Event::Key { .. }
                )
            })
    });
    ctx.memory(|m| {
        cached_memory.ui_memory = m.clone();
    });
    if should_persist_ui_memory {
        cached_memory.persist().ok();
    }
}

fn targets_for_messages(target_id: &str, messages: &[NapcatMessage]) -> Vec<NapcatSendTarget> {
    let Ok(target_id) = target_id.parse::<u64>() else {
        eprintln!("invalid NapCat target id: {target_id}");
        return Vec::new();
    };

    match messages.first().map(|message| &message.data.message_type) {
        Some(NapcatMessageType::Group) => vec![NapcatSendTarget::Group(target_id)],
        _ => vec![NapcatSendTarget::Private(target_id)],
    }
}

fn append_local_sent_message(
    manager: &mut NapcatMessageManager,
    target: NapcatSendTarget,
    text: &str,
) -> bool {
    let (target_id, message_type, group_id, recipient_id) = match target {
        NapcatSendTarget::Private(user_id) => (
            user_id.to_string(),
            NapcatMessageType::Private,
            None,
            Some(user_id),
        ),
        NapcatSendTarget::Group(group_id) => (
            group_id.to_string(),
            NapcatMessageType::Group,
            Some(group_id),
            None,
        ),
    };

    let Some(existing_messages) = manager.messages.get(&target_id) else {
        return false;
    };
    let Some(existing_message) = existing_messages.first() else {
        return false;
    };

    let self_id = existing_message.data.self_id;
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let campaign_id = manager.current_campaign_id();
    let (character_id, party_id, visibility) = if let Some(recipient_id) = recipient_id {
        let access = manager.player_access_for_user(recipient_id);
        (
            access.character_id,
            access.party_id,
            Visibility::Player(recipient_id),
        )
    } else {
        (None, None, Visibility::Public)
    };
    let message = NapcatMessage {
        data: NapcatMessageData {
            time,
            message_type,
            message: vec![NapcatMessageChain {
                variant: NapcatMessageChainType::Text {
                    data: TextData {
                        text: text.to_owned(),
                    },
                },
            }],
            self_id,
            user_id: self_id,
            group_id,
            group_name: None,
            target_id: recipient_id,
            sender: NapcatSender {
                user_id: self_id,
                nickname: "GM".to_owned(),
            },
            campaign_id,
            character_id,
            party_id,
            visibility,
        },
    };

    manager.messages.entry(target_id).or_default().push(message);
    true
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

    fn test_private_message(user_id: u64) -> NapcatMessage {
        NapcatMessage {
            data: NapcatMessageData {
                time: 1780132600,
                message_type: NapcatMessageType::Private,
                message: vec![NapcatMessageChain {
                    variant: NapcatMessageChainType::Text {
                        data: TextData {
                            text: "hello".to_owned(),
                        },
                    },
                }],
                self_id: 1,
                user_id,
                group_id: None,
                group_name: None,
                target_id: None,
                sender: NapcatSender {
                    user_id,
                    nickname: format!("user-{user_id}"),
                },
                campaign_id: "default".to_owned(),
                character_id: None,
                party_id: None,
                visibility: Visibility::Public,
            },
        }
    }

    fn test_group_message(user_id: u64, text: &str) -> NapcatMessage {
        NapcatMessage {
            data: NapcatMessageData {
                time: 1780132600,
                message_type: NapcatMessageType::Group,
                message: vec![NapcatMessageChain {
                    variant: NapcatMessageChainType::Text {
                        data: TextData {
                            text: text.to_owned(),
                        },
                    },
                }],
                self_id: 1,
                user_id,
                group_id: Some(99),
                group_name: Some("测试群".to_owned()),
                target_id: None,
                sender: NapcatSender {
                    user_id,
                    nickname: format!("user-{user_id}"),
                },
                campaign_id: "default".to_owned(),
                character_id: None,
                party_id: None,
                visibility: Visibility::Public,
            },
        }
    }

    fn split_party_summary_manager() -> NapcatMessageManager {
        let mut manager = empty_manager();
        let mut group = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned()],
            group_chats: vec!["99".to_owned()],
            ..Default::default()
        };
        group.ensure_party("red");
        group.ensure_party("blue");
        group.set_player_party("2", Some("red"));
        group.set_player_party("3", Some("blue"));
        manager.trpg_groups.insert("table".to_owned(), group);
        manager.current_trpg_group = Some("table".to_owned());
        manager
    }

    fn buff(name: &str, turns_remaining: i32) -> BuffSpec {
        BuffSpec {
            name: name.to_owned(),
            kind: BuffKind::Magic,
            priority: 0,
            turns_remaining,
            source_id: "gm".to_owned(),
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::DamageTakenModifier,
                value: BuffValue::Set(0.5),
            }],
        }
    }

    #[test]
    fn approval_onboarding_text_uses_current_group_guide_for_private_player() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![
            test_private_message(2),
        ]);
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            guide: "请先完成角色设定。".to_owned(),
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());

        assert!(manager.approve_chat_target("2"));

        assert_eq!(
            approval_onboarding_text(&manager, "2"),
            Some("团内引导：\n请先完成角色设定。".to_owned())
        );
    }

    #[test]
    fn approval_onboarding_text_skips_group_targets_and_empty_guides() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![
            test_private_message(2),
        ]);
        manager.messages.insert("99".to_owned(), vec![
            test_group_message(4, "hello"),
        ]);
        manager
            .trpg_groups
            .insert("table".to_owned(), TrpgGroup::default());
        manager.current_trpg_group = Some("table".to_owned());

        assert!(manager.approve_chat_target("2"));
        assert!(manager.approve_chat_target("99"));

        assert_eq!(
            approval_onboarding_text(&manager, "2"),
            None
        );
        assert_eq!(
            approval_onboarding_text(&manager, "99"),
            None
        );
    }

    #[test]
    fn group_broadcast_targets_default_to_all_private_members_deduped() {
        let mut manager = empty_manager();
        for user_id in [2, 3, 4] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message(user_id),
            ]);
        }
        let members = vec![
            "2".to_owned(),
            "3".to_owned(),
            "2".to_owned(),
            "4".to_owned(),
            "missing".to_owned(),
        ];

        let targets = group_broadcast_targets(
            None,
            &members,
            &manager,
            BROADCAST_SCOPE_ALL,
        );

        assert_eq!(targets, vec![
            NapcatSendTarget::Private(2),
            NapcatSendTarget::Private(3),
            NapcatSendTarget::Private(4),
        ]);
    }

    #[test]
    fn group_broadcast_targets_filter_to_selected_party() {
        let mut manager = empty_manager();
        for user_id in [2, 3, 4] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message(user_id),
            ]);
        }
        let mut group = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
            ..Default::default()
        };
        group.ensure_party("red");
        group.ensure_party("blue");
        group.set_player_party("2", Some("red"));
        group.set_player_party("3", Some("red"));
        group.set_player_party("4", Some("blue"));
        let members = vec!["2".to_owned(), "3".to_owned(), "4".to_owned()];

        let targets = group_broadcast_targets(
            Some(&group),
            &members,
            &manager,
            &broadcast_party_scope("red"),
        );

        assert_eq!(targets, vec![
            NapcatSendTarget::Private(2),
            NapcatSendTarget::Private(3),
        ]);
    }

    #[test]
    fn group_broadcast_targets_party_scope_requires_current_group() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![
            test_private_message(2),
        ]);
        let members = vec!["2".to_owned()];

        let targets = group_broadcast_targets(
            None,
            &members,
            &manager,
            &broadcast_party_scope("red"),
        );

        assert!(targets.is_empty());
    }

    #[test]
    fn group_summary_scope_filters_public_and_party_messages() {
        let manager = split_party_summary_manager();
        let messages = vec![
            test_group_message(4, "public clue"),
            test_group_message(2, "red clue"),
            test_group_message(3, "blue clue"),
        ];

        let public_lines = player_text_lines(&campaign_messages_for_summary_scope(
            &manager,
            "99",
            &messages,
            &SummaryScope::GroupPublic,
        ));
        let red_lines = player_text_lines(&campaign_messages_for_summary_scope(
            &manager,
            "99",
            &messages,
            &SummaryScope::GroupParty("red".to_owned()),
        ));

        let public_text = public_lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let red_text = red_lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(public_text.contains("public clue"));
        assert!(!public_text.contains("red clue"));
        assert!(!public_text.contains("blue clue"));
        assert!(red_text.contains("public clue"));
        assert!(red_text.contains("red clue"));
        assert!(!red_text.contains("blue clue"));
    }

    #[test]
    fn group_chat_summary_requests_use_scoped_keys_and_filtered_payloads() {
        let manager = split_party_summary_manager();
        let mut messages = Vec::new();
        for index in 0..5 {
            messages.push(test_group_message(
                4,
                &format!("public clue {index}"),
            ));
            messages.push(test_group_message(
                2,
                &format!("red clue {index}"),
            ));
            messages.push(test_group_message(
                3,
                &format!("blue clue {index}"),
            ));
        }
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        let deepseek_sender = DeepseekIOSender(sender);
        let mut deepseek_manager = DeepseekManager::default();

        assert!(queue_summaries_if_needed(
            &manager,
            "99",
            &messages,
            &HashMap::default(),
            Some(&deepseek_sender),
            &mut deepseek_manager,
        ));

        assert!(deepseek_manager.summaries.contains_key("group:99:public"));
        assert!(deepseek_manager
            .summaries
            .contains_key("group:99:party:red"));
        assert!(deepseek_manager
            .summaries
            .contains_key("group:99:party:blue"));
        assert!(!deepseek_manager.summaries.contains_key("99"));

        let mut request_texts = HashMap::new();
        while let Ok(message) = receiver.try_recv() {
            let Message::Text(text) = message else {
                continue;
            };
            let DeepseekRequest::Summary {
                target_id, text, ..
            } = serde_json::from_str::<DeepseekRequest>(&text)
                .expect("summary request should deserialize");
            request_texts.insert(target_id, text);
        }

        let public_text = &request_texts["group:99:public"];
        let red_text = &request_texts["group:99:party:red"];
        let blue_text = &request_texts["group:99:party:blue"];
        assert!(public_text.contains("public clue 0"));
        assert!(!public_text.contains("red clue 0"));
        assert!(!public_text.contains("blue clue 0"));
        assert!(red_text.contains("public clue 0"));
        assert!(red_text.contains("red clue 0"));
        assert!(!red_text.contains("blue clue 0"));
        assert!(blue_text.contains("public clue 0"));
        assert!(!blue_text.contains("red clue 0"));
        assert!(blue_text.contains("blue clue 0"));
    }

    #[test]
    fn inventory_stacks_matching_stackable_items() {
        let mut inventory = CharacterInventory::default();
        add_item_to_inventory(&mut inventory, InventoryItem {
            name: "治疗药水".to_owned(),
            stack: 3,
            max_stack: 5,
            ..Default::default()
        });
        add_item_to_inventory(&mut inventory, InventoryItem {
            name: "治疗药水".to_owned(),
            stack: 4,
            max_stack: 5,
            ..Default::default()
        });

        assert_eq!(inventory.items.len(), 2);
        assert_eq!(inventory.items[0].stack, 5);
        assert_eq!(inventory.items[1].stack, 2);
    }

    #[test]
    fn equipping_item_moves_previous_item_to_bag() {
        let mut inventory = CharacterInventory::default();
        inventory.items.push(InventoryItem {
            name: "旧剑".to_owned(),
            equipment_slot: EquipmentSlot::MainHand,
            ..Default::default()
        });
        inventory.items.push(InventoryItem {
            name: "新剑".to_owned(),
            equipment_slot: EquipmentSlot::MainHand,
            ..Default::default()
        });

        equip_inventory_item(&mut inventory, 0);
        equip_inventory_item(&mut inventory, 0);

        assert_eq!(
            inventory.equipment[&EquipmentSlot::MainHand].name,
            "新剑"
        );
        assert_eq!(inventory.items.len(), 1);
        assert_eq!(inventory.items[0].name, "旧剑");
    }

    #[test]
    fn random_pool_ignores_disabled_and_zero_weight_entries() {
        let pool = RandomPool {
            entries: vec![
                RandomPoolEntry {
                    item: InventoryItem {
                        name: "不会出现".to_owned(),
                        ..Default::default()
                    },
                    weight: 999.0,
                    enabled: false,
                    ..Default::default()
                },
                RandomPoolEntry {
                    item: InventoryItem {
                        name: "也不会出现".to_owned(),
                        ..Default::default()
                    },
                    weight: 0.0,
                    enabled: true,
                    ..Default::default()
                },
                RandomPoolEntry {
                    item: InventoryItem {
                        name: "固定结果".to_owned(),
                        ..Default::default()
                    },
                    weight: 1.0,
                    enabled: true,
                    ..Default::default()
                },
            ],
            last_pick: None,
            last_text_result: None,
        };

        assert_eq!(random_pool_total_weight(&pool), 1.0);
        assert_eq!(
            pick_random_pool_item(&pool).unwrap().name,
            "固定结果"
        );
    }

    #[test]
    fn random_pool_text_result_uses_fixed_count_and_label() {
        let entry = RandomPoolEntry {
            item: InventoryItem {
                name: "事件".to_owned(),
                ..Default::default()
            },
            result_text: "获得线索".to_owned(),
            min_count: 2,
            max_count: 2,
            ..Default::default()
        };

        let result = random_pool_entry_text_result(&entry).unwrap();

        assert_eq!(result.entry_name, "事件");
        assert_eq!(result.text, "获得线索");
        assert_eq!(result.count, 2);
        assert_eq!(
            random_pool_text_result_label(&result),
            "获得线索 x2"
        );
    }

    #[test]
    fn skill_pool_entry_copies_to_character_skills() {
        let mut character = PlayerCharacter::default();
        let entry = SkillPoolEntry {
            name: "烈焰箭".to_owned(),
            note: "主动使用对目标造成3点魔法伤害".to_owned(),
            mp_cost: 2.0,
            cooldown_turns: 1,
            source_character_id: Some("player-1".to_owned()),
            source_character_name: Some("法师".to_owned()),
            source_skill_index: Some(0),
            legacy_pool_id: Some("legacy-fire".to_owned()),
            category: Some("普通".to_owned()),
            ..Default::default()
        };

        add_skill_pool_entry_to_character(&mut character, &entry);

        assert_eq!(character.skill_names, vec![
            "烈焰箭".to_owned()
        ]);
        assert_eq!(character.skill_notes, vec![
            "主动使用对目标造成3点魔法伤害".to_owned()
        ]);
        assert_eq!(character.skill_mp_costs, vec![2.0]);
        assert_eq!(character.skill_cooldown_turns, vec![1]);
        assert_eq!(character.skill_metadata.len(), 1);
        assert_eq!(
            character.skill_metadata[0].source,
            CharacterSkillSourceKind::SkillPool
        );
        assert_eq!(
            character.skill_metadata[0].source_character_id.as_deref(),
            Some("player-1")
        );
        assert_eq!(
            character.skill_metadata[0].source_pool_id.as_deref(),
            Some("legacy-fire")
        );
        assert_eq!(
            character.skill_metadata[0].source_pool_label.as_deref(),
            Some("烈焰箭")
        );
    }

    #[test]
    fn advancing_character_buffs_decrements_and_expires_turn_limited_buffs() {
        let mut character = PlayerCharacter {
            active_buffs: vec![buff("expires", 1), buff("continues", 3)],
            ..Default::default()
        };

        assert!(advance_character_buffs(&mut character));

        assert_eq!(character.active_buffs.len(), 1);
        assert_eq!(
            character.active_buffs[0].name,
            "continues"
        );
        assert_eq!(
            character.active_buffs[0].turns_remaining,
            2
        );
    }

    #[test]
    fn advancing_character_buffs_preserves_zero_turn_permanent_buffs() {
        let mut character = PlayerCharacter {
            active_buffs: vec![buff("permanent", 0), buff("expires", 1)],
            ..Default::default()
        };

        assert!(advance_character_buffs(&mut character));

        assert_eq!(character.active_buffs.len(), 1);
        assert_eq!(
            character.active_buffs[0].name,
            "permanent"
        );
        assert_eq!(
            character.active_buffs[0].turns_remaining,
            0
        );
    }

    #[test]
    fn quick_cast_skills_exclude_unapproved_entries() {
        let mut character = PlayerCharacter {
            skill_names: vec!["已批准".to_owned(), "待批准".to_owned()],
            skill_notes: vec![
                "主动使用对目标造成1点物理伤害".to_owned(),
                "主动使用对目标造成9点物理伤害".to_owned(),
            ],
            skill_metadata: vec![CharacterSkillMetadata::default(), CharacterSkillMetadata {
                st_approved: false,
                ..Default::default()
            }],
            ..Default::default()
        };

        let skills = quick_cast_skills(&mut character);

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "已批准");
    }

    #[test]
    fn quick_cast_records_and_blocks_skill_cooldown_until_turn_passes() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                skill_names: vec!["旋风斩".to_owned()],
                skill_cooldown_turns: vec![2],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["caster".to_owned(), "target".to_owned()],
            player_turns: HashMap::from([(
                "caster".to_owned(),
                crate::napcat::TrpgPlayerTurnState::default(),
            )]),
            ..Default::default()
        });
        let skill = QuickCastSkill {
            index: 0,
            name: "旋风斩".to_owned(),
            note: String::new(),
            mp_cost: 0.0,
            cooldown_turns: 2,
        };
        let effect = QuickCastEffect::Damage {
            amount: 1.0,
            target: TargetSelector {
                actor: ActorRef::Target,
                area: None,
            },
        };

        assert_eq!(
            quick_cast_cooldown_turn(&manager, "caster"),
            0
        );
        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill: skill.clone(),
                targets: vec!["target".to_owned()],
                effect: Some(effect),
                cast_turn: 0,
                force: false,
            },
        ));
        let caster = &manager.player_characters["caster"];
        assert_eq!(
            quick_skill_cooldown_remaining(caster, 0, 2, 0),
            2
        );

        assert!(!apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill: skill.clone(),
                targets: vec!["target".to_owned()],
                effect: Some(effect),
                cast_turn: 0,
                force: false,
            },
        ));

        manager
            .trpg_groups
            .get_mut("party")
            .unwrap()
            .set_player_turns_passed("caster", 2);
        assert_eq!(
            quick_cast_cooldown_turn(&manager, "caster"),
            2
        );
        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(effect),
                cast_turn: 2,
                force: false,
            },
        ));
    }
}
