mod ime;
use std::cmp::min;

use bevy::prelude::*;
use bevy_egui::{
    egui,
    EguiContexts,
    EguiPlugin,
};
use bevy_persistent::Persistent;
use egui_extras::{
    Column,
    TableBuilder,
};
use ime::*;

use crate::mirai::{
    MiraiIOSender,
    MiraiMessageChainType,
    MiraiMessageManager,
};

#[derive(Resource, Default)]
pub struct MyApp {
    single_text: String,
    multi_text: String,
}

pub struct UIPlugin;

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin)
            .add_plugins(ImePlugin)
            .insert_resource(MyApp::default())
            .add_systems(Startup, setup_system)
            .add_systems(
                Update,
                ui_system.run_if(resource_exists::<MiraiIOSender>),
            );
    }
}

pub fn setup_system(mut egui_context: EguiContexts, mut windows: Query<&mut Window>) {
    let mut window = windows.single_mut();
    window.ime_enabled = true;
    let mut txt_font = egui::FontDefinitions::default();
    txt_font
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "Meiryo".to_owned());
    let fd = egui::FontData::from_static(include_bytes!(
        "C:/Windows/Fonts/simkai.ttf"
    ));
    txt_font.font_data.insert("Meiryo".to_owned(), fd);
    egui_context.ctx_mut().set_fonts(txt_font);
}

pub fn ui_system(
    mut contexts: EguiContexts,
    mut app: ResMut<MyApp>,
    mut ime: ResMut<ImeManager>,
    sender: Res<MiraiIOSender>,
    mut manager: ResMut<Persistent<MiraiMessageManager>>,
) {
    let ctx = contexts.ctx_mut();
    let mut total_rows = 0;
    let target_message_key = "1670426821";
    let mut heights: Vec<f32> = vec![];

    for message in manager.messages.get_mut(target_message_key).unwrap() {
        total_rows += 1;
        let mut height: f32 = 32.0;
        for chain in &message.data.message_chain {
            match &chain.variant {
                MiraiMessageChainType::Source(_) => {},
                MiraiMessageChainType::Image(image) => {
                    total_rows += 1;
                    height += f32::min(image.height, 200.0);
                },
                _ => {
                    height += 16.0;
                    total_rows += 1;
                },
            };
        }
        heights.push(height)
    }

    egui_extras::install_image_loaders(&ctx);
    egui::TopBottomPanel::top("top_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("Top resizeable panel");
            ui.allocate_rect(
                ui.available_rect_before_wrap(),
                egui::Sense::hover(),
            );
        });
    egui::SidePanel::left("left_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("Left resizeable panel");
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
        egui::TopBottomPanel::bottom("input_panel")
            .resizable(true)
            .show(ctx, |ui| {
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
                )
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(false)
                .auto_shrink(false)
                .cell_layout(egui::Layout::top_down(
                    egui::Align::LEFT,
                ))
                .stick_to_bottom(true)
                .column(Column::remainder())
                .min_scrolled_height(0.0)
                .body(|body| {
                    body.heterogeneous_rows(heights.into_iter(), |mut row| {
                        let row_index = row.index();
                        let message =
                            &manager.messages.get_mut(target_message_key).unwrap()[row_index];
                        row.col(|ui| {
                            ui.label(message.data.sender.nickname.to_owned());
                            for chain in &message.data.message_chain {
                                match &chain.variant {
                                    MiraiMessageChainType::Plain(plain) => {
                                        let text = format!("{}", plain.text);
                                        ui.add(egui::Label::new(text));
                                    },
                                    MiraiMessageChainType::Source(_) => {},
                                    MiraiMessageChainType::Image(image) => {
                                        ui.add(
                                            egui::Image::new(image.url.to_owned())
                                                .max_size(bevy_egui::egui::Vec2 {
                                                    x: 400.0,
                                                    y: 200.0,
                                                })
                                                .fit_to_exact_size(bevy_egui::egui::Vec2 {
                                                    x: image.width,
                                                    y: image.height,
                                                }),
                                        );
                                    },
                                }
                            }
                        });
                    })
                });
        });
    });
}
