use bevy::prelude::*;

use crate::GameState;

// ······
// Plugin
// ······

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Play), resume_camera)
            .add_systems(OnExit(GameState::Play), pause_camera);
    }
}

// ··········
// Components
// ··········

#[derive(Component)]
pub struct GameCamera;

// ·······
// Systems
// ·······

fn resume_camera(mut cam: Query<&mut Camera, With<GameCamera>>) {
    if let Ok(mut cam) = cam.single_mut() {
        cam.is_active = true;
    }
}

fn pause_camera(mut cam: Query<&mut Camera, With<GameCamera>>) {
    if let Ok(mut cam) = cam.single_mut() {
        cam.is_active = false;
    }
}
