mod ime;
use std::{
    collections::HashMap,
    hash::{
        Hash,
        Hasher,
    },
    path::Path,
};
mod components;

use std::collections::hash_map::DefaultHasher;

use bevy::prelude::*;
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

use crate::{
    deepseek::{
        DeepseekIOSender,
        DeepseekManager,
        DeepseekPlugin,
        DeepseekRequest,
        DeepseekSummaryBlock,
    },
    napcat::{
        ChatGroup,
        NapcatIOSender,
        NapcatMessage,
        NapcatMessageChainType,
        NapcatMessageManager,
        NapcatMessageType,
        NapcatSendManager,
    },
};
pub struct UIPlugin;
#[derive(Resource)]
pub struct GIFImages {
    images: HashMap<String, Vec<(TextureHandle, u32)>>,
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

fn file_menu_button(ui: &mut Ui, new_chat_group_modal_open: &mut bool) {
    let new_chat_group_shortcup = egui::KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::G);

    // NOTE: we must check the shortcuts OUTSIDE of the actual "File" menu,
    // or else they would only be checked if the "File" menu was actually open!

    if ui.input_mut(|i| i.consume_shortcut(&new_chat_group_shortcup)) {
        *new_chat_group_modal_open = true
    }

    ui.menu_button("Edit", |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

        if ui
            .add(
                egui::Button::new("New Chat Group")
                    .shortcut_text(ui.ctx().format_shortcut(&new_chat_group_shortcup)),
            )
            .clicked()
        {
            *new_chat_group_modal_open = true
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

pub fn setup_system(mut command: Commands, mut windows: Query<&mut Window>) {
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

    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    window.ime_enabled = true;
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
    *fonts_configured = true;
}

pub fn load_ui_memory(
    mut egui_context: EguiContexts,
    cached_memory: Res<Persistent<CachedMemory>>,
) {
    let Ok(ctx) = egui_context.ctx_mut() else {
        return;
    };
    ctx.memory_mut(|m| *m = cached_memory.ui_memory.clone());
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
    group_rects: &HashMap<String, Rect>,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    current_group: Option<&str>,
    group_delta: Option<Vec2>,
    unread_count: usize,
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
        .unwrap_or_else(|| {
            Id::new((
                id,
                target_id,
                "standalone_chat_window_v2",
            ))
        });
    let mut window = egui::Window::new(nickname)
        .open(&mut window_open)
        .id(window_id)
        .constrain_to(constraint_rect)
        .default_size(CHAT_WINDOW_SIZE)
        .min_size(window_min_size)
        .max_size(max_window_size)
        .max_height(GROUP_CHAT_MAX_HEIGHT);
    if current_group.is_some() {
        window = window.order(egui::Order::Foreground);
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
    let response = window.show(ctx, |ui| {
        if current_group.is_some() {
            ui.horizontal(|ui| {
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui.button("离开").on_hover_text("离开讨论组").clicked() {
                            leave_group = true;
                        }
                    },
                );
            });
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
            None,
        );
    });

    if current_group.is_some() && !window_open {
        leave_group = true;
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
                egui::ScrollArea::vertical()
                    .id_salt((target_id, "messages"))
                    .max_height(message_height)
                    .min_scrolled_height(message_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.with_layout(
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                for message in messages {
                                    message_row_ui(ui, message, message_width);
                                    ui.add_space(ui.spacing().item_spacing.y);
                                }
                            },
                        );
                    });
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

fn message_row_ui(ui: &mut Ui, message: &NapcatMessage, row_width: f32) {
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
                        message_text_ui(ui, message);
                    },
                );
            });
        } else {
            ui.vertical(|ui| {
                ui.set_width(max_message_width);
                ui.set_max_width(max_message_width);
                message_text_ui(ui, message);
            });
            ui.add_space(margin_width);
        }
    });
}

fn message_text_ui(ui: &mut Ui, message: &NapcatMessage) {
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
            NapcatMessageChainType::Source(_) => {},
            NapcatMessageChainType::Unsupported => {},
            // TODO: Support images
        }
    }
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
                NapcatMessageChainType::Unsupported => {},
                // TODO: Support images
                // NapcatMessageChainType::Image { data: image } => {
                //     height += 200.0;
                // },
            };
        }

        if message.data.sender.user_id.to_string() == *target_id {
            nickname = &message.data.sender.nickname;
        }
        lens.push(len)
    }

    (nickname, lens)
}

fn target_display_name(target_id: &str, messages: Option<&Vec<NapcatMessage>>) -> String {
    messages
        .map(|messages| get_nickname_lens(target_id.to_owned(), messages).0)
        .filter(|nickname| !nickname.is_empty())
        .unwrap_or(target_id)
        .to_owned()
}

fn chat_group_title(
    group_name: &str,
    group: &ChatGroup,
    messages: &HashMap<String, Vec<NapcatMessage>>,
) -> String {
    let member_names = group
        .members
        .iter()
        .map(|member_id| target_display_name(member_id, messages.get(member_id)))
        .collect::<Vec<_>>();

    if member_names.is_empty() {
        format!("讨论组: {}", group_name)
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

fn player_text_lines(messages: &[NapcatMessage]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| message.data.user_id != message.data.self_id)
        .filter_map(|message| {
            let text = message
                .data
                .message
                .iter()
                .filter_map(|chain| match &chain.variant {
                    NapcatMessageChainType::Text { data } => Some(data.text.trim()),
                    NapcatMessageChainType::Source(_) => None,
                    NapcatMessageChainType::Unsupported => None,
                })
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(" ");

            if text.is_empty() {
                None
            } else {
                Some(format!(
                    "{}: {}",
                    message.data.sender.nickname, text
                ))
            }
        })
        .collect()
}

fn queue_summary_if_needed(
    target_id: &str,
    messages: &[NapcatMessage],
    deepseek_sender: Option<&DeepseekIOSender>,
    deepseek_manager: &mut DeepseekManager,
) {
    let lines = player_text_lines(messages);
    let message_count = lines.len();
    if message_count == 0 || message_count % 5 != 0 {
        return;
    }

    if let Some(summary) = deepseek_manager.summaries.get(target_id) {
        if summary
            .blocks
            .iter()
            .any(|block| block.message_count == message_count)
        {
            return;
        }
    }

    let Some(deepseek_sender) = deepseek_sender else {
        return;
    };

    let request = DeepseekRequest::Summary {
        target_id: target_id.to_owned(),
        message_count,
        text: lines
            .iter()
            .rev()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n"),
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
        },
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

pub fn ui_system(
    mut contexts: EguiContexts,
    mut ime: ResMut<ImeManager>,
    napcat_sender: Option<Res<NapcatIOSender>>,
    deepseek_sender: Option<Res<DeepseekIOSender>>,
    mut deepseek_manager: ResMut<DeepseekManager>,
    mut send_manager: ResMut<NapcatSendManager>,
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
    mut cached_memory: ResMut<Persistent<CachedMemory>>,
    mut has_run_once: Local<bool>,
    mut new_chat_group_modal_string_open: Local<(String, bool)>,
    mut chat_input_msgs: Local<HashMap<String, String>>,
    mut previous_group_rects: Local<HashMap<String, Rect>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let napcat_sender = napcat_sender.as_deref();
    let deepseek_sender = deepseek_sender.as_deref();
    for (target_id, pending_text) in ime.apply_send_results(send_manager.results.drain(..)) {
        if let Some(text) = chat_input_msgs.get_mut(&target_id) {
            let should_clear = match pending_text.as_deref() {
                Some(pending_text) => text.trim() == pending_text,
                None => true,
            };
            if should_clear {
                text.clear();
            }
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

            ui.heading("New Chat Group");
            ui.label("Name:");
            ui.text_edit_singleline(&mut new_chat_group_modal_string_open.0);

            egui::Sides::new().show(
                ui,
                |_ui| {},
                |ui| {
                    if ui.button("Save").clicked() {
                        manager.groups.insert(
                            new_chat_group_modal_string_open.0.to_owned(),
                            ChatGroup { members: vec![] },
                        );
                        manager.persist().ok();
                        reset_data(&mut new_chat_group_modal_string_open);
                    }
                    if ui.button("Cancel").clicked() {
                        reset_data(&mut new_chat_group_modal_string_open);
                    }
                },
            );
        });

        if modal.should_close() {
            reset_data(&mut new_chat_group_modal_string_open);
        }
    }

    egui::TopBottomPanel::top("top_panel")
        .resizable(false)
        .show(ctx, |ui| {
            menu::bar(ui, |ui| {
                file_menu_button(
                    ui,
                    &mut new_chat_group_modal_string_open.1,
                );
            });
        });

    egui::SidePanel::right("right_panel")
        .resizable(true)
        .show(ctx, |ui| {
            if napcat_sender.is_none() {
                ui.label("NapCat websocket not connected");
            }
            if deepseek_sender.is_none() {
                ui.label("Deepseek worker not ready");
            }
            summary_panel(ui, &manager, &deepseek_manager);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            for (k, v) in &manager.groups.clone() {
                let group_title = chat_group_title(&k, v, &manager.messages);
                let unread_count = chat_group_unread_count(&manager, v);
                let group_size = group_chat_inner_size(v.members.len(), ui.max_rect());
                let response = egui::Window::new(group_title)
                    .open(&mut true)
                    .constrain_to(ui.max_rect())
                    .id(Id::new(k))
                    .default_pos(ui.max_rect().left_top() + egui::vec2(12.0, 12.0))
                    .default_size(group_size)
                    .min_size(CHAT_WINDOW_MIN_SIZE)
                    .max_size(ui.max_rect().size())
                    .show(ctx, |ui| {
                        group_drop_area_ui(ui, &k, &v.members);
                        group_broadcast_input_ui(
                            ui,
                            ctx,
                            &k,
                            &v.members,
                            &manager.messages,
                            napcat_sender,
                            &mut chat_input_msgs,
                            &mut ime,
                        );
                    });

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
            *previous_group_rects = latest_group_rects;

            for (target_id, messages) in manager.messages.clone() {
                let id = egui::Id::new(&target_id);
                let mut default_rect: Rect = Rect::from_pos(Pos2::new(0.0, 0.0));
                if !*has_run_once {
                    ctx.memory(|m| {
                        if let Some(rect) = m.area_rect(id) {
                            default_rect = rect;
                        }
                    });
                    *has_run_once = true
                }

                let current_group = manager.groups.iter().find_map(|(group_name, group)| {
                    group
                        .members
                        .contains(&target_id)
                        .then_some(group_name.clone())
                });
                let rect = if let Some(group_name) = current_group.as_deref() {
                    let Some(rect) = group_rects.get(group_name).copied() else {
                        continue;
                    };
                    rect
                } else {
                    ui.max_rect()
                };
                let (nickname, heights) = get_nickname_lens(target_id.clone(), &messages);
                let targets = targets_for_messages(&target_id, &messages);
                let unread_count = target_unread_count(&manager, &target_id);
                queue_summary_if_needed(
                    &target_id,
                    &messages,
                    deepseek_sender,
                    &mut deepseek_manager,
                );

                chat_window(
                    nickname,
                    id,
                    rect,
                    ctx,
                    heights,
                    &messages,
                    napcat_sender,
                    &target_id,
                    &mut chat_input_msgs,
                    targets,
                    &mut ime,
                    &group_rects,
                    &mut manager,
                    current_group.as_deref(),
                    current_group
                        .as_deref()
                        .and_then(|group_name| group_deltas.get(group_name).copied()),
                    unread_count,
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
