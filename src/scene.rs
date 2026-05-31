use std::{
    path::Path,
    sync::Arc,
};

use bevy::{
    asset::RenderAssetUsages,
    camera::{
        visibility::RenderLayers,
        RenderTarget,
    },
    input::mouse::MouseMotion,
    prelude::*,
    render::{
        render_resource::{
            Extent3d,
            TextureDimension,
            TextureFormat,
            TextureUsages,
        },
        view::screenshot::{
            Screenshot,
            ScreenshotCaptured,
        },
    },
    window::PrimaryWindow,
};
use bevy_egui::{
    egui,
    input::EguiWantsInput,
    EguiContexts,
    EguiPostUpdateSet,
    EguiPrimaryContextPass,
};
use bevy_persistent::{
    Persistent,
    StorageFormat,
};
use bevy_voxel_world::prelude::*;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::json;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::{
    camera::GameCamera,
    napcat::{
        NapcatIOSender,
        NapcatOutboundMessage,
    },
};

pub struct ScenePreviewPlugin;

const SCENE_GIZMO_RENDER_LAYER: usize = 1;

#[derive(Resource, Clone, Default)]
pub struct TrpgVoxelWorld;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoxelEditMode {
    Add,
    Erase,
}

#[derive(Resource)]
struct VoxelEditorState {
    enabled: bool,
    mode: VoxelEditMode,
    material: u8,
    brush_radius: i32,
    camera_speed: f32,
    mouse_sensitivity: f32,
}

impl Default for VoxelEditorState {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: VoxelEditMode::Add,
            material: 2,
            brush_radius: 0,
            camera_speed: 12.0,
            mouse_sensitivity: 0.003,
        }
    }
}

#[derive(Component)]
struct FreeCamera;

#[derive(Resource, Default)]
pub struct SceneCaptureRequests {
    pub requests: Vec<SceneCaptureRequest>,
}

pub struct SceneCaptureRequest {
    pub user_id: u64,
}

#[derive(Resource, Default)]
struct SceneCaptureState {
    next_request_id: u64,
    pending_captures: Vec<PendingSceneCapture>,
}

struct PendingSceneCapture {
    request_id: u64,
    user_id: u64,
    camera_entity: Entity,
    target: Handle<Image>,
    output_path: std::path::PathBuf,
}

#[derive(Resource, Default)]
struct PlayerSceneCameras {
    cameras: std::collections::HashMap<u64, PlayerSceneCamera>,
}

struct PlayerSceneCamera {
    entity: Entity,
    target: Handle<Image>,
}

#[derive(Component)]
struct PlayerCaptureCamera {
    user_id: u64,
}

#[derive(Resource)]
struct SceneCaptureEditorState {
    selected_user_id: Option<u64>,
    new_user_id: String,
    show_gizmo: bool,
}

impl Default for SceneCaptureEditorState {
    fn default() -> Self {
        Self {
            selected_user_id: None,
            new_user_id: String::new(),
            show_gizmo: true,
        }
    }
}

#[derive(Resource, Serialize, Deserialize, Default)]
struct VoxelSceneStore {
    #[serde(default)]
    edits: Vec<PersistedVoxelEdit>,
    #[serde(default)]
    capture_cameras: Vec<PersistedCaptureCamera>,
}

#[derive(Serialize, Deserialize, Clone)]
struct PersistedVoxelEdit {
    position: [i32; 3],
    voxel: PersistedVoxel,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
enum PersistedVoxel {
    Air,
    Solid(u8),
}

#[derive(Serialize, Deserialize, Clone)]
struct PersistedCaptureCamera {
    user_id: u64,
    translation: [f32; 3],
    rotation: [f32; 4],
}

impl VoxelWorldConfig for TrpgVoxelWorld {
    type ChunkUserBundle = ();
    type MaterialIndex = u8;

    fn spawning_distance(&self) -> u32 { 3 }

    fn min_despawn_distance(&self) -> u32 { 2 }

    fn chunk_despawn_strategy(&self) -> ChunkDespawnStrategy { ChunkDespawnStrategy::FarAway }

    fn chunk_spawn_strategy(&self) -> ChunkSpawnStrategy { ChunkSpawnStrategy::Close }

    fn max_spawn_per_frame(&self) -> usize { 24 }

    fn spawning_rays(&self) -> usize { 24 }

    fn texture_index_mapper(&self) -> TextureIndexMapperFn<Self::MaterialIndex> {
        Arc::new(|material| match material {
            1 => [1, 1, 1],
            2 => [2, 2, 2],
            3 => [3, 3, 3],
            _ => [0, 0, 0],
        })
    }

    fn voxel_lookup_delegate(&self) -> VoxelLookupDelegate<Self::MaterialIndex> {
        Box::new(|_, _, _| Box::new(starter_scene_voxel))
    }
}

impl Plugin for ScenePreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(VoxelWorldPlugin::with_config(
            TrpgVoxelWorld,
        ))
        .init_resource::<VoxelEditorState>()
        .init_resource::<SceneCaptureRequests>()
        .init_resource::<SceneCaptureState>()
        .init_resource::<PlayerSceneCameras>()
        .init_resource::<SceneCaptureEditorState>()
        .add_systems(Startup, setup_scene_preview)
        .add_systems(
            Update,
            (
                scene_capture_request_system,
                draw_capture_camera_gizmos,
            ),
        )
        .add_systems(Update, apply_saved_voxel_edits)
        .add_systems(
            PostUpdate,
            (
                free_camera_system,
                edit_voxel_world_system,
            )
                .after(EguiPostUpdateSet::ProcessOutput),
        )
        .add_systems(
            EguiPrimaryContextPass,
            (voxel_editor_panel, capture_camera_panel),
        );
    }
}

fn starter_scene_voxel(position: IVec3, _previous: Option<WorldVoxel<u8>>) -> WorldVoxel<u8> {
    let x = position.x;
    let y = position.y;
    let z = position.z;

    if !(-24..=24).contains(&x) || !(-24..=24).contains(&z) {
        return WorldVoxel::Air;
    }

    if y == -1 {
        return WorldVoxel::Solid(1);
    }

    if y == 0 && (x % 6 == 0 || z % 6 == 0) {
        return WorldVoxel::Solid(2);
    }

    if y == 0 && ((x - 8).abs() <= 2 && (z + 5).abs() <= 2) {
        return WorldVoxel::Solid(3);
    }

    if (0..=2).contains(&y) && ((x + 9).abs() <= 1 && (z - 7).abs() <= 1) {
        return WorldVoxel::Solid(3);
    }

    if (0..=1).contains(&y)
        && ((x == -14 && (-14..=-7).contains(&z)) || (z == 12 && (5..=14).contains(&x)))
    {
        return WorldVoxel::Solid(2);
    }

    WorldVoxel::Air
}

fn setup_scene_preview(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut gizmo_config: ResMut<GizmoConfigStore>,
    mut player_cameras: ResMut<PlayerSceneCameras>,
) {
    for (_, config, _) in gizmo_config.iter_mut() {
        config.render_layers = RenderLayers::layer(SCENE_GIZMO_RENDER_LAYER);
    }

    let config_dir = Path::new(".data").join("willowblossom");
    let voxel_scene_store = Persistent::<VoxelSceneStore>::builder()
        .name("voxel_scene")
        .format(StorageFormat::Toml)
        .path(config_dir.join("voxel_scene.toml"))
        .default(VoxelSceneStore::default())
        .build()
        .expect("failed to init voxel scene store");

    for persisted_camera in &voxel_scene_store.capture_cameras {
        spawn_player_capture_camera(
            &mut commands,
            &mut images,
            &mut player_cameras,
            persisted_camera.user_id,
            persisted_camera_transform(persisted_camera),
        );
    }

    commands.insert_resource(voxel_scene_store);

    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(8.0, 18.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.06, 0.07, 0.08)),
            ..default()
        },
        Transform::from_xyz(18.0, 16.0, 18.0).looking_at(Vec3::ZERO, Vec3::Y),
        VoxelWorldCamera::<TrpgVoxelWorld>::default(),
        RenderLayers::from_layers(&[0, SCENE_GIZMO_RENDER_LAYER]),
        GameCamera,
        FreeCamera,
    ));
}

fn voxel_editor_panel(
    mut contexts: EguiContexts,
    mut editor: ResMut<VoxelEditorState>,
    store: Option<Res<Persistent<VoxelSceneStore>>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Window::new("Voxel Tools")
        .default_pos(egui::pos2(12.0, 36.0))
        .default_width(220.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.checkbox(&mut editor.enabled, "Edit");
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut editor.mode,
                    VoxelEditMode::Add,
                    "Add",
                );
                ui.selectable_value(
                    &mut editor.mode,
                    VoxelEditMode::Erase,
                    "Erase",
                );
            });
            ui.add(egui::Slider::new(&mut editor.material, 0..=3).text("Material"));
            ui.add(egui::Slider::new(&mut editor.brush_radius, 0..=3).text("Brush"));
            ui.separator();
            ui.add(egui::Slider::new(&mut editor.camera_speed, 2.0..=40.0).text("Camera"));
            if let Some(store) = store {
                ui.label(format!(
                    "Saved edits: {}",
                    store.edits.len()
                ));
            }
        });
}

fn capture_camera_panel(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut images: ResMut<Assets<Image>>,
    mut editor: ResMut<SceneCaptureEditorState>,
    mut player_cameras: ResMut<PlayerSceneCameras>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    mut free_camera: Query<
        &mut Transform,
        (
            With<FreeCamera>,
            Without<PlayerCaptureCamera>,
        ),
    >,
    mut capture_cameras: Query<(&mut Transform, &PlayerCaptureCamera)>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let camera_ids = capture_cameras
        .iter()
        .map(|(_, camera)| camera.user_id)
        .collect::<Vec<_>>();
    if editor
        .selected_user_id
        .is_none_or(|selected| !camera_ids.contains(&selected))
    {
        editor.selected_user_id = camera_ids.first().copied();
    }

    egui::Window::new("Scene Capture Camera")
        .default_pos(egui::pos2(12.0, 270.0))
        .default_width(260.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.checkbox(&mut editor.show_gizmo, "Show gizmo");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut editor.new_user_id);
                if ui.button("Create").clicked() {
                    if let Ok(user_id) = editor.new_user_id.trim().parse::<u64>() {
                        if !player_cameras.cameras.contains_key(&user_id) {
                            let transform = free_camera
                                .single_mut()
                                .map(|transform| *transform)
                                .unwrap_or_else(|_| default_capture_camera_transform());
                            spawn_player_capture_camera(
                                &mut commands,
                                &mut images,
                                &mut player_cameras,
                                user_id,
                                transform,
                            );
                            editor.selected_user_id = Some(user_id);
                            if let Some(store) = store.as_deref_mut() {
                                upsert_persisted_capture_camera(store, user_id, &transform);
                                if let Err(err) = store.persist() {
                                    eprintln!("failed to persist capture camera: {err}");
                                }
                            }
                        }
                    }
                }
            });

            if camera_ids.is_empty() {
                ui.label("No player capture camera yet");
                return;
            }

            let mut selected_user_id = editor.selected_user_id.unwrap_or(camera_ids[0]);
            egui::ComboBox::from_label("Player")
                .selected_text(selected_user_id.to_string())
                .show_ui(ui, |ui| {
                    for user_id in &camera_ids {
                        ui.selectable_value(
                            &mut selected_user_id,
                            *user_id,
                            user_id.to_string(),
                        );
                    }
                });
            editor.selected_user_id = Some(selected_user_id);

            let Some((mut transform, _)) = capture_cameras
                .iter_mut()
                .find(|(_, camera)| camera.user_id == selected_user_id)
            else {
                return;
            };
            let mut transform_changed = false;

            ui.horizontal(|ui| {
                if ui.button("Use current view").clicked() {
                    if let Ok(free_transform) = free_camera.single_mut() {
                        *transform = *free_transform;
                        transform_changed = true;
                    }
                }
                if ui.button("View from player").clicked() {
                    if let Ok(mut free_transform) = free_camera.single_mut() {
                        *free_transform = *transform;
                    }
                }
                if ui.button("Reset").clicked() {
                    *transform = default_capture_camera_transform();
                    transform_changed = true;
                }
            });

            ui.separator();
            ui.label("Translation");
            ui.horizontal(|ui| {
                transform_changed |= ui
                    .add(
                        egui::DragValue::new(&mut transform.translation.x)
                            .speed(0.1)
                            .prefix("X "),
                    )
                    .changed();
                transform_changed |= ui
                    .add(
                        egui::DragValue::new(&mut transform.translation.y)
                            .speed(0.1)
                            .prefix("Y "),
                    )
                    .changed();
                transform_changed |= ui
                    .add(
                        egui::DragValue::new(&mut transform.translation.z)
                            .speed(0.1)
                            .prefix("Z "),
                    )
                    .changed();
            });

            let (yaw, pitch, roll): (f32, f32, f32) = transform.rotation.to_euler(EulerRot::YXZ);
            let mut yaw = yaw.to_degrees();
            let mut pitch = pitch.to_degrees();
            let mut roll = roll.to_degrees();
            ui.label("Rotation");
            let changed = ui
                .horizontal(|ui| {
                    let yaw_changed = ui
                        .add(egui::DragValue::new(&mut yaw).speed(0.25).prefix("Y "))
                        .changed();
                    let pitch_changed = ui
                        .add(egui::DragValue::new(&mut pitch).speed(0.25).prefix("P "))
                        .changed();
                    let roll_changed = ui
                        .add(egui::DragValue::new(&mut roll).speed(0.25).prefix("R "))
                        .changed();
                    yaw_changed || pitch_changed || roll_changed
                })
                .inner;
            if changed {
                transform.rotation = Quat::from_euler(
                    EulerRot::YXZ,
                    yaw.to_radians(),
                    pitch.to_radians(),
                    roll.to_radians(),
                );
                transform_changed = true;
            }

            if transform_changed {
                if let Some(store) = store.as_deref_mut() {
                    upsert_persisted_capture_camera(store, selected_user_id, &transform);
                    if let Err(err) = store.persist() {
                        eprintln!("failed to persist capture camera: {err}");
                    }
                }
            }
        });
}

fn free_camera_system(
    egui_wants_input: Res<EguiWantsInput>,
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    editor: Res<VoxelEditorState>,
    mut cameras: Query<&mut Transform, With<FreeCamera>>,
) {
    let Ok(mut transform) = cameras.single_mut() else {
        return;
    };
    let wants_pointer_input = egui_wants_input.wants_any_pointer_input();
    let wants_keyboard_input = egui_wants_input.wants_any_keyboard_input();

    if mouse_buttons.pressed(MouseButton::Right) && !wants_pointer_input {
        let delta = mouse_motion.read().fold(Vec2::ZERO, |acc, event| {
            acc + event.delta
        });
        if delta != Vec2::ZERO {
            let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);
            let pitch = (pitch - delta.y * editor.mouse_sensitivity).clamp(-1.45, 1.45);
            let yaw = yaw - delta.x * editor.mouse_sensitivity;
            transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
        }
    } else {
        mouse_motion.clear();
    }

    if wants_keyboard_input {
        return;
    }

    let mut direction = Vec3::ZERO;
    if keyboard.pressed(KeyCode::KeyW) {
        direction += *transform.forward();
    }
    if keyboard.pressed(KeyCode::KeyS) {
        direction -= *transform.forward();
    }
    if keyboard.pressed(KeyCode::KeyD) {
        direction += *transform.right();
    }
    if keyboard.pressed(KeyCode::KeyA) {
        direction -= *transform.right();
    }
    if keyboard.pressed(KeyCode::KeyE) || keyboard.pressed(KeyCode::Space) {
        direction += Vec3::Y;
    }
    if keyboard.pressed(KeyCode::KeyQ) || keyboard.pressed(KeyCode::ControlLeft) {
        direction -= Vec3::Y;
    }

    if direction == Vec3::ZERO {
        return;
    }

    let boost = if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
        3.0
    } else {
        1.0
    };
    transform.translation +=
        direction.normalize() * editor.camera_speed * boost * time.delta_secs();
}

fn draw_capture_camera_gizmos(
    editor: Res<SceneCaptureEditorState>,
    mut gizmos: Gizmos,
    capture_cameras: Query<(&Transform, &PlayerCaptureCamera)>,
) {
    if !editor.show_gizmo {
        return;
    }

    for (transform, camera) in &capture_cameras {
        let origin = transform.translation;
        let selected = editor.selected_user_id == Some(camera.user_id);
        let axis_length = if selected { 2.5 } else { 1.5 };
        let forward_length = if selected { 4.0 } else { 2.5 };

        gizmos.sphere(
            Isometry3d::from_translation(origin),
            0.2,
            Color::srgb(1.0, 0.85, 0.1),
        );
        gizmos.arrow(
            origin,
            origin + *transform.right() * axis_length,
            Color::srgb(0.95, 0.15, 0.15),
        );
        gizmos.arrow(
            origin,
            origin + *transform.up() * axis_length,
            Color::srgb(0.1, 0.85, 0.25),
        );
        gizmos.arrow(
            origin,
            origin + *transform.forward() * forward_length,
            Color::srgb(0.2, 0.45, 1.0),
        );
    }
}

fn edit_voxel_world_system(
    egui_wants_input: Res<EguiWantsInput>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_info: Query<(&Camera, &GlobalTransform), With<VoxelWorldCamera<TrpgVoxelWorld>>>,
    editor: Res<VoxelEditorState>,
    mut voxel_world: VoxelWorld<TrpgVoxelWorld>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
) {
    if !editor.enabled || !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if egui_wants_input.wants_any_pointer_input() {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera_info.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };
    let Some(hit) = voxel_world.raycast(ray, &|(_, voxel)| voxel.is_solid()) else {
        return;
    };

    let base_position = match editor.mode {
        VoxelEditMode::Add => hit.voxel_pos() + hit.voxel_normal().unwrap_or(IVec3::Y),
        VoxelEditMode::Erase => hit.voxel_pos(),
    };
    let voxel = match editor.mode {
        VoxelEditMode::Add => WorldVoxel::Solid(editor.material),
        VoxelEditMode::Erase => WorldVoxel::Air,
    };
    let persisted_voxel = match voxel {
        WorldVoxel::Air => PersistedVoxel::Air,
        WorldVoxel::Solid(material) => PersistedVoxel::Solid(material),
        WorldVoxel::Unset => return,
    };

    for position in brush_positions(base_position, editor.brush_radius) {
        voxel_world.set_voxel(position, voxel);
        if let Some(store) = store.as_deref_mut() {
            upsert_persisted_edit(store, position, persisted_voxel);
        }
    }

    if let Some(store) = store.as_deref_mut() {
        if let Err(err) = store.persist() {
            eprintln!("failed to persist voxel scene edits: {err}");
        }
    }
}

fn scene_capture_request_system(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut requests: ResMut<SceneCaptureRequests>,
    mut capture_state: ResMut<SceneCaptureState>,
    mut player_cameras: ResMut<PlayerSceneCameras>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    free_camera: Query<&Transform, With<FreeCamera>>,
    mut capture_camera_query: Query<&mut Camera, With<PlayerCaptureCamera>>,
) {
    let pending_captures = capture_state.pending_captures.drain(..).collect::<Vec<_>>();
    for pending in pending_captures {
        commands
            .spawn(Screenshot::image(
                pending.target.clone(),
            ))
            .observe(
                move |screenshot: On<ScreenshotCaptured>,
                      napcat_sender: Option<Res<NapcatIOSender>>,
                      mut cameras: Query<&mut Camera, With<PlayerCaptureCamera>>| {
                    if let Ok(mut camera) = cameras.get_mut(pending.camera_entity) {
                        camera.is_active = false;
                    }

                    let save_result = match screenshot.image.clone().try_into_dynamic() {
                        Ok(image) => image
                            .to_rgb8()
                            .save(&pending.output_path)
                            .map_err(|err| err.to_string()),
                        Err(err) => Err(err.to_string()),
                    };

                    if let Err(err) = save_result {
                        eprintln!("failed to save scene capture: {err}");
                        return;
                    }

                    let Some(napcat_sender) = napcat_sender else {
                        return;
                    };
                    let file = match napcat_file_uri(&pending.output_path) {
                        Ok(file) => file,
                        Err(err) => {
                            eprintln!("failed to build scene capture file uri: {err}");
                            return;
                        },
                    };
                    let message = Message::Text(
                        json!({
                            "action": "send_private_msg",
                            "params": {
                                "user_id": pending.user_id,
                                "message": [
                                    {
                                        "type": "image",
                                        "data": {
                                            "file": file,
                                            "summary": "场景观察"
                                        }
                                    }
                                ]
                            }
                        })
                        .to_string()
                        .into(),
                    );

                    if let Err(err) = napcat_sender.0.try_send(NapcatOutboundMessage {
                        request_id: pending.request_id,
                        target_id: pending.user_id.to_string(),
                        message,
                    }) {
                        eprintln!("failed to queue scene capture image: {err}");
                    }
                },
            );
    }

    let capture_requests = requests.requests.drain(..).collect::<Vec<_>>();
    if capture_requests.is_empty() {
        return;
    }

    let default_transform = free_camera
        .single()
        .map(|transform| *transform)
        .unwrap_or_else(|_| default_capture_camera_transform());

    for request in capture_requests {
        let player_camera =
            if let Some(player_camera) = player_cameras.cameras.get(&request.user_id) {
                PlayerSceneCamera {
                    entity: player_camera.entity,
                    target: player_camera.target.clone(),
                }
            } else {
                let player_camera = spawn_player_capture_camera(
                    &mut commands,
                    &mut images,
                    &mut player_cameras,
                    request.user_id,
                    default_transform,
                );
                if let Some(store) = store.as_deref_mut() {
                    upsert_persisted_capture_camera(
                        store,
                        request.user_id,
                        &default_transform,
                    );
                    if let Err(err) = store.persist() {
                        eprintln!("failed to persist capture camera: {err}");
                    }
                }
                player_camera
            };

        if let Ok(mut camera) = capture_camera_query.get_mut(player_camera.entity) {
            camera.is_active = true;
        }

        let output_dir = Path::new(".data")
            .join("willowblossom")
            .join("scene_captures");
        if let Err(err) = std::fs::create_dir_all(&output_dir) {
            eprintln!("failed to create scene capture directory: {err}");
            continue;
        }
        let output_path = output_dir.join(format!(
            "player_{}.png",
            request.user_id
        ));
        let request_id = capture_state.next_request_id;
        capture_state.next_request_id += 1;
        let user_id = request.user_id;

        capture_state.pending_captures.push(PendingSceneCapture {
            request_id,
            user_id,
            camera_entity: player_camera.entity,
            target: player_camera.target.clone(),
            output_path,
        });
    }
}

fn default_capture_camera_transform() -> Transform {
    Transform::from_xyz(18.0, 16.0, 18.0).looking_at(Vec3::ZERO, Vec3::Y)
}

fn spawn_player_capture_camera(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    player_cameras: &mut PlayerSceneCameras,
    user_id: u64,
    transform: Transform,
) -> PlayerSceneCamera {
    let target = images.add(scene_capture_image());
    let entity = commands
        .spawn((
            Camera3d::default(),
            Camera {
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.06, 0.07, 0.08)),
                order: -1,
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
            transform,
            PlayerCaptureCamera { user_id },
        ))
        .id();
    player_cameras.cameras.insert(user_id, PlayerSceneCamera {
        entity,
        target: target.clone(),
    });
    PlayerSceneCamera { entity, target }
}

fn persisted_camera_transform(camera: &PersistedCaptureCamera) -> Transform {
    Transform {
        translation: Vec3::from(camera.translation),
        rotation: Quat::from_array(camera.rotation),
        scale: Vec3::ONE,
    }
}

fn napcat_file_uri(path: &Path) -> Result<String, String> {
    let path = std::fs::canonicalize(path).map_err(|err| err.to_string())?;
    url::Url::from_file_path(&path)
        .map(|url| url.to_string())
        .map_err(|_| {
            format!(
                "path cannot be represented as a file uri: {}",
                path.display()
            )
        })
}

fn scene_capture_image() -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: 1024,
            height: 768,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;
    image
}

fn apply_saved_voxel_edits(
    mut applied: Local<bool>,
    store: Option<Res<Persistent<VoxelSceneStore>>>,
    mut voxel_world: VoxelWorld<TrpgVoxelWorld>,
) {
    if *applied {
        return;
    }
    let Some(store) = store else {
        return;
    };

    for edit in &store.edits {
        let position = IVec3::new(
            edit.position[0],
            edit.position[1],
            edit.position[2],
        );
        voxel_world.set_voxel(position, edit.voxel.into());
    }
    *applied = true;
}

fn brush_positions(center: IVec3, radius: i32) -> impl Iterator<Item = IVec3> {
    let radius = radius.max(0);
    (-radius..=radius).flat_map(move |x| {
        (-radius..=radius)
            .flat_map(move |y| (-radius..=radius).map(move |z| center + IVec3::new(x, y, z)))
    })
}

fn upsert_persisted_edit(
    store: &mut Persistent<VoxelSceneStore>,
    position: IVec3,
    voxel: PersistedVoxel,
) {
    let position = [position.x, position.y, position.z];
    if let Some(edit) = store
        .edits
        .iter_mut()
        .find(|edit| edit.position == position)
    {
        edit.voxel = voxel;
    } else {
        store.edits.push(PersistedVoxelEdit { position, voxel });
    }
}

fn upsert_persisted_capture_camera(
    store: &mut Persistent<VoxelSceneStore>,
    user_id: u64,
    transform: &Transform,
) {
    let persisted = PersistedCaptureCamera {
        user_id,
        translation: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
    };

    if let Some(camera) = store
        .capture_cameras
        .iter_mut()
        .find(|camera| camera.user_id == user_id)
    {
        *camera = persisted;
    } else {
        store.capture_cameras.push(persisted);
    }
}

impl From<PersistedVoxel> for WorldVoxel<u8> {
    fn from(value: PersistedVoxel) -> Self {
        match value {
            PersistedVoxel::Air => WorldVoxel::Air,
            PersistedVoxel::Solid(material) => WorldVoxel::Solid(material),
        }
    }
}
