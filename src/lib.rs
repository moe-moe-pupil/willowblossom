mod battle_round;
mod camera;
mod deepseek;
mod napcat;
pub mod rule_engine;
mod scene;
mod ui;

use std::path::Path;

use bevy::{
    log::LogPlugin,
    prelude::*,
    window::{
        PrimaryWindow,
        WindowMoved,
        WindowPosition,
        WindowResized,
        WindowResolution,
    },
};
use bevy_persistent::{
    Persistent,
    StorageFormat,
};
use serde::{
    Deserialize,
    Serialize,
};

// [CHANGE]: Game title and resolution
pub const GAME_TITLE: &str = "柳絮，只是另一个跑团软件";
const DEFAULT_WINDOW_WIDTH: u32 = 800;
const DEFAULT_WINDOW_HEIGHT: u32 = 600;
const MIN_WINDOW_WIDTH: u32 = 320;
const MIN_WINDOW_HEIGHT: u32 = 240;

#[derive(Resource, Serialize, Deserialize)]
struct AppSettings {
    #[serde(default = "default_window_width")]
    window_width: u32,
    #[serde(default = "default_window_height")]
    window_height: u32,
    #[serde(default)]
    window_x: Option<i32>,
    #[serde(default)]
    window_y: Option<i32>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            window_x: None,
            window_y: None,
        }
    }
}

impl AppSettings {
    fn window_size(&self) -> (u32, u32) {
        (
            self.window_width.max(MIN_WINDOW_WIDTH),
            self.window_height.max(MIN_WINDOW_HEIGHT),
        )
    }

    fn set_window_size(&mut self, width: u32, height: u32) -> bool {
        let width = width.max(MIN_WINDOW_WIDTH);
        let height = height.max(MIN_WINDOW_HEIGHT);
        let changed = self.window_width != width || self.window_height != height;

        if changed {
            self.window_width = width;
            self.window_height = height;
        }

        changed
    }

    fn window_position(&self) -> Option<IVec2> {
        Some(IVec2::new(
            self.window_x?,
            self.window_y?,
        ))
    }

    fn set_window_position(&mut self, position: IVec2) -> bool {
        let changed = self.window_x != Some(position.x) || self.window_y != Some(position.y);

        if changed {
            self.window_x = Some(position.x);
            self.window_y = Some(position.y);
        }

        changed
    }
}

fn default_window_width() -> u32 { DEFAULT_WINDOW_WIDTH }

fn default_window_height() -> u32 { DEFAULT_WINDOW_HEIGHT }

fn load_app_settings() -> Persistent<AppSettings> {
    let config_dir = Path::new(".data").join("willowblossom");
    Persistent::<AppSettings>::builder()
        .name("app_settings")
        .format(StorageFormat::Toml)
        .path(config_dir.join("app_settings.toml"))
        .default(AppSettings::default())
        .revertible(true)
        .revert_to_default_on_deserialization_errors(true)
        .build()
        .expect("failed to init app settings")
}

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
        let app_settings = load_app_settings();
        let (window_width, window_height) = app_settings.window_size();

        #[allow(unused_mut)]
        let mut window_plugin = WindowPlugin {
            primary_window: Some(Window {
                title: GAME_TITLE.into(),
                resolution: WindowResolution::new(window_width, window_height),
                position: app_settings
                    .window_position()
                    .map(WindowPosition::At)
                    .unwrap_or_default(),
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

        app.add_plugins(
            DefaultPlugins
                .set(LogPlugin {
                    filter: "wgpu=error,naga=warn,bevy_persistent=warn".to_string(),
                    ..default()
                })
                .set(window_plugin)
                .set(image_plugin),
        );
        app.insert_resource(app_settings);

        // Game
        app.add_plugins((
            battle_round::BattleRoundPlugin,
            camera::CameraPlugin,
            napcat::NapcatPlugin,
            rule_engine::RuleEnginePlugin,
            scene::ScenePreviewPlugin,
            ui::UIPlugin,
        ))
        .add_systems(Update, persist_primary_window_size);
    }
}

fn persist_primary_window_size(
    mut resize_events: MessageReader<WindowResized>,
    mut moved_events: MessageReader<WindowMoved>,
    primary_window: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut app_settings: ResMut<Persistent<AppSettings>>,
) {
    let Ok((primary_entity, window)) = primary_window.single() else {
        resize_events.clear();
        moved_events.clear();
        return;
    };

    let mut primary_window_resized = false;
    for event in resize_events.read() {
        if event.window == primary_entity {
            primary_window_resized = true;
        }
    }

    let mut primary_window_position = None;
    for event in moved_events.read() {
        if event.window == primary_entity {
            primary_window_position = Some(event.position);
        }
    }

    let mut changed = false;
    if primary_window_resized {
        changed |= app_settings.set_window_size(
            window.resolution.physical_width(),
            window.resolution.physical_height(),
        );
    }
    if let Some(position) = primary_window_position {
        changed |= app_settings.set_window_position(position);
    }

    if changed {
        if let Err(err) = app_settings.persist() {
            eprintln!("failed to persist app window settings: {err}");
        }
    }
}
