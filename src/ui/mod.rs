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

use crate::{
    deepseek::{
        DeepseekIOSender,
        DeepseekManager,
        DeepseekPlugin,
        DeepseekRequest,
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
    cached_memory: ResMut<Persistent<CachedMemory>>,
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
    lens: Vec<usize>,
    messages: &Vec<NapcatMessage>,
    napcat_sender: Option<&NapcatIOSender>,
    target_id: &str,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    targets: Vec<NapcatSendTarget>,
    ime: &mut ResMut<ImeManager>,
    group_rects: &HashMap<String, Rect>,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
) {
    // TODO: find the way to get the real font_height correctly
    let font_height = 64.0;
    let input_height = 84.0;
    egui::Window::new(nickname)
        .open(&mut true)
        .id(id)
        .constrain_to(rect)
        .show(ctx, |ui| {
            let width = ui.available_width();
            let message_height =
                (ui.available_height() - input_height - ui.spacing().item_spacing.y).max(0.0);
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .max_height(message_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (message, len) in messages.iter().zip(lens.iter()) {
                        ui.allocate_ui_with_layout(
                            egui::vec2(
                                width,
                                48.0 + *len as f32 * font_height / width,
                            ),
                            if message.data.self_id == message.data.user_id {
                                egui::Layout::top_down(egui::Align::RIGHT)
                            } else {
                                egui::Layout::top_down(egui::Align::LEFT)
                            },
                            |ui| {
                                ui.label(&message.data.sender.nickname);
                                for chain in &message.data.message {
                                    match &chain.variant {
                                        NapcatMessageChainType::Text { data: text_data } => {
                                            let text = format!("{}", text_data.text);
                                            ui.add(egui::Label::new(&text).wrap());
                                        },
                                        NapcatMessageChainType::Source(_) => {},
                                        // TODO: Support images
                                    }
                                }
                            },
                        );
                    }
                });

            ui.allocate_ui_with_layout(
                egui::vec2(width, input_height),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    if !chat_input_msgs.contains_key(target_id) {
                        chat_input_msgs.insert(target_id.to_owned(), String::new());
                    }

                    let text = chat_input_msgs.get_mut(target_id).unwrap();
                    let Some(napcat_sender) = napcat_sender else {
                        ui.add_enabled(
                            false,
                            egui::TextEdit::multiline(text)
                                .desired_width(ui.available_width())
                                .desired_rows(3),
                        );
                        return;
                    };
                    let _ = ime.chat_input_multiline(
                        target_id,
                        text,
                        ui.available_width(),
                        ui,
                        ctx,
                        napcat_sender,
                        targets,
                    );
                },
            );
        })
        .map(|response| {
            for (k, rect) in group_rects {
                let inside = rect.contains_rect(response.response.rect);
                if inside {
                    let members = &mut manager.groups.get_mut(k).unwrap().members;
                    if !members.contains(&target_id.to_owned()) {
                        members.push(target_id.to_string());
                    }
                }
            }
        });
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
                // TODO: Support images
                // NapcatMessageChainType::Image { data: image } => {
                //     height += 200.0;
                // },
                _ => {},
            };
        }

        if message.data.sender.user_id.to_string() == *target_id {
            nickname = &message.data.sender.nickname;
        }
        lens.push(len)
    }

    (nickname, lens)
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
        if summary.pending || summary.message_count == message_count {
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
            deepseek_manager.summaries.insert(
                target_id.to_owned(),
                crate::deepseek::DeepseekSummary {
                    latest: String::new(),
                    message_count,
                    pending: true,
                    error: None,
                },
            );
        },
        Err(error) => {
            deepseek_manager.summaries.insert(
                target_id.to_owned(),
                crate::deepseek::DeepseekSummary {
                    latest: String::new(),
                    message_count,
                    pending: false,
                    error: Some(error),
                },
            );
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
                    "{} / {} 条",
                    nickname, summary.message_count
                ));
                if summary.pending {
                    ui.label("总结中...");
                } else if let Some(error) = &summary.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                } else {
                    ui.label(summary.latest.trim());
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
            egui::Window::new(format!("讨论组: {}", k))
                .vscroll(true)
                .open(&mut true)
                .constrain_to(ui.max_rect())
                .id(Id::new(k))
                .order(egui::Order::Background)
                .show(ctx, |ui| {
                    group_rects.insert(k.clone(), ui.max_rect());
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
                        let (nickname, heights) = get_nickname_lens(member_id.clone(), &messages);
                        let id = egui::Id::new(member_id);
                        chat_window(
                            nickname,
                            id,
                            ui.max_rect(),
                            ctx,
                            heights,
                            &messages,
                            napcat_sender,
                            member_id,
                            &mut chat_input_msgs,
                            targets_for_messages(member_id, &messages),
                            &mut ime,
                            &group_rects,
                            &mut manager,
                        );
                    }
                });
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
