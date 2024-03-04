mod ime;
use bevy::prelude::*;
use bevy_egui::{
    egui,
    EguiContexts,
    EguiPlugin,
};
use ime::*;

use crate::mirai::{
    MiraiIOSender,
    MiraiMessageChain,
    MiraiMessageChainType,
    MiraiMessageManager,
    Plain,
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
    mut manager: ResMut<MiraiMessageManager>,
) {
    let ctx = contexts.ctx_mut();
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
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show_rows(
                ui,
                200.0,
                manager.messages.len(),
                |ui, row_range| {
                    for (id, message) in &manager.messages {
                        for (chain) in &message.data.message_chain {
                            match &chain.variant {
                                MiraiMessageChainType::Plain(plain) => {
                                    let text = format!(
                                        "{}:\n {}",
                                        message.data.sender.nickname, plain.text
                                    );
                                    ui.label(text);
                                },
                                MiraiMessageChainType::Source(_) => {},
                                MiraiMessageChainType::Image(_) => todo!(),
                            }
                        }
                    }
                },
            )
    });
    egui::TopBottomPanel::top("top_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("Top resizeable panel");
            ui.allocate_rect(
                ui.available_rect_before_wrap(),
                egui::Sense::hover(),
            );
        });
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
                    );
                },
            )
        });
}
