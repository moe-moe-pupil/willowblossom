mod ime;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use ime::*;

use crate::mirai::MiraiIOSender;


#[derive(Resource, Default)] 
pub struct MyApp{
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
            .add_systems(Update, ui_system.run_if(resource_exists::<MiraiIOSender>));
    }
}

pub fn setup_system(
  mut egui_context: EguiContexts,
  mut windows: Query<&mut Window>,
) {
  let mut window = windows.single_mut();
  window.ime_enabled = true;
  let mut txt_font = egui::FontDefinitions::default();
  txt_font.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "Meiryo".to_owned());
  let fd = egui::FontData::from_static(include_bytes!("C:/Windows/Fonts/simkai.ttf"));
  txt_font.font_data.insert("Meiryo".to_owned(), fd);
  egui_context.ctx_mut().set_fonts(txt_font); 
}

pub fn ui_system(
  mut contexts: EguiContexts, 
  mut app: ResMut<MyApp>, 
  mut ime: ResMut<ImeManager>, 
  sender: Res<MiraiIOSender>
) {
  let ctx = contexts.ctx_mut();
  egui::TopBottomPanel::bottom("input_panel").show(ctx, |ui| {
      ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
        let _teo_m = ime.text_edit_multiline(&mut app.multi_text, 400.0, ui, ctx, sender.as_ref());
      })      
  });
}