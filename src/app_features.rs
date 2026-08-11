use std::path::Path;

use bevy::prelude::*;
use bevy_persistent::{
    Persistent,
    StorageFormat,
};
use serde::{
    Deserialize,
    Serialize,
};

const APP_FEATURES_PATH: &str = ".data/willowblossom/app_features.toml";

#[derive(Resource, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AppFeatureSettings {
    /// Loads the voxel scene, physics, scene cameras, and replay runtime on startup.
    #[serde(default = "default_game_scene_enabled")]
    pub(crate) game_scene_enabled: bool,
    /// Uses the fixed-sidebar, single-conversation Moonberry chat workspace.
    #[serde(default)]
    pub(crate) legacy_chat_layout_enabled: bool,
}

impl Default for AppFeatureSettings {
    fn default() -> Self {
        Self {
            game_scene_enabled: default_game_scene_enabled(),
            legacy_chat_layout_enabled: false,
        }
    }
}

fn default_game_scene_enabled() -> bool { true }

pub(crate) fn load_app_feature_settings() -> Persistent<AppFeatureSettings> {
    Persistent::<AppFeatureSettings>::builder()
        .name("app_features")
        .format(StorageFormat::Toml)
        .path(Path::new(APP_FEATURES_PATH))
        .default(AppFeatureSettings::default())
        .revertible(true)
        .revert_to_default_on_deserialization_errors(true)
        .build()
        .expect("failed to initialize app feature settings")
}

/// Records what was actually loaded during this process. A persisted scene toggle can differ
/// until restart because Bevy plugins cannot be removed safely after the app starts.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AppFeatureRuntime {
    pub(crate) game_scene_loaded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_legacy_layout_is_disabled_by_default() {
        let settings = AppFeatureSettings::default();

        assert!(settings.game_scene_enabled);
        assert!(!settings.legacy_chat_layout_enabled);
    }

    #[test]
    fn older_empty_feature_files_keep_compatible_defaults() {
        let settings: AppFeatureSettings = toml::from_str("").expect("empty feature file");

        assert_eq!(settings, AppFeatureSettings::default());
    }

    #[test]
    fn feature_file_can_disable_scene_and_enable_legacy_chat_independently() {
        let settings: AppFeatureSettings =
            toml::from_str("game_scene_enabled = false\nlegacy_chat_layout_enabled = true\n")
                .expect("feature settings");

        assert!(!settings.game_scene_enabled);
        assert!(settings.legacy_chat_layout_enabled);
    }
}
