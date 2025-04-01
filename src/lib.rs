mod camera;
mod napcat;
mod ui;
mod deepseek;

use bevy::{
    asset::AssetMetaCheck,
    prelude::*, window::WindowResolution,
};

// [CHANGE]: Game title and resolution
pub const GAME_TITLE: &str = "Hello Bevy!";

// Game state
#[derive(States, Debug, Default, Clone, Eq, PartialEq, Hash)]
pub enum GameState {
    #[default]
    Loading,
    Menu,
    Play,
}

// Main game plugin
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        // Release only plugins (embedded assets)
        #[cfg(not(debug_assertions))]
        {
            use bevy_embedded_assets::{
                EmbeddedAssetPlugin,
                PluginMode,
            };
            app.add_plugins(EmbeddedAssetPlugin {
                mode: PluginMode::ReplaceDefault,
            });
        }

        // Default plugins
        #[allow(unused_mut)]
        let mut window_plugin = WindowPlugin {
            primary_window: Some(Window {
                title: GAME_TITLE.into(),
                resolution: WindowResolution::new(800.0, 600.0),                
                canvas: Some("#bevy".to_string()),
                ..default()
            }),
            ..default()
        };

        #[cfg(feature = "resizable")]
        {
            let win = window_plugin.primary_window.as_mut().unwrap();
            win.resizable = true;
            win.fit_canvas_to_parent = true;
        }

        #[cfg(not(feature = "pixel_perfect"))]
        let image_plugin = ImagePlugin::default();

        #[cfg(feature = "pixel_perfect")]
        let image_plugin = ImagePlugin::default_nearest();

        app.add_plugins(DefaultPlugins.set(window_plugin).set(image_plugin));

        // Game
        app.add_plugins((camera::CameraPlugin, napcat::NapcatPlugin, ui::UIPlugin));
    }
}
