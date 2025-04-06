mod ime;
use std::{
    cmp::min,
    io::Cursor,
    path::Path,
    time::Duration,
};
mod components;
use bevy::{
    prelude::*,
    utils::HashMap,
};
use bevy_egui::{
    egui::{
        self,
        epaint::CircleShape,
        menu,
        text_edit::TextEditOutput,
        Context,
        Id,
        Layout,
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
        Widget, WidgetText,
    },
    EguiContexts,
    EguiPlugin,
};
use bevy_persistent::{
    Persistent,
    StorageFormat,
};
use egui_extras::{
    Column,
    TableBuilder,
};
use ime::*;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::json;
use tungstenite::Message;

use crate::{
    deepseek::{
        self, DeepseekIOSender, DeepseekManager, DeepseekPlugin
    },
    napcat::{
        ChatGroup,
        NapcatIOSender,
        NapcatMessage,
        NapcatMessageChainType,
        NapcatMessageManager,
    },
};
pub struct UIPlugin;
#[derive(Resource)]
pub struct GIFImages {
    images: HashMap<String, Vec<(TextureHandle, u32)>>,
}

#[derive(Resource)]
pub struct AutoCompletionTime {
    /// track when the bomb should explode (non-repeating timer)
    pub timer: Timer,
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
        app.add_plugins(EguiPlugin)
            .add_plugins(ImePlugin)
            .add_plugins(DeepseekPlugin)
            .add_systems(Startup, setup_system)
            .add_systems(
                Update,
                load_ui_memory.run_if(resource_added::<Persistent<CachedMemory>>),
            )
            .add_systems(
                Update,
                ui_system
                    .run_if(resource_exists::<NapcatIOSender>)
                    .run_if(resource_exists::<DeepseekIOSender>)
                    .after(load_ui_memory),
            );
    }
}

pub fn setup_system(
    mut command: Commands,
    mut egui_context: EguiContexts,
    mut windows: Query<&mut Window>,
) {
    let ctx = egui_context.ctx_mut();
    let config_dir = Path::new(".data").join("willowblossom");
    let cached_memory = Persistent::<CachedMemory>::builder()
        .name("ui_memory")
        .format(StorageFormat::Ron)
        .path(config_dir.join("ui_memory.ron"))
        .default(CachedMemory {
            ui_memory: Memory::default(),
        })
        .build()
        .expect("failed to init messages");
    command.insert_resource(cached_memory);
    command.insert_resource(AutoCompletionTime {
        timer: Timer::new(Duration::from_secs(5), TimerMode::Once),
    });
    let mut window = windows.single_mut();
    window.ime_enabled = true;
    let mut txt_font = egui::FontDefinitions::default();
    egui_extras::install_image_loaders(ctx);
    txt_font
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "Meiryo".to_owned());
    let fd = egui::FontData::from_static(include_bytes!(
        "../../assets/fonts/AlibabaHealthFont.ttf"
    ));
    txt_font.font_data.insert("Meiryo".to_owned(), fd.into());
    egui_context.ctx_mut().set_fonts(txt_font);
    command.insert_resource(GIFImages {
        images: HashMap::default(),
    })
}

pub fn load_ui_memory(
    mut egui_context: EguiContexts,
    cached_memory: ResMut<Persistent<CachedMemory>>,
) {
    let ctx = egui_context.ctx_mut();
    ctx.memory_mut(|m| *m = cached_memory.ui_memory.clone());
}

fn chat_window(
    nickname: &str,
    id: Id,
    rect: Rect,
    ctx: &Context,
    lens: Vec<usize>,
    messages: &Vec<NapcatMessage>,
    napcat_sender: &NapcatIOSender,
    deepseek_sender: &DeepseekIOSender,
    target_id: &str,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    target_ids: Vec<String>,
    ime: &mut ResMut<ImeManager>,
    group_rects: &HashMap<String, Rect>,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    auto_completion_timer: &mut Timer,
    deepseek_manager: &mut DeepseekManager,
) {
    //TODO: find the way to get the real font_height correctly
    let font_height = 64.0;
    egui::Window::new(nickname)
        .vscroll(true)
        .open(&mut true)
        .id(id)
        .constrain_to(rect)
        .show(
            ctx,
            |ui| {
                let width = ui.max_rect().width();
                TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .auto_shrink(true)
                    .cell_layout(egui::Layout::top_down(
                        egui::Align::LEFT,
                    ))
                    .stick_to_bottom(true)
                    .column(Column::exact(width))
                    .min_scrolled_height(0.0)
                    .body(|body| {
                        body.heterogeneous_rows(lens.iter().map(|len| 48.0 + *len as f32 * font_height / width), |mut row| {
                            let row_index = row.index();
                            let message = &messages[row_index];
                            row.col(|ui: &mut egui::Ui| {
                                ui.with_layout(
                                    if message.data.self_id == message.data.user_id {
                                        egui::Layout::top_down(egui::Align::RIGHT)
                                    } else {
                                        egui::Layout::top_down(egui::Align::LEFT)
                                    },
                                    |ui| {
                                        ui.label(&message.data.sender.nickname);
                                        for chain in &message.data.message {
                                            match &chain.variant {
                                                NapcatMessageChainType::Text {
                                                    data: text_data,
                                                } => {
                                                    let text = format!("{}", text_data.text);
                                                    ui.add(bevy_egui::egui::Label::new(&text).wrap());
                                                },
                                                NapcatMessageChainType::Source(_) => {},
                                                // TODO: Support images
                                            }
                                        }
                                    },
                                );
                            });
                        })
                    });

                ui.with_layout(
                    egui::Layout::bottom_up(egui::Align::Center),
                    |ui| {
                        if !chat_input_msgs.contains_key(target_id) {
                            chat_input_msgs.insert(target_id.to_owned(), String::new());
                        }

                        let text = chat_input_msgs.get_mut(target_id).unwrap();
                        let _teo_m = ime.chat_input_multiline(
                            text,
                            ui.max_rect().width(),
                            ui,
                            ctx,
                            napcat_sender,
                            target_ids,
                            &mut deepseek_manager.last_fim_response,
                        );
                        if _teo_m.response.has_focus() && auto_completion_timer.finished() {
                            auto_completion_timer.reset();
                            if deepseek_manager.last_post_text != *text {
                                deepseek_manager.last_post_text = text.to_string();
                                let err = deepseek_sender
                                .0
                                .try_send(Message::Text(text.to_owned().into()))
                                .expect("can't send message to deepseek");
                            }
                        }
                    },
                );
            },
            |ui| {
                for (k, rect) in group_rects {
                    let inside = rect.contains_rect(ui.max_rect());
                    if inside {
                        let members = &mut manager.groups.get_mut(k).unwrap().members;
                        if !members.contains(&target_id.to_owned()) {
                            members.push(target_id.to_string());
                        }
                    }
                }
            },
        );
}

pub fn get_nickname_lens(target_id: String, messages: &Vec<NapcatMessage>) -> (&str, Vec<usize>) {
    let mut nickname = "";
    let mut lens: Vec<usize> = vec![];
    for message in messages {
        let mut len: usize = 0;
        for chain in &message.data.message {
            match &chain.variant {
                NapcatMessageChainType::Source(_) => {},
                NapcatMessageChainType::Text { data  } => {
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

pub fn ui_system(
    mut contexts: EguiContexts,
    mut ime: ResMut<ImeManager>,
    napcat_sender: Res<NapcatIOSender>,
    deepseek_sender: Res<DeepseekIOSender>,
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
    mut cached_memory: ResMut<Persistent<CachedMemory>>,
    mut has_run_once: Local<bool>,
    mut new_chat_group_modal_string_open: Local<(String, bool)>,
    mut chat_input_msgs: Local<HashMap<String, String>>,
    time: Res<Time>,
    mut auto_completion_time: ResMut<AutoCompletionTime>,
    mut deepseek_manager: ResMut<DeepseekManager>,
) {
    auto_completion_time.timer.tick(time.delta());

    let ctx = contexts.ctx_mut();

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
            ui.label("Right resizeable panel");
            ui.allocate_rect(
                ui.available_rect_before_wrap(),
                egui::Sense::hover(),
            );
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        for (k, v) in &manager.groups.clone() {
            egui::Window::new(format!("讨论组: {}", k))
                .vscroll(true)
                .open(&mut true)
                .constrain_to(ui.max_rect())
                .id(Id::new(k))
                .order(egui::Order::Background)
                .show(
                    ctx,
                    |ui| {
                        group_rects.insert(k.clone(), ui.max_rect());
                        for member_id in &v.members {
                            let messages = manager.messages.get(member_id).unwrap().clone();
                            let (nickname, heights) =
                                get_nickname_lens(member_id.clone(), &messages);
                            let id = egui::Id::new(member_id);
                            chat_window(
                                nickname,
                                id,
                                ui.max_rect(),
                                ctx,
                                heights,
                                &messages,
                                &napcat_sender,
                                &deepseek_sender,
                                member_id,
                                &mut chat_input_msgs,
                                vec![member_id.to_string()],
                                &mut ime,
                                &group_rects,
                                &mut manager,
                                &mut auto_completion_time.timer,
                                &mut deepseek_manager
                            );
                        }
                    },
                    |ui| {},
                );
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
            let target_ids = vec![target_id.clone()];

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
                &napcat_sender,
                &deepseek_sender,
                &target_id,
                &mut chat_input_msgs,
                target_ids,
                &mut ime,
                &group_rects,
                &mut manager,
                &mut auto_completion_time.timer,
                &mut deepseek_manager
            );
        }
    });

    ctx.memory(|m| {
        cached_memory.ui_memory = m.clone();
    });
    cached_memory.persist().ok();
}
