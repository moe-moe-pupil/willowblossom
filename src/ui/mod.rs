mod ime;
use std::{
    collections::{
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

use crate::{
    battle_round::BattleRoundUiState,
    deepseek::{
        DeepseekIOSender,
        DeepseekManager,
        DeepseekPlugin,
        DeepseekRequest,
        DeepseekSummaryBlock,
    },
    napcat::{
        update_character_from_status,
        CharacterBuffBaseStats,
        CharacterCreationStep,
        CharacterStatus,
        ChatGroup,
        ImageData,
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
        TextData,
        TrpgGroup,
        TrpgPlayerTurnState,
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
    focused_group_name: Option<String>,
    pending_character_delete: Option<String>,
}

#[derive(Default)]
pub(crate) struct CharacterEditState {
    unlocked_status_targets: HashSet<String>,
    gm_status_drafts: HashMap<String, CharacterStatus>,
    buff_drafts: HashMap<String, BuffDraft>,
    pending_character_reset: Option<String>,
    quick_cast_skill_index: HashMap<String, usize>,
    pending_force_cast: Option<(String, usize)>,
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

        if ui.button("玩家/群池").clicked() {
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
                        .on_hover_text("切换到这个玩家的场景捕捉相机")
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
    messages: &HashMap<String, Vec<NapcatMessage>>,
    napcat_sender: Option<&NapcatIOSender>,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    ime: &mut ResMut<ImeManager>,
) {
    let input_id = format!("group:{group_name}:broadcast");
    chat_input_msgs
        .entry(input_id.clone())
        .or_insert_with(String::new);

    ui.separator();
    let text = chat_input_msgs.get_mut(&input_id).unwrap();
    let targets = members
        .iter()
        .filter_map(|member_id| {
            if !matches!(
                messages
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
        })
        .collect::<Vec<_>>();

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

fn player_text_lines(messages: &[NapcatMessage]) -> Vec<PlayerTextLine> {
    let mut player_message_count = 0;
    let mut lines = Vec::new();

    for message in messages
        .iter()
        .filter(|message| message.data.user_id != message.data.self_id)
    {
        let text = message
            .data
            .message
            .iter()
            .filter_map(|chain| match &chain.variant {
                NapcatMessageChainType::Text { data } => Some(data.text.trim()),
                NapcatMessageChainType::Source(_) => None,
                NapcatMessageChainType::Image { .. } => None,
                NapcatMessageChainType::Unsupported => None,
            })
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

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
            text: format!(
                "{}: {}",
                message.data.sender.nickname, text
            ),
        });
    }

    lines
}

fn queue_summary_if_needed(
    target_id: &str,
    messages: &[NapcatMessage],
    summarized_message_count: usize,
    deepseek_sender: Option<&DeepseekIOSender>,
    deepseek_manager: &mut DeepseekManager,
) -> bool {
    let lines = player_text_lines(messages);
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

    if let Some(summary) = deepseek_manager.summaries.get(target_id) {
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
        target_id: target_id.to_owned(),
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
                .entry(target_id.to_owned())
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
                .entry(target_id.to_owned())
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
            let nickname = manager
                .messages
                .get(target_id)
                .map(|messages| {
                    get_nickname_lens(target_id.to_string(), messages)
                        .0
                        .to_owned()
                })
                .filter(|nickname| !nickname.is_empty())
                .unwrap_or_else(|| target_id.to_string());

            ui.group(|ui| {
                ui.label(format!(
                    "{} / {} 个总结",
                    nickname,
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
                                manager.open_chat_targets.insert(target_id.clone());
                                manager.pending_chat_targets.remove(&target_id);
                                changed = true;
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
        .map(|(index, name)| {
            let name = if name.trim().is_empty() {
                format!("技能{}", index + 1)
            } else {
                name.trim().to_owned()
            };
            QuickCastSkill {
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
            }
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
    changed |= character_skill_editor_ui(
        ui,
        target_id,
        character,
        rule_engine_state,
    );

    if derived_stats_changed {
        update_character_from_status(character);
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

fn character_skill_editor_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    rule_engine_state: &mut RuleEngineState,
) -> bool {
    let mut changed = false;
    let mut remove_index = None;

    changed |= normalize_character_skill_fields(character);

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
        shift_skill_last_cast_turns_after_remove(
            &mut character.skill_last_cast_turns,
            index,
        );
        changed = true;
    }

    sync_character_skill_rules(target_id, character, rule_engine_state);

    changed
}

fn normalize_character_skill_fields(character: &mut PlayerCharacter) -> bool {
    let mut changed = false;
    let skill_count = character
        .skill_names
        .len()
        .max(character.skill_notes.len())
        .max(character.skill_mp_costs.len())
        .max(character.skill_cooldown_turns.len());
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
        .filter_map(|note| parse_skill_note(note).ok().flatten())
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

fn trpg_group_settings_window(
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
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

    egui::Window::new("玩家/群池")
        .id(Id::new("trpg_group_settings_window"))
        .open(&mut state.open)
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

            ui.separator();
            ui.columns(2, |columns| {
                columns[0].heading("玩家池");
                if player_targets.is_empty() {
                    columns[0].label("还没有玩家私聊。");
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("player_pool_settings")
                        .max_height(140.0)
                        .show(&mut columns[0], |ui| {
                            for target_id in &player_targets {
                                ui.horizontal(|ui| {
                                    ui.label(target_display_name(manager, target_id));
                                    ui.small(target_id);
                                });
                            }
                        });
                }

                columns[1].heading("群聊池");
                if group_chat_targets.is_empty() {
                    columns[1].label("还没有QQ群聊。");
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("group_chat_pool_settings")
                        .max_height(140.0)
                        .show(&mut columns[1], |ui| {
                            for target_id in &group_chat_targets {
                                ui.horizontal(|ui| {
                                    ui.label(target_display_name(manager, target_id));
                                    ui.small(target_id);
                                });
                            }
                        });
                }
            });

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

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if manager.migrate_chat_window_state() || manager.sync_chat_targets() {
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
            pending_chat_requests_window(ctx, &mut manager);
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
                            &manager.messages,
                            napcat_sender,
                            chat_input_msgs,
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
                let summarized_message_count = manager
                    .summarized_message_counts
                    .get(&target_id)
                    .copied()
                    .unwrap_or_default();
                let summary_request_changed = queue_summary_if_needed(
                    &target_id,
                    &messages,
                    summarized_message_count,
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
        }
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
                TrpgPlayerTurnState::default(),
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
