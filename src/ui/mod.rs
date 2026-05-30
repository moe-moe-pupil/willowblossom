mod ime;
use std::{
    collections::HashMap,
    path::Path,
};
mod components;

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
const GROUP_CHAT_MAX_HEIGHT: f32 = 720.0;
const GROUP_CHAT_MIN_HEIGHT: f32 = 140.0;
const GROUP_CHAT_SEPARATOR_HEIGHT: f32 = 10.0;
const GROUP_MEMBER_CHAT_SIZE: Vec2 = Vec2::new(320.0, 420.0);
const GROUP_MEMBER_CHAT_MAX_SIZE: Vec2 = Vec2::new(480.0, 620.0);
const GROUP_MEMBER_RESIZE_HANDLE_SIZE: f32 = 16.0;
const GROUP_BROADCAST_INPUT_ROWS: usize = 3;

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
    _cached_memory: ResMut<Persistent<CachedMemory>>,
) {
    let Ok(ctx) = egui_context.ctx_mut() else {
        return;
    };
    ctx.memory_mut(|m| *m = Memory::default());
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
) {
    let mut window_open = true;
    let response = egui::Window::new(nickname)
        .open(&mut window_open)
        .id(id)
        .constrain_to(rect)
        .default_size(CHAT_WINDOW_SIZE)
        .min_size(CHAT_WINDOW_MIN_SIZE)
        .max_height(GROUP_CHAT_MAX_HEIGHT)
        .show(ctx, |ui| {
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

    if let Some(response) = response {
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
                    .min_scrolled_width(message_width)
                    .max_height(message_height)
                    .min_scrolled_height(message_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(message_width);
                        ui.set_width(message_width);
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

fn group_chat_inner_size(member_count: usize, max_rect: Rect) -> Vec2 {
    let broadcast_input_height = 96.0;
    let desired_height = if member_count == 0 {
        GROUP_CHAT_MIN_HEIGHT + broadcast_input_height
    } else {
        member_count as f32 * GROUP_MEMBER_CHAT_SIZE.y
            + member_count.saturating_sub(1) as f32 * GROUP_CHAT_SEPARATOR_HEIGHT
            + broadcast_input_height
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

fn resizable_chat_pane(
    ui: &mut Ui,
    ctx: &Context,
    group_id: &str,
    member_id: &str,
    pane_size: &mut Vec2,
    add_contents: impl FnOnce(&mut Ui),
) {
    let available = ui.available_size();
    let min_size = egui::vec2(
        CHAT_WINDOW_MIN_SIZE.x.min(available.x.max(1.0)),
        CHAT_WINDOW_MIN_SIZE.y.min(available.y.max(1.0)),
    );
    let max_size = egui::vec2(
        GROUP_MEMBER_CHAT_MAX_SIZE
            .x
            .min(available.x.max(min_size.x)),
        GROUP_MEMBER_CHAT_MAX_SIZE
            .y
            .min(available.y.max(min_size.y)),
    );
    pane_size.x = pane_size.x.clamp(min_size.x, max_size.x);
    pane_size.y = pane_size.y.clamp(min_size.y, max_size.y);

    let (rect, _) = ui.allocate_exact_size(*pane_size, Sense::hover());
    let mut pane_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(
                egui::Align::LEFT,
            )),
    );
    add_contents(&mut pane_ui);

    let handle_size = Vec2::splat(GROUP_MEMBER_RESIZE_HANDLE_SIZE);
    let handle_rect = Rect::from_min_size(
        rect.right_bottom() - handle_size,
        handle_size,
    );
    let handle_id = Id::new((
        group_id,
        member_id,
        "manual_chat_pane_resize",
    ));
    let handle_response = ui.interact(handle_rect, handle_id, Sense::drag());
    if handle_response.dragged() {
        *pane_size += handle_response.drag_delta();
        pane_size.x = pane_size.x.clamp(min_size.x, max_size.x);
        pane_size.y = pane_size.y.clamp(min_size.y, max_size.y);
        ctx.request_repaint();
    }

    let stroke = ui.style().interact(&handle_response).fg_stroke;
    let painter = ui.painter();
    for offset in [4.0, 8.0, 12.0] {
        painter.line_segment(
            [
                egui::pos2(
                    handle_rect.right() - offset,
                    handle_rect.bottom(),
                ),
                egui::pos2(
                    handle_rect.right(),
                    handle_rect.bottom() - offset,
                ),
            ],
            stroke,
        );
    }
    if handle_response.hovered() || handle_response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
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
    mut group_member_sizes: Local<HashMap<(String, String), Vec2>>,
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

    let mut group_rects = HashMap::default();
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

    egui::CentralPanel::default().show(ctx, |ui| {
        for (k, v) in &manager.groups.clone() {
            let group_title = chat_group_title(&k, v, &manager.messages);
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
                    for member_id in &v.members {
                        let Some(messages) = manager.messages.get(member_id).cloned() else {
                            continue;
                        };
                        queue_summary_if_needed(
                            member_id,
                            &messages,
                            deepseek_sender,
                            &mut deepseek_manager,
                        );

                        let pane_key = (k.clone(), member_id.clone());
                        let mut pane_size = *group_member_sizes
                            .entry(pane_key.clone())
                            .or_insert(GROUP_MEMBER_CHAT_SIZE);
                        resizable_chat_pane(
                            ui,
                            ctx,
                            k,
                            member_id,
                            &mut pane_size,
                            |ui| {
                                chat_body_ui(
                                    ui,
                                    ctx,
                                    &messages,
                                    napcat_sender,
                                    member_id,
                                    &mut chat_input_msgs,
                                    targets_for_messages(member_id, &messages),
                                    &mut ime,
                                    None,
                                );
                            },
                        );
                        group_member_sizes.insert(pane_key, pane_size);
                        if member_id != v.members.last().unwrap_or(member_id) {
                            ui.add_space(GROUP_CHAT_SEPARATOR_HEIGHT * 0.5);
                            ui.separator();
                            ui.add_space(GROUP_CHAT_SEPARATOR_HEIGHT * 0.5);
                        }
                    }

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
                group_rects.insert(k.clone(), response.response.rect);
                if v.members.len() == 1 {
                    let member_id = v.members[0].clone();
                    let button_size = egui::vec2(52.0, 22.0);
                    let button_pos = egui::pos2(
                        response.response.rect.right() - button_size.x - 8.0,
                        response.response.rect.top() + 34.0,
                    );
                    egui::Area::new(Id::new((
                        k,
                        member_id.as_str(),
                        "leave_group_overlay",
                    )))
                    .order(egui::Order::Foreground)
                    .fixed_pos(button_pos)
                    .show(ctx, |ui| {
                        if ui
                            .add_sized(button_size, egui::Button::new("离开"))
                            .on_hover_text("离开讨论组")
                            .clicked()
                        {
                            if let Some(group) = manager.groups.get_mut(k) {
                                group.members.retain(|id| id != &member_id);
                                manager.persist().ok();
                            }
                        }
                    });
                }
            }
        }

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

            let rect = ui.max_rect();
            let (nickname, heights) = get_nickname_lens(target_id.clone(), &messages);
            let targets = targets_for_messages(&target_id, &messages);
            queue_summary_if_needed(
                &target_id,
                &messages,
                deepseek_sender,
                &mut deepseek_manager,
            );

            if manager
                .groups
                .values()
                .any(|group| group.members.contains(&target_id))
            {
                continue;
            }

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
            );
        }
    });

    ctx.memory(|m| {
        cached_memory.ui_memory = m.clone();
    });
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
