mod ime;
use std::{
    cmp::min,
    io::Cursor,
    path::Path,
};
mod components;
use std::collections::HashMap;

use bevy::{
    prelude::*,
    render::render_resource::encase::rts_array::Length,
};
use bevy_egui::{
    egui::{
        self,
        epaint::CircleShape,
        Align2,
        ColorImage,
        Id,
        ImageButton,
        Layout,
        Memory,
        Painter,
        Pos2,
        Rect,
        Response,
        Sense,
        Stroke,
        TextureHandle,
        TextureOptions,
        Vec2,
        Widget,
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
use image::{
    codecs::gif::GifDecoder,
    io::Reader,
    AnimationDecoder,
};
use ime::*;
use serde::{
    Deserialize,
    Serialize,
};

use self::components::icon::Icon;
use crate::napcat::{
    NapcatIOSender,
    NapcatMessageChainType,
    NapcatMessageManager,
};

#[derive(Resource, Default)]
pub struct MyApp {
    single_text: String,
    multi_text: String,
}

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
            .insert_resource(MyApp::default())
            .add_systems(Startup, setup_system)
            .add_systems(
                Update,
                load_ui_memory.run_if(resource_added::<Persistent<CachedMemory>>),
            )
            .add_systems(
                Update,
                ui_system
                    .run_if(resource_exists::<NapcatIOSender>)
                    .after(load_ui_memory),
            );
    }
}

pub fn setup_system(
    mut command: Commands,
    mut egui_context: EguiContexts,
    mut windows: Query<&mut Window>,
) {
    let Ok(ctx) = egui_context.ctx_mut() else {
        return;
    };
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
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    window.ime_enabled = true;
    dbg!(window.physical_size());
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
    ctx.set_fonts(txt_font);
    command.insert_resource(GIFImages {
        images: HashMap::default(),
    })
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

pub fn ui_system(
    mut contexts: EguiContexts,
    mut app: ResMut<MyApp>,
    mut ime: ResMut<ImeManager>,
    sender: Res<NapcatIOSender>,
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
    mut cached_memory: ResMut<Persistent<CachedMemory>>,
    mut has_run_once: Local<bool>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let target_message_key = "1670426821";

    egui::TopBottomPanel::top("top_panel")
        .resizable(false)
        .show(ctx, |ui| {
            ui.allocate_rect(
                ui.available_rect_before_wrap(),
                egui::Sense::hover(),
            );
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
        let mut group_rect = Rect::from_pos(Pos2::new(-1.0, -1.0));
        egui::Window::new("讨论组")
            .vscroll(true)
            .open(&mut true)
            .constrain_to(ui.max_rect())
            .show(ctx, |ui| {
                group_rect = ui.max_rect();
            });

        if let Some(messages) = manager.messages.get_mut(target_message_key) {
            let id = egui::Id::new(target_message_key);

            let mut default_rect: Rect = Rect::from_pos(Pos2::new(0.0, 0.0));
            if !*has_run_once {
                ctx.memory(|m| {
                    if let Some(rect) = m.area_rect(id) {
                        default_rect = rect;
                    }
                });
                *has_run_once = true
            }
            let mut heights: Vec<f32> = vec![];
            let mut nickname = "";
            for message in messages {
                let mut height: f32 = 32.0;
                for chain in &message.data.message {
                    match &chain.variant {
                        NapcatMessageChainType::Source(_) => {},
                        // TODO: Support images
                        // NapcatMessageChainType::Image { data: image } => {
                        //     height += 200.0;
                        // },
                        _ => {
                            height += 32.0;
                        },
                    };
                }

                if &message.data.sender.user_id.to_string() == target_message_key {
                    nickname = &message.data.sender.nickname;
                }

                heights.push(height)
            }

            egui::Window::new(nickname)
                .vscroll(true)
                .open(&mut true)
                .id(id)
                .constrain_to(ui.max_rect())
                .show(ctx, |ui| {
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
                            body.heterogeneous_rows(heights.into_iter(), |mut row| {
                                let row_index = row.index();
                                let message =
                                    &manager.messages.get_mut(target_message_key).unwrap()
                                        [row_index];
                                row.col(|ui: &mut egui::Ui| {
                                    ui.with_layout(
                                        if message.data.self_id == message.data.user_id {
                                            egui::Layout::top_down(egui::Align::RIGHT)
                                        } else {
                                            egui::Layout::top_down(egui::Align::LEFT)
                                        },
                                        |ui| {
                                            ui.label(message.data.sender.nickname.to_owned());
                                            for chain in &message.data.message {
                                                match &chain.variant {
                                                    NapcatMessageChainType::Text {
                                                        data: text_data,
                                                    } => {
                                                        let text = format!("{}", text_data.text);
                                                        ui.label(text);
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
                            let _teo_m = ime.text_edit_multiline(
                                &mut app.multi_text,
                                ui.max_rect().width(),
                                ui,
                                ctx,
                                sender.as_ref(),
                                &mut manager,
                            );
                        },
                    );
                    dbg!(group_rect.contains_rect(ui.max_rect()));
                });
        }
    });

    ctx.memory(|m| {
        cached_memory.ui_memory = m.clone();
    });
    cached_memory.persist().ok();
}
