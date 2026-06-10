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
    SpatialQuery,
    SpatialQueryFilter,
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
        PlayerAccess,
    },
};

pub struct ScenePreviewPlugin;

const SCENE_GIZMO_RENDER_LAYER: usize = 1;
const SPACE_HIFI_MAP_ID: &str = "orbital-forge-v1";
const SPACE_HIFI_MAP_NAME: &str = "轨道熔炉科幻场";
const VOXEL_TEXTURE_LAYERS: u32 = 12;
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
const VOXEL_PLANET_MAX_ELEVATION: f32 = 260.0;
const VOXEL_PLANET_PREVIEW_BLOCK: i32 = 128;
const VOXEL_PLANET_PREVIEW_FACE_STEPS: i32 = 128;
const VOXEL_PLANET_DETAIL_PREVIEW_BLOCK: i32 = 16;
const VOXEL_PLANET_DETAIL_PREVIEW_RADIUS: i32 = 512;
const VOXEL_PLANET_PREVIEW_HIDE_ALTITUDE: f32 = 1400.0;
const VOXEL_PLANET_CITY_RADIUS: f32 = 2400.0;
const VOXEL_PLANET_CITY_CELL: f32 = 360.0;
const VOXEL_PLANET_LAKE_DEPTH: f32 = 58.0;
const PLANET_GRAVITY_ACCELERATION: f32 = 28.0;
const PLANET_PHYSICS_PROBE_RADIUS: f32 = 1.2;
const HELD_PHYSICS_VOXEL_DISTANCE: f32 = 6.0;
const HELD_BATTLE_SPACESHIP_DISTANCE: f32 = 180.0;
const PHYSICS_VOXEL_DROP_SPEED: f32 = 4.0;
const PHYSICS_VOXEL_GRAB_DEBOUNCE_SECONDS: f32 = 0.18;
const PHYSICS_VOXEL_GRAB_MAX_DISTANCE: f32 = 80.0;
const BATTLE_SPACESHIP_GRAB_MAX_DISTANCE: f32 = 1_500.0;
const PICKUP_INDICATOR_COLOR: Color = Color::srgb(0.15, 0.45, 1.0);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoxelEditMode {
    Add,
    Erase,
    Paint,
    Pick,
    BoxFill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoxelBrushShape {
    Single,
    Cube,
    Sphere,
    Plane,
}

#[derive(Resource)]
struct VoxelEditorState {
    enabled: bool,
    mode: VoxelEditMode,
    material: u8,
    brush_radius: i32,
    brush_shape: VoxelBrushShape,
    camera_speed: f32,
    camera_speed_dirty: bool,
    mouse_sensitivity: f32,
    new_map_name: String,
    rename_map_name: String,
    selected_map_id: Option<String>,
    selected_status_snapshot_id: Option<String>,
    box_anchor: Option<IVec3>,
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
            brush_shape: VoxelBrushShape::Single,
            camera_speed: DEFAULT_CAMERA_SPEED,
            camera_speed_dirty: false,
            mouse_sensitivity: 0.003,
            new_map_name: "新地图".to_owned(),
            rename_map_name: String::new(),
            selected_map_id: None,
            selected_status_snapshot_id: None,
            box_anchor: None,
        }
    }
}

#[derive(Resource, Default)]
pub struct VoxelMapRuntimeState {
    applied_map_id: Option<String>,
    applied_index: HashMap<IVec3, PersistedVoxelState>,
    reload_requested: bool,
    pending_map_id: Option<String>,
    pending_changes: Vec<(IVec3, WorldVoxel<u8>)>,
    apply_cursor: usize,
    edit_index_map_id: Option<String>,
    edit_index: HashMap<IVec3, PersistedVoxelState>,
    undo_stack: Vec<VoxelEditStroke>,
    redo_stack: Vec<VoxelEditStroke>,
    save_requested: bool,
    save_debounce_seconds: f32,
}

impl VoxelMapRuntimeState {
    pub fn request_reload(&mut self) { self.reload_requested = true; }
}

#[derive(Clone)]
struct VoxelEditStroke {
    changes: Vec<VoxelEditChange>,
}

#[derive(Clone)]
struct VoxelEditChange {
    position: IVec3,
    before: Option<PersistedVoxel>,
    after: Option<PersistedVoxel>,
}

#[derive(Resource, Default)]
struct PhysicsVoxelGrabState {
    held_entity: Option<Entity>,
    debounce_seconds: f32,
}

#[derive(Resource, Default)]
struct BattleSpaceshipGrabState {
    held: bool,
    grab_local_offset: Vec3,
}

#[derive(Component)]
struct BattleSpaceshipPreviewRoot;

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
}

#[derive(Component)]
struct SpaceHiFiVoxelPreview;

#[derive(Component)]
struct VoxelPlanetPreview;

#[derive(Component)]
struct VoxelPlanetFarPreview;

#[derive(Component)]
struct VoxelPlanetDetailPreview;

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
    pub restore_gm_view: bool,
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
    voxel_view_changes: Vec<SceneCaptureVoxelViewChange>,
}

#[derive(Resource, Default)]
struct ScenePlayerVoxelViewState {
    active_user_id: Option<u64>,
    applied_signature: Option<u64>,
    applied_changes: Vec<SceneCaptureVoxelViewChange>,
}

#[derive(Clone)]
struct SceneCaptureVoxelViewChange {
    position: IVec3,
    capture_voxel: WorldVoxel<u8>,
    restore_voxel: WorldVoxel<u8>,
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
pub struct VoxelSceneStore {
    #[serde(default = "default_camera_speed")]
    editor_camera_speed: f32,
    #[serde(default)]
    battle_spaceship_translation: [f32; 3],
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
            battle_spaceship_translation: [0.0; 3],
            active_map_id: None,
            maps: Vec::new(),
            map_status_snapshots: Vec::new(),
            edits: Vec::new(),
            capture_cameras: Vec::new(),
            character_standees: Vec::new(),
        }
    }
}

pub const VOXEL_SCENE_EXPORT_VERSION: u32 = 1;

#[derive(Serialize)]
struct VoxelSceneStoreExportRef<'a> {
    version: u32,
    export_type: String,
    store: &'a VoxelSceneStore,
}

#[derive(Deserialize)]
struct VoxelSceneStoreExportOwned {
    version: u32,
    export_type: String,
    store: VoxelSceneStore,
}

impl VoxelSceneStore {
    pub fn to_export_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&VoxelSceneStoreExportRef {
            version: VOXEL_SCENE_EXPORT_VERSION,
            export_type: "voxel_scene".to_owned(),
            store: self,
        })
        .map_err(|err| err.to_string())
    }

    pub fn merge_export_json(&mut self, text: &str) -> Result<usize, String> {
        let export: VoxelSceneStoreExportOwned =
            serde_json::from_str(text).map_err(|err| err.to_string())?;
        if export.version != VOXEL_SCENE_EXPORT_VERSION {
            return Err(format!(
                "unsupported voxel scene export version {}; expected {}",
                export.version, VOXEL_SCENE_EXPORT_VERSION
            ));
        }
        if export.export_type != "voxel_scene" {
            return Err(format!(
                "unsupported voxel scene export type {}",
                export.export_type
            ));
        }

        let imported = export.store;
        self.editor_camera_speed = imported.editor_camera_speed;
        self.battle_spaceship_translation = imported.battle_spaceship_translation;
        self.edits = imported.edits;

        let mut imported_map_ids = HashSet::new();
        for map in imported.maps {
            let id = map.id.trim();
            if id.is_empty() {
                return Err("voxel scene export contains an empty map id".to_owned());
            }
            imported_map_ids.insert(id.to_owned());
            upsert_by(&mut self.maps, map, |map| {
                map.id.clone()
            });
        }

        for snapshot in imported.map_status_snapshots {
            if snapshot.id.trim().is_empty() {
                return Err("voxel scene export contains an empty status id".to_owned());
            }
            if snapshot.map_id.trim().is_empty() {
                return Err("voxel scene export contains an empty status map id".to_owned());
            }
            upsert_by(
                &mut self.map_status_snapshots,
                snapshot,
                |snapshot| snapshot.id.clone(),
            );
        }

        for camera in imported.capture_cameras {
            upsert_by(
                &mut self.capture_cameras,
                camera,
                |camera| camera.user_id,
            );
        }

        for standee in imported.character_standees {
            if standee.target_id.trim().is_empty() {
                return Err("voxel scene export contains an empty standee target id".to_owned());
            }
            upsert_by(
                &mut self.character_standees,
                standee,
                |standee| standee.target_id.clone(),
            );
        }

        if imported
            .active_map_id
            .as_deref()
            .is_some_and(|active_id| self.maps.iter().any(|map| map.id == active_id))
        {
            self.active_map_id = imported.active_map_id;
        }

        Ok(imported_map_ids.len())
    }
}

fn upsert_by<T, K: PartialEq>(items: &mut Vec<T>, item: T, key: impl Fn(&T) -> K) {
    let item_key = key(&item);
    if let Some(existing) = items.iter_mut().find(|existing| key(existing) == item_key) {
        *existing = item;
    } else {
        items.push(item);
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct PersistedVoxelEdit {
    position: [i32; 3],
    voxel: PersistedVoxel,
    #[serde(default)]
    visibility: SceneVisibility,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
enum PersistedVoxel {
    Air,
    Solid(u8),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
enum SceneVisibility {
    Public,
    Party(String),
    Player(u64),
    Gm,
}

impl Default for SceneVisibility {
    fn default() -> Self { Self::Public }
}

impl SceneVisibility {
    fn can_read(&self, player_id: u64, party_id: Option<&str>, is_gm: bool) -> bool {
        if is_gm {
            return true;
        }

        match self {
            SceneVisibility::Public => true,
            SceneVisibility::Party(visible_party_id) => party_id == Some(visible_party_id.as_str()),
            SceneVisibility::Player(visible_player_id) => player_id == *visible_player_id,
            SceneVisibility::Gm => false,
        }
    }

    fn can_read_for_access(&self, access: &PlayerAccess) -> bool {
        self.can_read(
            access.player_id,
            access.party_id.as_deref(),
            access.is_gm,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PersistedVoxelState {
    voxel: PersistedVoxel,
    visibility: SceneVisibility,
}

impl PersistedVoxelState {
    #[cfg(test)]
    fn public(voxel: PersistedVoxel) -> Self {
        Self {
            voxel,
            visibility: SceneVisibility::Public,
        }
    }
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
    type ChunkUserBundle = ();
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
            .init_resource::<ScenePlayerVoxelViewState>()
            .init_resource::<ScenePointerState>()
            .init_resource::<CharacterStandeeAssets>()
            .init_resource::<VoxelMapRuntimeState>()
            .init_resource::<PhysicsVoxelGrabState>()
            .init_resource::<BattleSpaceshipGrabState>()
            .init_resource::<SceneWaypointState>()
            .add_systems(Startup, setup_scene_preview)
            .add_systems(
                Update,
                (
                    draw_capture_camera_gizmos,
                    draw_pickup_indicator_gizmo,
                    draw_voxel_edit_preview_gizmo,
                    sync_character_standees,
                ),
            )
            .add_systems(
                Update,
                (
                    apply_saved_voxel_edits,
                    maintain_scene_player_voxel_view,
                    scene_capture_request_system,
                    flush_voxel_edit_save_requests,
                )
                    .chain(),
            )
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
    let _ = position;
    WorldVoxel::Air
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

    let battle_spaceship_translation = Vec3::from(voxel_scene_store.battle_spaceship_translation);
    commands.insert_resource(voxel_scene_store);
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.46, 0.56, 0.68),
        brightness: 28.0,
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
        battle_spaceship_translation,
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
            clear_color: ClearColorConfig::Custom(Color::srgb(0.12, 0.14, 0.16)),
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
    battle_spaceship_translation: Vec3,
) {
    let voxels = space_hifi_decor_voxel_edits()
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
        let static_voxels = voxels
            .iter()
            .filter_map(|(&position, &voxel_material)| {
                battle_spaceship_preview_origin(position)
                    .is_none()
                    .then_some((position, voxel_material))
            })
            .collect::<HashMap<_, _>>();
        let mesh = build_voxel_preview_mesh(&static_voxels, material);
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

    let has_battle_spaceship = voxels
        .keys()
        .any(|&position| battle_spaceship_preview_origin(position).is_some());
    if has_battle_spaceship {
        commands
            .spawn((
                Transform::from_translation(battle_spaceship_translation),
                Visibility::Visible,
                BattleSpaceshipPreviewRoot,
                SpaceHiFiVoxelPreview,
            ))
            .with_children(|parent| {
                for material in MAT_STAR..=MAT_PLANET_LAND {
                    let ship_voxels = voxels
                        .iter()
                        .filter_map(|(&position, &voxel_material)| {
                            (voxel_material == material
                                && battle_spaceship_preview_origin(position).is_some())
                            .then_some((position, voxel_material))
                        })
                        .collect::<HashMap<_, _>>();
                    let mesh = build_voxel_preview_mesh(&ship_voxels, material);
                    if mesh.count_vertices() == 0 {
                        continue;
                    }
                    parent.spawn((
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: preview_material_color(material),
                            emissive: preview_material_emissive(material).into(),
                            perceptual_roughness: 0.82,
                            metallic: match material {
                                MAT_HULL_LIGHT | MAT_HULL_DARK | MAT_STATION_METAL
                                | MAT_STATION_TRIM => 0.35,
                                _ => 0.0,
                            },
                            unlit: matches!(
                                material,
                                MAT_STAR | MAT_WINDOW_CYAN | MAT_ENGINE_RED | MAT_SUN
                            ),
                            ..default()
                        })),
                        Transform::default(),
                        Visibility::Visible,
                    ));
                }

                for z in (-34..=36).step_by(14) {
                    spawn_scene_point_light_child(
                        parent,
                        Vec3::new(0.0, 9.0, z as f32),
                        Color::srgb(0.45, 0.95, 1.0),
                        75_000.0,
                        34.0,
                    );
                }
                for z in [-42.0, -34.0] {
                    spawn_scene_point_light_child(
                        parent,
                        Vec3::new(0.0, 7.0, z),
                        Color::srgb(1.0, 0.25, 0.12),
                        90_000.0,
                        30.0,
                    );
                }
                for z in (-28..=28).step_by(20) {
                    spawn_scene_point_light_child(
                        parent,
                        Vec3::new(-11.0, 8.0, z as f32),
                        Color::srgb(0.8, 0.9, 1.0),
                        42_000.0,
                        24.0,
                    );
                    spawn_scene_point_light_child(
                        parent,
                        Vec3::new(11.0, 8.0, z as f32),
                        Color::srgb(0.8, 0.9, 1.0),
                        42_000.0,
                        24.0,
                    );
                }
            });
    }
}

fn spawn_voxel_planet_preview(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let voxels = voxel_planet_preview_blocks();
    let detail_voxels = voxel_planet_detail_preview_blocks();
    for material in [MAT_PLANET_OCEAN, MAT_PLANET_LAND] {
        let mesh = build_voxel_planet_preview_mesh(
            &voxels,
            material,
            VOXEL_PLANET_PREVIEW_BLOCK,
        );
        if mesh.count_vertices() == 0 {
            continue;
        }
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: preview_material_color(material),
                emissive: preview_material_emissive(material).into(),
                perceptual_roughness: 0.9,
                metallic: 0.0,
                ..default()
            })),
            Transform::default(),
            Visibility::Visible,
            VoxelPlanetPreview,
            VoxelPlanetFarPreview,
        ));
    }
    for material in [MAT_PLANET_OCEAN, MAT_PLANET_LAND] {
        let mesh = build_voxel_planet_preview_mesh(
            &detail_voxels,
            material,
            VOXEL_PLANET_DETAIL_PREVIEW_BLOCK,
        );
        if mesh.count_vertices() == 0 {
            continue;
        }
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: preview_material_color(material),
                emissive: preview_material_emissive(material).into(),
                perceptual_roughness: 0.9,
                metallic: 0.0,
                ..default()
            })),
            Transform::default(),
            Visibility::Visible,
            VoxelPlanetPreview,
            VoxelPlanetDetailPreview,
        ));
    }
}

fn spawn_scene_point_light_child(
    parent: &mut ChildSpawnerCommands,
    position: Vec3,
    color: Color,
    intensity: f32,
    range: f32,
) {
    parent.spawn((
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
        PhysicsVoxel,
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

fn build_voxel_planet_preview_mesh(
    voxels: &HashMap<IVec3, u8>,
    material: u8,
    block_size: i32,
) -> Mesh {
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
            IVec3::splat(block_size),
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
        MAT_HULL_LIGHT => Color::srgb(0.055, 0.06, 0.065),
        MAT_HULL_DARK => Color::srgb(0.025, 0.03, 0.04),
        MAT_STATION_METAL => Color::srgb(0.035, 0.04, 0.045),
        MAT_STATION_TRIM => Color::srgb(0.055, 0.06, 0.07),
        MAT_SOLAR_PANEL => Color::srgb(0.006, 0.02, 0.075),
        MAT_PLANET_OCEAN => Color::srgb(0.0, 0.018, 0.07),
        MAT_PLANET_LAND => Color::srgb(0.012, 0.05, 0.018),
        _ => Color::srgb(0.018, 0.018, 0.018),
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
            ui.label("工具");
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
                ui.selectable_value(
                    &mut editor.mode,
                    VoxelEditMode::Paint,
                    "替换",
                );
            });
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut editor.mode,
                    VoxelEditMode::Pick,
                    "吸管",
                );
                ui.selectable_value(
                    &mut editor.mode,
                    VoxelEditMode::BoxFill,
                    "盒填",
                );
            });
            if let Some(anchor) = editor.box_anchor {
                ui.horizontal(|ui| {
                    ui.small(format!(
                        "起点 {} {} {}",
                        anchor.x, anchor.y, anchor.z
                    ));
                    if ui.button("取消").clicked() {
                        editor.box_anchor = None;
                    }
                });
            }
            ui.separator();
            ui.label("材质");
            voxel_material_palette_ui(ui, &mut editor.material);
            ui.separator();
            ui.label("笔刷");
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut editor.brush_shape,
                    VoxelBrushShape::Single,
                    "单格",
                );
                ui.selectable_value(
                    &mut editor.brush_shape,
                    VoxelBrushShape::Cube,
                    "立方",
                );
            });
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut editor.brush_shape,
                    VoxelBrushShape::Sphere,
                    "球形",
                );
                ui.selectable_value(
                    &mut editor.brush_shape,
                    VoxelBrushShape::Plane,
                    "平面",
                );
            });
            ui.add_enabled(
                editor.brush_shape != VoxelBrushShape::Single,
                egui::Slider::new(&mut editor.brush_radius, 0..=5).text("半径"),
            );
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
                let saved_edits = active_voxel_map(store).map_or(0, |map| {
                    if map_runtime.edit_index_map_id.as_deref() == Some(map.id.as_str()) {
                        map_runtime.edit_index.len()
                    } else {
                        map.edits.len()
                    }
                });
                ui.label(format!("已保存编辑：{}", saved_edits));
            }
        });
}

fn voxel_material_palette_ui(ui: &mut egui::Ui, material: &mut u8) {
    egui::Grid::new("voxel_material_palette")
        .num_columns(2)
        .spacing(egui::vec2(8.0, 4.0))
        .show(ui, |ui| {
            for material_id in MAT_STAR..=MAT_PLANET_LAND {
                let selected = *material == material_id;
                let color = minimap_material_color(material_id);
                let swatch = egui::Button::new("")
                    .min_size(egui::vec2(18.0, 18.0))
                    .fill(color);
                if ui
                    .add(swatch)
                    .on_hover_text(material_label(material_id))
                    .clicked()
                {
                    *material = material_id;
                }
                let label = if selected {
                    format!("> {}", material_label(material_id))
                } else {
                    material_label(material_id).to_owned()
                };
                if ui.selectable_label(selected, label).clicked() {
                    *material = material_id;
                }
                ui.end_row();
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
        flush_runtime_edits_before_map_action(store, runtime, "map selection");
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
            flush_runtime_edits_before_map_action(store, runtime, "map creation");
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
            flush_runtime_edits_before_map_action(store, runtime, "map duplication");
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
            flush_runtime_edits_before_map_action(store, runtime, "map deletion");
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
            flush_runtime_edits_before_map_action(store, runtime, "map clear");
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
            flush_runtime_edits_before_map_action(store, runtime, "map status snapshot");
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
            flush_runtime_edits_before_map_action(store, runtime, "map status revert");
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
                "体素星球：半径{} 远景{}格/块 近景{}格/块",
                EARTH_PLANET_RADIUS, VOXEL_PLANET_PREVIEW_BLOCK, VOXEL_PLANET_DETAIL_PREVIEW_BLOCK
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

fn material_label(material: u8) -> &'static str {
    match material {
        MAT_STAR => "星点",
        MAT_HULL_LIGHT => "浅色舰壳",
        MAT_HULL_DARK => "深色舰壳",
        MAT_WINDOW_CYAN => "青色舷窗",
        MAT_ENGINE_RED => "引擎红光",
        MAT_STATION_METAL => "站体金属",
        MAT_STATION_TRIM => "结构饰条",
        MAT_SUN => "恒星光",
        MAT_SOLAR_PANEL => "蓝色能板",
        MAT_PLANET_OCEAN => "行星海洋",
        MAT_PLANET_LAND => "行星陆地",
        _ => "未知材质",
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

fn ensure_voxel_maps(store: &mut Persistent<VoxelSceneStore>) { ensure_voxel_maps_inner(store); }

fn ensure_voxel_maps_inner(store: &mut VoxelSceneStore) {
    if store.maps.is_empty() {
        let legacy_edits = std::mem::take(&mut store.edits);
        if !legacy_edits.is_empty() {
            store.maps.push(PersistedVoxelMap {
                id: "default".to_owned(),
                name: "默认地图".to_owned(),
                edits: legacy_edits,
            });
        }
    } else if !store.edits.is_empty() {
        let legacy_edits = std::mem::take(&mut store.edits);
        if let Some(map) = store.maps.first_mut() {
            for edit in legacy_edits {
                let position = IVec3::new(
                    edit.position[0],
                    edit.position[1],
                    edit.position[2],
                );
                upsert_persisted_edit_with_visibility(
                    &mut map.edits,
                    position,
                    edit.voxel,
                    edit.visibility,
                );
            }
        }
    }

    let inserted_space_hifi = !store.maps.iter().any(|map| map.id == SPACE_HIFI_MAP_ID);
    if inserted_space_hifi {
        store.maps.push(PersistedVoxelMap {
            id: SPACE_HIFI_MAP_ID.to_owned(),
            name: SPACE_HIFI_MAP_NAME.to_owned(),
            edits: space_hifi_voxel_edits(),
        });
    }

    let active_exists = store
        .active_map_id
        .as_deref()
        .is_some_and(|active_id| store.maps.iter().any(|map| map.id == active_id));
    if !active_exists {
        store.active_map_id = Some(SPACE_HIFI_MAP_ID.to_owned());
    }
}

fn space_hifi_voxel_edits() -> Vec<PersistedVoxelEdit> {
    let mut edits = Vec::new();
    push_orbital_forge(&mut edits);
    edits
}

fn space_hifi_decor_voxel_edits() -> Vec<PersistedVoxelEdit> {
    let mut edits = Vec::new();
    push_starfield(&mut edits);
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
    let max_radius = EARTH_PLANET_RADIUS
        + VOXEL_PLANET_MAX_ELEVATION.ceil() as i32
        + VOXEL_PLANET_CITY_MAX_HEIGHT;
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
    if let Some(material) = voxel_planet_structure_material(position, direction, distance) {
        return Some(material);
    }
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
    let ridges = smooth_voxel_planet_noise(
        direction * 88.0 + Vec3::new(31.0, 7.0, 19.0),
        64_919,
    )
    .abs();
    let lake_cut = voxel_planet_lake_influence(direction) * VOXEL_PLANET_LAKE_DEPTH;

    (continent * 72.0 + hills * 28.0 + detail * 12.0 + ridges * 18.0 - lake_cut).clamp(
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
    if voxel_planet_lake_influence(direction) > 0.48 || continent + moisture * 0.35 < -0.22 {
        MAT_PLANET_OCEAN
    } else {
        MAT_PLANET_LAND
    }
}

const VOXEL_PLANET_CITY_MAX_HEIGHT: i32 = 1_400;

fn voxel_planet_structure_material(_position: IVec3, direction: Vec3, distance: f32) -> Option<u8> {
    for city_direction in voxel_planet_city_directions() {
        let city_direction = city_direction.normalize_or_zero();
        let (x, z) = planet_tangent_coords(direction, city_direction);
        if x.abs() > VOXEL_PLANET_CITY_RADIUS || z.abs() > VOXEL_PLANET_CITY_RADIUS {
            continue;
        }

        let base_radius = EARTH_PLANET_RADIUS as f32 + voxel_planet_elevation(direction);
        let altitude = distance - base_radius;
        if !(0.0..=VOXEL_PLANET_CITY_MAX_HEIGHT as f32).contains(&altitude) {
            continue;
        }

        let cell_x = (x / VOXEL_PLANET_CITY_CELL).floor() as i32;
        let cell_z = (z / VOXEL_PLANET_CITY_CELL).floor() as i32;
        let local_x = x.rem_euclid(VOXEL_PLANET_CITY_CELL);
        let local_z = z.rem_euclid(VOXEL_PLANET_CITY_CELL);
        let cell_hash = voxel_planet_hash_noise(IVec3::new(cell_x, 0, cell_z), 118_201);
        let road_width = 54.0;
        let arterial = cell_x.rem_euclid(4) == 0 || cell_z.rem_euclid(4) == 0;
        let road = local_x < road_width
            || local_z < road_width
            || arterial && (local_x < road_width * 1.5 || local_z < road_width * 1.5);
        let plaza = Vec2::new(x, z).length() < 260.0;
        if altitude <= 6.0 && (road || plaza) {
            return Some(if plaza { MAT_SOLAR_PANEL } else { MAT_STATION_TRIM });
        }
        if road || plaza {
            continue;
        }

        let center_x = VOXEL_PLANET_CITY_CELL * 0.5;
        let center_z = VOXEL_PLANET_CITY_CELL * 0.5;
        let dx = (local_x - center_x).abs();
        let dz = (local_z - center_z).abs();
        let half_width = 105.0 + (cell_hash + 1.0) * 34.0;
        let half_depth =
            100.0 + (voxel_planet_hash_noise(IVec3::new(cell_x, 17, cell_z), 27_711) + 1.0) * 36.0;
        if dx > half_width || dz > half_depth {
            continue;
        }

        let tower_hash = voxel_planet_hash_noise(IVec3::new(cell_x, 31, cell_z), 93_421);
        let height = if cell_x.rem_euclid(3) == 1 && cell_z.rem_euclid(3) == 1 {
            760.0 + (tower_hash + 1.0) * 260.0
        } else {
            320.0 + (tower_hash + 1.0) * 220.0
        };
        if altitude > height {
            continue;
        }

        let wall_thickness = 12.0;
        let edge = dx >= half_width - wall_thickness || dz >= half_depth - wall_thickness;
        let floor = altitude.floor() as i32;
        let roof = floor + 18 >= height as i32;
        let floor_slab = floor <= 6 || floor.rem_euclid(96) <= 5;
        let doorway =
            local_z <= center_z - half_depth + wall_thickness + 2.0 && dx < 34.0 && floor <= 64;
        let window_band =
            edge && floor > 36 && floor.rem_euclid(96) >= 28 && floor.rem_euclid(96) <= 54;
        let corner_pillar =
            dx >= half_width - wall_thickness - 2.0 && dz >= half_depth - wall_thickness - 2.0;

        if doorway {
            continue;
        }
        if !edge && !roof && !floor_slab {
            continue;
        }

        return if roof {
            Some(MAT_SOLAR_PANEL)
        } else if window_band && !corner_pillar {
            Some(MAT_WINDOW_CYAN)
        } else if corner_pillar || floor_slab {
            Some(MAT_STATION_TRIM)
        } else if cell_hash > 0.35 {
            Some(MAT_HULL_LIGHT)
        } else {
            Some(MAT_STATION_METAL)
        };
    }
    None
}

fn voxel_planet_city_directions() -> [Vec3; 5] {
    let landing = (earth_planet_near_point() - earth_planet_center())
        .as_vec3()
        .normalize_or_zero();
    [
        landing,
        Vec3::new(0.62, -0.18, 0.76),
        Vec3::new(-0.50, 0.58, 0.64),
        Vec3::new(0.28, 0.86, -0.43),
        Vec3::new(-0.82, -0.20, -0.54),
    ]
}

fn voxel_planet_lake_directions() -> [Vec3; 6] {
    [
        Vec3::new(0.56, 0.08, 0.82),
        Vec3::new(0.18, -0.42, 0.89),
        Vec3::new(-0.36, 0.70, 0.62),
        Vec3::new(-0.72, -0.06, 0.69),
        Vec3::new(0.72, 0.55, -0.41),
        Vec3::new(-0.45, -0.70, -0.55),
    ]
}

fn voxel_planet_lake_influence(direction: Vec3) -> f32 {
    voxel_planet_lake_directions()
        .into_iter()
        .map(|lake_direction| {
            let lake_direction = lake_direction.normalize_or_zero();
            let (_, distance) = planet_tangent_distance(direction, lake_direction);
            let radius = 115.0
                + (voxel_planet_hash_noise(
                    (lake_direction * 17.0).round().as_ivec3(),
                    77_019,
                ) + 1.0)
                    * 45.0;
            (1.0 - distance / radius).clamp(0.0, 1.0)
        })
        .fold(0.0, f32::max)
}

fn planet_tangent_distance(direction: Vec3, center_direction: Vec3) -> (Vec2, f32) {
    let (x, z) = planet_tangent_coords(direction, center_direction);
    let coords = Vec2::new(x, z);
    (coords, coords.length())
}

fn planet_tangent_coords(direction: Vec3, center_direction: Vec3) -> (f32, f32) {
    let (right, forward) = planet_tangent_basis(center_direction);
    let tangent_delta = direction - center_direction * direction.dot(center_direction);
    (
        tangent_delta.dot(right) * EARTH_PLANET_RADIUS as f32,
        tangent_delta.dot(forward) * EARTH_PLANET_RADIUS as f32,
    )
}

fn planet_tangent_basis(center_direction: Vec3) -> (Vec3, Vec3) {
    let reference_up = if center_direction.y.abs() > 0.92 { Vec3::X } else { Vec3::Y };
    let right = center_direction.cross(reference_up).normalize_or_zero();
    let forward = right.cross(center_direction).normalize_or_zero();
    (right, forward)
}

fn voxel_planet_preview_blocks() -> HashMap<IVec3, u8> {
    let mut voxels = HashMap::new();
    let center = earth_planet_center().as_vec3();
    let steps = VOXEL_PLANET_PREVIEW_FACE_STEPS.max(1);

    for face in 0..6 {
        for u_index in 0..steps {
            for v_index in 0..steps {
                let u = ((u_index as f32 + 0.5) / steps as f32) * 2.0 - 1.0;
                let v = ((v_index as f32 + 0.5) / steps as f32) * 2.0 - 1.0;
                let direction = cube_planet_direction(face, u, v);
                let radius = EARTH_PLANET_RADIUS as f32 + voxel_planet_elevation(direction);
                let position = center + direction * radius;
                let origin = quantized_planet_preview_origin(position, VOXEL_PLANET_PREVIEW_BLOCK);
                voxels.insert(
                    origin,
                    voxel_planet_material(direction, 0.0),
                );
            }
        }
    }

    voxels
}

fn voxel_planet_detail_preview_blocks() -> HashMap<IVec3, u8> {
    let mut voxels = HashMap::new();
    let center_direction = (earth_planet_near_point() - earth_planet_center())
        .as_vec3()
        .normalize_or_zero();
    let (right, forward) = planet_tangent_basis(center_direction);
    let block = VOXEL_PLANET_DETAIL_PREVIEW_BLOCK.max(1);
    let radius = VOXEL_PLANET_DETAIL_PREVIEW_RADIUS.max(block);
    let planet_center = earth_planet_center().as_vec3();

    for x in (-radius..=radius).step_by(block as usize) {
        for z in (-radius..=radius).step_by(block as usize) {
            if x * x + z * z > radius * radius {
                continue;
            }
            let direction = (center_direction
                + right * (x as f32 / EARTH_PLANET_RADIUS as f32)
                + forward * (z as f32 / EARTH_PLANET_RADIUS as f32))
                .normalize_or_zero();
            let surface_radius = EARTH_PLANET_RADIUS as f32 + voxel_planet_elevation(direction);
            let position = planet_center + direction * surface_radius;
            let origin = quantized_planet_preview_origin(position, block);
            voxels.insert(
                origin,
                voxel_planet_material(direction, 0.0),
            );
        }
    }

    voxels
}

fn cube_planet_direction(face: i32, u: f32, v: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, v, -u),
        1 => Vec3::new(-1.0, v, u),
        2 => Vec3::new(u, 1.0, -v),
        3 => Vec3::new(u, -1.0, v),
        4 => Vec3::new(u, v, 1.0),
        _ => Vec3::new(-u, v, -1.0),
    }
    .normalize_or_zero()
}

fn quantized_planet_preview_origin(position: Vec3, block: i32) -> IVec3 {
    let block = block.max(1);
    let position = position.floor().as_ivec3();
    IVec3::new(
        position.x.div_euclid(block) * block,
        position.y.div_euclid(block) * block,
        position.z.div_euclid(block) * block,
    )
}

#[cfg(test)]
fn planet_city_sample_position(
    city_direction: Vec3,
    x: f32,
    z: f32,
    altitude: f32,
) -> (IVec3, Vec3, f32) {
    let city_direction = city_direction.normalize_or_zero();
    let (right, forward) = planet_tangent_basis(city_direction);
    let surface_direction = (city_direction
        + right * (x / EARTH_PLANET_RADIUS as f32)
        + forward * (z / EARTH_PLANET_RADIUS as f32))
        .normalize_or_zero();
    let surface_radius = EARTH_PLANET_RADIUS as f32 + voxel_planet_elevation(surface_direction);
    let position = (earth_planet_center().as_vec3()
        + surface_direction * (surface_radius + altitude))
        .round()
        .as_ivec3();
    (
        position,
        surface_direction,
        surface_radius,
    )
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

fn push_orbital_forge(edits: &mut Vec<PersistedVoxelEdit>) {
    push_box(
        edits,
        IVec3::new(-28, 0, -18),
        IVec3::new(28, 0, 34),
        MAT_HULL_DARK,
    );
    for x in (-24..=24).step_by(8) {
        push_box(
            edits,
            IVec3::new(x, 1, -17),
            IVec3::new(x + 1, 1, 33),
            MAT_STATION_TRIM,
        );
    }
    for z in (-16..=32).step_by(8) {
        push_box(
            edits,
            IVec3::new(-27, 1, z),
            IVec3::new(27, 1, z + 1),
            MAT_STATION_TRIM,
        );
    }

    for x in [-31, 30] {
        push_box(
            edits,
            IVec3::new(x, 1, -18),
            IVec3::new(x, 8, 30),
            MAT_HULL_LIGHT,
        );
        for z in (-14..=26).step_by(8) {
            push_box(
                edits,
                IVec3::new(x, 4, z),
                IVec3::new(x, 5, z + 3),
                MAT_WINDOW_CYAN,
            );
        }
    }

    push_hollow_box(
        edits,
        IVec3::new(-12, 1, -30),
        IVec3::new(12, 12, -19),
        MAT_STATION_METAL,
    );
    push_box(
        edits,
        IVec3::new(-8, 8, -31),
        IVec3::new(8, 10, -31),
        MAT_WINDOW_CYAN,
    );
    push_box(
        edits,
        IVec3::new(-14, 13, -28),
        IVec3::new(14, 14, -21),
        MAT_SOLAR_PANEL,
    );

    push_reactor_core(edits);
    push_docking_rails(edits);
    push_energy_gate(edits);
    push_cargo_and_cover(edits);
    push_small_shuttle(edits);
}

fn push_reactor_core(edits: &mut Vec<PersistedVoxelEdit>) {
    push_box(
        edits,
        IVec3::new(-3, 1, 2),
        IVec3::new(3, 2, 8),
        MAT_STATION_TRIM,
    );
    push_ellipsoid_shell(
        edits,
        IVec3::new(0, 5, 5),
        IVec3::new(3, 3, 3),
        0.36,
        MAT_WINDOW_CYAN,
    );
    for axis in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
        for step in 4..=9 {
            push_voxel(
                edits,
                IVec3::new(0, 5, 5) + axis * step,
                MAT_STATION_TRIM,
            );
        }
    }
}

fn push_docking_rails(edits: &mut Vec<PersistedVoxelEdit>) {
    for x in [-16, -15, 15, 16] {
        push_box(
            edits,
            IVec3::new(x, 1, 30),
            IVec3::new(x, 2, 58),
            MAT_STATION_TRIM,
        );
    }
    for z in (34..=58).step_by(8) {
        push_box(
            edits,
            IVec3::new(-18, 1, z),
            IVec3::new(18, 1, z + 1),
            MAT_HULL_LIGHT,
        );
    }
}

fn push_energy_gate(edits: &mut Vec<PersistedVoxelEdit>) {
    let z = 64;
    push_box(
        edits,
        IVec3::new(-18, 0, z),
        IVec3::new(18, 2, z),
        MAT_STATION_METAL,
    );
    for x in [-18, 18] {
        push_box(
            edits,
            IVec3::new(x, 1, z),
            IVec3::new(x, 18, z),
            MAT_STATION_TRIM,
        );
    }
    push_box(
        edits,
        IVec3::new(-18, 17, z),
        IVec3::new(18, 18, z),
        MAT_STATION_TRIM,
    );
    for x in (-14..=14).step_by(4) {
        push_voxel(
            edits,
            IVec3::new(x, 9, z),
            MAT_WINDOW_CYAN,
        );
    }
    for y in (4..=16).step_by(4) {
        push_voxel(
            edits,
            IVec3::new(-18, y, z),
            MAT_WINDOW_CYAN,
        );
        push_voxel(
            edits,
            IVec3::new(18, y, z),
            MAT_WINDOW_CYAN,
        );
    }
}

fn push_cargo_and_cover(edits: &mut Vec<PersistedVoxelEdit>) {
    for (min, max, material) in [
        (
            IVec3::new(-24, 1, -10),
            IVec3::new(-20, 4, -6),
            MAT_STATION_METAL,
        ),
        (
            IVec3::new(-22, 1, -4),
            IVec3::new(-17, 3, 1),
            MAT_SOLAR_PANEL,
        ),
        (
            IVec3::new(18, 1, -12),
            IVec3::new(24, 5, -7),
            MAT_HULL_LIGHT,
        ),
        (
            IVec3::new(17, 1, 14),
            IVec3::new(25, 3, 20),
            MAT_STATION_METAL,
        ),
        (
            IVec3::new(-25, 1, 18),
            IVec3::new(-19, 4, 24),
            MAT_HULL_LIGHT,
        ),
    ] {
        push_box(edits, min, max, material);
    }
}

fn push_small_shuttle(edits: &mut Vec<PersistedVoxelEdit>) {
    push_box(
        edits,
        IVec3::new(-6, 2, 17),
        IVec3::new(6, 4, 28),
        MAT_HULL_LIGHT,
    );
    push_box(
        edits,
        IVec3::new(-4, 5, 20),
        IVec3::new(4, 7, 26),
        MAT_HULL_DARK,
    );
    push_box(
        edits,
        IVec3::new(-2, 6, 26),
        IVec3::new(2, 8, 29),
        MAT_WINDOW_CYAN,
    );
    for x in [-9, 7] {
        push_box(
            edits,
            IVec3::new(x, 3, 20),
            IVec3::new(x + 2, 3, 29),
            MAT_HULL_DARK,
        );
    }
    push_box(
        edits,
        IVec3::new(-3, 3, 14),
        IVec3::new(3, 5, 16),
        MAT_ENGINE_RED,
    );
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
    mut runtime: ResMut<VoxelMapRuntimeState>,
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
            flush_runtime_edits_before_map_action(
                store,
                &mut runtime,
                "auto map status init",
            );
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
    flush_runtime_edits_before_map_action(store, &mut runtime, "auto map status");
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
    let mut edits = map.edits.iter().collect::<Vec<_>>();
    edits.sort_by_key(|edit| edit.position);
    edits.len().hash(&mut hasher);
    for edit in edits {
        hash_persisted_voxel_edit(edit, &mut hasher);
    }
    Some(hasher.finish())
}

fn hash_persisted_voxel_edit(edit: &PersistedVoxelEdit, hasher: &mut DefaultHasher) {
    edit.position.hash(hasher);
    edit.visibility.hash(hasher);
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
    mut player_view_request: ResMut<ScenePlayerViewRequest>,
    player_view_state: Res<ScenePlayerVoxelViewState>,
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
            if let Some(active_user_id) = player_view_state.active_user_id {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "当前玩家视角：{}",
                        scene_player_display_name(manager.as_deref(), active_user_id)
                    ));
                    if ui.button("恢复GM视角").clicked() {
                        player_view_request.restore_gm_view = true;
                    }
                });
            }
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
                    player_view_request.user_id = Some(selected_user_id);
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
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    runtime: Res<VoxelMapRuntimeState>,
    mut voxel_world: VoxelWorld<TrpgVoxelWorld>,
    mut player_view_state: ResMut<ScenePlayerVoxelViewState>,
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
    if request.restore_gm_view {
        request.restore_gm_view = false;
        clear_scene_player_voxel_view(&mut voxel_world, &mut player_view_state);
    }

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
    player_view_state.active_user_id = Some(user_id);
    let access = scene_capture_player_access(manager.as_deref(), user_id);
    apply_scene_player_voxel_view(
        &mut voxel_world,
        &mut player_view_state,
        &runtime.edit_index,
        &access,
    );
}

fn maintain_scene_player_voxel_view(
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    runtime: Res<VoxelMapRuntimeState>,
    mut voxel_world: VoxelWorld<TrpgVoxelWorld>,
    mut player_view_state: ResMut<ScenePlayerVoxelViewState>,
) {
    let Some(user_id) = player_view_state.active_user_id else {
        if !player_view_state.applied_changes.is_empty() {
            restore_applied_scene_player_voxel_view(&mut voxel_world, &mut player_view_state);
        }
        return;
    };

    let access = scene_capture_player_access(manager.as_deref(), user_id);
    apply_scene_player_voxel_view(
        &mut voxel_world,
        &mut player_view_state,
        &runtime.edit_index,
        &access,
    );
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
    mut far_previews: Query<&mut Visibility, With<VoxelPlanetFarPreview>>,
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

    for mut preview_visibility in &mut far_previews {
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
        if !hit.voxel.is_solid() {
            return None;
        }
        return Some(VoxelEditTarget { position, normal });
    }

    let target = procedural_planet_edit_target_from_ray(ray)?;
    match voxel_world.get_voxel(target.position) {
        WorldVoxel::Air => None,
        WorldVoxel::Solid(_) => Some(target),
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
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_info: Query<
        (&Camera, &GlobalTransform, &Transform),
        (
            With<VoxelWorldCamera<TrpgVoxelWorld>>,
            With<FreeCamera>,
            Without<HeldPhysicsVoxel>,
            Without<PhysicsVoxel>,
            Without<BattleSpaceshipPreviewRoot>,
        ),
    >,
    mut physics_voxels: Query<
        (
            Entity,
            &mut Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        (
            With<PhysicsVoxel>,
            Without<FreeCamera>,
            Without<BattleSpaceshipPreviewRoot>,
        ),
    >,
    mut grab_state: ResMut<PhysicsVoxelGrabState>,
    mut ship_grab_state: ResMut<BattleSpaceshipGrabState>,
    mut battle_spaceships: Query<
        &mut Transform,
        (
            With<BattleSpaceshipPreviewRoot>,
            Without<PhysicsVoxel>,
            Without<FreeCamera>,
        ),
    >,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    spatial_query: SpatialQuery,
) {
    grab_state.debounce_seconds = (grab_state.debounce_seconds - time.delta_secs()).max(0.0);

    let Ok((camera, camera_global_transform, camera_transform)) = camera_info.single() else {
        return;
    };
    let held_transform = held_physics_voxel_transform(camera_transform);
    let held_ship_position = held_battle_spaceship_position(camera_transform);

    if ship_grab_state.held {
        if let Ok(mut ship_transform) = battle_spaceships.single_mut() {
            ship_transform.translation = held_ship_position - ship_grab_state.grab_local_offset;
        } else {
            ship_grab_state.held = false;
        }
    }

    if let Some(entity) = grab_state.held_entity {
        if let Ok((_, mut transform, mut linear_velocity, mut angular_velocity)) =
            physics_voxels.get_mut(entity)
        {
            *transform = held_transform;
            linear_velocity.0 = Vec3::ZERO;
            angular_velocity.0 = Vec3::ZERO;
        } else {
            grab_state.held_entity = None;
        }
    }

    if !keyboard.just_pressed(KeyCode::KeyF)
        || egui_wants_input.wants_any_keyboard_input()
        || grab_state.debounce_seconds > 0.0
    {
        return;
    }
    grab_state.debounce_seconds = PHYSICS_VOXEL_GRAB_DEBOUNCE_SECONDS;

    if ship_grab_state.held {
        ship_grab_state.held = false;
        if let (Ok(ship_transform), Some(store)) = (
            battle_spaceships.single(),
            store.as_deref_mut(),
        ) {
            store.battle_spaceship_translation = ship_transform.translation.to_array();
            if let Err(err) = store.persist() {
                eprintln!("failed to persist battle spaceship transform: {err}");
            }
        }
        return;
    }

    if let Some(entity) = grab_state.held_entity {
        if let Ok((_, mut transform, mut linear_velocity, mut angular_velocity)) =
            physics_voxels.get_mut(entity)
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

    if let Ok(ship_transform) = battle_spaceships.single() {
        if let Some(hit_distance) =
            battle_spaceship_ray_intersection(ray, ship_transform.translation)
        {
            let hit_position = ray.origin + *ray.direction * hit_distance;
            ship_grab_state.grab_local_offset = hit_position - ship_transform.translation;
            ship_grab_state.held = true;
            return;
        }
    }

    let filter = SpatialQueryFilter::default();
    let hit = spatial_query.cast_ray_predicate(
        ray.origin,
        ray.direction,
        PHYSICS_VOXEL_GRAB_MAX_DISTANCE,
        true,
        &filter,
        &|entity| physics_voxels.contains(entity),
    );
    let Some(hit) = hit else {
        return;
    };
    let Ok((entity, mut transform, mut linear_velocity, mut angular_velocity)) =
        physics_voxels.get_mut(hit.entity)
    else {
        return;
    };

    *transform = held_transform;
    linear_velocity.0 = Vec3::ZERO;
    angular_velocity.0 = Vec3::ZERO;
    commands
        .entity(entity)
        .remove::<PlanetGravityBody>()
        .insert((
            RigidBody::Kinematic,
            GravityScale(0.0),
            HeldPhysicsVoxel,
        ));
    grab_state.held_entity = Some(entity);
}

fn held_physics_voxel_transform(camera_transform: &Transform) -> Transform {
    let mut transform = Transform::from_translation(
        camera_transform.translation + *camera_transform.forward() * HELD_PHYSICS_VOXEL_DISTANCE,
    );
    transform.rotation = camera_transform.rotation;
    transform
}

fn held_battle_spaceship_position(camera_transform: &Transform) -> Vec3 {
    camera_transform.translation + *camera_transform.forward() * HELD_BATTLE_SPACESHIP_DISTANCE
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

fn pickup_indicator_target(
    ray: Ray3d,
    battle_spaceships: &Query<
        &Transform,
        (
            With<BattleSpaceshipPreviewRoot>,
            Without<FreeCamera>,
            Without<PhysicsVoxel>,
        ),
    >,
    physics_voxels: &Query<
        (Entity, &GlobalTransform),
        (
            With<PhysicsVoxel>,
            Without<FreeCamera>,
            Without<BattleSpaceshipPreviewRoot>,
        ),
    >,
    spatial_query: &SpatialQuery,
) -> Option<Vec3> {
    if let Ok(ship_transform) = battle_spaceships.single() {
        if battle_spaceship_ray_intersection(ray, ship_transform.translation).is_some() {
            return Some(ship_transform.translation);
        }
    }

    let filter = SpatialQueryFilter::default();
    let hit = spatial_query.cast_ray_predicate(
        ray.origin,
        ray.direction,
        PHYSICS_VOXEL_GRAB_MAX_DISTANCE,
        true,
        &filter,
        &|entity| physics_voxels.contains(entity),
    )?;
    physics_voxels
        .get(hit.entity)
        .ok()
        .map(|(_, transform)| transform.translation())
}

fn battle_spaceship_ray_intersection(ray: Ray3d, translation: Vec3) -> Option<f32> {
    let scale = BATTLE_SPACESHIP_SCALE as f32;
    let min = Vec3::new(-20.0 * scale, 0.0, -44.0 * scale) + translation;
    let max = Vec3::new(21.0 * scale, 20.0 * scale, 45.0 * scale) + translation;
    ray_aabb_intersection(ray.origin, *ray.direction, min, max)
        .filter(|distance| *distance <= BATTLE_SPACESHIP_GRAB_MAX_DISTANCE)
}

fn ray_aabb_intersection(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let mut t_min: f32 = 0.0;
    let mut t_max = f32::INFINITY;

    for axis in 0..3 {
        let origin_axis = origin[axis];
        let direction_axis = direction[axis];
        let min_axis = min[axis];
        let max_axis = max[axis];

        if direction_axis.abs() <= f32::EPSILON {
            if origin_axis < min_axis || origin_axis > max_axis {
                return None;
            }
            continue;
        }

        let inverse_direction = 1.0 / direction_axis;
        let mut t1 = (min_axis - origin_axis) * inverse_direction;
        let mut t2 = (max_axis - origin_axis) * inverse_direction;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        t_min = t_min.max(t1);
        t_max = t_max.min(t2);
        if t_min > t_max {
            return None;
        }
    }

    (t_max >= 0.0).then_some(t_min.max(0.0))
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
            if procedural_earth_voxel_planet(position).is_some() {
                return Some(VoxelEditTarget {
                    position,
                    normal: planet_axis_normal(position),
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

fn draw_pickup_indicator_gizmo(
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_info: Query<
        (&Camera, &GlobalTransform, &Transform),
        (
            With<VoxelWorldCamera<TrpgVoxelWorld>>,
            With<FreeCamera>,
            Without<PlayerCaptureCamera>,
            Without<BattleSpaceshipPreviewRoot>,
            Without<PhysicsVoxel>,
        ),
    >,
    battle_spaceships: Query<
        &Transform,
        (
            With<BattleSpaceshipPreviewRoot>,
            Without<FreeCamera>,
            Without<PhysicsVoxel>,
        ),
    >,
    physics_voxels: Query<
        (Entity, &GlobalTransform),
        (
            With<PhysicsVoxel>,
            Without<FreeCamera>,
            Without<BattleSpaceshipPreviewRoot>,
        ),
    >,
    spatial_query: SpatialQuery,
    mut gizmos: Gizmos,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((camera, camera_global_transform, camera_transform)) = camera_info.single() else {
        return;
    };
    let Some(ray) = center_screen_ray(window, camera, camera_global_transform) else {
        return;
    };
    let Some(target) = pickup_indicator_target(
        ray,
        &battle_spaceships,
        &physics_voxels,
        &spatial_query,
    ) else {
        return;
    };

    gizmos.arrow(
        camera_transform.translation,
        target,
        PICKUP_INDICATOR_COLOR,
    );
}

fn draw_voxel_edit_preview_gizmo(
    egui_wants_input: Res<EguiWantsInput>,
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
    voxel_world: VoxelWorld<TrpgVoxelWorld>,
    mut gizmos: Gizmos,
) {
    if !editor.enabled || egui_wants_input.wants_pointer_input() {
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
    let raycast_hit = voxel_world.raycast(ray.clone(), &|(_, voxel)| {
        voxel.is_solid()
    });
    let Some(target) = voxel_edit_target(&voxel_world, ray, raycast_hit) else {
        return;
    };

    let base_position = match editor.mode {
        VoxelEditMode::Add | VoxelEditMode::BoxFill => target.position + target.normal,
        VoxelEditMode::Erase | VoxelEditMode::Paint | VoxelEditMode::Pick => target.position,
    };
    let mut positions = if editor.mode == VoxelEditMode::BoxFill {
        editor
            .box_anchor
            .map(|anchor| box_fill_positions(anchor, base_position))
            .unwrap_or_else(|| vec![base_position])
    } else if editor.mode == VoxelEditMode::Pick {
        vec![base_position]
    } else {
        brush_positions(
            base_position,
            editor.brush_radius,
            editor.brush_shape,
            target.normal,
        )
    };

    positions.sort_by_key(|position| ivec3_sort_key(*position));
    positions.dedup();
    let color = match editor.mode {
        VoxelEditMode::Erase => Color::srgb(1.0, 0.18, 0.08),
        VoxelEditMode::Pick => Color::srgb(1.0, 0.86, 0.24),
        VoxelEditMode::Paint => Color::srgb(0.18, 0.86, 1.0),
        VoxelEditMode::BoxFill => Color::srgb(0.45, 0.95, 0.55),
        VoxelEditMode::Add => Color::srgb(0.45, 0.72, 1.0),
    };
    for position in positions.into_iter().take(96) {
        draw_voxel_wireframe(&mut gizmos, position, color);
    }
}

fn draw_voxel_wireframe(gizmos: &mut Gizmos, position: IVec3, color: Color) {
    let p = position.as_vec3();
    let corners = [
        p,
        p + Vec3::X,
        p + Vec3::X + Vec3::Y,
        p + Vec3::Y,
        p + Vec3::Z,
        p + Vec3::X + Vec3::Z,
        p + Vec3::X + Vec3::Y + Vec3::Z,
        p + Vec3::Y + Vec3::Z,
    ];
    for (a, b) in [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ] {
        gizmos.line(corners[a], corners[b], color);
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
    mut editor: ResMut<VoxelEditorState>,
    mut pointer_state: ResMut<ScenePointerState>,
    mut runtime: ResMut<VoxelMapRuntimeState>,
    mut voxel_world: VoxelWorld<TrpgVoxelWorld>,
    store: Option<Res<Persistent<VoxelSceneStore>>>,
) {
    let wants_keyboard_input = egui_wants_input.wants_any_keyboard_input();
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
        if runtime.save_requested {
            runtime.save_debounce_seconds = 0.0;
        }
    }
    if runtime.pending_map_id.is_some() {
        return;
    }
    if !wants_keyboard_input {
        handle_voxel_undo_redo(
            &keyboard,
            &mut runtime,
            &mut voxel_world,
        );
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

    if runtime.edit_index_map_id
        != store
            .as_deref()
            .and_then(|store| store.active_map_id.clone())
    {
        if let Some(store) = store.as_deref() {
            runtime.edit_index = active_voxel_map(store)
                .map(|map| voxel_edit_index(&map.edits))
                .unwrap_or_default();
            runtime.edit_index_map_id = store.active_map_id.clone();
            runtime.applied_index = runtime.edit_index.clone();
            runtime.applied_map_id = store.active_map_id.clone();
        }
    }

    if editor.mode == VoxelEditMode::Pick {
        if mouse_buttons.just_pressed(MouseButton::Left) {
            if let WorldVoxel::Solid(material) = effective_voxel_at(&voxel_world, target.position) {
                editor.material = material;
            }
        }
        return;
    }

    let mut base_position = match editor.mode {
        VoxelEditMode::Add | VoxelEditMode::BoxFill => target.position + target.normal,
        VoxelEditMode::Erase | VoxelEditMode::Paint | VoxelEditMode::Pick => target.position,
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

    if editor.mode == VoxelEditMode::BoxFill {
        if !mouse_buttons.just_pressed(MouseButton::Left) {
            return;
        }
        if let Some(anchor) = editor.box_anchor.take() {
            let positions = box_fill_positions(anchor, base_position);
            apply_voxel_edit_positions(
                &mut runtime,
                &mut voxel_world,
                positions,
                PersistedVoxel::Solid(editor.material),
            );
        } else {
            editor.box_anchor = Some(base_position);
        }
        pointer_state.last_edit_position = Some(base_position);
        return;
    }

    let persisted_voxel = match editor.mode {
        VoxelEditMode::Add | VoxelEditMode::Paint => PersistedVoxel::Solid(editor.material),
        VoxelEditMode::Erase => PersistedVoxel::Air,
        VoxelEditMode::Pick | VoxelEditMode::BoxFill => return,
    };
    let centers = pointer_state
        .last_edit_position
        .map(|last_position| voxel_line_positions(last_position, base_position))
        .unwrap_or_else(|| vec![base_position]);
    let positions = centers
        .into_iter()
        .flat_map(|center| {
            brush_positions(
                center,
                editor.brush_radius,
                editor.brush_shape,
                target.normal,
            )
        })
        .collect::<Vec<_>>();

    apply_voxel_edit_positions(
        &mut runtime,
        &mut voxel_world,
        positions,
        persisted_voxel,
    );
    pointer_state.last_edit_position = Some(base_position);
}

fn scene_capture_request_system(
    mut commands: Commands,
    mut requests: ResMut<SceneCaptureRequests>,
    mut capture_state: ResMut<SceneCaptureState>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    runtime: Res<VoxelMapRuntimeState>,
    mut player_view_state: ResMut<ScenePlayerVoxelViewState>,
    player_cameras: Res<PlayerSceneCameras>,
    mut voxel_world: VoxelWorld<TrpgVoxelWorld>,
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
            voxel_view_changes: Vec::new(),
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
        restore_applied_scene_player_voxel_view(&mut voxel_world, &mut player_view_state);
        let access = scene_capture_player_access(manager.as_deref(), current.user_id);
        current.voxel_view_changes =
            scene_capture_voxel_filter_changes(&runtime.edit_index, &access);
        apply_scene_capture_voxel_view(
            &mut voxel_world,
            &current.voxel_view_changes,
            SceneCaptureVoxelView::Capture,
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
                      manager: Option<Res<Persistent<NapcatMessageManager>>>,
                      runtime: Res<VoxelMapRuntimeState>,
                      mut player_view_state: ResMut<ScenePlayerVoxelViewState>,
                      free_camera: Query<Entity, With<FreeCamera>>,
                      mut voxel_world: VoxelWorld<TrpgVoxelWorld>,
                      mut cameras: Query<&mut Camera, With<PlayerCaptureCamera>>| {
                    if let Ok(mut camera) = cameras.get_mut(pending.camera_entity) {
                        camera.is_active = false;
                    }
                    apply_scene_capture_voxel_view(
                        &mut voxel_world,
                        &pending.voxel_view_changes,
                        SceneCaptureVoxelView::Restore,
                    );
                    if let Some(active_user_id) = player_view_state.active_user_id {
                        let access =
                            scene_capture_player_access(manager.as_deref(), active_user_id);
                        apply_scene_player_voxel_view(
                            &mut voxel_world,
                            &mut player_view_state,
                            &runtime.edit_index,
                            &access,
                        );
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

enum SceneCaptureVoxelView {
    Capture,
    Restore,
}

fn scene_capture_player_access(
    manager: Option<&Persistent<NapcatMessageManager>>,
    user_id: u64,
) -> PlayerAccess {
    manager
        .map(|manager| manager.player_access_for_user(user_id))
        .unwrap_or(PlayerAccess {
            player_id: user_id,
            ..Default::default()
        })
}

fn scene_capture_voxel_filter_changes(
    index: &HashMap<IVec3, PersistedVoxelState>,
    access: &PlayerAccess,
) -> Vec<SceneCaptureVoxelViewChange> {
    let mut changes = index
        .iter()
        .filter_map(|(&position, state)| {
            if state.visibility.can_read_for_access(access) {
                return None;
            }

            let capture_voxel = starter_scene_voxel(position, None);
            let restore_voxel = WorldVoxel::from(state.voxel);
            (capture_voxel != restore_voxel).then_some(SceneCaptureVoxelViewChange {
                position,
                capture_voxel,
                restore_voxel,
            })
        })
        .collect::<Vec<_>>();
    changes.sort_by_key(|change| ivec3_sort_key(change.position));
    changes
}

fn apply_scene_player_voxel_view(
    voxel_world: &mut VoxelWorld<TrpgVoxelWorld>,
    player_view_state: &mut ScenePlayerVoxelViewState,
    index: &HashMap<IVec3, PersistedVoxelState>,
    access: &PlayerAccess,
) {
    let signature = scene_player_voxel_view_signature(index, access);
    if player_view_state.applied_signature == Some(signature) {
        apply_scene_capture_voxel_view(
            voxel_world,
            &player_view_state.applied_changes,
            SceneCaptureVoxelView::Capture,
        );
        return;
    }

    restore_applied_scene_player_voxel_view(voxel_world, player_view_state);
    let changes = scene_capture_voxel_filter_changes(index, access);
    apply_scene_capture_voxel_view(
        voxel_world,
        &changes,
        SceneCaptureVoxelView::Capture,
    );
    player_view_state.applied_changes = changes;
    player_view_state.applied_signature = Some(signature);
}

fn clear_scene_player_voxel_view(
    voxel_world: &mut VoxelWorld<TrpgVoxelWorld>,
    player_view_state: &mut ScenePlayerVoxelViewState,
) {
    restore_applied_scene_player_voxel_view(voxel_world, player_view_state);
    player_view_state.active_user_id = None;
}

fn restore_applied_scene_player_voxel_view(
    voxel_world: &mut VoxelWorld<TrpgVoxelWorld>,
    player_view_state: &mut ScenePlayerVoxelViewState,
) {
    apply_scene_capture_voxel_view(
        voxel_world,
        &player_view_state.applied_changes,
        SceneCaptureVoxelView::Restore,
    );
    player_view_state.applied_changes.clear();
    player_view_state.applied_signature = None;
}

fn scene_player_voxel_view_signature(
    index: &HashMap<IVec3, PersistedVoxelState>,
    access: &PlayerAccess,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    access.player_id.hash(&mut hasher);
    access.party_id.hash(&mut hasher);
    access.is_gm.hash(&mut hasher);
    let mut states = index.iter().collect::<Vec<_>>();
    states.sort_by_key(|(position, _)| ivec3_sort_key(**position));
    states.len().hash(&mut hasher);
    for (position, state) in states {
        position.x.hash(&mut hasher);
        position.y.hash(&mut hasher);
        position.z.hash(&mut hasher);
        state.visibility.hash(&mut hasher);
        match state.voxel {
            PersistedVoxel::Air => 0_u8.hash(&mut hasher),
            PersistedVoxel::Solid(material) => {
                1_u8.hash(&mut hasher);
                material.hash(&mut hasher);
            },
        }
    }
    hasher.finish()
}

fn apply_scene_capture_voxel_view(
    voxel_world: &mut VoxelWorld<TrpgVoxelWorld>,
    changes: &[SceneCaptureVoxelViewChange],
    view: SceneCaptureVoxelView,
) {
    for change in changes {
        let voxel = match view {
            SceneCaptureVoxelView::Capture => change.capture_voxel.clone(),
            SceneCaptureVoxelView::Restore => change.restore_voxel.clone(),
        };
        voxel_world.set_voxel(change.position, voxel);
    }
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
                clear_color: ClearColorConfig::Custom(Color::srgb(0.12, 0.14, 0.16)),
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
        || (runtime.pending_map_id.is_none() && runtime.applied_map_id != active_map_id)
        || runtime.edit_index_map_id != active_map_id;
    if should_start_load {
        let next_index = active_voxel_map(&store)
            .map(|map| map.edits.clone())
            .map(|edits| voxel_edit_index(&edits))
            .unwrap_or_default();
        runtime.pending_changes = voxel_index_diff(&runtime.applied_index, &next_index);
        runtime.pending_map_id = active_map_id.clone();
        runtime.apply_cursor = 0;
        runtime.edit_index = next_index;
        runtime.edit_index_map_id = active_map_id.clone();
        runtime.undo_stack.clear();
        runtime.redo_stack.clear();
        runtime.save_requested = false;
        runtime.save_debounce_seconds = 0.0;
        runtime.reload_requested = false;
    } else if runtime.pending_map_id.is_none() {
        return;
    }

    let mut budget = VOXEL_MAP_APPLY_BUDGET_PER_FRAME;
    while runtime.apply_cursor < runtime.pending_changes.len() && budget > 0 {
        let (position, voxel) = runtime.pending_changes[runtime.apply_cursor];
        voxel_world.set_voxel(position, voxel);
        runtime.apply_cursor += 1;
        budget -= 1;
    }

    if runtime.apply_cursor >= runtime.pending_changes.len() {
        runtime.applied_index = runtime.edit_index.clone();
        runtime.applied_map_id = runtime.pending_map_id.take();
        runtime.pending_changes.clear();
        runtime.apply_cursor = 0;
    }
}

fn flush_voxel_edit_save_requests(
    time: Res<Time>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    mut runtime: ResMut<VoxelMapRuntimeState>,
) {
    if !runtime.save_requested {
        return;
    }
    runtime.save_debounce_seconds = (runtime.save_debounce_seconds - time.delta_secs()).max(0.0);
    if runtime.save_debounce_seconds > 0.0 {
        return;
    }

    let Some(store) = store.as_deref_mut() else {
        return;
    };
    if write_runtime_index_to_store(store, &runtime) {
        persist_voxel_store(store, "voxel edit batch");
    }
    runtime.save_requested = false;
}

fn voxel_edit_index(edits: &[PersistedVoxelEdit]) -> HashMap<IVec3, PersistedVoxelState> {
    edits
        .iter()
        .map(|edit| {
            (
                IVec3::new(
                    edit.position[0],
                    edit.position[1],
                    edit.position[2],
                ),
                PersistedVoxelState {
                    voxel: edit.voxel,
                    visibility: edit.visibility.clone(),
                },
            )
        })
        .collect()
}

fn voxel_index_to_edits(index: &HashMap<IVec3, PersistedVoxelState>) -> Vec<PersistedVoxelEdit> {
    let mut edits = index
        .iter()
        .map(
            |(&position, state)| PersistedVoxelEdit {
                position: [position.x, position.y, position.z],
                voxel: state.voxel,
                visibility: state.visibility.clone(),
            },
        )
        .collect::<Vec<_>>();
    edits.sort_by_key(|edit| edit.position);
    edits
}

fn voxel_index_diff(
    previous: &HashMap<IVec3, PersistedVoxelState>,
    next: &HashMap<IVec3, PersistedVoxelState>,
) -> Vec<(IVec3, WorldVoxel<u8>)> {
    let mut positions = previous
        .keys()
        .chain(next.keys())
        .copied()
        .collect::<Vec<_>>();
    positions.sort_by_key(|position| ivec3_sort_key(*position));
    positions.dedup();

    positions
        .into_iter()
        .filter_map(|position| {
            let previous_voxel = previous.get(&position).map(|state| state.voxel);
            let next_voxel = next.get(&position).map(|state| state.voxel);
            (previous_voxel != next_voxel).then(|| {
                (
                    position,
                    next_voxel
                        .map(WorldVoxel::from)
                        .unwrap_or_else(|| starter_scene_voxel(position, None)),
                )
            })
        })
        .collect()
}

fn write_runtime_index_to_store(
    store: &mut VoxelSceneStore,
    runtime: &VoxelMapRuntimeState,
) -> bool {
    let Some(map_id) = runtime.edit_index_map_id.as_deref() else {
        return false;
    };
    let Some(map) = store.maps.iter_mut().find(|map| map.id == map_id) else {
        return false;
    };
    let edits = voxel_index_to_edits(&runtime.edit_index);
    if map.edits == edits {
        return false;
    }
    map.edits = edits;
    true
}

fn request_voxel_edit_save(runtime: &mut VoxelMapRuntimeState) {
    runtime.save_requested = true;
    runtime.save_debounce_seconds = 0.25;
}

fn flush_runtime_edits_before_map_action(
    store: &mut Persistent<VoxelSceneStore>,
    runtime: &mut VoxelMapRuntimeState,
    reason: &str,
) {
    if !runtime.save_requested {
        return;
    }
    if write_runtime_index_to_store(store, runtime) {
        persist_voxel_store(store, reason);
    }
    runtime.save_requested = false;
    runtime.save_debounce_seconds = 0.0;
}

fn handle_voxel_undo_redo(
    keyboard: &ButtonInput<KeyCode>,
    runtime: &mut VoxelMapRuntimeState,
    voxel_world: &mut VoxelWorld<TrpgVoxelWorld>,
) {
    let control_held =
        keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !control_held {
        return;
    }

    if keyboard.just_pressed(KeyCode::KeyZ) {
        let Some(stroke) = runtime.undo_stack.pop() else {
            return;
        };
        apply_voxel_stroke(runtime, voxel_world, &stroke, true);
        runtime.redo_stack.push(stroke);
        request_voxel_edit_save(runtime);
    } else if keyboard.just_pressed(KeyCode::KeyY) {
        let Some(stroke) = runtime.redo_stack.pop() else {
            return;
        };
        apply_voxel_stroke(runtime, voxel_world, &stroke, false);
        runtime.undo_stack.push(stroke);
        request_voxel_edit_save(runtime);
    }
}

fn apply_voxel_edit_positions(
    runtime: &mut VoxelMapRuntimeState,
    voxel_world: &mut VoxelWorld<TrpgVoxelWorld>,
    positions: Vec<IVec3>,
    after: PersistedVoxel,
) {
    let stroke = voxel_edit_stroke(runtime, positions, after);
    if stroke.changes.is_empty() {
        return;
    }
    apply_voxel_stroke(runtime, voxel_world, &stroke, false);
    runtime.undo_stack.push(stroke);
    if runtime.undo_stack.len() > 64 {
        runtime.undo_stack.remove(0);
    }
    runtime.redo_stack.clear();
    request_voxel_edit_save(runtime);
}

fn voxel_edit_stroke(
    runtime: &VoxelMapRuntimeState,
    positions: Vec<IVec3>,
    after: PersistedVoxel,
) -> VoxelEditStroke {
    let mut by_position: HashMap<IVec3, VoxelEditChange> = HashMap::new();
    for position in positions {
        let before = runtime.edit_index.get(&position).map(|state| state.voxel);
        let after = Some(after);
        if before == after {
            continue;
        }
        by_position
            .entry(position)
            .and_modify(|change| change.after = after)
            .or_insert(VoxelEditChange {
                position,
                before,
                after,
            });
    }

    let mut changes = by_position.into_values().collect::<Vec<_>>();
    changes.sort_by_key(|change| ivec3_sort_key(change.position));
    VoxelEditStroke { changes }
}

fn apply_voxel_stroke(
    runtime: &mut VoxelMapRuntimeState,
    voxel_world: &mut VoxelWorld<TrpgVoxelWorld>,
    stroke: &VoxelEditStroke,
    undo: bool,
) {
    for change in &stroke.changes {
        let next = if undo { change.before } else { change.after };
        set_runtime_voxel_state(runtime, change.position, next);
        voxel_world.set_voxel(
            change.position,
            persisted_state_to_world_voxel(change.position, next),
        );
    }
}

fn set_runtime_voxel_state(
    runtime: &mut VoxelMapRuntimeState,
    position: IVec3,
    state: Option<PersistedVoxel>,
) {
    match state {
        Some(voxel) => {
            let visibility = runtime
                .edit_index
                .get(&position)
                .or_else(|| runtime.applied_index.get(&position))
                .map(|state| state.visibility.clone())
                .unwrap_or_default();
            let state = PersistedVoxelState { voxel, visibility };
            runtime.edit_index.insert(position, state.clone());
            runtime.applied_index.insert(position, state);
        },
        None => {
            runtime.edit_index.remove(&position);
            runtime.applied_index.remove(&position);
        },
    }
}

fn persisted_state_to_world_voxel(
    position: IVec3,
    state: Option<PersistedVoxel>,
) -> WorldVoxel<u8> {
    state
        .map(WorldVoxel::from)
        .unwrap_or_else(|| starter_scene_voxel(position, None))
}

fn effective_voxel_at(
    voxel_world: &VoxelWorld<'_, TrpgVoxelWorld>,
    position: IVec3,
) -> WorldVoxel<u8> {
    match voxel_world.get_voxel(position) {
        WorldVoxel::Unset => starter_scene_voxel(position, None),
        voxel => voxel,
    }
}

fn brush_positions(
    center: IVec3,
    radius: i32,
    shape: VoxelBrushShape,
    normal: IVec3,
) -> Vec<IVec3> {
    let radius = radius.max(0);
    match shape {
        VoxelBrushShape::Single => vec![center],
        VoxelBrushShape::Cube => (-radius..=radius)
            .flat_map(move |x| {
                (-radius..=radius).flat_map(move |y| {
                    (-radius..=radius).map(move |z| center + IVec3::new(x, y, z))
                })
            })
            .collect(),
        VoxelBrushShape::Sphere => {
            let radius_squared = radius * radius;
            (-radius..=radius)
                .flat_map(move |x| {
                    (-radius..=radius).flat_map(move |y| {
                        (-radius..=radius).filter_map(move |z| {
                            (x * x + y * y + z * z <= radius_squared)
                                .then_some(center + IVec3::new(x, y, z))
                        })
                    })
                })
                .collect()
        },
        VoxelBrushShape::Plane => plane_brush_positions(center, radius, normal),
    }
}

fn plane_brush_positions(center: IVec3, radius: i32, normal: IVec3) -> Vec<IVec3> {
    let normal_abs = normal.abs();
    let axis = if normal_abs.x >= normal_abs.y && normal_abs.x >= normal_abs.z {
        0
    } else if normal_abs.y >= normal_abs.z {
        1
    } else {
        2
    };
    let mut positions = Vec::new();
    for a in -radius..=radius {
        for b in -radius..=radius {
            let offset = match axis {
                0 => IVec3::new(0, a, b),
                1 => IVec3::new(a, 0, b),
                _ => IVec3::new(a, b, 0),
            };
            positions.push(center + offset);
        }
    }
    positions
}

fn box_fill_positions(a: IVec3, b: IVec3) -> Vec<IVec3> {
    let min = a.min(b);
    let max = a.max(b);
    let mut positions = Vec::new();
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                positions.push(IVec3::new(x, y, z));
            }
        }
    }
    positions
}

fn voxel_line_positions(start: IVec3, end: IVec3) -> Vec<IVec3> {
    let delta = end - start;
    let steps = delta.x.abs().max(delta.y.abs()).max(delta.z.abs());
    if steps == 0 {
        return vec![end];
    }
    (1..=steps)
        .map(|step| {
            let t = step as f32 / steps as f32;
            (start.as_vec3() + delta.as_vec3() * t).round().as_ivec3()
        })
        .collect()
}

fn ivec3_sort_key(position: IVec3) -> (i32, i32, i32) { (position.x, position.y, position.z) }

fn upsert_persisted_edit(
    edits: &mut Vec<PersistedVoxelEdit>,
    position: IVec3,
    voxel: PersistedVoxel,
) {
    upsert_persisted_edit_preserving_visibility(edits, position, voxel);
}

fn upsert_persisted_edit_preserving_visibility(
    edits: &mut Vec<PersistedVoxelEdit>,
    position: IVec3,
    voxel: PersistedVoxel,
) {
    let position = [position.x, position.y, position.z];
    if let Some(edit) = edits.iter_mut().find(|edit| edit.position == position) {
        edit.voxel = voxel;
    } else {
        edits.push(PersistedVoxelEdit {
            position,
            voxel,
            visibility: SceneVisibility::Public,
        });
    }
}

fn upsert_persisted_edit_with_visibility(
    edits: &mut Vec<PersistedVoxelEdit>,
    position: IVec3,
    voxel: PersistedVoxel,
    visibility: SceneVisibility,
) {
    let position = [position.x, position.y, position.z];
    if let Some(edit) = edits.iter_mut().find(|edit| edit.position == position) {
        edit.voxel = voxel;
        edit.visibility = visibility;
    } else {
        edits.push(PersistedVoxelEdit {
            position,
            voxel,
            visibility,
        });
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
    fn starter_scene_lookup_stays_empty_for_streaming_performance() {
        let center = earth_planet_center();
        let outward = (earth_planet_near_point() - center)
            .as_vec3()
            .normalize_or_zero();
        let solid_position = (center.as_vec3()
            + outward * (EARTH_PLANET_RADIUS as f32 - VOXEL_PLANET_MAX_ELEVATION - 8.0))
            .round()
            .as_ivec3();

        assert_eq!(
            starter_scene_voxel(solid_position, None),
            WorldVoxel::Air
        );
    }

    #[test]
    fn voxel_planet_radius_is_large_enough_for_playable_horizon() {
        assert!(EARTH_PLANET_RADIUS >= 9_000);
        assert!(VOXEL_PLANET_PREVIEW_BLOCK <= 128);
        assert!(VOXEL_PLANET_PREVIEW_FACE_STEPS <= 160);
        assert!(VOXEL_PLANET_DETAIL_PREVIEW_BLOCK <= VOXEL_PLANET_PREVIEW_BLOCK / 8);
        assert!(VOXEL_PLANET_DETAIL_PREVIEW_RADIUS <= 600);
        assert!(VOXEL_PLANET_CITY_RADIUS >= 2_000.0);
        assert!(VOXEL_PLANET_CITY_MAX_HEIGHT >= 1_200);
    }

    #[test]
    fn voxel_planet_preview_is_blocky_without_exploding_block_count() {
        let preview = voxel_planet_preview_blocks();

        assert!(preview.len() > 35_000);
        assert!(preview.len() < 140_000);
        assert!(preview
            .values()
            .any(|material| *material == MAT_PLANET_LAND));
        assert!(preview
            .values()
            .any(|material| *material == MAT_PLANET_OCEAN));
    }

    #[test]
    fn voxel_planet_detail_preview_uses_much_smaller_blocks() {
        let preview = voxel_planet_detail_preview_blocks();

        assert!(VOXEL_PLANET_DETAIL_PREVIEW_BLOCK <= 16);
        assert!(preview.len() > 1_500);
        assert!(preview.len() < 8_000);
        assert!(preview
            .values()
            .any(|material| *material == MAT_PLANET_LAND));
    }

    #[test]
    fn voxel_planet_far_preview_hides_before_detail_preview() {
        assert!(VOXEL_PLANET_PREVIEW_HIDE_ALTITUDE > VOXEL_PLANET_DETAIL_PREVIEW_BLOCK as f32);
        assert!(VOXEL_PLANET_DETAIL_PREVIEW_RADIUS < VOXEL_PLANET_PREVIEW_HIDE_ALTITUDE as i32);
    }

    #[test]
    fn orbital_forge_default_map_is_editable_and_bounded() {
        let edits = space_hifi_voxel_edits();
        let index = voxel_edit_index(&edits);

        assert!(edits.len() > 4_000);
        assert!(edits.len() < 12_000);
        assert_eq!(edits.len(), index.len());
        assert!(index
            .values()
            .any(|state| state.voxel == PersistedVoxel::Solid(MAT_WINDOW_CYAN)));
        assert!(index
            .values()
            .any(|state| state.voxel == PersistedVoxel::Solid(MAT_ENGINE_RED)));
        assert!(index.contains_key(&IVec3::new(0, 0, 0)));
    }

    #[test]
    fn ensure_voxel_maps_preserves_old_space_hifi_user_maps() {
        let mut store = VoxelSceneStore {
            active_map_id: Some("space-hifi-wide-ship10".to_owned()),
            maps: vec![PersistedVoxelMap {
                id: "space-hifi-wide-ship10".to_owned(),
                name: "旧太空地图".to_owned(),
                edits: vec![PersistedVoxelEdit {
                    position: [1, 2, 3],
                    voxel: PersistedVoxel::Solid(MAT_WINDOW_CYAN),
                    visibility: SceneVisibility::Public,
                }],
            }],
            map_status_snapshots: vec![PersistedVoxelMapStatusSnapshot {
                id: "status-old".to_owned(),
                map_id: "space-hifi-wide-ship10".to_owned(),
                name: "旧状态".to_owned(),
                reason: "手动".to_owned(),
                created_at: 1,
                edits: Vec::new(),
            }],
            ..Default::default()
        };

        ensure_voxel_maps_inner(&mut store);

        assert!(store
            .maps
            .iter()
            .any(|map| map.id == "space-hifi-wide-ship10"));
        assert!(store
            .map_status_snapshots
            .iter()
            .any(|snapshot| snapshot.id == "status-old"));
        assert!(store.maps.iter().any(|map| map.id == SPACE_HIFI_MAP_ID));
        assert_eq!(
            store.active_map_id.as_deref(),
            Some("space-hifi-wide-ship10")
        );
    }

    #[test]
    fn ensure_voxel_maps_keeps_blank_user_map_active() {
        let mut store = VoxelSceneStore {
            active_map_id: Some("blank".to_owned()),
            maps: vec![PersistedVoxelMap {
                id: "blank".to_owned(),
                name: "空白地图".to_owned(),
                edits: Vec::new(),
            }],
            ..Default::default()
        };

        ensure_voxel_maps_inner(&mut store);

        assert_eq!(
            store.active_map_id.as_deref(),
            Some("blank")
        );
        assert!(store.maps.iter().any(|map| map.id == SPACE_HIFI_MAP_ID));
    }

    #[test]
    fn ensure_voxel_maps_fresh_store_uses_orbital_forge() {
        let mut store = VoxelSceneStore::default();

        ensure_voxel_maps_inner(&mut store);

        assert_eq!(store.maps.len(), 1);
        assert_eq!(store.maps[0].id, SPACE_HIFI_MAP_ID);
        assert_eq!(
            store.active_map_id.as_deref(),
            Some(SPACE_HIFI_MAP_ID)
        );
    }

    #[test]
    fn voxel_scene_export_json_preserves_maps_visibility_and_scene_metadata() {
        let store = VoxelSceneStore {
            editor_camera_speed: 72.0,
            battle_spaceship_translation: [1.0, 2.0, 3.0],
            active_map_id: Some("map-b".to_owned()),
            maps: vec![
                PersistedVoxelMap {
                    id: "map-a".to_owned(),
                    name: "公开地图".to_owned(),
                    edits: vec![PersistedVoxelEdit {
                        position: [1, 2, 3],
                        voxel: PersistedVoxel::Solid(MAT_WINDOW_CYAN),
                        visibility: SceneVisibility::Public,
                    }],
                },
                PersistedVoxelMap {
                    id: "map-b".to_owned(),
                    name: "红队地图".to_owned(),
                    edits: vec![PersistedVoxelEdit {
                        position: [4, 5, 6],
                        voxel: PersistedVoxel::Air,
                        visibility: SceneVisibility::Party("red".to_owned()),
                    }],
                },
            ],
            map_status_snapshots: vec![PersistedVoxelMapStatusSnapshot {
                id: "status-1".to_owned(),
                map_id: "map-b".to_owned(),
                name: "红队地图 手动 @ 1".to_owned(),
                reason: "手动".to_owned(),
                created_at: 1,
                edits: vec![PersistedVoxelEdit {
                    position: [7, 8, 9],
                    voxel: PersistedVoxel::Solid(MAT_ENGINE_RED),
                    visibility: SceneVisibility::Player(2),
                }],
            }],
            edits: vec![PersistedVoxelEdit {
                position: [0, 0, 0],
                voxel: PersistedVoxel::Solid(MAT_HULL_LIGHT),
                visibility: SceneVisibility::Gm,
            }],
            capture_cameras: vec![PersistedCaptureCamera {
                user_id: 2,
                translation: [9.0, 8.0, 7.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            }],
            character_standees: vec![PersistedCharacterStandee {
                target_id: "2".to_owned(),
                image_source: "portrait.png".to_owned(),
                translation: [3.0, 2.0, 1.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            }],
        };

        let json = store.to_export_json().unwrap();
        let export: VoxelSceneStoreExportOwned = serde_json::from_str(&json).unwrap();

        assert_eq!(
            export.version,
            VOXEL_SCENE_EXPORT_VERSION
        );
        assert_eq!(export.export_type, "voxel_scene");
        assert_eq!(
            export.store.active_map_id.as_deref(),
            Some("map-b")
        );
        assert_eq!(export.store.maps.len(), 2);
        assert_eq!(
            export.store.maps[1].edits[0].visibility,
            SceneVisibility::Party("red".to_owned())
        );
        assert_eq!(
            export.store.map_status_snapshots[0].edits[0].visibility,
            SceneVisibility::Player(2)
        );
        assert_eq!(
            export.store.edits[0].visibility,
            SceneVisibility::Gm
        );
        assert_eq!(
            export.store.capture_cameras[0].user_id,
            2
        );
        assert_eq!(
            export.store.character_standees[0].image_source,
            "portrait.png"
        );
    }

    #[test]
    fn voxel_scene_export_json_merges_by_scene_keys_and_preserves_visibility() {
        let source = VoxelSceneStore {
            editor_camera_speed: 72.0,
            battle_spaceship_translation: [1.0, 2.0, 3.0],
            active_map_id: Some("shared".to_owned()),
            maps: vec![
                PersistedVoxelMap {
                    id: "shared".to_owned(),
                    name: "导入地图".to_owned(),
                    edits: vec![PersistedVoxelEdit {
                        position: [4, 5, 6],
                        voxel: PersistedVoxel::Solid(MAT_WINDOW_CYAN),
                        visibility: SceneVisibility::Party("red".to_owned()),
                    }],
                },
                PersistedVoxelMap {
                    id: "new-map".to_owned(),
                    name: "新地图".to_owned(),
                    edits: Vec::new(),
                },
            ],
            map_status_snapshots: vec![PersistedVoxelMapStatusSnapshot {
                id: "status-shared".to_owned(),
                map_id: "shared".to_owned(),
                name: "导入状态".to_owned(),
                reason: "手动".to_owned(),
                created_at: 2,
                edits: vec![PersistedVoxelEdit {
                    position: [7, 8, 9],
                    voxel: PersistedVoxel::Solid(MAT_ENGINE_RED),
                    visibility: SceneVisibility::Player(2),
                }],
            }],
            edits: vec![PersistedVoxelEdit {
                position: [0, 0, 0],
                voxel: PersistedVoxel::Air,
                visibility: SceneVisibility::Gm,
            }],
            capture_cameras: vec![PersistedCaptureCamera {
                user_id: 2,
                translation: [9.0, 8.0, 7.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            }],
            character_standees: vec![PersistedCharacterStandee {
                target_id: "2".to_owned(),
                image_source: "imported.png".to_owned(),
                translation: [3.0, 2.0, 1.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            }],
        };
        let json = source.to_export_json().unwrap();
        let mut store = VoxelSceneStore {
            editor_camera_speed: 12.0,
            active_map_id: Some("local".to_owned()),
            maps: vec![
                PersistedVoxelMap {
                    id: "local".to_owned(),
                    name: "本地地图".to_owned(),
                    edits: Vec::new(),
                },
                PersistedVoxelMap {
                    id: "shared".to_owned(),
                    name: "旧地图".to_owned(),
                    edits: vec![PersistedVoxelEdit {
                        position: [1, 1, 1],
                        voxel: PersistedVoxel::Solid(MAT_HULL_LIGHT),
                        visibility: SceneVisibility::Public,
                    }],
                },
            ],
            map_status_snapshots: vec![PersistedVoxelMapStatusSnapshot {
                id: "status-shared".to_owned(),
                map_id: "shared".to_owned(),
                name: "旧状态".to_owned(),
                reason: "手动".to_owned(),
                created_at: 1,
                edits: Vec::new(),
            }],
            capture_cameras: vec![
                PersistedCaptureCamera {
                    user_id: 2,
                    translation: [0.0, 0.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                },
                PersistedCaptureCamera {
                    user_id: 9,
                    translation: [1.0, 1.0, 1.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                },
            ],
            character_standees: vec![
                PersistedCharacterStandee {
                    target_id: "2".to_owned(),
                    image_source: "old.png".to_owned(),
                    translation: [0.0, 0.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                },
                PersistedCharacterStandee {
                    target_id: "9".to_owned(),
                    image_source: "local.png".to_owned(),
                    translation: [1.0, 1.0, 1.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                },
            ],
            ..Default::default()
        };

        let imported = store.merge_export_json(&json).unwrap();

        assert_eq!(imported, 2);
        assert_eq!(store.editor_camera_speed, 72.0);
        assert_eq!(store.battle_spaceship_translation, [
            1.0, 2.0, 3.0
        ]);
        assert_eq!(
            store.active_map_id.as_deref(),
            Some("shared")
        );
        assert!(store.maps.iter().any(|map| map.id == "local"));
        let shared = store.maps.iter().find(|map| map.id == "shared").unwrap();
        assert_eq!(shared.name, "导入地图");
        assert_eq!(
            shared.edits[0].visibility,
            SceneVisibility::Party("red".to_owned())
        );
        assert!(store.maps.iter().any(|map| map.id == "new-map"));
        assert_eq!(
            store.map_status_snapshots[0].name,
            "导入状态"
        );
        assert_eq!(
            store.map_status_snapshots[0].edits[0].visibility,
            SceneVisibility::Player(2)
        );
        assert_eq!(
            store.edits[0].visibility,
            SceneVisibility::Gm
        );
        assert_eq!(
            store
                .capture_cameras
                .iter()
                .find(|camera| camera.user_id == 2)
                .unwrap()
                .translation,
            [9.0, 8.0, 7.0]
        );
        assert!(store
            .capture_cameras
            .iter()
            .any(|camera| camera.user_id == 9));
        assert_eq!(
            store
                .character_standees
                .iter()
                .find(|standee| standee.target_id == "2")
                .unwrap()
                .image_source,
            "imported.png"
        );
        assert!(store
            .character_standees
            .iter()
            .any(|standee| standee.target_id == "9"));
    }

    #[test]
    fn voxel_scene_import_rejects_wrong_export_shape() {
        let json = serde_json::json!({
            "version": VOXEL_SCENE_EXPORT_VERSION,
            "export_type": "deepseek_summaries",
            "store": VoxelSceneStore::default(),
        })
        .to_string();
        let mut store = VoxelSceneStore::default();

        let error = store
            .merge_export_json(&json)
            .err()
            .expect("wrong export type should fail");

        assert!(error.contains("unsupported voxel scene export type"));
        assert!(store.maps.is_empty());
    }

    #[test]
    fn active_voxel_map_signature_hashes_repaints_not_just_last_edit() {
        let mut store = VoxelSceneStore {
            active_map_id: Some("map".to_owned()),
            maps: vec![PersistedVoxelMap {
                id: "map".to_owned(),
                name: "地图".to_owned(),
                edits: vec![
                    PersistedVoxelEdit {
                        position: [0, 0, 0],
                        voxel: PersistedVoxel::Solid(MAT_HULL_LIGHT),
                        visibility: SceneVisibility::Public,
                    },
                    PersistedVoxelEdit {
                        position: [9, 0, 0],
                        voxel: PersistedVoxel::Solid(MAT_HULL_DARK),
                        visibility: SceneVisibility::Public,
                    },
                ],
            }],
            ..Default::default()
        };
        let before = active_voxel_map_edit_signature(&store);

        store.maps[0].edits[0].voxel = PersistedVoxel::Solid(MAT_WINDOW_CYAN);
        let after = active_voxel_map_edit_signature(&store);

        assert_ne!(before, after);
    }

    #[test]
    fn active_voxel_map_signature_hashes_visibility_changes() {
        let mut store = VoxelSceneStore {
            active_map_id: Some("map".to_owned()),
            maps: vec![PersistedVoxelMap {
                id: "map".to_owned(),
                name: "地图".to_owned(),
                edits: vec![PersistedVoxelEdit {
                    position: [0, 0, 0],
                    voxel: PersistedVoxel::Solid(MAT_HULL_LIGHT),
                    visibility: SceneVisibility::Public,
                }],
            }],
            ..Default::default()
        };
        let before = active_voxel_map_edit_signature(&store);

        store.maps[0].edits[0].visibility = SceneVisibility::Party("red".to_owned());
        let after = active_voxel_map_edit_signature(&store);

        assert_ne!(before, after);
    }

    #[test]
    fn legacy_voxel_edit_deserializes_as_public_visibility() {
        let edit = serde_json::from_value::<PersistedVoxelEdit>(serde_json::json!({
            "position": [1, 2, 3],
            "voxel": { "Solid": MAT_HULL_LIGHT }
        }))
        .expect("legacy persisted voxel edit should deserialize");

        assert_eq!(edit.visibility, SceneVisibility::Public);
    }

    #[test]
    fn scene_visibility_access_matches_party_player_and_gm_rules() {
        assert!(SceneVisibility::Public.can_read(2, None, false));
        assert!(SceneVisibility::Party("red".to_owned()).can_read(2, Some("red"), false));
        assert!(!SceneVisibility::Party("red".to_owned()).can_read(3, Some("blue"), false));
        assert!(SceneVisibility::Player(2).can_read(2, None, false));
        assert!(!SceneVisibility::Player(2).can_read(3, None, false));
        assert!(!SceneVisibility::Gm.can_read(2, Some("red"), false));
        assert!(SceneVisibility::Gm.can_read(9, None, true));
    }

    #[test]
    fn scene_capture_voxel_filter_hides_invisible_edits_for_player() {
        let index = HashMap::from([
            (
                IVec3::new(0, 0, 0),
                PersistedVoxelState {
                    voxel: PersistedVoxel::Solid(MAT_HULL_LIGHT),
                    visibility: SceneVisibility::Public,
                },
            ),
            (
                IVec3::new(1, 0, 0),
                PersistedVoxelState {
                    voxel: PersistedVoxel::Solid(MAT_HULL_DARK),
                    visibility: SceneVisibility::Party("red".to_owned()),
                },
            ),
            (
                IVec3::new(2, 0, 0),
                PersistedVoxelState {
                    voxel: PersistedVoxel::Solid(MAT_WINDOW_CYAN),
                    visibility: SceneVisibility::Party("blue".to_owned()),
                },
            ),
            (
                IVec3::new(3, 0, 0),
                PersistedVoxelState {
                    voxel: PersistedVoxel::Solid(MAT_ENGINE_RED),
                    visibility: SceneVisibility::Player(2),
                },
            ),
            (
                IVec3::new(4, 0, 0),
                PersistedVoxelState {
                    voxel: PersistedVoxel::Solid(MAT_STATION_METAL),
                    visibility: SceneVisibility::Player(3),
                },
            ),
            (
                IVec3::new(5, 0, 0),
                PersistedVoxelState {
                    voxel: PersistedVoxel::Solid(MAT_STATION_TRIM),
                    visibility: SceneVisibility::Gm,
                },
            ),
        ]);
        let access = PlayerAccess {
            player_id: 2,
            party_id: Some("red".to_owned()),
            ..Default::default()
        };

        let changes = scene_capture_voxel_filter_changes(&index, &access);
        let filtered = changes
            .iter()
            .map(|change| {
                (
                    change.position,
                    change.capture_voxel.clone(),
                    change.restore_voxel.clone(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(filtered, vec![
            (
                IVec3::new(2, 0, 0),
                WorldVoxel::Air,
                WorldVoxel::Solid(MAT_WINDOW_CYAN),
            ),
            (
                IVec3::new(4, 0, 0),
                WorldVoxel::Air,
                WorldVoxel::Solid(MAT_STATION_METAL),
            ),
            (
                IVec3::new(5, 0, 0),
                WorldVoxel::Air,
                WorldVoxel::Solid(MAT_STATION_TRIM),
            ),
        ]);
    }

    #[test]
    fn scene_capture_voxel_filter_keeps_full_map_for_gm() {
        let index = HashMap::from([(
            IVec3::new(0, 0, 0),
            PersistedVoxelState {
                voxel: PersistedVoxel::Solid(MAT_HULL_LIGHT),
                visibility: SceneVisibility::Gm,
            },
        )]);
        let access = PlayerAccess {
            player_id: 9,
            is_gm: true,
            ..Default::default()
        };

        assert!(scene_capture_voxel_filter_changes(&index, &access).is_empty());
    }

    #[test]
    fn scene_player_voxel_view_signature_tracks_access_and_visibility() {
        let access_red = PlayerAccess {
            player_id: 2,
            party_id: Some("red".to_owned()),
            ..Default::default()
        };
        let access_blue = PlayerAccess {
            player_id: 2,
            party_id: Some("blue".to_owned()),
            ..Default::default()
        };
        let mut index = HashMap::from([
            (
                IVec3::new(2, 0, 0),
                PersistedVoxelState {
                    voxel: PersistedVoxel::Solid(MAT_HULL_LIGHT),
                    visibility: SceneVisibility::Party("red".to_owned()),
                },
            ),
            (
                IVec3::new(1, 0, 0),
                PersistedVoxelState {
                    voxel: PersistedVoxel::Solid(MAT_HULL_DARK),
                    visibility: SceneVisibility::Public,
                },
            ),
        ]);

        let red_signature = scene_player_voxel_view_signature(&index, &access_red);
        let blue_signature = scene_player_voxel_view_signature(&index, &access_blue);

        assert_ne!(red_signature, blue_signature);

        index.get_mut(&IVec3::new(2, 0, 0)).unwrap().visibility =
            SceneVisibility::Party("blue".to_owned());
        assert_ne!(
            red_signature,
            scene_player_voxel_view_signature(&index, &access_red)
        );
    }

    #[test]
    fn voxel_edit_index_round_trips_without_duplicate_positions() {
        let edits = vec![
            PersistedVoxelEdit {
                position: [2, 0, 0],
                voxel: PersistedVoxel::Solid(MAT_HULL_LIGHT),
                visibility: SceneVisibility::Public,
            },
            PersistedVoxelEdit {
                position: [1, 0, 0],
                voxel: PersistedVoxel::Air,
                visibility: SceneVisibility::Public,
            },
            PersistedVoxelEdit {
                position: [2, 0, 0],
                voxel: PersistedVoxel::Solid(MAT_WINDOW_CYAN),
                visibility: SceneVisibility::Party("red".to_owned()),
            },
        ];

        let index = voxel_edit_index(&edits);
        let round_trip = voxel_index_to_edits(&index);

        assert_eq!(index.len(), 2);
        assert_eq!(
            index.get(&IVec3::new(2, 0, 0)).map(|state| state.voxel),
            Some(PersistedVoxel::Solid(MAT_WINDOW_CYAN))
        );
        assert_eq!(
            round_trip
                .iter()
                .find(|edit| edit.position == [2, 0, 0])
                .map(|edit| &edit.visibility),
            Some(&SceneVisibility::Party(
                "red".to_owned()
            ))
        );
        assert_eq!(
            round_trip
                .iter()
                .map(|edit| edit.position)
                .collect::<Vec<_>>(),
            vec![[1, 0, 0], [2, 0, 0]]
        );
    }

    #[test]
    fn voxel_index_diff_only_emits_changed_positions() {
        let previous = HashMap::from([
            (
                IVec3::new(0, 0, 0),
                PersistedVoxelState::public(PersistedVoxel::Solid(MAT_HULL_LIGHT)),
            ),
            (
                IVec3::new(1, 0, 0),
                PersistedVoxelState::public(PersistedVoxel::Solid(MAT_HULL_DARK)),
            ),
        ]);
        let next = HashMap::from([
            (
                IVec3::new(1, 0, 0),
                PersistedVoxelState::public(PersistedVoxel::Solid(MAT_HULL_DARK)),
            ),
            (
                IVec3::new(2, 0, 0),
                PersistedVoxelState::public(PersistedVoxel::Air),
            ),
        ]);

        let diff = voxel_index_diff(&previous, &next);

        assert_eq!(diff.len(), 2);
        assert!(diff.contains(&(IVec3::new(0, 0, 0), WorldVoxel::Air)));
        assert!(diff.contains(&(IVec3::new(2, 0, 0), WorldVoxel::Air)));
    }

    #[test]
    fn brush_shapes_have_predictable_counts() {
        let center = IVec3::ZERO;

        assert_eq!(
            brush_positions(
                center,
                5,
                VoxelBrushShape::Single,
                IVec3::Y
            )
            .len(),
            1
        );
        assert_eq!(
            brush_positions(
                center,
                1,
                VoxelBrushShape::Cube,
                IVec3::Y
            )
            .len(),
            27
        );
        assert_eq!(
            brush_positions(
                center,
                1,
                VoxelBrushShape::Sphere,
                IVec3::Y
            )
            .len(),
            7
        );
        assert_eq!(
            brush_positions(
                center,
                1,
                VoxelBrushShape::Plane,
                IVec3::Y
            )
            .len(),
            9
        );
    }

    #[test]
    fn voxel_edit_stroke_coalesces_duplicate_positions() {
        let mut runtime = VoxelMapRuntimeState::default();
        runtime.edit_index.insert(
            IVec3::ZERO,
            PersistedVoxelState::public(PersistedVoxel::Solid(MAT_HULL_LIGHT)),
        );

        let stroke = voxel_edit_stroke(
            &runtime,
            vec![IVec3::ZERO, IVec3::ZERO, IVec3::X],
            PersistedVoxel::Solid(MAT_WINDOW_CYAN),
        );

        assert_eq!(stroke.changes.len(), 2);
        let zero_change = stroke
            .changes
            .iter()
            .find(|change| change.position == IVec3::ZERO)
            .unwrap();
        assert_eq!(
            zero_change.before,
            Some(PersistedVoxel::Solid(MAT_HULL_LIGHT))
        );
        assert_eq!(
            zero_change.after,
            Some(PersistedVoxel::Solid(MAT_WINDOW_CYAN))
        );
    }

    #[test]
    fn material_palette_labels_cover_all_solid_materials() {
        for material in MAT_STAR..=MAT_PLANET_LAND {
            assert_ne!(material_label(material), "未知材质");
        }
    }

    #[test]
    fn procedural_planet_edit_target_hits_surface_without_streamed_chunks() {
        let waypoint = planet_surface_waypoint();
        let direction = Dir3::new(waypoint.focus - waypoint.eye).unwrap();
        let ray = Ray3d::new(waypoint.eye, direction);
        let target = procedural_planet_edit_target_from_ray(ray)
            .expect("planet surface should be targetable analytically");

        assert!(procedural_earth_voxel_planet(target.position).is_some());
        assert_ne!(target.normal, IVec3::ZERO);
    }

    #[test]
    fn voxel_planet_has_lake_material_on_lake_basin_surface() {
        let center = earth_planet_center();
        let direction = voxel_planet_lake_directions()[0].normalize_or_zero();
        let radius = EARTH_PLANET_RADIUS as f32 + voxel_planet_elevation(direction) - 2.0;
        let position = (center.as_vec3() + direction * radius).round().as_ivec3();

        assert_eq!(
            procedural_earth_voxel_planet(position),
            Some(MAT_PLANET_OCEAN)
        );
    }

    #[test]
    fn voxel_planet_has_voxel_buildings_near_landing_city() {
        let city_direction = voxel_planet_city_directions()[0].normalize_or_zero();
        let local_x = VOXEL_PLANET_CITY_CELL * 2.5;
        let local_z = VOXEL_PLANET_CITY_CELL * 1.5;
        let (position, ..) = planet_city_sample_position(city_direction, local_x, local_z, 4.0);

        assert!(matches!(
            procedural_earth_voxel_planet(position),
            Some(
                MAT_HULL_LIGHT
                    | MAT_STATION_METAL
                    | MAT_STATION_TRIM
                    | MAT_WINDOW_CYAN
                    | MAT_SOLAR_PANEL
            )
        ));
    }

    #[test]
    fn voxel_planet_buildings_have_walkable_hollow_interiors() {
        let city_direction = voxel_planet_city_directions()[0].normalize_or_zero();
        let local_x = VOXEL_PLANET_CITY_CELL * 2.5;
        let local_z = VOXEL_PLANET_CITY_CELL * 1.5;
        let (position, ..) = planet_city_sample_position(city_direction, local_x, local_z, 140.0);

        assert_eq!(
            procedural_earth_voxel_planet(position),
            None
        );
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
