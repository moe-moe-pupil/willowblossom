use std::{
    collections::{
        hash_map::DefaultHasher,
        HashMap,
        HashSet,
    },
    fs,
    hash::{
        Hash,
        Hasher,
    },
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use avian3d::prelude::{
    AngularVelocity,
    Collider,
    Gravity,
    GravityScale,
    LinearDamping,
    LinearVelocity,
    PhysicsPlugins,
    PhysicsSchedule,
    PhysicsStepSystems,
    RigidBody,
};
use bevy::{
    asset::RenderAssetUsages,
    camera::{
        visibility::RenderLayers,
        RenderTarget,
    },
    input::mouse::MouseMotion,
    mesh::{
        Indices,
        PrimitiveTopology,
    },
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
    transform::TransformSystems,
    window::PrimaryWindow,
};
use bevy_egui::{
    egui,
    input::EguiWantsInput,
    EguiContexts,
    EguiPostUpdateSet,
    EguiPrimaryContextPass,
    PrimaryEguiContext,
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
        NapcatMessage,
        NapcatMessageManager,
        NapcatOutboundMessage,
    },
};

pub struct ScenePreviewPlugin;

const SCENE_GIZMO_RENDER_LAYER: usize = 1;
const SPACE_HIFI_MAP_ID: &str = "space-hifi-wide-ship10";
const SPACE_HIFI_MAP_NAME: &str = "太空HiFi宽体飞船10x";
const VOXEL_TEXTURE_LAYERS: u32 = 12;
const MAT_VOID: u8 = 0;
const MAT_STAR: u8 = 1;
const MAT_HULL_LIGHT: u8 = 2;
const MAT_HULL_DARK: u8 = 3;
const MAT_WINDOW_CYAN: u8 = 4;
const MAT_ENGINE_RED: u8 = 5;
const MAT_STATION_METAL: u8 = 6;
const MAT_STATION_TRIM: u8 = 7;
const MAT_SUN: u8 = 8;
const MAT_SOLAR_PANEL: u8 = 9;
const MAT_PLANET_OCEAN: u8 = 10;
const MAT_PLANET_LAND: u8 = 11;
const MAX_AUTO_MAP_STATUS_SNAPSHOTS_PER_MAP: usize = 40;
const VOXEL_MAP_APPLY_BUDGET_PER_FRAME: usize = 600;
const BATTLE_SPACESHIP_SCALE: i32 = 10;
const SPACE_HIFI_MAP_SCALE: i32 = 100;
const EARTH_PLANET_SCALE: i32 = 100;
const LEGACY_DEFAULT_CAMERA_SPEED: f32 = 12.0;
const DEFAULT_CAMERA_SPEED: f32 = 64.0;
const MAX_CAMERA_SPEED: f32 = 1800.0;
const SCENE_CAPTURE_PREPARE_FRAMES: u8 = 12;
const SPACE_HIFI_STATION_A_CENTER: IVec3 = IVec3::new(-54, 13, 24);
const SPACE_HIFI_STATION_B_CENTER: IVec3 = IVec3::new(58, 14, -28);
const SPACE_HIFI_SUN_CENTER: IVec3 = IVec3::new(-88, 38, -76);
const SPACE_HIFI_SUN_RADIUS: i32 = 8;
const EARTH_PLANET_NEAR_POINT: IVec3 = IVec3::new(72, 28, 70);
const EARTH_PLANET_RADIUS: i32 = 96 * EARTH_PLANET_SCALE;
const VOXEL_PLANET_MAX_ELEVATION: f32 = 220.0;
const VOXEL_PLANET_PREVIEW_STEP: i32 = 512;
const VOXEL_PLANET_PREVIEW_HIDE_ALTITUDE: f32 = 1400.0;
const PLANET_GRAVITY_ACCELERATION: f32 = 28.0;
const PLANET_PHYSICS_PROBE_RADIUS: f32 = 1.2;
const HELD_PHYSICS_VOXEL_DISTANCE: f32 = 6.0;
const PHYSICS_VOXEL_DROP_SPEED: f32 = 4.0;
const ORPHANED_VOXEL_COLLIDER_FRAME_GRACE: u8 = 3;

fn planet_outward_at(position: Vec3) -> Vec3 {
    (position - earth_planet_center().as_vec3())
        .try_normalize()
        .unwrap_or(Vec3::Y)
}

fn planet_gravity_direction_at(position: Vec3) -> Vec3 {
    (earth_planet_center().as_vec3() - position)
        .try_normalize()
        .unwrap_or(Vec3::ZERO)
}

fn planet_gravity_delta_velocity(position: Vec3, delta_seconds: f32) -> Vec3 {
    if delta_seconds <= 0.0 {
        return Vec3::ZERO;
    }
    planet_gravity_direction_at(position) * PLANET_GRAVITY_ACCELERATION * delta_seconds
}

fn scene_camera_fog() -> DistanceFog {
    DistanceFog {
        color: Color::srgb(0.006, 0.01, 0.016),
        falloff: FogFalloff::ExponentialSquared { density: 0.00018 },
        directional_light_color: Color::srgb(0.12, 0.18, 0.28),
        directional_light_exponent: 18.0,
    }
}

#[derive(Resource, Clone, Default)]
pub struct TrpgVoxelWorld;

#[derive(Bundle, Clone)]
pub struct VoxelChunkPhysicsBundle {
    rigid_body: RigidBody,
    collider: Collider,
    terrain_collider: VoxelChunkTerrainCollider,
}

#[derive(Component, Clone)]
pub struct VoxelChunkTerrainCollider {
    frames_without_mesh: u8,
}

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
    camera_speed_dirty: bool,
    mouse_sensitivity: f32,
    new_map_name: String,
    rename_map_name: String,
    selected_map_id: Option<String>,
    selected_status_snapshot_id: Option<String>,
}

#[derive(Resource, Default)]
struct ScenePointerState {
    left_started_over_ui: bool,
    last_edit_cursor_position: Option<Vec2>,
    last_edit_position: Option<IVec3>,
    stationary_edit_seconds: f32,
    shift_locked_edit_y: Option<i32>,
    right_started_over_ui: bool,
}

impl Default for VoxelEditorState {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: VoxelEditMode::Add,
            material: 2,
            brush_radius: 0,
            camera_speed: DEFAULT_CAMERA_SPEED,
            camera_speed_dirty: false,
            mouse_sensitivity: 0.003,
            new_map_name: "新地图".to_owned(),
            rename_map_name: String::new(),
            selected_map_id: None,
            selected_status_snapshot_id: None,
        }
    }
}

#[derive(Resource, Default)]
struct VoxelMapRuntimeState {
    applied_map_id: Option<String>,
    applied_edits: Vec<PersistedVoxelEdit>,
    reload_requested: bool,
    pending_map_id: Option<String>,
    pending_clear_edits: Vec<PersistedVoxelEdit>,
    pending_apply_edits: Vec<PersistedVoxelEdit>,
    clear_cursor: usize,
    apply_cursor: usize,
}

#[derive(Resource, Default)]
struct PhysicsVoxelGrabState {
    held_entity: Option<Entity>,
}

#[derive(Resource)]
struct SceneWaypointState {
    selected_index: usize,
    custom_waypoints: Vec<SceneWaypoint>,
    new_waypoint_name: String,
}

impl Default for SceneWaypointState {
    fn default() -> Self {
        Self {
            selected_index: 0,
            custom_waypoints: Vec::new(),
            new_waypoint_name: "路径点".to_owned(),
        }
    }
}

#[derive(Clone)]
struct SceneWaypoint {
    name: String,
    eye: Vec3,
    focus: Vec3,
    builtin: bool,
}

#[derive(Clone, Copy)]
struct VoxelEditTarget {
    position: IVec3,
    normal: IVec3,
    material: Option<u8>,
}

#[derive(Component)]
struct SpaceHiFiVoxelPreview;

#[derive(Component)]
struct VoxelPlanetPreview;

#[derive(Component)]
struct PlanetGravityBody;

#[derive(Component)]
struct PlanetPhysicsProbe;

#[derive(Component)]
struct PhysicsVoxel;

#[derive(Component)]
struct HeldPhysicsVoxel;

#[derive(Component)]
struct FreeCamera;

#[derive(Resource, Default)]
pub struct SceneCaptureRequests {
    pub requests: Vec<SceneCaptureRequest>,
}

#[derive(Resource, Default)]
pub struct SceneCharacterPositions {
    pub positions: HashMap<String, Vec3>,
}

#[derive(Resource, Default)]
pub struct ScenePlayerCameraPositions {
    pub positions: HashMap<u64, Vec3>,
}

#[derive(Resource, Default)]
pub struct ScenePlayerViewRequest {
    pub user_id: Option<u64>,
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
    prepare_frames_remaining: u8,
    started_preparing: bool,
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

#[derive(Resource, Default)]
struct CharacterStandeeAssets {
    entities: HashMap<String, Entity>,
    textures: HashMap<String, Handle<Image>>,
    failed_sources: HashSet<String>,
}

#[derive(Component)]
struct CharacterStandee {
    target_id: String,
    image_source: String,
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

#[derive(Resource, Serialize, Deserialize)]
struct VoxelSceneStore {
    #[serde(default = "default_camera_speed")]
    editor_camera_speed: f32,
    #[serde(default)]
    active_map_id: Option<String>,
    #[serde(default)]
    maps: Vec<PersistedVoxelMap>,
    #[serde(default)]
    map_status_snapshots: Vec<PersistedVoxelMapStatusSnapshot>,
    #[serde(default)]
    edits: Vec<PersistedVoxelEdit>,
    #[serde(default)]
    capture_cameras: Vec<PersistedCaptureCamera>,
    #[serde(default)]
    character_standees: Vec<PersistedCharacterStandee>,
}

impl Default for VoxelSceneStore {
    fn default() -> Self {
        Self {
            editor_camera_speed: DEFAULT_CAMERA_SPEED,
            active_map_id: None,
            maps: Vec::new(),
            map_status_snapshots: Vec::new(),
            edits: Vec::new(),
            capture_cameras: Vec::new(),
            character_standees: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct PersistedVoxelMap {
    id: String,
    name: String,
    #[serde(default)]
    edits: Vec<PersistedVoxelEdit>,
}

#[derive(Serialize, Deserialize, Clone)]
struct PersistedVoxelMapStatusSnapshot {
    id: String,
    map_id: String,
    name: String,
    reason: String,
    created_at: u64,
    #[serde(default)]
    edits: Vec<PersistedVoxelEdit>,
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

#[derive(Serialize, Deserialize, Clone)]
struct PersistedCharacterStandee {
    target_id: String,
    image_source: String,
    translation: [f32; 3],
    rotation: [f32; 4],
}

impl VoxelWorldConfig for TrpgVoxelWorld {
    type ChunkUserBundle = VoxelChunkPhysicsBundle;
    type MaterialIndex = u8;

    fn spawning_distance(&self) -> u32 { 6 }

    fn min_despawn_distance(&self) -> u32 { 3 }

    fn chunk_despawn_strategy(&self) -> ChunkDespawnStrategy { ChunkDespawnStrategy::FarAway }

    fn chunk_spawn_strategy(&self) -> ChunkSpawnStrategy { ChunkSpawnStrategy::Close }

    fn max_spawn_per_frame(&self) -> usize { 12 }

    fn spawning_rays(&self) -> usize { 16 }

    fn texture_index_mapper(&self) -> TextureIndexMapperFn<Self::MaterialIndex> {
        Arc::new(|material| match material {
            MAT_STAR => [MAT_STAR as u32; 3],
            MAT_HULL_LIGHT => [MAT_HULL_LIGHT as u32; 3],
            MAT_HULL_DARK => [MAT_HULL_DARK as u32; 3],
            MAT_WINDOW_CYAN => [MAT_WINDOW_CYAN as u32; 3],
            MAT_ENGINE_RED => [MAT_ENGINE_RED as u32; 3],
            MAT_STATION_METAL => [MAT_STATION_METAL as u32; 3],
            MAT_STATION_TRIM => [MAT_STATION_TRIM as u32; 3],
            MAT_SUN => [MAT_SUN as u32; 3],
            MAT_SOLAR_PANEL => [MAT_SOLAR_PANEL as u32; 3],
            MAT_PLANET_OCEAN => [MAT_PLANET_OCEAN as u32; 3],
            MAT_PLANET_LAND => [MAT_PLANET_LAND as u32; 3],
            _ => [0, 0, 0],
        })
    }

    fn voxel_lookup_delegate(&self) -> VoxelLookupDelegate<Self::MaterialIndex> {
        Box::new(|_, _, _| Box::new(starter_scene_voxel))
    }

    fn chunk_meshing_delegate(
        &self,
    ) -> ChunkMeshingDelegate<Self::MaterialIndex, Self::ChunkUserBundle> {
        Some(Box::new(
            |position, lod, data_shape, mesh_shape, previous_data| {
                let mut default_mesher =
                    default_chunk_meshing_delegate::<u8, VoxelChunkPhysicsBundle>(
                        position,
                        lod,
                        data_shape,
                        mesh_shape,
                        previous_data,
                    );
                Box::new(
                    move |voxels, data_shape_in, mesh_shape_in, texture_index_mapper| {
                        let (mesh, _) = default_mesher(
                            voxels,
                            data_shape_in,
                            mesh_shape_in,
                            texture_index_mapper,
                        );
                        let physics_bundle = Collider::trimesh_from_mesh(&mesh).map(|collider| {
                            VoxelChunkPhysicsBundle {
                                rigid_body: RigidBody::Static,
                                collider,
                                terrain_collider: VoxelChunkTerrainCollider {
                                    frames_without_mesh: 0,
                                },
                            }
                        });
                        (mesh, physics_bundle)
                    },
                )
            },
        ))
    }

    fn voxel_texture(&self) -> Option<(String, u32)> {
        Some((
            "textures/voxel_space_hifi.png".to_owned(),
            VOXEL_TEXTURE_LAYERS,
        ))
    }
}

impl Plugin for ScenePreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsPlugins::default())
            .insert_resource(Gravity::ZERO)
            .add_plugins(VoxelWorldPlugin::with_config(
                TrpgVoxelWorld,
            ))
            .init_resource::<VoxelEditorState>()
            .init_resource::<SceneCaptureRequests>()
            .init_resource::<SceneCharacterPositions>()
            .init_resource::<ScenePlayerCameraPositions>()
            .init_resource::<ScenePlayerViewRequest>()
            .init_resource::<SceneCaptureState>()
            .init_resource::<PlayerSceneCameras>()
            .init_resource::<SceneCaptureEditorState>()
            .init_resource::<ScenePointerState>()
            .init_resource::<CharacterStandeeAssets>()
            .init_resource::<VoxelMapRuntimeState>()
            .init_resource::<PhysicsVoxelGrabState>()
            .init_resource::<SceneWaypointState>()
            .add_systems(Startup, setup_scene_preview)
            .add_systems(
                Update,
                (
                    scene_capture_request_system,
                    draw_capture_camera_gizmos,
                    sync_character_standees,
                ),
            )
            .add_systems(Update, apply_saved_voxel_edits)
            .add_systems(
                Update,
                auto_save_map_status_for_battle_turn,
            )
            .add_systems(
                PhysicsSchedule,
                apply_planet_radial_gravity.before(PhysicsStepSystems::First),
            )
            .add_systems(
                PostUpdate,
                (
                    apply_scene_player_view_request,
                    free_camera_system,
                    physics_voxel_grab_drop_system,
                    edit_voxel_world_system,
                    sync_voxel_planet_preview_visibility,
                    cleanup_orphaned_voxel_chunk_colliders,
                )
                    .chain()
                    .after(EguiPostUpdateSet::ProcessOutput),
            )
            .add_systems(
                PostUpdate,
                (
                    sync_scene_character_positions,
                    sync_scene_player_camera_positions,
                )
                    .after(TransformSystems::Propagate),
            )
            .add_systems(
                EguiPrimaryContextPass,
                (
                    voxel_editor_panel,
                    voxel_minimap_panel,
                    scene_waypoint_panel,
                    capture_camera_panel,
                ),
            );
    }
}

fn starter_scene_voxel(position: IVec3, _previous: Option<WorldVoxel<u8>>) -> WorldVoxel<u8> {
    space_hifi_procedural_voxel(position)
}

fn setup_scene_preview(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmo_config: ResMut<GizmoConfigStore>,
    mut editor: ResMut<VoxelEditorState>,
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
    let mut voxel_scene_store = voxel_scene_store;
    ensure_voxel_maps(&mut voxel_scene_store);
    let normalized_editor_settings = normalize_persisted_editor_settings(&mut voxel_scene_store);
    editor.camera_speed = voxel_scene_store.editor_camera_speed;
    if normalized_editor_settings {
        if let Err(err) = voxel_scene_store.persist() {
            eprintln!("failed to persist voxel scene normalization: {err}");
        }
    }

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
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.32, 0.42, 0.55),
        brightness: 18.0,
        ..default()
    });

    let sun_position = scaled_space_hifi_point(SPACE_HIFI_SUN_CENTER).as_vec3();
    let planet_center = earth_planet_center().as_vec3();
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.88, 0.68),
            illuminance: 68_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_translation(sun_position).looking_at(planet_center, Vec3::Y),
    ));
    spawn_space_hifi_lights(&mut commands);
    spawn_space_hifi_voxel_preview(
        &mut commands,
        &mut meshes,
        &mut materials,
    );
    spawn_voxel_planet_preview(
        &mut commands,
        &mut meshes,
        &mut materials,
    );
    spawn_planet_physics_probe(
        &mut commands,
        &mut meshes,
        &mut materials,
    );

    let starting_waypoint = planet_surface_waypoint();
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.06, 0.07, 0.08)),
            ..default()
        },
        scene_camera_fog(),
        waypoint_transform(&starting_waypoint),
        VoxelWorldCamera::<TrpgVoxelWorld>::default(),
        RenderLayers::from_layers(&[0, SCENE_GIZMO_RENDER_LAYER]),
        PrimaryEguiContext,
        GameCamera,
        FreeCamera,
    ));
}

fn spawn_space_hifi_lights(commands: &mut Commands) {
    for z in (-34..=36).step_by(14) {
        spawn_scene_point_light(
            commands,
            Vec3::new(0.0, 9.0, z as f32),
            Color::srgb(0.45, 0.95, 1.0),
            75_000.0,
            34.0,
        );
    }
    for z in [-42.0, -34.0] {
        spawn_scene_point_light(
            commands,
            Vec3::new(0.0, 7.0, z),
            Color::srgb(1.0, 0.25, 0.12),
            90_000.0,
            30.0,
        );
    }
    for z in (-28..=28).step_by(20) {
        spawn_scene_point_light(
            commands,
            Vec3::new(-11.0, 8.0, z as f32),
            Color::srgb(0.8, 0.9, 1.0),
            42_000.0,
            24.0,
        );
        spawn_scene_point_light(
            commands,
            Vec3::new(11.0, 8.0, z as f32),
            Color::srgb(0.8, 0.9, 1.0),
            42_000.0,
            24.0,
        );
    }

    for center in [
        scaled_space_hifi_point(SPACE_HIFI_STATION_A_CENTER).as_vec3(),
        scaled_space_hifi_point(SPACE_HIFI_STATION_B_CENTER).as_vec3(),
    ] {
        spawn_scene_point_light(
            commands,
            center,
            Color::srgb(0.35, 0.85, 1.0),
            120_000.0,
            72.0,
        );
        for offset in [
            Vec3::new(0.0, 0.0, 20.0),
            Vec3::new(0.0, 0.0, -20.0),
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(-20.0, 0.0, 0.0),
        ] {
            spawn_scene_point_light(
                commands,
                center + offset,
                Color::srgb(0.75, 0.88, 1.0),
                48_000.0,
                36.0,
            );
        }
    }

    spawn_scene_point_light(
        commands,
        scaled_space_hifi_point(SPACE_HIFI_SUN_CENTER).as_vec3(),
        Color::srgb(1.0, 0.74, 0.28),
        3_800_000.0,
        9_000.0,
    );
}

fn spawn_scene_point_light(
    commands: &mut Commands,
    position: Vec3,
    color: Color,
    intensity: f32,
    range: f32,
) {
    commands.spawn((
        PointLight {
            color,
            intensity,
            range,
            radius: 1.8,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_translation(position),
    ));
}

fn spawn_space_hifi_voxel_preview(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let voxels = space_hifi_voxel_edits()
        .into_iter()
        .filter_map(|edit| {
            let PersistedVoxel::Solid(material) = edit.voxel else {
                return None;
            };
            Some((
                IVec3::new(
                    edit.position[0],
                    edit.position[1],
                    edit.position[2],
                ),
                material,
            ))
        })
        .collect::<HashMap<_, _>>();

    for material in MAT_STAR..=MAT_PLANET_LAND {
        let mesh = build_voxel_preview_mesh(&voxels, material);
        if mesh.count_vertices() == 0 {
            continue;
        }
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: preview_material_color(material),
                emissive: preview_material_emissive(material).into(),
                perceptual_roughness: 0.82,
                metallic: match material {
                    MAT_HULL_LIGHT | MAT_HULL_DARK | MAT_STATION_METAL | MAT_STATION_TRIM => 0.35,
                    _ => 0.0,
                },
                unlit: matches!(
                    material,
                    MAT_STAR | MAT_WINDOW_CYAN | MAT_ENGINE_RED | MAT_SUN
                ),
                ..default()
            })),
            Transform::default(),
            SpaceHiFiVoxelPreview,
        ));
    }
}

fn spawn_voxel_planet_preview(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let voxels = voxel_planet_preview_blocks();
    for material in [MAT_PLANET_OCEAN, MAT_PLANET_LAND] {
        let mesh = build_voxel_planet_preview_mesh(&voxels, material);
        if mesh.count_vertices() == 0 {
            continue;
        }
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: preview_material_color(material),
                perceptual_roughness: 0.9,
                metallic: 0.0,
                ..default()
            })),
            Transform::default(),
            Visibility::Visible,
            VoxelPlanetPreview,
        ));
    }
}

fn spawn_planet_physics_probe(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(PLANET_PHYSICS_PROBE_RADIUS).mesh().uv(24, 12))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.86, 0.24),
            emissive: Color::srgb(0.04, 0.025, 0.0).into(),
            perceptual_roughness: 0.72,
            metallic: 0.0,
            ..default()
        })),
        planet_physics_probe_transform(),
        RigidBody::Dynamic,
        Collider::sphere(PLANET_PHYSICS_PROBE_RADIUS),
        LinearVelocity::ZERO,
        AngularVelocity::ZERO,
        GravityScale(0.0),
        LinearDamping(0.35),
        PlanetGravityBody,
        PlanetPhysicsProbe,
    ));
}

fn planet_physics_probe_transform() -> Transform {
    let center = earth_planet_center().as_vec3();
    let outward = (earth_planet_near_point().as_vec3() - center)
        .try_normalize()
        .unwrap_or(Vec3::Y);
    Transform::from_translation(
        center
            + outward
                * (EARTH_PLANET_RADIUS as f32
                    + VOXEL_PLANET_MAX_ELEVATION
                    + PLANET_PHYSICS_PROBE_RADIUS
                    + 24.0),
    )
}

fn build_voxel_preview_mesh(voxels: &HashMap<IVec3, u8>, material: u8) -> Mesh {
    let mut positions = Vec::<[f32; 3]>::new();
    let mut normals = Vec::<[f32; 3]>::new();
    let mut uvs = Vec::<[f32; 2]>::new();
    let mut indices = Vec::<u32>::new();
    let ship_cuboids = voxels
        .iter()
        .filter_map(|(&position, &material)| {
            battle_spaceship_preview_origin(position).map(|origin| (origin, material))
        })
        .collect::<HashMap<_, _>>();

    for (&position, &voxel_material) in voxels {
        if voxel_material != material {
            continue;
        }
        if let Some(origin) = battle_spaceship_preview_origin(position) {
            append_visible_cuboid_faces(
                origin,
                IVec3::splat(BATTLE_SPACESHIP_SCALE),
                &ship_cuboids,
                &mut positions,
                &mut normals,
                &mut uvs,
                &mut indices,
            );
        } else {
            append_visible_voxel_faces(
                position,
                voxels,
                &mut positions,
                &mut normals,
                &mut uvs,
                &mut indices,
            );
        }
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

fn build_voxel_planet_preview_mesh(voxels: &HashMap<IVec3, u8>, material: u8) -> Mesh {
    let mut positions = Vec::<[f32; 3]>::new();
    let mut normals = Vec::<[f32; 3]>::new();
    let mut uvs = Vec::<[f32; 2]>::new();
    let mut indices = Vec::<u32>::new();

    for (&origin, &voxel_material) in voxels {
        if voxel_material != material {
            continue;
        }
        append_visible_cuboid_faces(
            origin,
            IVec3::splat(VOXEL_PLANET_PREVIEW_STEP),
            voxels,
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
        );
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

fn append_visible_cuboid_faces(
    origin: IVec3,
    size: IVec3,
    cuboids: &HashMap<IVec3, u8>,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
) {
    const FACES: [(IVec3, [[f32; 3]; 4]); 6] = [
        (IVec3::X, [
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
        ]),
        (IVec3::NEG_X, [
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ]),
        (IVec3::Y, [
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ]),
        (IVec3::NEG_Y, [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ]),
        (IVec3::Z, [
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 0.0, 1.0],
        ]),
        (IVec3::NEG_Z, [
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
        ]),
    ];

    let base = origin.as_vec3();
    let size_vec = size.as_vec3();
    for (normal, corners) in FACES {
        let neighbor_origin = origin
            + IVec3::new(
                normal.x * size.x,
                normal.y * size.y,
                normal.z * size.z,
            );
        if cuboids.contains_key(&neighbor_origin) {
            continue;
        }
        let start = positions.len() as u32;
        for corner in corners {
            let corner = Vec3::from(corner) * size_vec;
            positions.push((base + corner).to_array());
            normals.push(normal.as_vec3().to_array());
        }
        uvs.extend_from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
    }
}

fn append_visible_voxel_faces(
    position: IVec3,
    voxels: &HashMap<IVec3, u8>,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
) {
    const FACES: [(IVec3, [[f32; 3]; 4]); 6] = [
        (IVec3::X, [
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
        ]),
        (IVec3::NEG_X, [
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ]),
        (IVec3::Y, [
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ]),
        (IVec3::NEG_Y, [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ]),
        (IVec3::Z, [
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 0.0, 1.0],
        ]),
        (IVec3::NEG_Z, [
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
        ]),
    ];

    let base = position.as_vec3();
    for (normal, corners) in FACES {
        if voxels.contains_key(&(position + normal)) {
            continue;
        }
        let start = positions.len() as u32;
        for corner in corners {
            let corner = Vec3::from(corner);
            positions.push((base + corner).to_array());
            normals.push(normal.as_vec3().to_array());
        }
        uvs.extend_from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
    }
}

fn preview_material_color(material: u8) -> Color {
    match material {
        MAT_STAR => Color::srgb(0.95, 0.98, 1.0),
        MAT_HULL_LIGHT => Color::srgb(0.72, 0.78, 0.82),
        MAT_HULL_DARK => Color::srgb(0.25, 0.29, 0.34),
        MAT_WINDOW_CYAN => Color::srgb(0.15, 0.95, 1.0),
        MAT_ENGINE_RED => Color::srgb(1.0, 0.18, 0.08),
        MAT_STATION_METAL => Color::srgb(0.46, 0.48, 0.50),
        MAT_STATION_TRIM => Color::srgb(0.68, 0.72, 0.78),
        MAT_SUN => Color::srgb(1.0, 0.66, 0.16),
        MAT_SOLAR_PANEL => Color::srgb(0.08, 0.24, 0.78),
        MAT_PLANET_OCEAN => Color::srgb(0.05, 0.36, 0.95),
        MAT_PLANET_LAND => Color::srgb(0.10, 0.58, 0.22),
        _ => Color::WHITE,
    }
}

fn preview_material_emissive(material: u8) -> Color {
    match material {
        MAT_STAR => Color::srgb(1.0, 1.0, 1.0),
        MAT_WINDOW_CYAN => Color::srgb(0.0, 0.65, 0.85),
        MAT_ENGINE_RED => Color::srgb(1.0, 0.08, 0.02),
        MAT_SUN => Color::srgb(1.0, 0.44, 0.05),
        _ => Color::BLACK,
    }
}

fn voxel_editor_panel(
    mut contexts: EguiContexts,
    mut editor: ResMut<VoxelEditorState>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    mut map_runtime: ResMut<VoxelMapRuntimeState>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Window::new("体素工具")
        .default_pos(egui::pos2(12.0, 36.0))
        .default_width(220.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.checkbox(&mut editor.enabled, "编辑");
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut editor.mode,
                    VoxelEditMode::Add,
                    "添加",
                );
                ui.selectable_value(
                    &mut editor.mode,
                    VoxelEditMode::Erase,
                    "擦除",
                );
            });
            ui.add(
                egui::Slider::new(
                    &mut editor.material,
                    MAT_VOID..=MAT_PLANET_LAND,
                )
                .text("材质"),
            );
            ui.add(egui::Slider::new(&mut editor.brush_radius, 0..=3).text("笔刷"));
            ui.separator();
            let camera_speed_response = ui.add(
                egui::Slider::new(
                    &mut editor.camera_speed,
                    2.0..=MAX_CAMERA_SPEED,
                )
                .text("相机"),
            );
            if let Some(store) = store.as_deref_mut() {
                ensure_voxel_maps(store);
                if camera_speed_response.changed() {
                    editor.camera_speed = normalized_camera_speed(editor.camera_speed);
                    if store.editor_camera_speed != editor.camera_speed {
                        store.editor_camera_speed = editor.camera_speed;
                        editor.camera_speed_dirty = true;
                    }
                }
                let should_persist_camera_speed = camera_speed_response.drag_stopped()
                    || (camera_speed_response.changed() && !camera_speed_response.dragged());
                if editor.camera_speed_dirty && should_persist_camera_speed {
                    persist_voxel_store(store, "camera speed");
                    editor.camera_speed_dirty = false;
                }
                voxel_map_manager_ui(ui, &mut editor, store, &mut map_runtime);
                ui.separator();
                ui.label(format!(
                    "已保存编辑：{}",
                    active_voxel_map(store).map_or(0, |map| map.edits.len())
                ));
            }
        });
}

fn voxel_map_manager_ui(
    ui: &mut egui::Ui,
    editor: &mut VoxelEditorState,
    store: &mut Persistent<VoxelSceneStore>,
    runtime: &mut VoxelMapRuntimeState,
) {
    let active_map_id = store.active_map_id.clone();
    if editor.selected_map_id != active_map_id {
        editor.selected_map_id = active_map_id.clone();
        editor.rename_map_name = active_voxel_map(store)
            .map(|map| map.name.clone())
            .unwrap_or_default();
        editor.selected_status_snapshot_id =
            latest_status_snapshot_for_active_map(store).map(|snapshot| snapshot.id.clone());
    }

    ui.label("体素地图");
    let selected_text = active_voxel_map(store)
        .map(|map| map.name.as_str())
        .unwrap_or("无地图");
    let mut selected_map_id = active_map_id.unwrap_or_default();
    egui::ComboBox::from_label("当前")
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
            for map in &store.maps {
                ui.selectable_value(
                    &mut selected_map_id,
                    map.id.clone(),
                    map.name.as_str(),
                );
            }
        });
    if store
        .active_map_id
        .as_deref()
        .is_none_or(|active| active != selected_map_id)
        && store.maps.iter().any(|map| map.id == selected_map_id)
    {
        store.active_map_id = Some(selected_map_id.clone());
        editor.selected_map_id = Some(selected_map_id);
        editor.rename_map_name = active_voxel_map(store)
            .map(|map| map.name.clone())
            .unwrap_or_default();
        runtime.reload_requested = true;
        persist_voxel_store(store, "map selection");
    }

    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut editor.new_map_name);
        if ui.button("创建").clicked() {
            let name = clean_voxel_map_name(&editor.new_map_name);
            let id = new_voxel_map_id(&store.maps);
            let name = unique_voxel_map_name(&store.maps, &name, None);
            store.maps.push(PersistedVoxelMap {
                id: id.clone(),
                name,
                edits: Vec::new(),
            });
            store.active_map_id = Some(id.clone());
            editor.selected_map_id = Some(id);
            editor.rename_map_name = active_voxel_map(store)
                .map(|map| map.name.clone())
                .unwrap_or_default();
            runtime.reload_requested = true;
            persist_voxel_store(store, "map creation");
        }
    });

    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut editor.rename_map_name);
        if ui.button("重命名").clicked() {
            let active_id = store.active_map_id.clone();
            let name = clean_voxel_map_name(&editor.rename_map_name);
            let unique_name = unique_voxel_map_name(&store.maps, &name, active_id.as_deref());
            if let Some(map) = active_voxel_map_mut(store) {
                map.name = unique_name.clone();
                editor.rename_map_name = unique_name;
                persist_voxel_store(store, "map rename");
            }
        }
    });

    ui.horizontal(|ui| {
        if ui.button("复制").clicked() {
            if let Some(map) = active_voxel_map(store).cloned() {
                let id = new_voxel_map_id(&store.maps);
                let name = unique_voxel_map_name(
                    &store.maps,
                    &format!("{} 副本", map.name.trim()),
                    None,
                );
                store.maps.push(PersistedVoxelMap {
                    id: id.clone(),
                    name,
                    edits: map.edits,
                });
                store.active_map_id = Some(id.clone());
                editor.selected_map_id = Some(id);
                editor.rename_map_name = active_voxel_map(store)
                    .map(|map| map.name.clone())
                    .unwrap_or_default();
                runtime.reload_requested = true;
                persist_voxel_store(store, "map duplication");
            }
        }
        let can_delete = store.maps.len() > 1;
        if ui
            .add_enabled(can_delete, egui::Button::new("删除"))
            .clicked()
        {
            if let Some(active_id) = store.active_map_id.clone() {
                store.maps.retain(|map| map.id != active_id);
                store
                    .map_status_snapshots
                    .retain(|snapshot| snapshot.map_id != active_id);
                store.active_map_id = store.maps.first().map(|map| map.id.clone());
                editor.selected_map_id = store.active_map_id.clone();
                editor.rename_map_name = active_voxel_map(store)
                    .map(|map| map.name.clone())
                    .unwrap_or_default();
                editor.selected_status_snapshot_id = latest_status_snapshot_for_active_map(store)
                    .map(|snapshot| snapshot.id.clone());
                runtime.reload_requested = true;
                persist_voxel_store(store, "map deletion");
            }
        }
        if ui.button("清空").clicked() {
            if let Some(map) = active_voxel_map_mut(store) {
                map.edits.clear();
                runtime.reload_requested = true;
                persist_voxel_store(store, "map clear");
            }
        }
    });

    ui.separator();
    ui.label("地图状态");
    ui.horizontal(|ui| {
        if ui.button("保存当前状态").clicked() {
            let snapshot_id = save_active_map_status(store, "手动", false);
            editor.selected_status_snapshot_id = snapshot_id;
            persist_voxel_store(store, "map status snapshot");
        }

        let can_revert = selected_status_snapshot(store, editor).is_some_and(|snapshot| {
            snapshot.map_id == active_voxel_map_id(store).unwrap_or_default()
        });
        if ui
            .add_enabled(
                can_revert,
                egui::Button::new("恢复到状态"),
            )
            .clicked()
        {
            if let Some(snapshot) = selected_status_snapshot(store, editor) {
                if let Some(map) = active_voxel_map_mut(store) {
                    map.edits = snapshot.edits;
                    runtime.reload_requested = true;
                    persist_voxel_store(store, "map status revert");
                }
            }
        }
    });

    let snapshots = status_snapshots_for_active_map(store);
    if snapshots.is_empty() {
        ui.small("还没有保存状态。");
    } else {
        if editor
            .selected_status_snapshot_id
            .as_ref()
            .is_none_or(|selected_id| !snapshots.iter().any(|snapshot| snapshot.id == *selected_id))
        {
            editor.selected_status_snapshot_id =
                snapshots.first().map(|snapshot| snapshot.id.clone());
        }

        let mut selected_snapshot_id = editor
            .selected_status_snapshot_id
            .clone()
            .unwrap_or_else(|| snapshots[0].id.clone());
        let selected_text = snapshots
            .iter()
            .find(|snapshot| snapshot.id == selected_snapshot_id)
            .map(status_snapshot_label)
            .unwrap_or_else(|| "选择状态".to_owned());
        egui::ComboBox::from_label("已保存")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                for snapshot in &snapshots {
                    ui.selectable_value(
                        &mut selected_snapshot_id,
                        snapshot.id.clone(),
                        status_snapshot_label(snapshot),
                    );
                }
            });
        editor.selected_status_snapshot_id = Some(selected_snapshot_id.clone());

        ui.horizontal(|ui| {
            ui.small(format!("已保存{}个", snapshots.len()));
            if ui.button("删除状态").clicked() {
                store
                    .map_status_snapshots
                    .retain(|snapshot| snapshot.id != selected_snapshot_id);
                editor.selected_status_snapshot_id = latest_status_snapshot_for_active_map(store)
                    .map(|snapshot| snapshot.id.clone());
                persist_voxel_store(store, "map status deletion");
            }
        });
    }
}

fn voxel_minimap_panel(
    mut contexts: EguiContexts,
    store: Option<Res<Persistent<VoxelSceneStore>>>,
    mut free_camera: Query<&mut Transform, With<FreeCamera>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Some(store) = store else {
        return;
    };
    let Some(map) = active_voxel_map(&store) else {
        return;
    };

    egui::Window::new("体素小地图")
        .default_pos(egui::pos2(12.0, 520.0))
        .default_width(220.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.small(map.name.as_str());
            let Some(bounds) = minimap_bounds(&map.edits) else {
                ui.small("当前地图没有体素。");
                return;
            };

            let map_size = egui::vec2(196.0, 196.0);
            let (rect, response) = ui.allocate_exact_size(map_size, egui::Sense::click());
            let painter = ui.painter_at(rect);
            painter.rect_filled(
                rect,
                4.0,
                egui::Color32::from_rgb(8, 10, 14),
            );
            painter.rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(90)),
                egui::StrokeKind::Inside,
            );

            let columns = minimap_columns(&map.edits);
            for ((x, z), (_, material)) in &columns {
                let pos = minimap_world_to_screen(bounds, rect, *x as f32, *z as f32);
                painter.rect_filled(
                    egui::Rect::from_center_size(pos, egui::vec2(2.5, 2.5)),
                    0.0,
                    minimap_material_color(*material),
                );
            }

            if let Ok(camera) = free_camera.single_mut() {
                let pos = minimap_world_to_screen(
                    bounds,
                    rect,
                    camera.translation.x,
                    camera.translation.z,
                );
                if rect.contains(pos) {
                    painter.circle_stroke(
                        pos,
                        4.0,
                        egui::Stroke::new(1.5, egui::Color32::WHITE),
                    );
                }
            }

            if response.clicked_by(egui::PointerButton::Primary) {
                if let Some(pointer_pos) = response.interact_pointer_pos() {
                    let (x, z) = minimap_screen_to_world(bounds, rect, pointer_pos);
                    let target = minimap_landing_target(&columns, x, z);
                    if let Ok(mut camera) = free_camera.single_mut() {
                        *camera = Transform::from_xyz(
                            target.x,
                            target.y + 28.0,
                            target.z + 0.1,
                        )
                        .looking_at(target, Vec3::Y);
                    }
                }
            }

            ui.small(format!(
                "X {}..{}  Z {}..{}",
                bounds.min_x, bounds.max_x, bounds.min_z, bounds.max_z
            ));
        });
}

fn scene_waypoint_panel(
    mut contexts: EguiContexts,
    mut waypoint_state: ResMut<SceneWaypointState>,
    mut free_camera: Query<&mut Transform, With<FreeCamera>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let default_count = default_scene_waypoints().len();
    let mut waypoints = default_scene_waypoints();
    waypoints.extend(waypoint_state.custom_waypoints.iter().cloned());
    waypoint_state.selected_index = waypoint_state.selected_index.min(waypoints.len() - 1);

    egui::Window::new("路径点")
        .default_pos(egui::pos2(238.0, 60.0))
        .default_width(260.0)
        .resizable(false)
        .show(ctx, |ui| {
            let selected = &waypoints[waypoint_state.selected_index];
            egui::ComboBox::from_label("目标")
                .selected_text(selected.name.as_str())
                .show_ui(ui, |ui| {
                    for (index, waypoint) in waypoints.iter().enumerate() {
                        ui.selectable_value(
                            &mut waypoint_state.selected_index,
                            index,
                            waypoint.name.as_str(),
                        );
                    }
                });

            let selected = &waypoints[waypoint_state.selected_index];
            ui.small(format!(
                "视点 X {:.0} Y {:.0} Z {:.0}",
                selected.eye.x, selected.eye.y, selected.eye.z
            ));
            ui.small(format!(
                "朝向 X {:.0} Y {:.0} Z {:.0}",
                selected.focus.x, selected.focus.y, selected.focus.z
            ));
            ui.small(format!(
                "体素星球：半径{} 预览{}格/块",
                EARTH_PLANET_RADIUS, VOXEL_PLANET_PREVIEW_STEP
            ));

            ui.horizontal(|ui| {
                if ui.button("传送").clicked() {
                    if let Ok(mut camera) = free_camera.single_mut() {
                        *camera = waypoint_transform(selected);
                    }
                }

                let custom_index = waypoint_state.selected_index.checked_sub(default_count);
                let can_delete = custom_index.is_some() && !selected.builtin;
                if ui
                    .add_enabled(
                        can_delete,
                        egui::Button::new("删除自定义"),
                    )
                    .clicked()
                {
                    if let Some(custom_index) = custom_index {
                        if custom_index < waypoint_state.custom_waypoints.len() {
                            waypoint_state.custom_waypoints.remove(custom_index);
                            waypoint_state.selected_index =
                                waypoint_state.selected_index.saturating_sub(1);
                        }
                    }
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                ui.label("名称");
                ui.text_edit_singleline(&mut waypoint_state.new_waypoint_name);
            });
            if ui.button("保存当前视角").clicked() {
                if let Ok(camera) = free_camera.single_mut() {
                    let name = waypoint_state.new_waypoint_name.trim();
                    let name = if name.is_empty() { "路径点" } else { name }.to_owned();
                    waypoint_state.custom_waypoints.push(SceneWaypoint {
                        name,
                        eye: camera.translation,
                        focus: camera.translation + *camera.forward() * 64.0,
                        builtin: false,
                    });
                    waypoint_state.selected_index =
                        default_count + waypoint_state.custom_waypoints.len() - 1;
                }
            }
        });
}

fn waypoint_transform(waypoint: &SceneWaypoint) -> Transform {
    Transform::from_translation(waypoint.eye).looking_at(waypoint.focus, Vec3::Y)
}

fn default_scene_waypoints() -> Vec<SceneWaypoint> {
    let mut waypoints = vec![
        look_at_waypoint(
            "战舰",
            Vec3::new(24.0, 18.0, 32.0),
            Vec3::new(0.0, 8.0, 0.0),
        ),
        look_at_waypoint(
            "空间站A",
            scaled_space_hifi_point(SPACE_HIFI_STATION_A_CENTER).as_vec3()
                + Vec3::new(220.0, 120.0, 220.0),
            scaled_space_hifi_point(SPACE_HIFI_STATION_A_CENTER).as_vec3(),
        ),
        look_at_waypoint(
            "空间站B",
            scaled_space_hifi_point(SPACE_HIFI_STATION_B_CENTER).as_vec3()
                + Vec3::new(-220.0, 120.0, 220.0),
            scaled_space_hifi_point(SPACE_HIFI_STATION_B_CENTER).as_vec3(),
        ),
        look_at_waypoint(
            "太阳",
            scaled_space_hifi_point(SPACE_HIFI_SUN_CENTER).as_vec3()
                + Vec3::new(260.0, 160.0, 260.0),
            scaled_space_hifi_point(SPACE_HIFI_SUN_CENTER).as_vec3(),
        ),
        planet_surface_waypoint(),
        look_at_waypoint(
            "行星中心",
            earth_planet_center().as_vec3()
                + Vec3::new(
                    0.0,
                    0.0,
                    EARTH_PLANET_RADIUS as f32 * 2.2,
                ),
            earth_planet_center().as_vec3(),
        ),
        look_at_waypoint(
            "月球",
            earth_moon_center().as_vec3() + Vec3::new(80.0, 52.0, 80.0),
            earth_moon_center().as_vec3(),
        ),
    ];

    for (index, center) in [
        scaled_space_hifi_point(IVec3::new(-24, 22, 42)),
        scaled_space_hifi_point(IVec3::new(38, 31, -62)),
    ]
    .into_iter()
    .enumerate()
    {
        waypoints.push(look_at_waypoint(
            &format!("小行星{}", index + 1),
            center.as_vec3() + Vec3::new(180.0, 96.0, 180.0),
            center.as_vec3(),
        ));
    }

    waypoints
}

fn look_at_waypoint(name: &str, eye: Vec3, focus: Vec3) -> SceneWaypoint {
    SceneWaypoint {
        name: name.to_owned(),
        eye,
        focus,
        builtin: true,
    }
}

fn planet_surface_waypoint() -> SceneWaypoint {
    let focus = earth_planet_near_point().as_vec3();
    let outward = (focus - earth_planet_center().as_vec3())
        .try_normalize()
        .unwrap_or(Vec3::Z);
    look_at_waypoint(
        "行星表面",
        focus + outward * 128.0,
        focus,
    )
}

#[derive(Clone, Copy)]
struct MinimapBounds {
    min_x: i32,
    max_x: i32,
    min_z: i32,
    max_z: i32,
}

fn minimap_bounds(edits: &[PersistedVoxelEdit]) -> Option<MinimapBounds> {
    let mut bounds: Option<MinimapBounds> = None;
    for edit in edits {
        if edit.voxel == PersistedVoxel::Air {
            continue;
        }
        let x = edit.position[0];
        let z = edit.position[2];
        bounds = Some(match bounds {
            Some(bounds) => MinimapBounds {
                min_x: bounds.min_x.min(x),
                max_x: bounds.max_x.max(x),
                min_z: bounds.min_z.min(z),
                max_z: bounds.max_z.max(z),
            },
            None => MinimapBounds {
                min_x: x,
                max_x: x,
                min_z: z,
                max_z: z,
            },
        });
    }
    bounds.map(|mut bounds| {
        if bounds.min_x == bounds.max_x {
            bounds.min_x -= 1;
            bounds.max_x += 1;
        }
        if bounds.min_z == bounds.max_z {
            bounds.min_z -= 1;
            bounds.max_z += 1;
        }
        bounds
    })
}

fn minimap_columns(edits: &[PersistedVoxelEdit]) -> HashMap<(i32, i32), (i32, u8)> {
    let mut columns = HashMap::new();
    for edit in edits {
        let PersistedVoxel::Solid(material) = edit.voxel else {
            continue;
        };
        let x = edit.position[0];
        let y = edit.position[1];
        let z = edit.position[2];
        columns
            .entry((x, z))
            .and_modify(|(top_y, top_material)| {
                if y >= *top_y {
                    *top_y = y;
                    *top_material = material;
                }
            })
            .or_insert((y, material));
    }
    columns
}

fn minimap_world_to_screen(bounds: MinimapBounds, rect: egui::Rect, x: f32, z: f32) -> egui::Pos2 {
    let width = (bounds.max_x - bounds.min_x).max(1) as f32;
    let depth = (bounds.max_z - bounds.min_z).max(1) as f32;
    let nx = ((x - bounds.min_x as f32) / width).clamp(0.0, 1.0);
    let nz = ((z - bounds.min_z as f32) / depth).clamp(0.0, 1.0);
    egui::pos2(
        rect.left() + nx * rect.width(),
        rect.bottom() - nz * rect.height(),
    )
}

fn minimap_screen_to_world(bounds: MinimapBounds, rect: egui::Rect, pos: egui::Pos2) -> (f32, f32) {
    let width = (bounds.max_x - bounds.min_x).max(1) as f32;
    let depth = (bounds.max_z - bounds.min_z).max(1) as f32;
    let nx = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    let nz = ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0);
    (
        bounds.min_x as f32 + nx * width,
        bounds.min_z as f32 + nz * depth,
    )
}

fn minimap_landing_target(columns: &HashMap<(i32, i32), (i32, u8)>, x: f32, z: f32) -> Vec3 {
    let center = IVec3::new(x.round() as i32, 0, z.round() as i32);
    let mut best: Option<(i32, i32, i32, i32)> = None;
    for radius in 0..=18 {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let key = (center.x + dx, center.z + dz);
                if let Some((top_y, material)) = columns.get(&key) {
                    if *material == MAT_STAR {
                        continue;
                    }
                    let distance = dx * dx + dz * dz;
                    best = Some(match best {
                        Some((best_distance, best_x, best_z, best_y))
                            if best_distance <= distance =>
                        {
                            (best_distance, best_x, best_z, best_y)
                        },
                        _ => (distance, key.0, key.1, *top_y),
                    });
                }
            }
        }
        if let Some((_, best_x, best_z, top_y)) = best {
            return Vec3::new(
                best_x as f32 + 0.5,
                top_y as f32 + 1.0,
                best_z as f32 + 0.5,
            );
        }
    }
    Vec3::new(x, 8.0, z)
}

fn minimap_material_color(material: u8) -> egui::Color32 {
    match material {
        MAT_STAR => egui::Color32::from_rgb(230, 240, 255),
        MAT_HULL_LIGHT => egui::Color32::from_rgb(170, 178, 188),
        MAT_HULL_DARK => egui::Color32::from_rgb(72, 82, 100),
        MAT_WINDOW_CYAN => egui::Color32::from_rgb(0, 220, 245),
        MAT_ENGINE_RED => egui::Color32::from_rgb(255, 82, 42),
        MAT_STATION_METAL => egui::Color32::from_rgb(120, 130, 140),
        MAT_STATION_TRIM => egui::Color32::from_rgb(210, 204, 184),
        MAT_SUN => egui::Color32::from_rgb(255, 176, 36),
        MAT_SOLAR_PANEL => egui::Color32::from_rgb(36, 82, 170),
        MAT_PLANET_OCEAN => egui::Color32::from_rgb(26, 108, 194),
        MAT_PLANET_LAND => egui::Color32::from_rgb(58, 150, 78),
        _ => egui::Color32::from_gray(100),
    }
}

fn default_camera_speed() -> f32 { DEFAULT_CAMERA_SPEED }

fn normalized_camera_speed(speed: f32) -> f32 {
    if speed.is_finite() {
        speed.clamp(2.0, MAX_CAMERA_SPEED)
    } else {
        DEFAULT_CAMERA_SPEED
    }
}

fn normalize_persisted_editor_settings(store: &mut VoxelSceneStore) -> bool {
    let camera_speed =
        if (store.editor_camera_speed - LEGACY_DEFAULT_CAMERA_SPEED).abs() <= f32::EPSILON {
            DEFAULT_CAMERA_SPEED
        } else {
            normalized_camera_speed(store.editor_camera_speed)
        };
    let changed = store.editor_camera_speed != camera_speed;
    store.editor_camera_speed = camera_speed;
    changed
}

fn ensure_voxel_maps(store: &mut Persistent<VoxelSceneStore>) {
    if store.maps.is_empty() {
        let legacy_edits = std::mem::take(&mut store.edits);
        store.maps.push(PersistedVoxelMap {
            id: "default".to_owned(),
            name: "默认地图".to_owned(),
            edits: legacy_edits,
        });
    } else if !store.edits.is_empty() {
        let legacy_edits = std::mem::take(&mut store.edits);
        if let Some(map) = store.maps.first_mut() {
            for edit in legacy_edits {
                let position = IVec3::new(
                    edit.position[0],
                    edit.position[1],
                    edit.position[2],
                );
                upsert_persisted_edit(&mut map.edits, position, edit.voxel);
            }
        }
    }

    store
        .maps
        .retain(|map| map.id == SPACE_HIFI_MAP_ID || !map.id.starts_with("space-hifi"));
    store.map_status_snapshots.retain(|snapshot| {
        snapshot.map_id == SPACE_HIFI_MAP_ID || !snapshot.map_id.starts_with("space-hifi")
    });

    let inserted_space_hifi = !store.maps.iter().any(|map| map.id == SPACE_HIFI_MAP_ID);
    if inserted_space_hifi {
        store.maps.push(PersistedVoxelMap {
            id: SPACE_HIFI_MAP_ID.to_owned(),
            name: SPACE_HIFI_MAP_NAME.to_owned(),
            edits: space_hifi_voxel_edits(),
        });
        store.active_map_id = Some(SPACE_HIFI_MAP_ID.to_owned());
    } else if let Some(map) = store
        .maps
        .iter_mut()
        .find(|map| map.id == SPACE_HIFI_MAP_ID && map.edits.len() < 10_000)
    {
        map.edits = space_hifi_voxel_edits();
        store.active_map_id = Some(SPACE_HIFI_MAP_ID.to_owned());
    }

    let active_exists = store
        .active_map_id
        .as_deref()
        .is_some_and(|active_id| store.maps.iter().any(|map| map.id == active_id));
    if !active_exists {
        store.active_map_id = Some(SPACE_HIFI_MAP_ID.to_owned());
    } else if active_voxel_map(store).is_some_and(|map| map.edits.is_empty())
        && store.maps.iter().any(|map| map.id == SPACE_HIFI_MAP_ID)
    {
        store.active_map_id = Some(SPACE_HIFI_MAP_ID.to_owned());
    }
}

fn space_hifi_voxel_edits() -> Vec<PersistedVoxelEdit> {
    let mut edits = Vec::new();
    push_starfield(&mut edits);
    push_battle_spaceship(&mut edits);
    push_space_station(
        &mut edits,
        scaled_space_hifi_point(SPACE_HIFI_STATION_A_CENTER),
        false,
    );
    push_space_station(
        &mut edits,
        scaled_space_hifi_point(SPACE_HIFI_STATION_B_CENTER),
        true,
    );
    push_sun(
        &mut edits,
        scaled_space_hifi_point(SPACE_HIFI_SUN_CENTER),
        SPACE_HIFI_SUN_RADIUS,
    );
    push_earth_moon(&mut edits, earth_moon_center(), 8);
    push_asteroid_cluster(
        &mut edits,
        scaled_space_hifi_point(IVec3::new(-24, 22, 42)),
        5,
        8,
    );
    push_asteroid_cluster(
        &mut edits,
        scaled_space_hifi_point(IVec3::new(38, 31, -62)),
        4,
        7,
    );
    edits
}

fn space_hifi_procedural_voxel(position: IVec3) -> WorldVoxel<u8> {
    let mut material = None;
    let x = position.x;
    let y = position.y;
    let z = position.z;

    if procedural_star(position) {
        material = Some(MAT_STAR);
    }
    if let Some(ship_material) = procedural_battle_spaceship(position) {
        material = Some(ship_material);
    }
    if let Some(station_material) = procedural_space_station(
        position,
        scaled_space_hifi_point(SPACE_HIFI_STATION_A_CENTER),
        false,
    ) {
        material = Some(station_material);
    }
    if let Some(station_material) = procedural_space_station(
        position,
        scaled_space_hifi_point(SPACE_HIFI_STATION_B_CENTER),
        true,
    ) {
        material = Some(station_material);
    }
    if let Some(detail_material) = procedural_space_details(position) {
        material = Some(detail_material);
    }
    if let Some(planet_material) = procedural_earth_voxel_planet(position) {
        material = Some(planet_material);
    }
    let sun_center = scaled_space_hifi_point(SPACE_HIFI_SUN_CENTER);
    if procedural_ellipsoid_shell(
        position,
        sun_center,
        IVec3::splat(SPACE_HIFI_SUN_RADIUS),
        0.74,
    ) || ((position - sun_center).abs().cmple(IVec3::splat(18)).all()
        && ((x == sun_center.x && y == sun_center.y)
            || (z == sun_center.z && y == sun_center.y)
            || (x == sun_center.x && z == sun_center.z)))
    {
        material = Some(MAT_SUN);
    }
    material.map_or(WorldVoxel::Air, WorldVoxel::Solid)
}

fn scaled_space_hifi_point(point: IVec3) -> IVec3 { point * SPACE_HIFI_MAP_SCALE }

fn scaled_battle_spaceship_position(position: IVec3) -> IVec3 { position * BATTLE_SPACESHIP_SCALE }

fn battle_spaceship_preview_origin(position: IVec3) -> Option<IVec3> {
    let unscaled = unscaled_battle_spaceship_position(position);
    let expected_position =
        scaled_battle_spaceship_position(unscaled) + IVec3::Y * (BATTLE_SPACESHIP_SCALE - 1);
    (position == expected_position && procedural_battle_spaceship_unscaled(unscaled).is_some())
        .then(|| scaled_battle_spaceship_position(unscaled))
}

fn unscaled_battle_spaceship_position(position: IVec3) -> IVec3 {
    IVec3::new(
        position.x.div_euclid(BATTLE_SPACESHIP_SCALE),
        position.y.div_euclid(BATTLE_SPACESHIP_SCALE),
        position.z.div_euclid(BATTLE_SPACESHIP_SCALE),
    )
}

fn earth_planet_near_point() -> IVec3 { scaled_space_hifi_point(EARTH_PLANET_NEAR_POINT) }

fn earth_planet_center() -> IVec3 {
    let near_point = earth_planet_near_point();
    let direction = near_point.as_vec3().normalize_or_zero();
    near_point + (direction * EARTH_PLANET_RADIUS as f32).round().as_ivec3()
}

fn earth_moon_center() -> IVec3 { earth_planet_near_point() + IVec3::new(380, 86, -260) }

fn procedural_earth_voxel_planet(position: IVec3) -> Option<u8> {
    let center = earth_planet_center();
    let offset = position - center;
    let max_radius = EARTH_PLANET_RADIUS + VOXEL_PLANET_MAX_ELEVATION.ceil() as i32;
    if offset.abs().cmpgt(IVec3::splat(max_radius)).any() {
        return None;
    }

    let distance_squared = offset.as_vec3().length_squared();
    let min_uniform_radius = EARTH_PLANET_RADIUS as f32 - VOXEL_PLANET_MAX_ELEVATION;
    if distance_squared <= min_uniform_radius * min_uniform_radius {
        return Some(MAT_PLANET_LAND);
    }

    if distance_squared > (max_radius * max_radius) as f32 {
        return None;
    }

    let distance = distance_squared.sqrt();
    let direction = offset.as_vec3().try_normalize().unwrap_or(Vec3::Y);
    let surface_radius = EARTH_PLANET_RADIUS as f32 + voxel_planet_elevation(direction);
    if distance > surface_radius {
        return None;
    }

    Some(voxel_planet_material(
        direction,
        surface_radius - distance,
    ))
}

fn voxel_planet_elevation(direction: Vec3) -> f32 {
    let continent = smooth_voxel_planet_noise(direction * 3.6, 7_331);
    let hills = smooth_voxel_planet_noise(
        direction * 13.0 + Vec3::splat(17.0),
        19_327,
    );
    let detail = smooth_voxel_planet_noise(
        direction * 41.0 + Vec3::new(3.0, 11.0, 23.0),
        91_337,
    );

    (continent * 44.0 + hills * 16.0 + detail * 8.0).clamp(
        -VOXEL_PLANET_MAX_ELEVATION,
        VOXEL_PLANET_MAX_ELEVATION,
    )
}

fn voxel_planet_material(direction: Vec3, depth_below_surface: f32) -> u8 {
    if depth_below_surface > 12.0 {
        return MAT_PLANET_LAND;
    }

    let continent = smooth_voxel_planet_noise(direction * 3.6, 7_331);
    let moisture = smooth_voxel_planet_noise(
        direction * 9.0 + Vec3::splat(5.0),
        38_411,
    );
    if continent + moisture * 0.35 < -0.22 {
        MAT_PLANET_OCEAN
    } else {
        MAT_PLANET_LAND
    }
}

fn voxel_planet_preview_blocks() -> HashMap<IVec3, u8> {
    let mut voxels = HashMap::new();
    let center = earth_planet_center();
    let step = VOXEL_PLANET_PREVIEW_STEP;
    let half_step = step / 2;
    let extent = EARTH_PLANET_RADIUS + VOXEL_PLANET_MAX_ELEVATION.ceil() as i32 + step;

    for x in (-extent..=extent).step_by(step as usize) {
        for y in (-extent..=extent).step_by(step as usize) {
            for z in (-extent..=extent).step_by(step as usize) {
                let origin = center + IVec3::new(x, y, z) - IVec3::splat(half_step);
                let sample = origin + IVec3::splat(half_step);
                if let Some(material) = procedural_earth_voxel_planet(sample) {
                    voxels.insert(origin, material);
                }
            }
        }
    }

    voxels
}

fn smooth_voxel_planet_noise(position: Vec3, seed: u32) -> f32 {
    let cell = position.floor().as_ivec3();
    let local = position - cell.as_vec3();
    let fade =
        local * local * local * (local * (local * 6.0 - Vec3::splat(15.0)) + Vec3::splat(10.0));

    let mut values = [[[0.0; 2]; 2]; 2];
    for x in 0..=1 {
        for y in 0..=1 {
            for z in 0..=1 {
                values[x][y][z] = voxel_planet_hash_noise(
                    cell + IVec3::new(x as i32, y as i32, z as i32),
                    seed,
                );
            }
        }
    }

    let x00 = lerp(values[0][0][0], values[1][0][0], fade.x);
    let x10 = lerp(values[0][1][0], values[1][1][0], fade.x);
    let x01 = lerp(values[0][0][1], values[1][0][1], fade.x);
    let x11 = lerp(values[0][1][1], values[1][1][1], fade.x);
    let y0 = lerp(x00, x10, fade.y);
    let y1 = lerp(x01, x11, fade.y);
    lerp(y0, y1, fade.z)
}

fn voxel_planet_hash_noise(cell: IVec3, seed: u32) -> f32 {
    let mut h = seed
        ^ (cell.x as u32).wrapping_mul(0x8da6_b343)
        ^ (cell.y as u32).wrapping_mul(0xd816_3841)
        ^ (cell.z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }

fn procedural_star(position: IVec3) -> bool {
    for i in 0i32..80 {
        let base_x = ((i * 83) % 181) - 90;
        let base_y = ((i * 47) % 48) + 8;
        let base_z = ((i * 109) % 181) - 90;
        if base_x.abs() < 34 && base_z.abs() < 44 {
            continue;
        }
        let x = base_x * SPACE_HIFI_MAP_SCALE;
        let y = base_y * SPACE_HIFI_MAP_SCALE;
        let z = base_z * SPACE_HIFI_MAP_SCALE;
        if position == IVec3::new(x, y, z) {
            return true;
        }
    }
    false
}

fn procedural_space_details(position: IVec3) -> Option<u8> {
    if procedural_ellipsoid_shell(
        position,
        earth_moon_center(),
        IVec3::new(8, 8, 8),
        0.68,
    ) {
        return Some(MAT_STATION_METAL);
    }

    for (center, radius, count) in [
        (
            scaled_space_hifi_point(IVec3::new(-24, 22, 42)),
            5,
            8,
        ),
        (
            scaled_space_hifi_point(IVec3::new(38, 31, -62)),
            4,
            7,
        ),
    ] {
        if (position - center).abs().cmpgt(IVec3::splat(240)).any() {
            continue;
        }
        for index in 0..count {
            let asteroid_center = asteroid_position(center, index);
            let asteroid_radius = asteroid_radius(radius, index);
            if procedural_ellipsoid_shell(
                position,
                asteroid_center,
                IVec3::splat(asteroid_radius),
                0.52,
            ) {
                return Some(MAT_STATION_METAL);
            }
        }
    }

    None
}

fn procedural_battle_spaceship(position: IVec3) -> Option<u8> {
    let position = unscaled_battle_spaceship_position(position);
    procedural_battle_spaceship_unscaled(position)
}

fn procedural_battle_spaceship_unscaled(position: IVec3) -> Option<u8> {
    let x = position.x;
    let y = position.y;
    let z = position.z;
    let mut material = None;

    if (-42..=44).contains(&z) && (-14..=14).contains(&x) && (0..=15).contains(&y) {
        let width = if z > 26 {
            ((44 - z) as f32 * 0.35 + 3.0).max(3.0)
        } else if z < -32 {
            ((z + 42) as f32 * 0.35 + 4.0).max(4.0)
        } else {
            12.0
        };
        let height = if z > 28 { 4.5 } else { 7.0 };
        let dx = x as f32 / width;
        let dy = (y as f32 - 7.5) / height;
        let shell = dx * dx + dy * dy;
        if (0.7..=1.0).contains(&shell) {
            material = Some(
                if y >= 12 || x.abs() >= width.round() as i32 - 1 {
                    MAT_HULL_LIGHT
                } else {
                    MAT_HULL_DARK
                },
            );
        }
    }

    for (min, max, box_material) in [
        (
            IVec3::new(-10, 3, -35),
            IVec3::new(10, 3, 34),
            MAT_HULL_DARK,
        ),
        (
            IVec3::new(-8, 8, -28),
            IVec3::new(8, 8, 26),
            MAT_HULL_DARK,
        ),
        (
            IVec3::new(-3, 4, -40),
            IVec3::new(3, 8, -36),
            MAT_ENGINE_RED,
        ),
        (
            IVec3::new(-12, 4, -28),
            IVec3::new(-12, 11, 22),
            MAT_HULL_LIGHT,
        ),
        (
            IVec3::new(12, 4, -28),
            IVec3::new(12, 11, 22),
            MAT_HULL_LIGHT,
        ),
        (
            IVec3::new(-20, 5, -12),
            IVec3::new(-13, 8, 28),
            MAT_HULL_DARK,
        ),
        (
            IVec3::new(13, 5, -12),
            IVec3::new(20, 8, 28),
            MAT_HULL_DARK,
        ),
        (
            IVec3::new(-4, 13, 24),
            IVec3::new(4, 16, 30),
            MAT_WINDOW_CYAN,
        ),
    ] {
        if point_in_box(position, min, max) {
            material = Some(box_material);
        }
    }

    if point_in_hollow_box(
        position,
        IVec3::new(-5, 11, 0),
        IVec3::new(5, 19, 28),
    ) {
        material = Some(MAT_HULL_LIGHT);
    }
    for z in (-28..=22).step_by(16) {
        if point_in_box(
            position,
            IVec3::new(-10, 4, z),
            IVec3::new(-3, 8, z),
        ) || point_in_box(
            position,
            IVec3::new(3, 4, z),
            IVec3::new(10, 8, z),
        ) {
            material = Some(MAT_STATION_TRIM);
        }
    }
    for z in (-30..=30).step_by(10) {
        if point_in_box(
            position,
            IVec3::new(-14, 7, z),
            IVec3::new(-14, 9, z + 3),
        ) || point_in_box(
            position,
            IVec3::new(14, 7, z),
            IVec3::new(14, 9, z + 3),
        ) {
            material = Some(MAT_WINDOW_CYAN);
        }
    }
    for x in [-7, 5] {
        if point_in_box(
            position,
            IVec3::new(x, 4, -35),
            IVec3::new(x + 2, 8, -44),
        ) {
            material = Some(MAT_ENGINE_RED);
        }
    }

    material
}

fn procedural_space_station(position: IVec3, center: IVec3, rotated: bool) -> Option<u8> {
    let local = position - center;
    let mut material = None;
    if point_in_hollow_box(
        local,
        IVec3::new(-7, -5, -7),
        IVec3::new(7, 9, 7),
    ) {
        material = Some(MAT_STATION_METAL);
    }
    if point_in_box(
        local,
        IVec3::new(-6, 0, -6),
        IVec3::new(6, 0, 6),
    ) || procedural_ellipsoid_shell(
        position,
        center,
        IVec3::new(12, 3, 12),
        0.82,
    ) {
        material = Some(MAT_STATION_TRIM);
    }
    let d2 = local.x * local.x + local.z * local.z;
    if (300..=400).contains(&d2) && (0..=1).contains(&local.y) {
        material = Some(MAT_STATION_METAL);
    }

    if rotated {
        if point_in_box(
            local,
            IVec3::new(-1, 0, -30),
            IVec3::new(1, 1, 30),
        ) {
            material = Some(MAT_STATION_TRIM);
        }
        if point_in_box(
            local,
            IVec3::new(-12, -1, -42),
            IVec3::new(12, 1, -34),
        ) || point_in_box(
            local,
            IVec3::new(-12, -1, 34),
            IVec3::new(12, 1, 42),
        ) {
            material = Some(MAT_SOLAR_PANEL);
        }
    } else {
        if point_in_box(
            local,
            IVec3::new(-30, 0, -1),
            IVec3::new(30, 1, 1),
        ) {
            material = Some(MAT_STATION_TRIM);
        }
        if point_in_box(
            local,
            IVec3::new(-42, -1, -12),
            IVec3::new(-34, 1, 12),
        ) || point_in_box(
            local,
            IVec3::new(34, -1, -12),
            IVec3::new(42, 1, 12),
        ) {
            material = Some(MAT_SOLAR_PANEL);
        }
    }

    material
}

fn procedural_ellipsoid_shell(
    position: IVec3,
    center: IVec3,
    radii: IVec3,
    inner_ratio: f32,
) -> bool {
    let p = position - center;
    if p.abs().cmpgt(radii).any() {
        return false;
    }
    let dx = p.x as f32 / radii.x.max(1) as f32;
    let dy = p.y as f32 / radii.y.max(1) as f32;
    let dz = p.z as f32 / radii.z.max(1) as f32;
    let shell = dx * dx + dy * dy + dz * dz;
    ((inner_ratio * inner_ratio)..=1.0).contains(&shell)
}

fn point_in_box(position: IVec3, min: IVec3, max: IVec3) -> bool {
    let (min, max) = normalized_box_bounds(min, max);
    position.cmpge(min).all() && position.cmple(max).all()
}

fn point_in_hollow_box(position: IVec3, min: IVec3, max: IVec3) -> bool {
    let (min, max) = normalized_box_bounds(min, max);
    if !point_in_box(position, min, max) {
        return false;
    }
    let boundary = position.x == min.x
        || position.x == max.x
        || position.y == min.y
        || position.y == max.y
        || position.z == min.z
        || position.z == max.z;
    let doorway = position.y <= min.y + 5
        && position.x.abs_diff((min.x + max.x) / 2) <= 2
        && (position.z == min.z || position.z == max.z);
    boundary && !doorway
}

fn normalized_box_bounds(min: IVec3, max: IVec3) -> (IVec3, IVec3) { (min.min(max), min.max(max)) }

fn push_starfield(edits: &mut Vec<PersistedVoxelEdit>) {
    for i in 0i32..80 {
        let base_x = ((i * 83) % 181) - 90;
        let base_y = ((i * 47) % 48) + 8;
        let base_z = ((i * 109) % 181) - 90;
        if base_x.abs() < 34 && base_z.abs() < 44 {
            continue;
        }
        let x = base_x * SPACE_HIFI_MAP_SCALE;
        let y = base_y * SPACE_HIFI_MAP_SCALE;
        let z = base_z * SPACE_HIFI_MAP_SCALE;
        push_voxel(edits, IVec3::new(x, y, z), MAT_STAR);
    }
}

fn push_battle_spaceship(edits: &mut Vec<PersistedVoxelEdit>) {
    for z in -42i32..=44 {
        let width = if z > 26 {
            ((44 - z) as f32 * 0.35 + 3.0).max(3.0)
        } else if z < -32 {
            ((z + 42) as f32 * 0.35 + 4.0).max(4.0)
        } else {
            12.0
        };
        let height = if z > 28 { 4.5 } else { 7.0 };
        for x in -14i32..=14 {
            for y in 0i32..=15 {
                let dx = x as f32 / width;
                let dy = (y as f32 - 7.5) / height;
                let shell = dx * dx + dy * dy;
                if (0.7..=1.0).contains(&shell) {
                    let mat = if y >= 12 || x.abs() >= width.round() as i32 - 1 {
                        MAT_HULL_LIGHT
                    } else {
                        MAT_HULL_DARK
                    };
                    push_battle_spaceship_voxel(edits, IVec3::new(x, y, z), mat);
                }
            }
        }
    }

    push_battle_spaceship_box(
        edits,
        IVec3::new(-10, 3, -35),
        IVec3::new(10, 3, 34),
        MAT_HULL_DARK,
    );
    push_battle_spaceship_box(
        edits,
        IVec3::new(-8, 8, -28),
        IVec3::new(8, 8, 26),
        MAT_HULL_DARK,
    );
    push_battle_spaceship_box(
        edits,
        IVec3::new(-3, 4, -40),
        IVec3::new(3, 8, -36),
        MAT_ENGINE_RED,
    );
    push_battle_spaceship_box(
        edits,
        IVec3::new(-12, 4, -28),
        IVec3::new(-12, 11, 22),
        MAT_HULL_LIGHT,
    );
    push_battle_spaceship_box(
        edits,
        IVec3::new(12, 4, -28),
        IVec3::new(12, 11, 22),
        MAT_HULL_LIGHT,
    );
    push_battle_spaceship_box(
        edits,
        IVec3::new(-20, 5, -12),
        IVec3::new(-13, 8, 28),
        MAT_HULL_DARK,
    );
    push_battle_spaceship_box(
        edits,
        IVec3::new(13, 5, -12),
        IVec3::new(20, 8, 28),
        MAT_HULL_DARK,
    );
    push_battle_spaceship_hollow_box(
        edits,
        IVec3::new(-5, 11, 0),
        IVec3::new(5, 19, 28),
        MAT_HULL_LIGHT,
    );
    push_battle_spaceship_box(
        edits,
        IVec3::new(-4, 13, 24),
        IVec3::new(4, 16, 30),
        MAT_WINDOW_CYAN,
    );

    for z in (-28..=22).step_by(16) {
        push_battle_spaceship_box(
            edits,
            IVec3::new(-10, 4, z),
            IVec3::new(-3, 8, z),
            MAT_STATION_TRIM,
        );
        push_battle_spaceship_box(
            edits,
            IVec3::new(3, 4, z),
            IVec3::new(10, 8, z),
            MAT_STATION_TRIM,
        );
    }
    for z in (-30..=30).step_by(10) {
        push_battle_spaceship_box(
            edits,
            IVec3::new(-14, 7, z),
            IVec3::new(-14, 9, z + 3),
            MAT_WINDOW_CYAN,
        );
        push_battle_spaceship_box(
            edits,
            IVec3::new(14, 7, z),
            IVec3::new(14, 9, z + 3),
            MAT_WINDOW_CYAN,
        );
    }
    for x in [-7, 5] {
        push_battle_spaceship_box(
            edits,
            IVec3::new(x, 4, -35),
            IVec3::new(x + 2, 8, -44),
            MAT_ENGINE_RED,
        );
    }
}

fn push_battle_spaceship_voxel(edits: &mut Vec<PersistedVoxelEdit>, position: IVec3, material: u8) {
    push_voxel(
        edits,
        scaled_battle_spaceship_position(position) + IVec3::Y * (BATTLE_SPACESHIP_SCALE - 1),
        material,
    );
}

fn push_battle_spaceship_box(
    edits: &mut Vec<PersistedVoxelEdit>,
    min: IVec3,
    max: IVec3,
    material: u8,
) {
    let (min, max) = normalized_box_bounds(min, max);
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                push_battle_spaceship_voxel(edits, IVec3::new(x, y, z), material);
            }
        }
    }
}

fn push_battle_spaceship_hollow_box(
    edits: &mut Vec<PersistedVoxelEdit>,
    min: IVec3,
    max: IVec3,
    material: u8,
) {
    let (min, max) = normalized_box_bounds(min, max);
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let position = IVec3::new(x, y, z);
                if point_in_hollow_box(position, min, max) {
                    push_battle_spaceship_voxel(edits, position, material);
                }
            }
        }
    }
}

fn push_space_station(edits: &mut Vec<PersistedVoxelEdit>, center: IVec3, rotated: bool) {
    push_hollow_box(
        edits,
        center + IVec3::new(-7, -5, -7),
        center + IVec3::new(7, 9, 7),
        MAT_STATION_METAL,
    );
    push_box(
        edits,
        center + IVec3::new(-6, 0, -6),
        center + IVec3::new(6, 0, 6),
        MAT_STATION_TRIM,
    );
    push_ellipsoid_shell(
        edits,
        center,
        IVec3::new(12, 3, 12),
        0.82,
        MAT_STATION_TRIM,
    );

    for x in -20i32..=20 {
        for z in -20i32..=20 {
            let d2 = x * x + z * z;
            if (300..=400).contains(&d2) {
                for y in 0i32..=1 {
                    push_voxel(
                        edits,
                        center + IVec3::new(x, y, z),
                        MAT_STATION_METAL,
                    );
                }
            }
        }
    }

    if rotated {
        push_box(
            edits,
            center + IVec3::new(-1, 0, -30),
            center + IVec3::new(1, 1, 30),
            MAT_STATION_TRIM,
        );
        push_box(
            edits,
            center + IVec3::new(-12, -1, -42),
            center + IVec3::new(12, 1, -34),
            MAT_SOLAR_PANEL,
        );
        push_box(
            edits,
            center + IVec3::new(-12, -1, 34),
            center + IVec3::new(12, 1, 42),
            MAT_SOLAR_PANEL,
        );
    } else {
        push_box(
            edits,
            center + IVec3::new(-30, 0, -1),
            center + IVec3::new(30, 1, 1),
            MAT_STATION_TRIM,
        );
        push_box(
            edits,
            center + IVec3::new(-42, -1, -12),
            center + IVec3::new(-34, 1, 12),
            MAT_SOLAR_PANEL,
        );
        push_box(
            edits,
            center + IVec3::new(34, -1, -12),
            center + IVec3::new(42, 1, 12),
            MAT_SOLAR_PANEL,
        );
    }
}

fn push_sun(edits: &mut Vec<PersistedVoxelEdit>, center: IVec3, radius: i32) {
    push_ellipsoid_shell(
        edits,
        center,
        IVec3::splat(radius),
        0.74,
        MAT_SUN,
    );
    for ray in [
        IVec3::X,
        IVec3::NEG_X,
        IVec3::Y,
        IVec3::NEG_Y,
        IVec3::Z,
        IVec3::NEG_Z,
    ] {
        for step in radius..=(radius + 10) {
            push_voxel(edits, center + ray * step, MAT_SUN);
        }
    }
}

fn push_earth_moon(edits: &mut Vec<PersistedVoxelEdit>, center: IVec3, radius: i32) {
    push_ellipsoid_shell(
        edits,
        center,
        IVec3::splat(radius),
        0.68,
        MAT_STATION_METAL,
    );
    for crater in [
        IVec3::new(-6, 8, 3),
        IVec3::new(4, 1, -9),
        IVec3::new(7, -4, 6),
    ] {
        push_box(
            edits,
            center + crater,
            center + crater + IVec3::new(2, 0, 2),
            MAT_HULL_DARK,
        );
    }
}

fn push_asteroid_cluster(
    edits: &mut Vec<PersistedVoxelEdit>,
    center: IVec3,
    base_radius: i32,
    count: i32,
) {
    for index in 0..count {
        let asteroid_center = asteroid_position(center, index);
        let asteroid_radius = asteroid_radius(base_radius, index);
        push_ellipsoid_shell(
            edits,
            asteroid_center,
            IVec3::splat(asteroid_radius),
            0.52,
            MAT_STATION_METAL,
        );
    }
}

fn asteroid_position(center: IVec3, index: i32) -> IVec3 {
    center
        + IVec3::new(
            ((index * 47) % 401) - 200,
            ((index * 29) % 121) - 60,
            ((index * 73) % 401) - 200,
        )
}

fn asteroid_radius(base_radius: i32, index: i32) -> i32 {
    (base_radius + (index * 7).rem_euclid(11) - 5).max(5)
}

fn push_ellipsoid_shell(
    edits: &mut Vec<PersistedVoxelEdit>,
    center: IVec3,
    radii: IVec3,
    inner_ratio: f32,
    material: u8,
) {
    let inner = inner_ratio * inner_ratio;
    for x in -radii.x..=radii.x {
        for y in -radii.y..=radii.y {
            for z in -radii.z..=radii.z {
                let dx = x as f32 / radii.x.max(1) as f32;
                let dy = y as f32 / radii.y.max(1) as f32;
                let dz = z as f32 / radii.z.max(1) as f32;
                let shell = dx * dx + dy * dy + dz * dz;
                if (inner..=1.0).contains(&shell) {
                    push_voxel(
                        edits,
                        center + IVec3::new(x, y, z),
                        material,
                    );
                }
            }
        }
    }
}

fn push_hollow_box(edits: &mut Vec<PersistedVoxelEdit>, min: IVec3, max: IVec3, material: u8) {
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let boundary = x == min.x
                    || x == max.x
                    || y == min.y
                    || y == max.y
                    || z == min.z
                    || z == max.z;
                let doorway = y <= min.y + 5
                    && x.abs_diff((min.x + max.x) / 2) <= 2
                    && (z == min.z || z == max.z);
                if boundary && !doorway {
                    push_voxel(edits, IVec3::new(x, y, z), material);
                }
            }
        }
    }
}

fn push_box(edits: &mut Vec<PersistedVoxelEdit>, min: IVec3, max: IVec3, material: u8) {
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                push_voxel(edits, IVec3::new(x, y, z), material);
            }
        }
    }
}

fn push_voxel(edits: &mut Vec<PersistedVoxelEdit>, position: IVec3, material: u8) {
    upsert_persisted_edit(
        edits,
        position,
        PersistedVoxel::Solid(material),
    );
}

fn active_voxel_map(store: &VoxelSceneStore) -> Option<&PersistedVoxelMap> {
    store
        .active_map_id
        .as_deref()
        .and_then(|active_id| store.maps.iter().find(|map| map.id == active_id))
        .or_else(|| store.maps.first())
}

fn active_voxel_map_id(store: &VoxelSceneStore) -> Option<String> {
    active_voxel_map(store).map(|map| map.id.clone())
}

fn active_voxel_map_mut(store: &mut VoxelSceneStore) -> Option<&mut PersistedVoxelMap> {
    let active_map_id = store.active_map_id.clone();
    if let Some(active_map_id) = active_map_id {
        if let Some(index) = store.maps.iter().position(|map| map.id == active_map_id) {
            return store.maps.get_mut(index);
        }
    }
    store.maps.first_mut()
}

fn clean_voxel_map_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        "未命名地图".to_owned()
    } else {
        name.to_owned()
    }
}

fn unique_voxel_map_name(
    maps: &[PersistedVoxelMap],
    preferred_name: &str,
    allowed_id: Option<&str>,
) -> String {
    let preferred_name = clean_voxel_map_name(preferred_name);
    let mut name = preferred_name.clone();
    let mut suffix = 2;
    while maps
        .iter()
        .any(|map| allowed_id != Some(map.id.as_str()) && map.name.eq_ignore_ascii_case(&name))
    {
        name = format!("{preferred_name} {suffix}");
        suffix += 1;
    }
    name
}

fn new_voxel_map_id(maps: &[PersistedVoxelMap]) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let mut id = format!("map-{millis}");
    let mut suffix = 2;
    while maps.iter().any(|map| map.id == id) {
        id = format!("map-{millis}-{suffix}");
        suffix += 1;
    }
    id
}

fn save_active_map_status(store: &mut VoxelSceneStore, reason: &str, auto: bool) -> Option<String> {
    let map = active_voxel_map(store)?.clone();
    let created_at = unix_timestamp_secs();
    let id = new_map_status_snapshot_id(&store.map_status_snapshots);
    let reason = reason.to_owned();
    store
        .map_status_snapshots
        .push(PersistedVoxelMapStatusSnapshot {
            id: id.clone(),
            map_id: map.id.clone(),
            name: status_snapshot_name(&map.name, &reason, created_at),
            reason,
            created_at,
            edits: map.edits,
        });

    if auto {
        prune_auto_map_status_snapshots(store, &map.id);
    }
    Some(id)
}

fn status_snapshots_for_active_map(
    store: &VoxelSceneStore,
) -> Vec<PersistedVoxelMapStatusSnapshot> {
    let Some(map_id) = active_voxel_map(store).map(|map| map.id.as_str()) else {
        return Vec::new();
    };
    let mut snapshots = store
        .map_status_snapshots
        .iter()
        .filter(|snapshot| snapshot.map_id == map_id)
        .cloned()
        .collect::<Vec<_>>();
    snapshots.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    snapshots
}

fn latest_status_snapshot_for_active_map(
    store: &VoxelSceneStore,
) -> Option<PersistedVoxelMapStatusSnapshot> {
    status_snapshots_for_active_map(store).into_iter().next()
}

fn selected_status_snapshot(
    store: &VoxelSceneStore,
    editor: &VoxelEditorState,
) -> Option<PersistedVoxelMapStatusSnapshot> {
    let selected_id = editor.selected_status_snapshot_id.as_deref()?;
    store
        .map_status_snapshots
        .iter()
        .find(|snapshot| snapshot.id == selected_id)
        .cloned()
}

fn status_snapshot_label(snapshot: &PersistedVoxelMapStatusSnapshot) -> String {
    format!(
        "{} - {}次编辑",
        snapshot.name,
        snapshot.edits.len()
    )
}

fn status_snapshot_name(map_name: &str, reason: &str, created_at: u64) -> String {
    format!(
        "{} {} @ {}",
        map_name.trim(),
        status_snapshot_reason_label(reason),
        created_at
    )
}

fn status_snapshot_reason_label(reason: &str) -> &str {
    match reason {
        "Manual" => "手动",
        "Auto turn" => "自动轮次",
        other => other,
    }
}

fn new_map_status_snapshot_id(snapshots: &[PersistedVoxelMapStatusSnapshot]) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let mut id = format!("status-{millis}");
    let mut suffix = 2;
    while snapshots.iter().any(|snapshot| snapshot.id == id) {
        id = format!("status-{millis}-{suffix}");
        suffix += 1;
    }
    id
}

fn prune_auto_map_status_snapshots(store: &mut VoxelSceneStore, map_id: &str) {
    let mut auto_snapshot_ids = store
        .map_status_snapshots
        .iter()
        .filter(|snapshot| snapshot.map_id == map_id && snapshot.reason == "Auto turn")
        .map(|snapshot| (snapshot.created_at, snapshot.id.clone()))
        .collect::<Vec<_>>();
    if auto_snapshot_ids.len() <= MAX_AUTO_MAP_STATUS_SNAPSHOTS_PER_MAP {
        return;
    }

    auto_snapshot_ids
        .sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let keep = auto_snapshot_ids
        .into_iter()
        .take(MAX_AUTO_MAP_STATUS_SNAPSHOTS_PER_MAP)
        .map(|(_, id)| id)
        .collect::<HashSet<_>>();
    store.map_status_snapshots.retain(|snapshot| {
        snapshot.map_id != map_id || snapshot.reason != "Auto turn" || keep.contains(&snapshot.id)
    });
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn auto_save_map_status_for_battle_turn(
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    mut last_turn_signature: Local<Option<u64>>,
    mut last_auto_map_signature: Local<Option<u64>>,
) {
    let Some(manager) = manager else {
        return;
    };
    let signature = trpg_group_turn_signature(&manager);
    if *last_turn_signature == Some(signature) {
        return;
    }

    let initialized = last_turn_signature.is_some();
    *last_turn_signature = Some(signature);
    if !initialized {
        if let Some(store) = store.as_deref_mut() {
            ensure_voxel_maps(store);
            *last_auto_map_signature = active_voxel_map_edit_signature(store);
        }
        return;
    }
    if !trpg_manager_has_started_turns(&manager) {
        return;
    }

    let Some(store) = store.as_deref_mut() else {
        return;
    };
    ensure_voxel_maps(store);
    let map_signature = active_voxel_map_edit_signature(store);
    if *last_auto_map_signature == map_signature {
        return;
    }
    *last_auto_map_signature = map_signature;

    save_active_map_status(store, "Auto turn", true);
}

fn trpg_manager_has_started_turns(manager: &NapcatMessageManager) -> bool {
    manager.trpg_groups.values().any(|group| {
        group.world_turn > 0
            || group
                .player_turns
                .values()
                .any(|turn| turn.turns_passed > 0 || turn.acted || turn.skipped)
    })
}

fn trpg_group_turn_signature(manager: &NapcatMessageManager) -> u64 {
    let mut hasher = DefaultHasher::new();
    manager.current_trpg_group.hash(&mut hasher);
    let mut group_names = manager.trpg_groups.keys().collect::<Vec<_>>();
    group_names.sort();
    for group_name in group_names {
        group_name.hash(&mut hasher);
        let group = &manager.trpg_groups[group_name];
        group.world_turn.hash(&mut hasher);
        for player_id in &group.players {
            player_id.hash(&mut hasher);
            if let Some(turn) = group.player_turns.get(player_id) {
                turn.turns_passed.hash(&mut hasher);
                turn.acted.hash(&mut hasher);
                turn.skipped.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

fn active_voxel_map_edit_signature(store: &VoxelSceneStore) -> Option<u64> {
    let map = active_voxel_map(store)?;
    let mut hasher = DefaultHasher::new();
    map.id.hash(&mut hasher);
    map.edits.len().hash(&mut hasher);
    if let Some(edit) = map.edits.last() {
        hash_persisted_voxel_edit(edit, &mut hasher);
    }
    Some(hasher.finish())
}

fn hash_persisted_voxel_edit(edit: &PersistedVoxelEdit, hasher: &mut DefaultHasher) {
    edit.position.hash(hasher);
    match edit.voxel {
        PersistedVoxel::Air => 0_u8.hash(hasher),
        PersistedVoxel::Solid(material) => {
            1_u8.hash(hasher);
            material.hash(hasher);
        },
    }
}

fn persist_voxel_store(store: &mut Persistent<VoxelSceneStore>, reason: &str) {
    if let Err(err) = store.persist() {
        eprintln!("failed to persist voxel {reason}: {err}");
    }
}

fn current_trpg_group_player_ids(manager: &Persistent<NapcatMessageManager>) -> Vec<u64> {
    manager
        .current_trpg_group
        .as_deref()
        .and_then(|group_name| manager.trpg_groups.get(group_name))
        .map(|group| {
            group
                .players
                .iter()
                .filter_map(|target_id| target_id.parse::<u64>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn scene_player_display_name(
    manager: Option<&Persistent<NapcatMessageManager>>,
    user_id: u64,
) -> String {
    let target_id = user_id.to_string();
    let Some(manager) = manager else {
        return target_id;
    };

    if let Some(character) = manager.player_characters.get(&target_id) {
        let nickname = character.nickname.trim();
        if !nickname.is_empty() {
            return format!("{nickname} ({target_id})");
        }
        let name = character.name.trim();
        if !name.is_empty() {
            return format!("{name} ({target_id})");
        }
    }

    if let Some(metadata) = manager.chat_targets.get(&target_id) {
        let display_name = metadata.display_name.trim();
        if !display_name.is_empty() {
            return format!("{display_name} ({target_id})");
        }
        let automatic_name = metadata.automatic_name.trim();
        if !automatic_name.is_empty() {
            return format!("{automatic_name} ({target_id})");
        }
    }

    if let Some(nickname) = manager
        .messages
        .get(&target_id)
        .and_then(|messages| latest_sender_nickname(messages, user_id))
    {
        return format!("{nickname} ({target_id})");
    }

    target_id
}

fn latest_sender_nickname(messages: &[NapcatMessage], user_id: u64) -> Option<&str> {
    messages.iter().rev().find_map(|message| {
        if message.data.sender.user_id != user_id {
            return None;
        }
        let nickname = message.data.sender.nickname.trim();
        (!nickname.is_empty()).then_some(nickname)
    })
}

fn capture_camera_panel(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut images: ResMut<Assets<Image>>,
    mut editor: ResMut<SceneCaptureEditorState>,
    mut player_cameras: ResMut<PlayerSceneCameras>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    mut free_camera: Query<
        &mut Transform,
        (
            With<FreeCamera>,
            Without<PlayerCaptureCamera>,
        ),
    >,
    mut capture_cameras: Query<(
        Entity,
        &mut Transform,
        &PlayerCaptureCamera,
    )>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let default_transform = free_camera
        .single_mut()
        .map(|transform| *transform)
        .unwrap_or_else(|_| default_capture_camera_transform());
    let mut created_group_camera = false;
    if let Some(manager) = manager.as_deref() {
        for user_id in current_trpg_group_player_ids(manager) {
            if player_cameras.cameras.contains_key(&user_id) {
                continue;
            }
            spawn_player_capture_camera(
                &mut commands,
                &mut images,
                &mut player_cameras,
                user_id,
                default_transform,
            );
            if let Some(store) = store.as_deref_mut() {
                upsert_persisted_capture_camera(store, user_id, &default_transform);
            }
            created_group_camera = true;
        }
    }
    if created_group_camera {
        if let Some(store) = store.as_deref_mut() {
            if let Err(err) = store.persist() {
                eprintln!("failed to persist current group capture cameras: {err}");
            }
        }
    }

    let camera_ids = capture_cameras
        .iter()
        .map(|(_, _, camera)| camera.user_id)
        .collect::<Vec<_>>();
    if editor
        .selected_user_id
        .is_none_or(|selected| !camera_ids.contains(&selected))
    {
        editor.selected_user_id = camera_ids.first().copied();
    }

    egui::Window::new("场景捕捉相机")
        .default_pos(egui::pos2(12.0, 270.0))
        .default_width(260.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.checkbox(&mut editor.show_gizmo, "显示控件");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut editor.new_user_id);
                if ui.button("创建").clicked() {
                    if let Ok(user_id) = editor.new_user_id.trim().parse::<u64>() {
                        if !player_cameras.cameras.contains_key(&user_id) {
                            let transform = free_camera
                                .single_mut()
                                .map(|transform| *transform)
                                .unwrap_or(default_transform);
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
                ui.label("还没有玩家捕捉相机");
                return;
            }

            let mut selected_user_id = editor.selected_user_id.unwrap_or(camera_ids[0]);
            let selected_text = scene_player_display_name(manager.as_deref(), selected_user_id);
            egui::ComboBox::from_label("玩家")
                .selected_text(selected_text)
                .show_ui(ui, |ui| {
                    for user_id in &camera_ids {
                        let label = scene_player_display_name(manager.as_deref(), *user_id);
                        ui.selectable_value(&mut selected_user_id, *user_id, label);
                    }
                });
            editor.selected_user_id = Some(selected_user_id);

            let Some((entity, mut transform, _)) = capture_cameras
                .iter_mut()
                .find(|(_, _, camera)| camera.user_id == selected_user_id)
            else {
                return;
            };
            let mut transform_changed = false;

            ui.horizontal(|ui| {
                if ui.button("使用当前视角").clicked() {
                    if let Ok(free_transform) = free_camera.single_mut() {
                        *transform = *free_transform;
                        transform_changed = true;
                    }
                }
                if ui.button("查看玩家视角").clicked() {
                    if let Ok(mut free_transform) = free_camera.single_mut() {
                        *free_transform = *transform;
                    }
                }
                if ui.button("重置").clicked() {
                    *transform = default_capture_camera_transform();
                    transform_changed = true;
                }
                if ui.button("删除").clicked() {
                    commands.entity(entity).despawn();
                    player_cameras.cameras.remove(&selected_user_id);
                    editor.selected_user_id = camera_ids
                        .iter()
                        .copied()
                        .find(|user_id| *user_id != selected_user_id);
                    if let Some(store) = store.as_deref_mut() {
                        remove_persisted_capture_camera(store, selected_user_id);
                        if let Err(err) = store.persist() {
                            eprintln!("failed to persist capture camera deletion: {err}");
                        }
                    }
                    return;
                }
            });

            ui.separator();
            ui.label("位移");
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
            ui.label("旋转");
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

fn apply_scene_player_view_request(
    mut request: ResMut<ScenePlayerViewRequest>,
    mut free_camera: Query<
        &mut Transform,
        (
            With<FreeCamera>,
            Without<PlayerCaptureCamera>,
        ),
    >,
    capture_cameras: Query<
        (&Transform, &PlayerCaptureCamera),
        (
            With<PlayerCaptureCamera>,
            Without<FreeCamera>,
        ),
    >,
) {
    let Some(user_id) = request.user_id.take() else {
        return;
    };
    let Ok(mut free_transform) = free_camera.single_mut() else {
        return;
    };
    let Some((capture_transform, _)) = capture_cameras
        .iter()
        .find(|(_, camera)| camera.user_id == user_id)
    else {
        return;
    };

    *free_transform = *capture_transform;
}

fn free_camera_system(
    egui_wants_input: Res<EguiWantsInput>,
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    editor: Res<VoxelEditorState>,
    mut pointer_state: ResMut<ScenePointerState>,
    mut cameras: Query<&mut Transform, With<FreeCamera>>,
) {
    let Ok(mut transform) = cameras.single_mut() else {
        return;
    };
    let wants_pointer_input = egui_wants_input.wants_pointer_input();
    let wants_keyboard_input = egui_wants_input.wants_any_keyboard_input();

    if mouse_buttons.just_pressed(MouseButton::Right) {
        pointer_state.right_started_over_ui = wants_pointer_input;
    }
    if mouse_buttons.just_released(MouseButton::Right) {
        pointer_state.right_started_over_ui = false;
    }

    let right_rotating =
        mouse_buttons.pressed(MouseButton::Right) && !pointer_state.right_started_over_ui;

    if right_rotating {
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
    let local_up = planet_outward_at(transform.translation);
    if keyboard.pressed(KeyCode::KeyE) || keyboard.pressed(KeyCode::Space) {
        direction += local_up;
    }
    if keyboard.pressed(KeyCode::KeyQ) || keyboard.pressed(KeyCode::ControlLeft) {
        direction -= local_up;
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

fn sync_voxel_planet_preview_visibility(
    active_voxel_cameras: Query<&Transform, With<VoxelWorldCamera<TrpgVoxelWorld>>>,
    mut previews: Query<&mut Visibility, With<VoxelPlanetPreview>>,
) {
    let Some(camera_transform) = active_voxel_cameras.iter().next() else {
        return;
    };
    let altitude = (camera_transform
        .translation
        .distance(earth_planet_center().as_vec3())
        - EARTH_PLANET_RADIUS as f32)
        .abs();
    let visibility = if altitude < VOXEL_PLANET_PREVIEW_HIDE_ALTITUDE {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };

    for mut preview_visibility in &mut previews {
        *preview_visibility = visibility;
    }
}

fn apply_planet_radial_gravity(
    time: Res<Time>,
    mut bodies: Query<(&Transform, &mut LinearVelocity), With<PlanetGravityBody>>,
) {
    for (transform, mut velocity) in &mut bodies {
        velocity.0 += planet_gravity_delta_velocity(transform.translation, time.delta_secs());
    }
}

fn voxel_edit_target(
    voxel_world: &VoxelWorld<'_, TrpgVoxelWorld>,
    ray: Ray3d,
    raycast_hit: Option<VoxelRaycastResult<u8>>,
) -> Option<VoxelEditTarget> {
    if let Some(hit) = raycast_hit {
        let position = hit.voxel_pos();
        let normal = hit.voxel_normal().unwrap_or_else(|| {
            if procedural_earth_voxel_planet(position).is_some() {
                planet_axis_normal(position)
            } else {
                ray_fallback_voxel_normal(*ray.direction)
            }
        });
        let material = match hit.voxel {
            WorldVoxel::Solid(material) => Some(material),
            WorldVoxel::Air | WorldVoxel::Unset => None,
        };
        return Some(VoxelEditTarget {
            position,
            normal,
            material,
        });
    }

    let target = procedural_planet_edit_target_from_ray(ray)?;
    match voxel_world.get_voxel(target.position) {
        WorldVoxel::Air => None,
        WorldVoxel::Solid(material) => Some(VoxelEditTarget {
            material: Some(material),
            ..target
        }),
        WorldVoxel::Unset => Some(target),
    }
}

fn ray_fallback_voxel_normal(direction: Vec3) -> IVec3 {
    let abs = direction.abs();
    if abs.x >= abs.y && abs.x >= abs.z {
        IVec3::new(
            if direction.x > 0.0 { -1 } else { 1 },
            0,
            0,
        )
    } else if abs.y >= abs.z {
        IVec3::new(
            0,
            if direction.y > 0.0 { -1 } else { 1 },
            0,
        )
    } else {
        IVec3::new(
            0,
            0,
            if direction.z > 0.0 { -1 } else { 1 },
        )
    }
}

fn physics_voxel_grab_drop_system(
    mut commands: Commands,
    egui_wants_input: Res<EguiWantsInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_info: Query<
        (&Camera, &GlobalTransform, &Transform),
        (
            With<VoxelWorldCamera<TrpgVoxelWorld>>,
            With<FreeCamera>,
            Without<HeldPhysicsVoxel>,
        ),
    >,
    mut held_voxels: Query<
        (
            Entity,
            &mut Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        (
            With<HeldPhysicsVoxel>,
            Without<FreeCamera>,
        ),
    >,
    mut grab_state: ResMut<PhysicsVoxelGrabState>,
    mut voxel_world: VoxelWorld<TrpgVoxelWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
) {
    let Ok((camera, camera_global_transform, camera_transform)) = camera_info.single() else {
        return;
    };
    let held_transform = held_physics_voxel_transform(camera_transform);

    if let Some(entity) = grab_state.held_entity {
        if let Ok((_, mut transform, mut linear_velocity, mut angular_velocity)) =
            held_voxels.get_mut(entity)
        {
            *transform = held_transform;
            linear_velocity.0 = Vec3::ZERO;
            angular_velocity.0 = Vec3::ZERO;
        } else {
            grab_state.held_entity = None;
        }
    }

    if !keyboard.just_pressed(KeyCode::KeyF) || egui_wants_input.wants_any_keyboard_input() {
        return;
    }

    if let Some(entity) = grab_state.held_entity {
        if let Ok((_, mut transform, mut linear_velocity, mut angular_velocity)) =
            held_voxels.get_mut(entity)
        {
            *transform = held_transform;
            linear_velocity.0 = *camera_transform.forward() * PHYSICS_VOXEL_DROP_SPEED;
            angular_velocity.0 = Vec3::ZERO;
            commands
                .entity(entity)
                .remove::<HeldPhysicsVoxel>()
                .insert((RigidBody::Dynamic, PlanetGravityBody));
        }
        grab_state.held_entity = None;
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(ray) = center_screen_ray(window, camera, camera_global_transform) else {
        return;
    };
    let raycast_hit = voxel_world.raycast(ray.clone(), &|(_, voxel)| {
        voxel.is_solid()
    });
    let Some(target) = voxel_edit_target(&voxel_world, ray, raycast_hit) else {
        return;
    };
    let Some(material) = target.material else {
        return;
    };

    voxel_world.set_voxel(target.position, WorldVoxel::Air);
    persist_single_voxel_edit(
        &mut store,
        target.position,
        PersistedVoxel::Air,
        "physics voxel grab",
    );

    let entity = commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::default())),
            MeshMaterial3d(materials.add(physics_voxel_material(material))),
            held_transform,
            RigidBody::Kinematic,
            Collider::cuboid(1.0, 1.0, 1.0),
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
            GravityScale(0.0),
            LinearDamping(0.18),
            PhysicsVoxel,
            HeldPhysicsVoxel,
        ))
        .id();
    grab_state.held_entity = Some(entity);
}

fn held_physics_voxel_transform(camera_transform: &Transform) -> Transform {
    let mut transform = Transform::from_translation(
        camera_transform.translation + *camera_transform.forward() * HELD_PHYSICS_VOXEL_DISTANCE,
    );
    transform.rotation = camera_transform.rotation;
    transform
}

fn center_screen_ray(
    window: &Window,
    camera: &Camera,
    camera_transform: &GlobalTransform,
) -> Option<Ray3d> {
    let center = Vec2::new(
        window.width() * 0.5,
        window.height() * 0.5,
    );
    camera.viewport_to_world(camera_transform, center).ok()
}

fn physics_voxel_material(material: u8) -> StandardMaterial {
    StandardMaterial {
        base_color: preview_material_color(material),
        emissive: preview_material_emissive(material).into(),
        perceptual_roughness: 0.78,
        metallic: match material {
            MAT_HULL_LIGHT | MAT_HULL_DARK | MAT_STATION_METAL | MAT_STATION_TRIM => 0.35,
            _ => 0.0,
        },
        unlit: matches!(
            material,
            MAT_STAR | MAT_WINDOW_CYAN | MAT_ENGINE_RED | MAT_SUN
        ),
        ..default()
    }
}

fn procedural_planet_edit_target_from_ray(ray: Ray3d) -> Option<VoxelEditTarget> {
    let center = earth_planet_center().as_vec3();
    let outer_radius =
        EARTH_PLANET_RADIUS as f32 + VOXEL_PLANET_MAX_ELEVATION + HELD_PHYSICS_VOXEL_DISTANCE;
    let (entry, exit) = ray_sphere_intersection_distances(&ray, center, outer_radius)?;
    if exit < 0.0 {
        return None;
    }

    let start = entry.max(0.0);
    let end = exit.min(start + VOXEL_PLANET_MAX_ELEVATION * 2.0 + 1024.0);
    if end < start {
        return None;
    }

    let mut last_position = None;
    let mut distance = start;
    while distance <= end {
        let position = ray.get_point(distance).floor().as_ivec3();
        if last_position != Some(position) {
            if let Some(material) = procedural_earth_voxel_planet(position) {
                return Some(VoxelEditTarget {
                    position,
                    normal: planet_axis_normal(position),
                    material: Some(material),
                });
            }
            last_position = Some(position);
        }
        distance += 1.0;
    }

    None
}

fn ray_sphere_intersection_distances(ray: &Ray3d, center: Vec3, radius: f32) -> Option<(f32, f32)> {
    let offset = ray.origin - center;
    let direction = *ray.direction;
    let half_b = offset.dot(direction);
    let c = offset.length_squared() - radius * radius;
    let discriminant = half_b * half_b - c;
    if discriminant < 0.0 {
        return None;
    }

    let root = discriminant.sqrt();
    Some((-half_b - root, -half_b + root))
}

fn planet_axis_normal(position: IVec3) -> IVec3 {
    let direction = (position.as_vec3() + Vec3::splat(0.5) - earth_planet_center().as_vec3())
        .try_normalize()
        .unwrap_or(Vec3::Y);
    dominant_axis_normal(direction)
}

fn dominant_axis_normal(direction: Vec3) -> IVec3 {
    let abs = direction.abs();
    if abs.x >= abs.y && abs.x >= abs.z {
        IVec3::new(axis_sign(direction.x), 0, 0)
    } else if abs.y >= abs.z {
        IVec3::new(0, axis_sign(direction.y), 0)
    } else {
        IVec3::new(0, 0, axis_sign(direction.z))
    }
}

fn axis_sign(value: f32) -> i32 {
    if value >= 0.0 {
        1
    } else {
        -1
    }
}

fn cleanup_orphaned_voxel_chunk_colliders(
    mut commands: Commands,
    mut chunks: Query<(Entity, &mut VoxelChunkTerrainCollider), Without<Mesh3d>>,
) {
    for (entity, mut terrain_collider) in &mut chunks {
        terrain_collider.frames_without_mesh =
            terrain_collider.frames_without_mesh.saturating_add(1);
        if terrain_collider.frames_without_mesh < ORPHANED_VOXEL_COLLIDER_FRAME_GRACE {
            continue;
        }
        commands.entity(entity).remove::<(
            RigidBody,
            Collider,
            VoxelChunkTerrainCollider,
        )>();
    }
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
    time: Res<Time>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_info: Query<
        (&Camera, &GlobalTransform),
        (
            With<VoxelWorldCamera<TrpgVoxelWorld>>,
            With<FreeCamera>,
            Without<PlayerCaptureCamera>,
        ),
    >,
    editor: Res<VoxelEditorState>,
    mut pointer_state: ResMut<ScenePointerState>,
    mut voxel_world: VoxelWorld<TrpgVoxelWorld>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
) {
    if mouse_buttons.just_pressed(MouseButton::Left) {
        pointer_state.left_started_over_ui = egui_wants_input.wants_pointer_input();
        pointer_state.last_edit_cursor_position = None;
        pointer_state.last_edit_position = None;
        pointer_state.stationary_edit_seconds = 0.0;
        pointer_state.shift_locked_edit_y = None;
    }
    if mouse_buttons.just_released(MouseButton::Left) {
        pointer_state.left_started_over_ui = false;
        pointer_state.last_edit_cursor_position = None;
        pointer_state.last_edit_position = None;
        pointer_state.stationary_edit_seconds = 0.0;
        pointer_state.shift_locked_edit_y = None;
    }
    if !editor.enabled || !mouse_buttons.pressed(MouseButton::Left) {
        return;
    }
    if pointer_state.left_started_over_ui {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let cursor_moved = pointer_state
        .last_edit_cursor_position
        .is_some_and(|last_position| last_position.distance(cursor_position) > 2.0);
    let repeat_due = if mouse_buttons.just_pressed(MouseButton::Left) || cursor_moved {
        pointer_state.stationary_edit_seconds = 0.0;
        true
    } else {
        pointer_state.stationary_edit_seconds += time.delta_secs();
        pointer_state.stationary_edit_seconds >= 0.25
    };
    pointer_state.last_edit_cursor_position = Some(cursor_position);
    if !repeat_due {
        return;
    }
    pointer_state.stationary_edit_seconds = 0.0;

    let Ok((camera, camera_transform)) = camera_info.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };
    let raycast_hit = voxel_world.raycast(ray.clone(), &|(_, voxel)| {
        voxel.is_solid()
    });
    let Some(target) = voxel_edit_target(&voxel_world, ray, raycast_hit) else {
        return;
    };

    let mut base_position = match editor.mode {
        VoxelEditMode::Add => target.position + target.normal,
        VoxelEditMode::Erase => target.position,
    };
    let shift_held = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if shift_held {
        let locked_y = *pointer_state
            .shift_locked_edit_y
            .get_or_insert(base_position.y);
        base_position.y = locked_y;
    } else {
        pointer_state.shift_locked_edit_y = None;
    }
    if pointer_state.last_edit_position == Some(base_position) {
        return;
    }
    pointer_state.last_edit_position = Some(base_position);

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
            ensure_voxel_maps(store);
            if let Some(map) = active_voxel_map_mut(store) {
                upsert_persisted_edit(
                    &mut map.edits,
                    position,
                    persisted_voxel,
                );
            }
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
    mut requests: ResMut<SceneCaptureRequests>,
    mut capture_state: ResMut<SceneCaptureState>,
    player_cameras: Res<PlayerSceneCameras>,
    mut capture_camera_query: Query<&mut Camera, With<PlayerCaptureCamera>>,
    voxel_camera_entities: Query<Entity, With<VoxelWorldCamera<TrpgVoxelWorld>>>,
) {
    let capture_requests = requests.requests.drain(..).collect::<Vec<_>>();
    for request in capture_requests {
        let Some(player_camera) = player_cameras.cameras.get(&request.user_id) else {
            eprintln!(
                "ignored scene capture request from {} without a configured capture camera",
                request.user_id
            );
            continue;
        };
        let player_camera = PlayerSceneCamera {
            entity: player_camera.entity,
            target: player_camera.target.clone(),
        };

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
            prepare_frames_remaining: SCENE_CAPTURE_PREPARE_FRAMES,
            started_preparing: false,
        });
    }

    let Some(current) = capture_state.pending_captures.first_mut() else {
        return;
    };

    if !current.started_preparing {
        if let Ok(mut camera) = capture_camera_query.get_mut(current.camera_entity) {
            camera.is_active = true;
        }
        set_single_voxel_world_camera(
            &mut commands,
            voxel_camera_entities.iter(),
            current.camera_entity,
        );
        current.started_preparing = true;
        return;
    }

    if current.prepare_frames_remaining > 0 {
        current.prepare_frames_remaining -= 1;
        return;
    }

    let pending = capture_state.pending_captures.remove(0);
    {
        commands
            .spawn(Screenshot::image(
                pending.target.clone(),
            ))
            .observe(
                move |screenshot: On<ScreenshotCaptured>,
                      mut commands: Commands,
                      napcat_sender: Option<Res<NapcatIOSender>>,
                      free_camera: Query<Entity, With<FreeCamera>>,
                      mut cameras: Query<&mut Camera, With<PlayerCaptureCamera>>| {
                    if let Ok(mut camera) = cameras.get_mut(pending.camera_entity) {
                        camera.is_active = false;
                    }
                    commands
                        .entity(pending.camera_entity)
                        .remove::<VoxelWorldCamera<TrpgVoxelWorld>>();
                    if let Ok(free_camera) = free_camera.single() {
                        commands
                            .entity(free_camera)
                            .try_insert(VoxelWorldCamera::<TrpgVoxelWorld>::default());
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
}

fn set_single_voxel_world_camera(
    commands: &mut Commands,
    voxel_camera_entities: impl Iterator<Item = Entity>,
    target: Entity,
) {
    for entity in voxel_camera_entities {
        if entity != target {
            commands
                .entity(entity)
                .remove::<VoxelWorldCamera<TrpgVoxelWorld>>();
        }
    }
    commands
        .entity(target)
        .try_insert(VoxelWorldCamera::<TrpgVoxelWorld>::default());
}

fn default_capture_camera_transform() -> Transform {
    Transform::from_xyz(24.0, 18.0, 32.0).looking_at(Vec3::new(0.0, 8.0, 0.0), Vec3::Y)
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
            scene_camera_fog(),
            transform,
            RenderLayers::layer(0),
            PlayerCaptureCamera { user_id },
        ))
        .id();
    player_cameras.cameras.insert(user_id, PlayerSceneCamera {
        entity,
        target: target.clone(),
    });
    PlayerSceneCamera { entity, target }
}

fn sync_character_standees(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut standees: ResMut<CharacterStandeeAssets>,
    mut player_cameras: ResMut<PlayerSceneCameras>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    existing: Query<(Entity, &CharacterStandee)>,
    capture_cameras: Query<(&Transform, &PlayerCaptureCamera), Without<CharacterStandee>>,
    mut standee_transforms: Query<
        &mut Transform,
        (
            With<CharacterStandee>,
            Without<PlayerCaptureCamera>,
        ),
    >,
) {
    let Some(manager) = manager else {
        return;
    };
    let Some(store) = store.as_deref_mut() else {
        return;
    };

    standees.entities.clear();
    for (entity, standee) in &existing {
        standees.entities.insert(standee.target_id.clone(), entity);
    }

    let active_targets = manager
        .player_characters
        .iter()
        .filter_map(|(target_id, character)| {
            let image_source = character.image.trim();
            (character.inited && !image_source.is_empty()).then(|| {
                (
                    target_id.clone(),
                    image_source.to_owned(),
                )
            })
        })
        .collect::<Vec<_>>();
    let active_ids = active_targets
        .iter()
        .map(|(target_id, _)| target_id.as_str())
        .collect::<HashSet<_>>();
    let capture_camera_transforms = capture_cameras
        .iter()
        .map(|(transform, camera)| (camera.user_id, *transform))
        .collect::<HashMap<_, _>>();

    let mut changed = false;
    for (entity, standee) in &existing {
        if active_ids.contains(standee.target_id.as_str()) {
            continue;
        }
        commands.entity(entity).despawn();
        standees.entities.remove(&standee.target_id);
        changed |= remove_persisted_character_standee(store, &standee.target_id);
    }

    for (target_id, image_source) in active_targets {
        let standee_index = store.character_standees.len();
        let persisted = persisted_character_standee(store, &target_id)
            .cloned()
            .unwrap_or_else(|| {
                let standee = default_character_standee(&target_id, &image_source);
                store.character_standees.push(standee.clone());
                changed = true;
                standee
            });
        let transform = standee_camera_transform(
            &target_id,
            standee_index,
            &mut commands,
            &mut images,
            &mut player_cameras,
            store,
            &capture_camera_transforms,
            &mut changed,
        )
        .unwrap_or_else(|| persisted_standee_transform(&persisted));

        if persisted.image_source != image_source {
            upsert_persisted_character_standee(store, PersistedCharacterStandee {
                translation: transform.translation.to_array(),
                rotation: transform.rotation.to_array(),
                image_source: image_source.clone(),
                ..persisted.clone()
            });
            standees.failed_sources.remove(&persisted.image_source);
            changed = true;
        }

        if let Some(entity) = standees.entities.get(&target_id).copied() {
            if let Ok((_, existing_standee)) = existing.get(entity) {
                if existing_standee.image_source == image_source {
                    if let Ok(mut standee_transform) = standee_transforms.get_mut(entity) {
                        *standee_transform = transform;
                    }
                    continue;
                }
            }
            commands.entity(entity).despawn();
            standees.entities.remove(&target_id);
        }

        if standees.failed_sources.contains(&image_source) {
            continue;
        }

        match load_character_standee_texture(
            &image_source,
            &mut images,
            &mut standees.textures,
        ) {
            Ok((texture, size)) => {
                let height = 2.4;
                let width = (size.x / size.y.max(1.0) * height).clamp(0.6, 3.6);
                let entity = commands
                    .spawn((
                        Mesh3d(
                            meshes.add(
                                Plane3d::new(
                                    Vec3::Z,
                                    Vec2::new(width * 0.5, height * 0.5),
                                )
                                .mesh(),
                            ),
                        ),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: Color::WHITE,
                            base_color_texture: Some(texture),
                            alpha_mode: AlphaMode::Blend,
                            cull_mode: None,
                            unlit: true,
                            ..default()
                        })),
                        transform,
                        CharacterStandee {
                            target_id: target_id.clone(),
                            image_source,
                        },
                    ))
                    .id();
                standees.entities.insert(target_id, entity);
            },
            Err(err) => {
                standees.failed_sources.insert(image_source.clone());
                eprintln!("failed to load character standee image for {target_id}: {err}");
            },
        }
    }

    if changed {
        if let Err(err) = store.persist() {
            eprintln!("failed to persist character standees: {err}");
        }
    }
}

fn sync_scene_character_positions(
    mut positions: ResMut<SceneCharacterPositions>,
    characters: Query<(&CharacterStandee, &GlobalTransform)>,
) {
    positions.positions.clear();
    for (character, transform) in &characters {
        positions.positions.insert(
            character.target_id.clone(),
            transform.translation(),
        );
    }
}

fn sync_scene_player_camera_positions(
    mut positions: ResMut<ScenePlayerCameraPositions>,
    cameras: Query<(&PlayerCaptureCamera, &GlobalTransform), Without<CharacterStandee>>,
) {
    positions.positions.clear();
    for (camera, transform) in &cameras {
        positions
            .positions
            .insert(camera.user_id, transform.translation());
    }
}

fn persisted_camera_transform(camera: &PersistedCaptureCamera) -> Transform {
    Transform {
        translation: Vec3::from(camera.translation),
        rotation: Quat::from_array(camera.rotation),
        scale: Vec3::ONE,
    }
}

fn persisted_standee_transform(standee: &PersistedCharacterStandee) -> Transform {
    Transform {
        translation: Vec3::from(standee.translation),
        rotation: Quat::from_array(standee.rotation),
        scale: Vec3::ONE,
    }
}

fn default_character_standee(target_id: &str, image_source: &str) -> PersistedCharacterStandee {
    let transform = default_capture_camera_transform();
    PersistedCharacterStandee {
        target_id: target_id.to_owned(),
        image_source: image_source.to_owned(),
        translation: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
    }
}

fn standee_camera_transform(
    target_id: &str,
    index: usize,
    commands: &mut Commands,
    images: &mut Assets<Image>,
    player_cameras: &mut PlayerSceneCameras,
    store: &mut Persistent<VoxelSceneStore>,
    capture_camera_transforms: &HashMap<u64, Transform>,
    changed: &mut bool,
) -> Option<Transform> {
    let user_id = target_id.parse::<u64>().ok()?;
    if let Some(transform) = capture_camera_transforms.get(&user_id) {
        return Some(*transform);
    }

    let transform = persisted_capture_camera(store, user_id)
        .map(persisted_camera_transform)
        .unwrap_or_else(|| default_character_camera_transform(index));
    if !player_cameras.cameras.contains_key(&user_id) {
        spawn_player_capture_camera(
            commands,
            images,
            player_cameras,
            user_id,
            transform,
        );
    }
    if persisted_capture_camera(store, user_id).is_none() {
        upsert_persisted_capture_camera(store, user_id, &transform);
        *changed = true;
    }

    Some(transform)
}

fn default_character_camera_transform(index: usize) -> Transform {
    let row = index / 6;
    let column = index % 6;
    Transform::from_xyz(
        -7.5 + column as f32 * 3.0,
        1.2,
        -3.0 - row as f32 * 1.2,
    )
    .looking_at(Vec3::new(0.0, 1.2, -3.0), Vec3::Y)
}

fn persisted_capture_camera(
    store: &Persistent<VoxelSceneStore>,
    user_id: u64,
) -> Option<&PersistedCaptureCamera> {
    store
        .capture_cameras
        .iter()
        .find(|camera| camera.user_id == user_id)
}

fn persisted_character_standee<'a>(
    store: &'a Persistent<VoxelSceneStore>,
    target_id: &str,
) -> Option<&'a PersistedCharacterStandee> {
    store
        .character_standees
        .iter()
        .find(|standee| standee.target_id == target_id)
}

fn upsert_persisted_character_standee(
    store: &mut Persistent<VoxelSceneStore>,
    standee: PersistedCharacterStandee,
) {
    if let Some(existing) = store
        .character_standees
        .iter_mut()
        .find(|existing| existing.target_id == standee.target_id)
    {
        *existing = standee;
    } else {
        store.character_standees.push(standee);
    }
}

fn remove_persisted_character_standee(
    store: &mut Persistent<VoxelSceneStore>,
    target_id: &str,
) -> bool {
    let len = store.character_standees.len();
    store
        .character_standees
        .retain(|standee| standee.target_id != target_id);
    len != store.character_standees.len()
}

fn load_character_standee_texture(
    source: &str,
    images: &mut Assets<Image>,
    cache: &mut HashMap<String, Handle<Image>>,
) -> Result<(Handle<Image>, Vec2), String> {
    if let Some(texture) = cache.get(source) {
        let Some(image) = images.get(texture) else {
            return Err("cached texture handle no longer exists".to_owned());
        };
        return Ok((
            texture.clone(),
            Vec2::new(
                image.texture_descriptor.size.width as f32,
                image.texture_descriptor.size.height as f32,
            ),
        ));
    }

    let path = cached_or_local_image_path(source)?;
    let bytes = fs::read(&path).map_err(|err| err.to_string())?;
    let decoded = image::load_from_memory(&bytes)
        .map_err(|err| err.to_string())?
        .to_rgba8();
    let size = Extent3d {
        width: decoded.width(),
        height: decoded.height(),
        depth_or_array_layers: 1,
    };
    let mut image = Image::new(
        size,
        TextureDimension::D2,
        decoded.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    let texture = images.add(image);
    cache.insert(source.to_owned(), texture.clone());
    Ok((
        texture,
        Vec2::new(size.width as f32, size.height as f32),
    ))
}

fn cached_or_local_image_path(source: &str) -> Result<PathBuf, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("empty image source".to_owned());
    }

    if source.starts_with("http://") || source.starts_with("https://") {
        return cache_remote_standee_image(source);
    }

    if let Ok(url) = url::Url::parse(source) {
        if url.scheme() == "file" {
            return url
                .to_file_path()
                .map_err(|_| format!("file uri is not a local path: {source}"));
        }
    }

    let path = PathBuf::from(source);
    if path.exists() {
        return Ok(path);
    }

    Err(format!(
        "image source is not a local path or URL: {source}"
    ))
}

fn cache_remote_standee_image(url: &str) -> Result<PathBuf, String> {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let cache_name = format!("{:016x}", hasher.finish());
    for cache_dir in [
        Path::new(".data")
            .join("willowblossom")
            .join("character_standees"),
        Path::new(".data").join("willowblossom").join("image_cache"),
    ] {
        let base_path = cache_dir.join(&cache_name);
        for extension in ["png", "jpg", "jpeg", "webp", "bmp"] {
            let path = base_path.with_extension(extension);
            if path.exists() {
                return Ok(path);
            }
        }
    }

    let cache_dir = Path::new(".data")
        .join("willowblossom")
        .join("character_standees");
    fs::create_dir_all(&cache_dir).map_err(|err| err.to_string())?;
    let base_path = cache_dir.join(cache_name);

    let response = reqwest::blocking::get(url).map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let bytes = response.bytes().map_err(|err| err.to_string())?;
    let format = image::guess_format(&bytes).map_err(|err| err.to_string())?;
    let extension = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Bmp => "bmp",
        _ => "img",
    };
    let path = base_path.with_extension(extension);
    fs::write(&path, &bytes).map_err(|err| err.to_string())?;
    Ok(path)
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
    store: Option<Res<Persistent<VoxelSceneStore>>>,
    mut runtime: ResMut<VoxelMapRuntimeState>,
    mut voxel_world: VoxelWorld<TrpgVoxelWorld>,
) {
    let Some(store) = store else {
        return;
    };
    let active_map_id = store.active_map_id.clone();
    let pending_matches_active = runtime.pending_map_id == active_map_id;
    let should_start_load = runtime.reload_requested
        || runtime
            .pending_map_id
            .as_ref()
            .is_some_and(|_| !pending_matches_active)
        || (runtime.pending_map_id.is_none() && runtime.applied_map_id != active_map_id);
    if should_start_load {
        runtime.pending_clear_edits = runtime.applied_edits.clone();
        runtime.pending_apply_edits = active_voxel_map(&store)
            .map(|map| map.edits.clone())
            .unwrap_or_default();
        runtime.pending_map_id = active_map_id.clone();
        runtime.clear_cursor = 0;
        runtime.apply_cursor = 0;
        runtime.reload_requested = false;
    } else if runtime.pending_map_id.is_none() {
        return;
    }

    let mut budget = VOXEL_MAP_APPLY_BUDGET_PER_FRAME;
    while runtime.clear_cursor < runtime.pending_clear_edits.len() && budget > 0 {
        let edit = &runtime.pending_clear_edits[runtime.clear_cursor];
        let position = IVec3::new(
            edit.position[0],
            edit.position[1],
            edit.position[2],
        );
        voxel_world.set_voxel(
            position,
            starter_scene_voxel(position, None),
        );
        runtime.clear_cursor += 1;
        budget -= 1;
    }

    while runtime.clear_cursor >= runtime.pending_clear_edits.len()
        && runtime.apply_cursor < runtime.pending_apply_edits.len()
        && budget > 0
    {
        let edit = &runtime.pending_apply_edits[runtime.apply_cursor];
        let position = IVec3::new(
            edit.position[0],
            edit.position[1],
            edit.position[2],
        );
        voxel_world.set_voxel(position, edit.voxel.into());
        runtime.apply_cursor += 1;
        budget -= 1;
    }

    if runtime.clear_cursor >= runtime.pending_clear_edits.len()
        && runtime.apply_cursor >= runtime.pending_apply_edits.len()
    {
        runtime.applied_edits = std::mem::take(&mut runtime.pending_apply_edits);
        runtime.applied_map_id = runtime.pending_map_id.take();
        runtime.pending_clear_edits.clear();
        runtime.clear_cursor = 0;
        runtime.apply_cursor = 0;
    }
}

fn brush_positions(center: IVec3, radius: i32) -> impl Iterator<Item = IVec3> {
    let radius = radius.max(0);
    (-radius..=radius).flat_map(move |x| {
        (-radius..=radius)
            .flat_map(move |y| (-radius..=radius).map(move |z| center + IVec3::new(x, y, z)))
    })
}

fn upsert_persisted_edit(
    edits: &mut Vec<PersistedVoxelEdit>,
    position: IVec3,
    voxel: PersistedVoxel,
) {
    let position = [position.x, position.y, position.z];
    if let Some(edit) = edits.iter_mut().find(|edit| edit.position == position) {
        edit.voxel = voxel;
    } else {
        edits.push(PersistedVoxelEdit { position, voxel });
    }
}

fn persist_single_voxel_edit(
    store: &mut Option<ResMut<Persistent<VoxelSceneStore>>>,
    position: IVec3,
    voxel: PersistedVoxel,
    reason: &str,
) {
    if let Some(store) = store.as_deref_mut() {
        ensure_voxel_maps(store);
        if let Some(map) = active_voxel_map_mut(store) {
            upsert_persisted_edit(&mut map.edits, position, voxel);
        }
        persist_voxel_store(store, reason);
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

fn remove_persisted_capture_camera(store: &mut Persistent<VoxelSceneStore>, user_id: u64) {
    store
        .capture_cameras
        .retain(|camera| camera.user_id != user_id);
}

impl From<PersistedVoxel> for WorldVoxel<u8> {
    fn from(value: PersistedVoxel) -> Self {
        match value {
            PersistedVoxel::Air => WorldVoxel::Air,
            PersistedVoxel::Solid(material) => WorldVoxel::Solid(material),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voxel_planet_is_solid_below_surface_and_empty_above_at_landing_direction() {
        let center = earth_planet_center();
        let outward = (earth_planet_near_point() - center)
            .as_vec3()
            .normalize_or_zero();
        let solid_position = (center.as_vec3()
            + outward * (EARTH_PLANET_RADIUS as f32 - VOXEL_PLANET_MAX_ELEVATION - 8.0))
            .round()
            .as_ivec3();
        let empty_position = (center.as_vec3()
            + outward * (EARTH_PLANET_RADIUS as f32 + VOXEL_PLANET_MAX_ELEVATION + 32.0))
            .round()
            .as_ivec3();

        assert!(matches!(
            procedural_earth_voxel_planet(solid_position),
            Some(MAT_PLANET_LAND | MAT_PLANET_OCEAN)
        ));
        assert_eq!(
            procedural_earth_voxel_planet(empty_position),
            None
        );
    }

    #[test]
    fn starter_scene_lookup_includes_voxel_planet() {
        let center = earth_planet_center();
        let outward = (earth_planet_near_point() - center)
            .as_vec3()
            .normalize_or_zero();
        let solid_position = (center.as_vec3()
            + outward * (EARTH_PLANET_RADIUS as f32 - VOXEL_PLANET_MAX_ELEVATION - 8.0))
            .round()
            .as_ivec3();

        assert!(starter_scene_voxel(solid_position, None).is_solid());
    }

    #[test]
    fn voxel_planet_radius_is_large_enough_for_playable_horizon() {
        assert!(EARTH_PLANET_RADIUS >= 9_000);
        assert!(VOXEL_PLANET_PREVIEW_STEP >= 256);
    }

    #[test]
    fn procedural_planet_edit_target_hits_surface_without_streamed_chunks() {
        let waypoint = planet_surface_waypoint();
        let direction = Dir3::new(waypoint.focus - waypoint.eye).unwrap();
        let ray = Ray3d::new(waypoint.eye, direction);
        let target = procedural_planet_edit_target_from_ray(ray)
            .expect("planet surface should be targetable analytically");

        assert!(procedural_earth_voxel_planet(target.position).is_some());
        assert!(target.material.is_some());
        assert_ne!(target.normal, IVec3::ZERO);
    }

    #[test]
    fn planet_gravity_points_to_planet_center_instead_of_world_down() {
        let center = earth_planet_center().as_vec3();
        let position = center + Vec3::X * 256.0;

        let direction = planet_gravity_direction_at(position);
        let delta_velocity = planet_gravity_delta_velocity(position, 0.5);

        assert!(direction.abs_diff_eq(Vec3::NEG_X, 0.0001));
        assert!(delta_velocity.abs_diff_eq(
            Vec3::NEG_X * PLANET_GRAVITY_ACCELERATION * 0.5,
            0.0001,
        ));
        assert!(!direction.abs_diff_eq(Vec3::NEG_Y, 0.0001));
    }

    #[test]
    fn planet_gravity_is_zero_at_exact_center() {
        let center = earth_planet_center().as_vec3();

        assert_eq!(
            planet_gravity_direction_at(center),
            Vec3::ZERO
        );
        assert_eq!(
            planet_gravity_delta_velocity(center, 1.0),
            Vec3::ZERO
        );
    }
}
