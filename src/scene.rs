// The active voxel renderer lives in `voxel.rs`. This module remains compiled
// for shared persistence types, migrations, and legacy regression tests, so
// most of its old runtime systems are intentionally not scheduled.
#![allow(dead_code)]

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
    AngularInertia,
    AngularVelocity,
    CenterOfMass,
    Collider,
    Forces,
    Gravity,
    GravityScale,
    LinearDamping,
    LinearVelocity,
    Mass,
    PhysicsPlugins,
    PhysicsSchedule,
    PhysicsStepSystems,
    Position,
    ReadRigidBodyForces,
    RigidBody,
    Rotation,
    SpatialQuery,
    SpatialQueryFilter,
    WriteRigidBodyForces,
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
const UNIT_TEMPLATE_STANDEE_PREFIX: &str = "unit:";
const UNIT_TEMPLATE_TOKEN_PREFIX: &str = "unit-token:";
const UNIT_SCENE_TOKEN_Y: f32 = 0.35;
const UNIT_SCENE_TOKEN_SPACING: f32 = 1.6;
const LEGACY_AREA_MARKER_SCALE: f32 = 0.1;
const LEGACY_AREA_MARKER_Y: f32 = 0.08;
const LEGACY_AREA_MARKER_VOXEL_Y: i32 = 1;
const LEGACY_AREA_MARKER_FILL_VOXEL_LIMIT: usize = 10_000;
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
const BATTLE_SPACESHIP_BLOCK_MASS: f32 = 18.0;
const BATTLE_SPACESHIP_THRUST_PER_SAIL_POWER: f32 = 0.012;
const BATTLE_SPACESHIP_MAX_THROTTLE: f32 = 2.0;
const BATTLE_SPACESHIP_LINEAR_DAMPENER: f32 = 0.35;
const BATTLE_SPACESHIP_MAX_DAMPENER_ACCELERATION: f32 = 18.0;
const BATTLE_SPACESHIP_ANGULAR_DAMPENER: f32 = 0.7;
const BATTLE_SPACESHIP_AIRFLOW_PER_SAIL_ROOT: f32 = 8.0;
const BATTLE_SPACESHIP_AIRFLOW_FRICTION_SCALE: f32 = 0.08;
const BATTLE_SPACESHIP_AIRFLOW_LIFETIME: f32 = 26.0;
const BATTLE_SPACESHIP_AIRFLOW_RESPONSE: f32 = 0.125;
const BATTLE_SPACESHIP_AIRFLOW_MAX_ACCELERATION: f32 = 36.0;
const BATTLE_SPACESHIP_AIRFLOW_MIN_RADIUS: f32 = 18.0;
const BATTLE_SPACESHIP_AIRFLOW_MAX_RADIUS: f32 = 90.0;
const BATTLE_SPACESHIP_DISASSEMBLE_MAX_SPEED: f32 = 8.0;
const BATTLE_SPACESHIP_DISASSEMBLE_MAX_ANGULAR_SPEED: f32 = 0.7;
const BATTLE_SPACESHIP_DISASSEMBLE_MAX_ROTATION_DEGREES: f32 = 25.0;
const BATTLE_SPACESHIP_DISASSEMBLY_ALIGN_MAX_SECONDS: f32 = 8.0;
const BATTLE_SPACESHIP_DISASSEMBLY_ALIGN_SPEED: f32 = 140.0;
const BATTLE_SPACESHIP_DISASSEMBLY_ALIGN_ROTATION_SPEED: f32 = 2.8;
const BATTLE_SPACESHIP_DISASSEMBLY_READY_TICKS: u8 = 6;
const BATTLE_SPACESHIP_DISASSEMBLY_TRANSLATION_TOLERANCE: f32 = 2.0;
const BATTLE_SPACESHIP_DISASSEMBLY_ROTATION_TOLERANCE_DEGREES: f32 = 2.0;
const BATTLE_SPACESHIP_UNSCALED_MIN: IVec3 = IVec3::new(-20, 0, -44);
const BATTLE_SPACESHIP_UNSCALED_MAX: IVec3 = IVec3::new(20, 19, 44);
const MAX_STATIC_VOXEL_COLLIDER_BOXES: usize = 8_192;
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
    edit_visibility: SceneVisibility,
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
            edit_visibility: SceneVisibility::Public,
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
    before: Option<PersistedVoxelState>,
    after: Option<PersistedVoxelState>,
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

#[derive(Component)]
struct BattleSpaceshipDocked;

#[derive(Component, Clone, Copy)]
struct BattleSpaceshipDisassemblyAlignment {
    target_translation: Vec3,
    target_rotation: Quat,
    ready_ticks: u8,
    elapsed_seconds: f32,
}

#[derive(Resource)]
struct BattleSpaceshipControlState {
    enabled: bool,
    throttle: f32,
    lift: f32,
    airflow: f32,
    dampeners: bool,
}

impl Default for BattleSpaceshipControlState {
    fn default() -> Self {
        Self {
            enabled: true,
            throttle: 0.35,
            lift: 1.0,
            airflow: 1.0,
            dampeners: true,
        }
    }
}

#[derive(Component, Clone)]
struct BattleSpaceshipPhysicsAssembly {
    local_center_of_mass: Vec3,
    mass: f32,
    angular_inertia: Vec3,
    bounds_min: Vec3,
    bounds_max: Vec3,
    propellers: Vec<BattleSpaceshipForcePoint>,
    lift_points: Vec<BattleSpaceshipForcePoint>,
    total_lift_strength: f32,
}

#[derive(Clone, Copy)]
struct BattleSpaceshipForcePoint {
    local_position: Vec3,
    local_direction: Vec3,
    strength: f32,
    airflow_speed: f32,
    airflow_radius: f32,
}

#[derive(Default)]
struct BattleSpaceshipForceAccumulator {
    weighted_position: Vec3,
    strength: f32,
}

impl BattleSpaceshipForceAccumulator {
    fn add(&mut self, local_position: Vec3, strength: f32) {
        let strength = strength.max(0.0);
        self.weighted_position += local_position * strength;
        self.strength += strength;
    }

    fn into_force_point(self, local_direction: Vec3) -> Option<BattleSpaceshipForcePoint> {
        (self.strength > 0.0).then(|| BattleSpaceshipForcePoint {
            local_position: self.weighted_position / self.strength,
            local_direction,
            strength: self.strength,
            airflow_speed: 0.0,
            airflow_radius: 0.0,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BattleSpaceshipDisassemblyStatus {
    Ready,
    TooFast,
    TooTilted,
}

impl BattleSpaceshipDisassemblyStatus {
    fn label(self) -> &'static str {
        match self {
            BattleSpaceshipDisassemblyStatus::Ready => "可解体",
            BattleSpaceshipDisassemblyStatus::TooFast => "速度过高",
            BattleSpaceshipDisassemblyStatus::TooTilted => "姿态未对齐",
        }
    }
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
}

#[derive(Component)]
struct SpaceHiFiVoxelPreview;

#[derive(Component)]
struct StaticVoxelCollisionRoot {
    boxes: usize,
}

#[derive(Component)]
struct SpaceHiFiDecorCollision;

#[derive(Component)]
struct VoxelPlanetDetailCollision;

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
    user_id: Option<u64>,
    restore_gm_view: bool,
    use_capture_camera: bool,
}

impl ScenePlayerViewRequest {
    pub fn view_with_capture_camera(&mut self, user_id: u64) {
        self.user_id = Some(user_id);
        self.use_capture_camera = true;
    }

    pub fn filter_current_view(&mut self, user_id: u64) {
        self.user_id = Some(user_id);
        self.use_capture_camera = false;
    }

    pub fn restore_gm_view(&mut self) { self.restore_gm_view = true; }
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
    standee_visibility_changes: Vec<SceneStandeeVisibilityChange>,
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

#[derive(Clone)]
struct SceneStandeeVisibilityChange {
    target_id: String,
    restore_visibility: Visibility,
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
    visibility: SceneVisibility,
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
    #[serde(default)]
    unit_scene_tokens: Vec<PersistedUnitSceneToken>,
    #[serde(default)]
    legacy_area_markers: Vec<PersistedLegacyAreaMarker>,
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
            unit_scene_tokens: Vec::new(),
            legacy_area_markers: Vec::new(),
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

        for token in imported.unit_scene_tokens {
            if token.token_id.trim().is_empty() {
                return Err("voxel scene export contains an empty unit scene token id".to_owned());
            }
            upsert_by(
                &mut self.unit_scene_tokens,
                token,
                |token| token.token_id.clone(),
            );
        }

        for marker in imported.legacy_area_markers {
            if marker.marker_id.trim().is_empty() {
                return Err(
                    "voxel scene export contains an empty legacy area marker id".to_owned(),
                );
            }
            upsert_by(
                &mut self.legacy_area_markers,
                marker,
                |marker| marker.marker_id.clone(),
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

pub fn unit_template_standee_target_id(unit_id: &str) -> String {
    format!(
        "{UNIT_TEMPLATE_STANDEE_PREFIX}{}",
        unit_id.trim()
    )
}

pub fn place_unit_template_standee(
    store: &mut VoxelSceneStore,
    unit_id: &str,
    image_source: &str,
) -> Result<bool, String> {
    let unit_id = unit_id.trim();
    if unit_id.is_empty() {
        return Err("单位ID为空".to_owned());
    }
    let image_source = image_source.trim();
    if image_source.is_empty() {
        return Err("单位模板还没有立绘".to_owned());
    }

    let target_id = unit_template_standee_target_id(unit_id);
    if let Some(existing) = store
        .character_standees
        .iter_mut()
        .find(|standee| standee.target_id == target_id)
    {
        if existing.image_source == image_source {
            return Ok(false);
        }
        existing.image_source = image_source.to_owned();
        return Ok(true);
    }

    let transform = default_character_camera_transform(store.character_standees.len());
    store.character_standees.push(PersistedCharacterStandee {
        target_id,
        image_source: image_source.to_owned(),
        translation: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
        visibility: SceneVisibility::Public,
    });
    Ok(true)
}

pub fn remove_unit_template_standee(store: &mut VoxelSceneStore, unit_id: &str) -> bool {
    let target_id = unit_template_standee_target_id(unit_id);
    let len = store.character_standees.len();
    store
        .character_standees
        .retain(|standee| standee.target_id != target_id);
    len != store.character_standees.len()
}

pub fn has_unit_template_standee(store: &VoxelSceneStore, unit_id: &str) -> bool {
    let target_id = unit_template_standee_target_id(unit_id);
    store
        .character_standees
        .iter()
        .any(|standee| standee.target_id == target_id)
}

pub fn unit_template_token_id(unit_id: &str) -> String {
    format!(
        "{UNIT_TEMPLATE_TOKEN_PREFIX}{}",
        unit_id.trim()
    )
}

pub fn legacy_world_unit_token_id(group_name: &str, world_id: &str, unit_id: &str) -> String {
    format!(
        "{UNIT_TEMPLATE_TOKEN_PREFIX}legacy-world:{}:{}:{}",
        group_name.trim(),
        world_id.trim(),
        unit_id.trim()
    )
}

fn legacy_world_unit_token_prefix(group_name: &str, world_id: &str) -> String {
    format!(
        "{UNIT_TEMPLATE_TOKEN_PREFIX}legacy-world:{}:{}:",
        group_name.trim(),
        world_id.trim()
    )
}

pub fn legacy_area_unit_token_id(
    group_name: &str,
    world_id: &str,
    area_id: &str,
    unit_id: &str,
) -> String {
    format!(
        "{UNIT_TEMPLATE_TOKEN_PREFIX}legacy-area:{}:{}:{}:{}",
        group_name.trim(),
        world_id.trim(),
        area_id.trim(),
        unit_id.trim()
    )
}

fn legacy_area_unit_token_prefix(group_name: &str, world_id: &str, area_id: &str) -> String {
    format!(
        "{UNIT_TEMPLATE_TOKEN_PREFIX}legacy-area:{}:{}:{}:",
        group_name.trim(),
        world_id.trim(),
        area_id.trim()
    )
}

pub fn place_unit_template_token(
    store: &mut VoxelSceneStore,
    unit_id: &str,
    label: &str,
) -> Result<bool, String> {
    let token_id = unit_template_token_id(unit_id);
    let translation = default_unit_scene_token_translation(store.unit_scene_tokens.len());
    upsert_unit_scene_token(
        store,
        &token_id,
        unit_id,
        label,
        translation,
        SceneVisibility::Public,
    )
}

pub fn place_legacy_world_unit_token(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    world_name: &str,
    unit_id: &str,
    label: &str,
    visible: bool,
) -> Result<bool, String> {
    let group_name = group_name.trim();
    let world_id = world_id.trim();
    if group_name.is_empty() {
        return Err("团名为空".to_owned());
    }
    if world_id.is_empty() {
        return Err("旧世界ID为空".to_owned());
    }
    let token_id = legacy_world_unit_token_id(group_name, world_id, unit_id);
    let world_name = world_name.trim();
    let label = if world_name.is_empty() {
        label.to_owned()
    } else {
        format!("{world_name}/{label}")
    };
    let translation = default_unit_scene_token_translation(store.unit_scene_tokens.len());
    upsert_unit_scene_token(
        store,
        &token_id,
        unit_id,
        &label,
        translation,
        if visible { SceneVisibility::Public } else { SceneVisibility::Gm },
    )
}

pub fn place_legacy_area_unit_token(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    area_id: &str,
    area_name: &str,
    unit_id: &str,
    label: &str,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    visible: bool,
    index: usize,
) -> Result<bool, String> {
    let group_name = group_name.trim();
    let world_id = world_id.trim();
    let area_id = area_id.trim();
    if group_name.is_empty() {
        return Err("团名为空".to_owned());
    }
    if world_id.is_empty() {
        return Err("旧世界ID为空".to_owned());
    }
    if area_id.is_empty() {
        return Err("旧区域ID为空".to_owned());
    }
    let token_id = legacy_area_unit_token_id(group_name, world_id, area_id, unit_id);
    let area_name = area_name.trim();
    let label = if area_name.is_empty() {
        label.to_owned()
    } else {
        format!("{area_name}/{label}")
    };
    upsert_unit_scene_token(
        store,
        &token_id,
        unit_id,
        &label,
        legacy_area_unit_token_translation(x, y, width, height, index),
        if visible { SceneVisibility::Public } else { SceneVisibility::Gm },
    )
}

pub fn prune_legacy_world_unit_tokens(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    keep_unit_ids: &[String],
) -> usize {
    let prefix = legacy_world_unit_token_prefix(group_name, world_id);
    let keep_token_ids = keep_unit_ids
        .iter()
        .map(|unit_id| legacy_world_unit_token_id(group_name, world_id, unit_id))
        .collect::<HashSet<_>>();
    let len = store.unit_scene_tokens.len();
    store.unit_scene_tokens.retain(|token| {
        !token.token_id.starts_with(&prefix) || keep_token_ids.contains(&token.token_id)
    });
    len - store.unit_scene_tokens.len()
}

pub fn prune_legacy_area_unit_tokens(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    area_id: &str,
    keep_unit_ids: &[String],
) -> usize {
    let prefix = legacy_area_unit_token_prefix(group_name, world_id, area_id);
    let keep_token_ids = keep_unit_ids
        .iter()
        .map(|unit_id| legacy_area_unit_token_id(group_name, world_id, area_id, unit_id))
        .collect::<HashSet<_>>();
    let len = store.unit_scene_tokens.len();
    store.unit_scene_tokens.retain(|token| {
        !token.token_id.starts_with(&prefix) || keep_token_ids.contains(&token.token_id)
    });
    len - store.unit_scene_tokens.len()
}

pub fn remove_legacy_world_unit_tokens(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
) -> usize {
    prune_legacy_world_unit_tokens(store, group_name, world_id, &[])
}

pub fn remove_legacy_area_unit_tokens(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    area_id: &str,
) -> usize {
    let keep_unit_ids: &[String] = &[];
    prune_legacy_area_unit_tokens(
        store,
        group_name,
        world_id,
        area_id,
        keep_unit_ids,
    )
}

fn upsert_unit_scene_token(
    store: &mut VoxelSceneStore,
    token_id: &str,
    unit_id: &str,
    label: &str,
    translation: [f32; 3],
    visibility: SceneVisibility,
) -> Result<bool, String> {
    let unit_id = unit_id.trim();
    if unit_id.is_empty() {
        return Err("单位ID为空".to_owned());
    }
    let token_id = token_id.trim();
    if token_id.is_empty() {
        return Err("单位标记ID为空".to_owned());
    }
    let label = label.trim();
    let label = if label.is_empty() { unit_id } else { label };

    if let Some(existing) = store
        .unit_scene_tokens
        .iter_mut()
        .find(|token| token.token_id == token_id)
    {
        let mut changed = false;
        if existing.unit_id != unit_id {
            existing.unit_id = unit_id.to_owned();
            changed = true;
        }
        if existing.label != label {
            existing.label = label.to_owned();
            changed = true;
        }
        if !changed {
            return Ok(false);
        }
        return Ok(true);
    }

    store.unit_scene_tokens.push(PersistedUnitSceneToken {
        token_id: token_id.to_owned(),
        unit_id: unit_id.to_owned(),
        label: label.to_owned(),
        translation,
        visibility,
    });
    Ok(true)
}

pub fn remove_unit_template_token(store: &mut VoxelSceneStore, unit_id: &str) -> bool {
    let token_id = unit_template_token_id(unit_id);
    let len = store.unit_scene_tokens.len();
    store
        .unit_scene_tokens
        .retain(|token| token.token_id != token_id);
    len != store.unit_scene_tokens.len()
}

pub fn has_unit_template_token(store: &VoxelSceneStore, unit_id: &str) -> bool {
    let token_id = unit_template_token_id(unit_id);
    store
        .unit_scene_tokens
        .iter()
        .any(|token| token.token_id == token_id)
}

fn default_unit_scene_token_translation(index: usize) -> [f32; 3] {
    let column = (index % 6) as f32;
    let row = (index / 6) as f32;
    [
        column * UNIT_SCENE_TOKEN_SPACING,
        UNIT_SCENE_TOKEN_Y,
        -3.0 - row * UNIT_SCENE_TOKEN_SPACING,
    ]
}

fn legacy_area_unit_token_translation(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    index: usize,
) -> [f32; 3] {
    let center_x = (x + width.max(1.0) * 0.5) * LEGACY_AREA_MARKER_SCALE;
    let center_z = (y + height.max(1.0) * 0.5) * LEGACY_AREA_MARKER_SCALE;
    let column = (index % 4) as f32 - 1.5;
    let row = (index / 4) as f32;
    [
        center_x + column * UNIT_SCENE_TOKEN_SPACING * 0.5,
        UNIT_SCENE_TOKEN_Y,
        center_z + row * UNIT_SCENE_TOKEN_SPACING * 0.5,
    ]
}

fn update_unit_scene_token_state(
    store: &mut VoxelSceneStore,
    token_id: &str,
    translation: [f32; 3],
    visibility: SceneVisibility,
) -> bool {
    let Some(token) = store
        .unit_scene_tokens
        .iter_mut()
        .find(|token| token.token_id == token_id)
    else {
        return false;
    };

    let mut changed = false;
    if token.translation != translation {
        token.translation = translation;
        changed = true;
    }
    if token.visibility != visibility {
        token.visibility = visibility;
        changed = true;
    }
    changed
}

pub fn legacy_area_marker_id(group_name: &str, world_id: &str, area_id: &str) -> String {
    format!(
        "legacy-area:{}:{}:{}",
        group_name.trim(),
        world_id.trim(),
        area_id.trim()
    )
}

pub fn place_legacy_area_marker(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    world_name: &str,
    area_id: &str,
    area_name: &str,
    combat: bool,
    members: &[String],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    visible: bool,
) -> Result<bool, String> {
    let group_name = group_name.trim();
    let world_id = world_id.trim();
    let area_id = area_id.trim();
    if group_name.is_empty() {
        return Err("团名为空".to_owned());
    }
    if world_id.is_empty() {
        return Err("旧世界ID为空".to_owned());
    }
    if area_id.is_empty() {
        return Err("旧区域ID为空".to_owned());
    }

    let marker_id = legacy_area_marker_id(group_name, world_id, area_id);
    let mut marker_members = members
        .iter()
        .map(|member| member.trim())
        .filter(|member| !member.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    marker_members.sort();
    marker_members.dedup();

    let marker = PersistedLegacyAreaMarker {
        marker_id,
        group_name: group_name.to_owned(),
        world_id: world_id.to_owned(),
        world_name: if world_name.trim().is_empty() {
            world_id.to_owned()
        } else {
            world_name.trim().to_owned()
        },
        area_id: area_id.to_owned(),
        area_name: if area_name.trim().is_empty() {
            area_id.to_owned()
        } else {
            area_name.trim().to_owned()
        },
        combat,
        members: marker_members,
        x,
        y,
        width: width.max(0.0),
        height: height.max(0.0),
        visibility: if visible { SceneVisibility::Public } else { SceneVisibility::Gm },
    };

    if let Some(existing) = store
        .legacy_area_markers
        .iter_mut()
        .find(|existing| existing.marker_id == marker.marker_id)
    {
        if existing == &marker {
            return Ok(false);
        }
        *existing = marker;
        return Ok(true);
    }

    store.legacy_area_markers.push(marker);
    Ok(true)
}

pub fn remove_legacy_area_marker(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    area_id: &str,
) -> bool {
    let marker_id = legacy_area_marker_id(group_name, world_id, area_id);
    let len = store.legacy_area_markers.len();
    store
        .legacy_area_markers
        .retain(|marker| marker.marker_id != marker_id);
    len != store.legacy_area_markers.len()
}

pub fn has_legacy_area_marker(
    store: &VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    area_id: &str,
) -> bool {
    let marker_id = legacy_area_marker_id(group_name, world_id, area_id);
    store
        .legacy_area_markers
        .iter()
        .any(|marker| marker.marker_id == marker_id)
}

pub fn stamp_legacy_area_marker_voxel_outline(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    area_id: &str,
) -> Result<usize, String> {
    stamp_legacy_area_marker_voxels(
        store, group_name, world_id, area_id, false,
    )
}

pub fn stamp_legacy_area_marker_voxel_fill(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    area_id: &str,
) -> Result<usize, String> {
    stamp_legacy_area_marker_voxels(
        store, group_name, world_id, area_id, true,
    )
}

fn stamp_legacy_area_marker_voxels(
    store: &mut VoxelSceneStore,
    group_name: &str,
    world_id: &str,
    area_id: &str,
    filled: bool,
) -> Result<usize, String> {
    ensure_voxel_maps_inner(store);
    let marker_id = legacy_area_marker_id(group_name, world_id, area_id);
    let Some(marker) = store
        .legacy_area_markers
        .iter()
        .find(|marker| marker.marker_id == marker_id)
        .cloned()
    else {
        return Err("场景里没有这个旧区域标记".to_owned());
    };
    let positions = if filled {
        legacy_area_marker_voxel_fill_positions(&marker)?
    } else {
        legacy_area_marker_voxel_outline_positions(&marker)
    };
    let material = legacy_area_marker_voxel_material(&marker);
    let visibility = marker.visibility.clone();
    let Some(map) = active_voxel_map_mut(store) else {
        return Err("没有可写入的场景地图".to_owned());
    };
    for position in &positions {
        upsert_persisted_edit_with_visibility(
            &mut map.edits,
            *position,
            PersistedVoxel::Solid(material),
            visibility.clone(),
        );
    }
    Ok(positions.len())
}

fn legacy_area_marker_voxel_material(marker: &PersistedLegacyAreaMarker) -> u8 {
    if marker.combat {
        MAT_ENGINE_RED
    } else {
        MAT_WINDOW_CYAN
    }
}

fn legacy_area_marker_voxel_outline_positions(marker: &PersistedLegacyAreaMarker) -> Vec<IVec3> {
    let (min_x, max_x, min_z, max_z) = legacy_area_marker_voxel_bounds(marker);
    let mut positions = Vec::new();
    for x in min_x..=max_x {
        for z in min_z..=max_z {
            if x == min_x || x == max_x || z == min_z || z == max_z {
                positions.push(IVec3::new(
                    x,
                    LEGACY_AREA_MARKER_VOXEL_Y,
                    z,
                ));
            }
        }
    }
    positions.sort_by_key(|position| ivec3_sort_key(*position));
    positions.dedup();
    positions
}

fn legacy_area_marker_voxel_fill_positions(
    marker: &PersistedLegacyAreaMarker,
) -> Result<Vec<IVec3>, String> {
    let (min_x, max_x, min_z, max_z) = legacy_area_marker_voxel_bounds(marker);
    let width = (max_x - min_x + 1).max(0) as usize;
    let depth = (max_z - min_z + 1).max(0) as usize;
    let Some(count) = width.checked_mul(depth) else {
        return Err("旧区域太大，无法写入体素填充".to_owned());
    };
    if count > LEGACY_AREA_MARKER_FILL_VOXEL_LIMIT {
        return Err(format!(
            "旧区域填充需要 {count} 格，超过上限 {} 格",
            LEGACY_AREA_MARKER_FILL_VOXEL_LIMIT
        ));
    }

    let mut positions = Vec::with_capacity(count);
    for x in min_x..=max_x {
        for z in min_z..=max_z {
            positions.push(IVec3::new(
                x,
                LEGACY_AREA_MARKER_VOXEL_Y,
                z,
            ));
        }
    }
    positions.sort_by_key(|position| ivec3_sort_key(*position));
    positions.dedup();
    Ok(positions)
}

fn legacy_area_marker_voxel_bounds(marker: &PersistedLegacyAreaMarker) -> (i32, i32, i32, i32) {
    let min_x = (marker.x * LEGACY_AREA_MARKER_SCALE).floor() as i32;
    let min_z = (marker.y * LEGACY_AREA_MARKER_SCALE).floor() as i32;
    let width = (marker.width.max(1.0) * LEGACY_AREA_MARKER_SCALE)
        .ceil()
        .max(1.0) as i32;
    let depth = (marker.height.max(1.0) * LEGACY_AREA_MARKER_SCALE)
        .ceil()
        .max(1.0) as i32;
    (
        min_x,
        min_x + width,
        min_z,
        min_z + depth,
    )
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct PersistedLegacyAreaMarker {
    marker_id: String,
    group_name: String,
    world_id: String,
    world_name: String,
    area_id: String,
    area_name: String,
    #[serde(default)]
    combat: bool,
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    width: f32,
    #[serde(default)]
    height: f32,
    #[serde(default)]
    visibility: SceneVisibility,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct PersistedUnitSceneToken {
    token_id: String,
    unit_id: String,
    label: String,
    #[serde(default)]
    translation: [f32; 3],
    #[serde(default)]
    visibility: SceneVisibility,
}

#[derive(Serialize, Deserialize, Clone)]
struct PersistedCharacterStandee {
    target_id: String,
    image_source: String,
    translation: [f32; 3],
    rotation: [f32; 4],
    #[serde(default)]
    visibility: SceneVisibility,
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
            .init_resource::<BattleSpaceshipControlState>()
            .init_resource::<SceneWaypointState>()
            .add_systems(Startup, setup_scene_preview)
            .add_systems(
                Update,
                (
                    draw_capture_camera_gizmos,
                    draw_legacy_area_marker_gizmos,
                    draw_unit_scene_token_gizmos,
                    draw_pickup_indicator_gizmo,
                    draw_battle_spaceship_airflow_gizmos,
                    draw_voxel_edit_preview_gizmo,
                    sync_character_standees,
                ),
            )
            .add_systems(
                Update,
                (
                    apply_saved_voxel_edits,
                    maintain_scene_player_voxel_view,
                    maintain_scene_player_standee_visibility,
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
                (
                    apply_planet_radial_gravity,
                    apply_battle_spaceship_aeronautics_forces,
                    apply_battle_spaceship_propeller_airflow,
                )
                    .chain()
                    .before(PhysicsStepSystems::First),
            )
            .add_systems(
                PostUpdate,
                (
                    apply_scene_player_view_request,
                    free_camera_system,
                    update_battle_spaceship_disassembly_alignment,
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
                    unit_scene_token_panel,
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
            shadow_maps_enabled: false,
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
    spawn_static_voxel_collision_previews(&mut commands);
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
            shadow_maps_enabled: false,
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

    let ship_voxels = procedural_battle_spaceship_voxels();
    if let Some(assembly) = battle_spaceship_physics_assembly(&ship_voxels) {
        let collider = battle_spaceship_assembly_collider(&assembly);
        let mass = Mass(assembly.mass);
        let center_of_mass = CenterOfMass(assembly.local_center_of_mass);
        let angular_inertia = AngularInertia::new(assembly.angular_inertia);
        commands
            .spawn((
                Transform::from_translation(battle_spaceship_translation),
                Visibility::Visible,
                BattleSpaceshipPreviewRoot,
                SpaceHiFiVoxelPreview,
                assembly,
                RigidBody::Dynamic,
                collider,
                mass,
                center_of_mass,
                angular_inertia,
                LinearVelocity::ZERO,
                AngularVelocity::ZERO,
                GravityScale(0.0),
                LinearDamping(0.16),
            ))
            .with_children(|parent| {
                for material in MAT_STAR..=MAT_PLANET_LAND {
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

fn spawn_static_voxel_collision_previews(commands: &mut Commands) {
    let decor_positions = space_hifi_decor_voxel_edits()
        .into_iter()
        .filter_map(|edit| {
            let PersistedVoxel::Solid(material) = edit.voxel else {
                return None;
            };
            (material != MAT_STAR).then_some(IVec3::new(
                edit.position[0],
                edit.position[1],
                edit.position[2],
            ))
        })
        .collect::<Vec<_>>();
    spawn_static_voxel_collision_root(
        commands,
        decor_positions,
        1,
        SpaceHiFiDecorCollision,
    );

    let detail_positions = voxel_planet_detail_preview_blocks()
        .keys()
        .copied()
        .collect::<Vec<_>>();
    spawn_static_voxel_collision_root(
        commands,
        detail_positions,
        VOXEL_PLANET_DETAIL_PREVIEW_BLOCK,
        VoxelPlanetDetailCollision,
    );
}

fn spawn_static_voxel_collision_root<T: Component>(
    commands: &mut Commands,
    positions: Vec<IVec3>,
    block_size: i32,
    marker: T,
) {
    let boxes = voxel_collision_boxes_from_positions(
        positions,
        block_size,
        MAX_STATIC_VOXEL_COLLIDER_BOXES,
    );
    let Some(collider) = voxel_collision_compound(&boxes) else {
        return;
    };
    commands.spawn((
        Transform::default(),
        RigidBody::Static,
        collider,
        StaticVoxelCollisionRoot { boxes: boxes.len() },
        marker,
    ));
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
            shadow_maps_enabled: false,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VoxelCollisionBox {
    origin: IVec3,
    size: IVec3,
}

fn voxel_collision_boxes_from_positions(
    positions: impl IntoIterator<Item = IVec3>,
    block_size: i32,
    max_boxes: usize,
) -> Vec<VoxelCollisionBox> {
    if block_size <= 0 || max_boxes == 0 {
        return Vec::new();
    }

    let mut remaining = positions
        .into_iter()
        .map(|position| {
            IVec3::new(
                position.x.div_euclid(block_size),
                position.y.div_euclid(block_size),
                position.z.div_euclid(block_size),
            )
        })
        .collect::<HashSet<_>>();
    let mut boxes = Vec::new();

    while !remaining.is_empty() && boxes.len() < max_boxes {
        let start = *remaining
            .iter()
            .min_by_key(|position| ivec3_sort_key(**position))
            .expect("remaining is not empty");
        let mut max_x = start.x;
        while remaining.contains(&IVec3::new(max_x + 1, start.y, start.z)) {
            max_x += 1;
        }

        let mut max_y = start.y;
        'expand_y: loop {
            let next_y = max_y + 1;
            for x in start.x..=max_x {
                if !remaining.contains(&IVec3::new(x, next_y, start.z)) {
                    break 'expand_y;
                }
            }
            max_y = next_y;
        }

        let mut max_z = start.z;
        'expand_z: loop {
            let next_z = max_z + 1;
            for x in start.x..=max_x {
                for y in start.y..=max_y {
                    if !remaining.contains(&IVec3::new(x, y, next_z)) {
                        break 'expand_z;
                    }
                }
            }
            max_z = next_z;
        }

        for x in start.x..=max_x {
            for y in start.y..=max_y {
                for z in start.z..=max_z {
                    remaining.remove(&IVec3::new(x, y, z));
                }
            }
        }

        let cell_size = IVec3::new(
            max_x - start.x + 1,
            max_y - start.y + 1,
            max_z - start.z + 1,
        );
        boxes.push(VoxelCollisionBox {
            origin: start * block_size,
            size: cell_size * block_size,
        });
    }

    boxes
}

fn voxel_collision_compound(boxes: &[VoxelCollisionBox]) -> Option<Collider> {
    if boxes.is_empty() {
        return None;
    }
    Some(Collider::compound(
        boxes
            .iter()
            .map(|collision_box| {
                let size = collision_box.size.as_vec3();
                let center = collision_box.origin.as_vec3() + size * 0.5;
                (
                    Position::new(center),
                    Rotation::IDENTITY,
                    Collider::cuboid(size.x, size.y, size.z),
                )
            })
            .collect::<Vec<_>>(),
    ))
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
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut editor: ResMut<VoxelEditorState>,
    mut ship_control: ResMut<BattleSpaceshipControlState>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    mut map_runtime: ResMut<VoxelMapRuntimeState>,
    mut battle_spaceships: Query<
        (
            Entity,
            &BattleSpaceshipPhysicsAssembly,
            &Transform,
            &RigidBody,
            &mut LinearVelocity,
            &mut AngularVelocity,
            Option<&BattleSpaceshipDocked>,
            Option<&BattleSpaceshipDisassemblyAlignment>,
        ),
        With<BattleSpaceshipPreviewRoot>,
    >,
    collision_roots: Query<&StaticVoxelCollisionRoot>,
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
            scene_visibility_selector_ui(
                ui,
                manager.as_deref(),
                &mut editor.edit_visibility,
                "新编辑可见性",
            );
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
                ui.label("飞船物理");
                if let Ok((
                    entity,
                    assembly,
                    ship_transform,
                    rigid_body,
                    mut linear_velocity,
                    mut angular_velocity,
                    docked,
                    alignment,
                )) = battle_spaceships.single_mut()
                {
                    let is_aligning = alignment.is_some() && docked.is_none();
                    let is_docked = docked.is_some() || (rigid_body.is_kinematic() && !is_aligning);
                    let disassembly_status = battle_spaceship_disassembly_status(
                        ship_transform.rotation,
                        linear_velocity.0,
                        angular_velocity.0,
                    );
                    ui.horizontal(|ui| {
                        if is_docked {
                            if ui.button("组装").clicked() {
                                linear_velocity.0 = Vec3::ZERO;
                                angular_velocity.0 = Vec3::ZERO;
                                commands
                                    .entity(entity)
                                    .insert(RigidBody::Dynamic)
                                    .remove::<BattleSpaceshipDocked>()
                                    .remove::<BattleSpaceshipDisassemblyAlignment>();
                                ship_control.enabled = true;
                            }
                        } else if is_aligning {
                            if ui.button("取消对齐").clicked() {
                                linear_velocity.0 = Vec3::ZERO;
                                angular_velocity.0 = Vec3::ZERO;
                                commands
                                    .entity(entity)
                                    .insert(RigidBody::Dynamic)
                                    .remove::<BattleSpaceshipDisassemblyAlignment>();
                                ship_control.enabled = true;
                            }
                        } else if ui
                            .add_enabled(
                                disassembly_status != BattleSpaceshipDisassemblyStatus::TooFast,
                                egui::Button::new("解体对齐"),
                            )
                            .clicked()
                        {
                            let target_rotation = battle_spaceship_disassembly_target_rotation(
                                ship_transform.rotation,
                            );
                            let target_translation =
                                battle_spaceship_disassembly_target_translation(
                                    ship_transform.translation,
                                    ship_transform.rotation,
                                    target_rotation,
                                    assembly.local_center_of_mass,
                                );
                            linear_velocity.0 = Vec3::ZERO;
                            angular_velocity.0 = Vec3::ZERO;
                            commands.entity(entity).insert((
                                RigidBody::Kinematic,
                                BattleSpaceshipDisassemblyAlignment {
                                    target_translation,
                                    target_rotation,
                                    ready_ticks: 0,
                                    elapsed_seconds: 0.0,
                                },
                            ));
                            ship_control.enabled = false;
                        }
                    });
                    ui.small(format!(
                        "状态 {}  {}",
                        if is_aligning {
                            "对齐中"
                        } else if is_docked {
                            "已停泊"
                        } else {
                            "已组装"
                        },
                        disassembly_status.label()
                    ));
                    if let Some(alignment) = alignment {
                        ui.small(format!(
                            "对齐 {:.1}s  稳定 {}/{}",
                            alignment.elapsed_seconds,
                            alignment.ready_ticks,
                            BATTLE_SPACESHIP_DISASSEMBLY_READY_TICKS
                        ));
                    }
                    ui.checkbox(&mut ship_control.enabled, "动力");
                    let controls_enabled = ship_control.enabled && !is_docked && !is_aligning;
                    ui.add_enabled(
                        controls_enabled,
                        egui::Slider::new(
                            &mut ship_control.throttle,
                            -1.0..=BATTLE_SPACESHIP_MAX_THROTTLE,
                        )
                        .text("推力"),
                    );
                    ui.add_enabled(
                        controls_enabled,
                        egui::Slider::new(&mut ship_control.lift, 0.0..=1.6).text("升力"),
                    );
                    ui.add_enabled(
                        controls_enabled,
                        egui::Slider::new(&mut ship_control.airflow, 0.0..=2.5).text("气流"),
                    );
                    ui.add_enabled_ui(controls_enabled, |ui| {
                        ui.checkbox(&mut ship_control.dampeners, "阻尼");
                    });
                    let max_airflow_range = assembly
                        .propellers
                        .iter()
                        .map(|propeller| {
                            battle_spaceship_airflow_range(
                                propeller.airflow_speed
                                    * ship_control.throttle.abs()
                                    * ship_control.airflow,
                            )
                            .abs()
                        })
                        .fold(0.0, f32::max);
                    ui.small(format!("质量 {:.0}", assembly.mass));
                    ui.small(format!(
                        "推进器 {}  浮力点 {}  气流 {:.0}",
                        assembly.propellers.len(),
                        assembly.lift_points.len(),
                        max_airflow_range
                    ));
                } else {
                    ui.small("未生成飞船");
                }
                let static_collision_boxes =
                    collision_roots.iter().map(|root| root.boxes).sum::<usize>();
                ui.small(format!(
                    "静态碰撞 {}",
                    static_collision_boxes
                ));
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

fn scene_visibility_selector_ui(
    ui: &mut egui::Ui,
    manager: Option<&Persistent<NapcatMessageManager>>,
    visibility: &mut SceneVisibility,
    label: &str,
) {
    scene_visibility_selector_ui_with_id(ui, manager, visibility, label, label);
}

fn scene_visibility_selector_ui_with_id(
    ui: &mut egui::Ui,
    manager: Option<&Persistent<NapcatMessageManager>>,
    visibility: &mut SceneVisibility,
    label: &str,
    id: impl Hash + std::fmt::Debug,
) {
    ui.label(label);
    egui::ComboBox::from_id_salt(("scene_visibility_selector", id))
        .selected_text(scene_visibility_label(
            manager, visibility,
        ))
        .show_ui(ui, |ui| {
            ui.selectable_value(
                visibility,
                SceneVisibility::Public,
                "公开",
            );
            ui.selectable_value(visibility, SceneVisibility::Gm, "仅GM");

            if let Some(group) = manager.and_then(|manager| manager.current_group()) {
                let mut parties = group
                    .parties
                    .iter()
                    .map(|(party_id, party)| {
                        let name = party.name.trim();
                        let label = if name.is_empty() || name == party_id {
                            format!("小队 {party_id}")
                        } else {
                            format!("小队 {name} ({party_id})")
                        };
                        (label, party_id.clone())
                    })
                    .collect::<Vec<_>>();
                parties.sort_by(|left, right| left.0.cmp(&right.0));
                for (label, party_id) in parties {
                    ui.selectable_value(
                        visibility,
                        SceneVisibility::Party(party_id),
                        label,
                    );
                }

                let mut players = group
                    .players
                    .iter()
                    .filter_map(|target_id| {
                        let user_id = target_id.parse::<u64>().ok()?;
                        Some((
                            scene_player_display_name(manager, user_id),
                            user_id,
                        ))
                    })
                    .collect::<Vec<_>>();
                players.sort_by(|left, right| left.0.cmp(&right.0));
                for (label, user_id) in players {
                    ui.selectable_value(
                        visibility,
                        SceneVisibility::Player(user_id),
                        format!("玩家 {label}"),
                    );
                }
            }
        });
}

fn scene_visibility_label(
    manager: Option<&Persistent<NapcatMessageManager>>,
    visibility: &SceneVisibility,
) -> String {
    match visibility {
        SceneVisibility::Public => "公开".to_owned(),
        SceneVisibility::Gm => "仅GM".to_owned(),
        SceneVisibility::Party(party_id) => manager
            .and_then(|manager| manager.current_group())
            .and_then(|group| group.parties.get(party_id))
            .map(|party| party.name.trim())
            .filter(|name| !name.is_empty() && *name != party_id.as_str())
            .map(|name| format!("小队 {name} ({party_id})"))
            .unwrap_or_else(|| format!("小队 {party_id}")),
        SceneVisibility::Player(user_id) => {
            format!(
                "玩家 {}",
                scene_player_display_name(manager, *user_id)
            )
        },
    }
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

fn unit_scene_token_panel(
    mut contexts: EguiContexts,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    mut store: Option<ResMut<Persistent<VoxelSceneStore>>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Window::new("单位场景标记")
        .default_pos(egui::pos2(512.0, 60.0))
        .default_width(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            let Some(store) = store.as_deref_mut() else {
                ui.small("场景未就绪");
                return;
            };
            if store.unit_scene_tokens.is_empty() {
                ui.small("还没有单位场景标记。");
                return;
            }

            let token_rows = store
                .unit_scene_tokens
                .iter()
                .enumerate()
                .map(|(index, token)| {
                    (
                        index,
                        token.token_id.clone(),
                        token.unit_id.clone(),
                        token.label.clone(),
                        token.translation,
                        token.visibility.clone(),
                    )
                })
                .collect::<Vec<_>>();
            let mut changed = false;
            for (index, token_id, unit_id, label, mut translation, mut visibility) in token_rows {
                let title = if label.trim().is_empty() {
                    format!("{unit_id} ({token_id})")
                } else {
                    format!("{} ({unit_id})", label.trim())
                };
                ui.collapsing(title, |ui| {
                    ui.small(format!("标记ID {token_id}"));
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut translation[0])
                                    .speed(0.1)
                                    .prefix("X "),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut translation[1])
                                    .speed(0.1)
                                    .prefix("Y "),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut translation[2])
                                    .speed(0.1)
                                    .prefix("Z "),
                            )
                            .changed();
                    });
                    if ui.button("重置位置").clicked() {
                        translation = default_unit_scene_token_translation(index);
                        changed = true;
                    }
                    let before_visibility = visibility.clone();
                    scene_visibility_selector_ui_with_id(
                        ui,
                        manager.as_deref(),
                        &mut visibility,
                        "可见范围",
                        token_id.as_str(),
                    );
                    changed |= visibility != before_visibility;
                });
                changed |= update_unit_scene_token_state(
                    store,
                    &token_id,
                    translation,
                    visibility,
                );
            }

            if changed {
                persist_voxel_store(store, "unit scene tokens");
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

fn battle_spaceship_preview_position(unscaled: IVec3) -> IVec3 {
    scaled_battle_spaceship_position(unscaled) + IVec3::Y * (BATTLE_SPACESHIP_SCALE - 1)
}

fn battle_spaceship_preview_origin(position: IVec3) -> Option<IVec3> {
    let unscaled = unscaled_battle_spaceship_position(position);
    let expected_position = battle_spaceship_preview_position(unscaled);
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

fn procedural_battle_spaceship_voxels() -> HashMap<IVec3, u8> {
    let mut voxels = HashMap::new();
    for x in BATTLE_SPACESHIP_UNSCALED_MIN.x..=BATTLE_SPACESHIP_UNSCALED_MAX.x {
        for y in BATTLE_SPACESHIP_UNSCALED_MIN.y..=BATTLE_SPACESHIP_UNSCALED_MAX.y {
            for z in BATTLE_SPACESHIP_UNSCALED_MIN.z..=BATTLE_SPACESHIP_UNSCALED_MAX.z {
                let unscaled = IVec3::new(x, y, z);
                if let Some(material) = procedural_battle_spaceship_unscaled(unscaled) {
                    voxels.insert(
                        battle_spaceship_preview_position(unscaled),
                        material,
                    );
                }
            }
        }
    }
    voxels
}

fn battle_spaceship_physics_assembly(
    voxels: &HashMap<IVec3, u8>,
) -> Option<BattleSpaceshipPhysicsAssembly> {
    let scale = BATTLE_SPACESHIP_SCALE as f32;
    let mut blocks = Vec::new();
    let mut mass = 0.0;
    let mut weighted_center = Vec3::ZERO;
    let mut bounds_min = Vec3::splat(f32::INFINITY);
    let mut bounds_max = Vec3::splat(f32::NEG_INFINITY);
    let mut propeller_groups = HashMap::<i32, BattleSpaceshipForceAccumulator>::new();
    let mut lift_groups = HashMap::<(i32, i32), BattleSpaceshipForceAccumulator>::new();

    for (&position, &material) in voxels {
        let Some(origin) = battle_spaceship_preview_origin(position) else {
            continue;
        };
        let unscaled = unscaled_battle_spaceship_position(position);
        let local_min = origin.as_vec3();
        let local_max = (origin + IVec3::splat(BATTLE_SPACESHIP_SCALE)).as_vec3();
        let local_center = local_min + Vec3::splat(scale * 0.5);
        let block_mass = BATTLE_SPACESHIP_BLOCK_MASS * battle_spaceship_material_mass(material);

        bounds_min = bounds_min.min(local_min);
        bounds_max = bounds_max.max(local_max);
        mass += block_mass;
        weighted_center += local_center * block_mass;
        blocks.push((local_center, block_mass));

        if material == MAT_ENGINE_RED {
            propeller_groups
                .entry(battle_spaceship_propeller_group(
                    unscaled,
                ))
                .or_default()
                .add(local_center, 1.0);
        }
        if battle_spaceship_is_lift_cell(material, unscaled) {
            lift_groups
                .entry(battle_spaceship_lift_group(unscaled))
                .or_default()
                .add(
                    local_center,
                    battle_spaceship_lift_strength(material, unscaled),
                );
        }
    }

    if mass <= 0.0 || blocks.is_empty() {
        return None;
    }

    let local_center_of_mass = weighted_center / mass;
    let mut angular_inertia = Vec3::ZERO;
    let cuboid_axis_inertia = scale * scale / 6.0;
    for (local_center, block_mass) in blocks {
        let offset = local_center - local_center_of_mass;
        angular_inertia.x +=
            block_mass * (cuboid_axis_inertia + offset.y * offset.y + offset.z * offset.z);
        angular_inertia.y +=
            block_mass * (cuboid_axis_inertia + offset.x * offset.x + offset.z * offset.z);
        angular_inertia.z +=
            block_mass * (cuboid_axis_inertia + offset.x * offset.x + offset.y * offset.y);
    }

    let propellers = propeller_groups
        .into_values()
        .filter_map(|accumulator| {
            let sail_power = accumulator.strength;
            let strength = battle_spaceship_propeller_acceleration(sail_power, 1.0);
            let mut point = accumulator.into_force_point(Vec3::Z)?;
            point.strength = strength;
            point.airflow_speed = battle_spaceship_propeller_airflow_speed(sail_power);
            point.airflow_radius = battle_spaceship_propeller_airflow_radius(sail_power);
            Some(point)
        })
        .collect::<Vec<_>>();
    let lift_points = lift_groups
        .into_values()
        .filter_map(|accumulator| accumulator.into_force_point(Vec3::Y))
        .collect::<Vec<_>>();
    let total_lift_strength = lift_points.iter().map(|point| point.strength).sum();

    Some(BattleSpaceshipPhysicsAssembly {
        local_center_of_mass,
        mass,
        angular_inertia,
        bounds_min,
        bounds_max,
        propellers,
        lift_points,
        total_lift_strength,
    })
}

fn battle_spaceship_assembly_collider(assembly: &BattleSpaceshipPhysicsAssembly) -> Collider {
    let size = (assembly.bounds_max - assembly.bounds_min).max(Vec3::splat(1.0));
    let center = assembly.bounds_min + size * 0.5;
    Collider::compound(vec![(
        Position::new(center),
        Rotation::IDENTITY,
        Collider::cuboid(size.x, size.y, size.z),
    )])
}

fn battle_spaceship_material_mass(material: u8) -> f32 {
    match material {
        MAT_ENGINE_RED => 2.2,
        MAT_STATION_TRIM => 1.5,
        MAT_HULL_DARK => 1.2,
        MAT_HULL_LIGHT => 0.9,
        MAT_WINDOW_CYAN => 0.35,
        _ => 1.0,
    }
}

fn battle_spaceship_propeller_group(unscaled: IVec3) -> i32 {
    if unscaled.x < -3 {
        -1
    } else if unscaled.x > 3 {
        1
    } else {
        0
    }
}

fn battle_spaceship_propeller_acceleration(sail_power: f32, throttle: f32) -> f32 {
    sail_power.max(0.0).powf(1.5)
        * BATTLE_SPACESHIP_THRUST_PER_SAIL_POWER
        * throttle.clamp(-1.0, BATTLE_SPACESHIP_MAX_THROTTLE)
}

fn battle_spaceship_propeller_airflow_speed(sail_power: f32) -> f32 {
    sail_power.max(0.0).sqrt() * BATTLE_SPACESHIP_AIRFLOW_PER_SAIL_ROOT
}

fn battle_spaceship_propeller_airflow_radius(sail_power: f32) -> f32 {
    (sail_power.max(0.0).sqrt() * BATTLE_SPACESHIP_SCALE as f32).clamp(
        BATTLE_SPACESHIP_AIRFLOW_MIN_RADIUS,
        BATTLE_SPACESHIP_AIRFLOW_MAX_RADIUS,
    )
}

fn battle_spaceship_airflow_range(airflow_speed: f32) -> f32 {
    let tick_speed = airflow_speed / 20.0;
    if tick_speed.abs() <= f32::EPSILON {
        return 0.0;
    }
    tick_speed.signum()
        * ((tick_speed.abs()
            * BATTLE_SPACESHIP_AIRFLOW_FRICTION_SCALE
            * BATTLE_SPACESHIP_AIRFLOW_LIFETIME
            + 1.0)
            .ln()
            / BATTLE_SPACESHIP_AIRFLOW_FRICTION_SCALE)
        * BATTLE_SPACESHIP_SCALE as f32
}

fn battle_spaceship_airflow_delta_velocity(
    target_position: Vec3,
    target_velocity: Vec3,
    propeller_position: Vec3,
    airflow_direction: Vec3,
    airflow_speed: f32,
    airflow_radius: f32,
    throttle: f32,
    airflow_scale: f32,
    delta_seconds: f32,
) -> Vec3 {
    if delta_seconds <= 0.0 || airflow_radius <= 0.0 {
        return Vec3::ZERO;
    }

    let signed_speed = airflow_speed * throttle * airflow_scale;
    if signed_speed.abs() <= 0.001 {
        return Vec3::ZERO;
    }
    let mut direction = airflow_direction.try_normalize().unwrap_or(Vec3::ZERO);
    if direction == Vec3::ZERO {
        return Vec3::ZERO;
    }
    if signed_speed < 0.0 {
        direction = -direction;
    }

    let speed = signed_speed.abs();
    let range = battle_spaceship_airflow_range(speed).abs();
    if range <= 0.0 {
        return Vec3::ZERO;
    }

    let offset = target_position - propeller_position;
    let axial_distance = direction.dot(offset);
    if axial_distance <= 0.0 || axial_distance > range {
        return Vec3::ZERO;
    }

    let radial_offset = offset - direction * axial_distance;
    let radial_distance = radial_offset.length();
    if radial_distance > airflow_radius {
        return Vec3::ZERO;
    }

    let distance_blocks = axial_distance / BATTLE_SPACESHIP_SCALE as f32;
    let radial_t = (radial_distance / airflow_radius).clamp(0.0, 1.0);
    let falloff =
        (-distance_blocks * BATTLE_SPACESHIP_AIRFLOW_FRICTION_SCALE - radial_t.powi(4) * 0.8).exp();
    let desired_velocity = direction * speed * falloff;
    let max_delta = BATTLE_SPACESHIP_AIRFLOW_MAX_ACCELERATION * delta_seconds;

    (desired_velocity - target_velocity).clamp_length_max(max_delta)
        * BATTLE_SPACESHIP_AIRFLOW_RESPONSE
}

fn battle_spaceship_is_lift_cell(material: u8, unscaled: IVec3) -> bool {
    matches!(
        material,
        MAT_HULL_LIGHT | MAT_WINDOW_CYAN
    ) && unscaled.y >= 8
}

fn battle_spaceship_lift_group(unscaled: IVec3) -> (i32, i32) {
    let x_side = if unscaled.x < 0 { -1 } else { 1 };
    let z_side = if unscaled.z < 0 { -1 } else { 1 };
    (x_side, z_side)
}

fn battle_spaceship_lift_strength(material: u8, unscaled: IVec3) -> f32 {
    let height_bias = ((unscaled.y - 8) as f32 / 12.0).clamp(0.0, 1.0);
    match material {
        MAT_WINDOW_CYAN => 1.4 + height_bias,
        MAT_HULL_LIGHT => 0.35 + height_bias * 0.25,
        _ => 0.0,
    }
}

fn battle_spaceship_disassembly_status(
    rotation: Quat,
    linear_velocity: Vec3,
    angular_velocity: Vec3,
) -> BattleSpaceshipDisassemblyStatus {
    if linear_velocity.length() > BATTLE_SPACESHIP_DISASSEMBLE_MAX_SPEED
        || angular_velocity.length() > BATTLE_SPACESHIP_DISASSEMBLE_MAX_ANGULAR_SPEED
    {
        return BattleSpaceshipDisassemblyStatus::TooFast;
    }

    if rotation.angle_between(Quat::IDENTITY).to_degrees()
        > BATTLE_SPACESHIP_DISASSEMBLE_MAX_ROTATION_DEGREES
    {
        return BattleSpaceshipDisassemblyStatus::TooTilted;
    }

    BattleSpaceshipDisassemblyStatus::Ready
}

fn battle_spaceship_disassembly_target_rotation(rotation: Quat) -> Quat {
    let (yaw, ..) = rotation.to_euler(EulerRot::YXZ);
    let snapped_yaw = (yaw / std::f32::consts::FRAC_PI_2).round() * std::f32::consts::FRAC_PI_2;
    Quat::from_rotation_y(snapped_yaw)
}

fn battle_spaceship_disassembly_target_translation(
    translation: Vec3,
    rotation: Quat,
    target_rotation: Quat,
    local_center_of_mass: Vec3,
) -> Vec3 {
    let current_center_of_mass = translation + rotation * local_center_of_mass;
    let grid = BATTLE_SPACESHIP_SCALE as f32;
    let target_center_of_mass = Vec3::new(
        battle_spaceship_nearest_grid_center(current_center_of_mass.x, grid),
        battle_spaceship_nearest_grid_center(current_center_of_mass.y, grid),
        battle_spaceship_nearest_grid_center(current_center_of_mass.z, grid),
    );

    target_center_of_mass - target_rotation * local_center_of_mass
}

fn battle_spaceship_nearest_grid_center(value: f32, grid: f32) -> f32 {
    ((value / grid) - 0.5).round().mul_add(grid, grid * 0.5)
}

fn battle_spaceship_move_towards(current: Vec3, target: Vec3, max_step: f32) -> Vec3 {
    let offset = target - current;
    let distance = offset.length();
    if distance <= max_step || distance <= f32::EPSILON {
        target
    } else {
        current + offset / distance * max_step
    }
}

fn battle_spaceship_rotate_towards(current: Quat, target: Quat, max_angle: f32) -> Quat {
    let angle = current.angle_between(target);
    if angle <= max_angle || angle <= f32::EPSILON {
        target
    } else {
        current
            .slerp(
                target,
                (max_angle / angle).clamp(0.0, 1.0),
            )
            .normalize()
    }
}

fn battle_spaceship_alignment_is_ready(
    transform: &Transform,
    alignment: &BattleSpaceshipDisassemblyAlignment,
) -> bool {
    transform.translation.distance(alignment.target_translation)
        <= BATTLE_SPACESHIP_DISASSEMBLY_TRANSLATION_TOLERANCE
        && transform
            .rotation
            .angle_between(alignment.target_rotation)
            .to_degrees()
            <= BATTLE_SPACESHIP_DISASSEMBLY_ROTATION_TOLERANCE_DEGREES
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
                        player_view_request.restore_gm_view();
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

            ui.separator();
            ui.label("立绘可见性");
            let selected_target_id = selected_user_id.to_string();
            if let Some(store) = store.as_deref_mut() {
                if let Some(index) = store
                    .character_standees
                    .iter()
                    .position(|standee| standee.target_id == selected_target_id)
                {
                    let before = store.character_standees[index].visibility.clone();
                    scene_visibility_selector_ui(
                        ui,
                        manager.as_deref(),
                        &mut store.character_standees[index].visibility,
                        "可见范围",
                    );
                    if store.character_standees[index].visibility != before {
                        if let Err(err) = store.persist() {
                            eprintln!("failed to persist standee visibility: {err}");
                        }
                    }
                } else {
                    ui.small("这个玩家还没有角色立绘。");
                }
            }

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
                if ui.button("过滤当前视角").clicked() {
                    player_view_request.filter_current_view(selected_user_id);
                }
                if ui.button("查看捕捉视角").clicked() {
                    player_view_request.view_with_capture_camera(selected_user_id);
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
    if request.use_capture_camera {
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

fn maintain_scene_player_standee_visibility(
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    capture_state: Res<SceneCaptureState>,
    player_view_state: Res<ScenePlayerVoxelViewState>,
    mut standees: Query<(&CharacterStandee, &mut Visibility)>,
) {
    if capture_state
        .pending_captures
        .iter()
        .any(|pending| pending.started_preparing)
    {
        return;
    }

    let access = player_view_state
        .active_user_id
        .map(|user_id| scene_capture_player_access(manager.as_deref(), user_id));
    apply_scene_player_standee_visibility(&mut standees, access.as_ref());
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

fn apply_battle_spaceship_aeronautics_forces(
    control: Res<BattleSpaceshipControlState>,
    mut ships: Query<
        (
            &BattleSpaceshipPhysicsAssembly,
            Option<&BattleSpaceshipDocked>,
            Option<&BattleSpaceshipDisassemblyAlignment>,
            Forces,
        ),
        With<BattleSpaceshipPreviewRoot>,
    >,
) {
    let throttle = control.throttle.clamp(-1.0, BATTLE_SPACESHIP_MAX_THROTTLE);
    let lift = control.lift.clamp(0.0, 1.6);

    for (assembly, docked, alignment, mut forces) in &mut ships {
        if docked.is_some() || alignment.is_some() {
            continue;
        }
        let position = forces.position().0;
        let rotation = forces.rotation().0;
        let world_center_of_mass = position + rotation * assembly.local_center_of_mass;

        let gravity_direction = planet_gravity_direction_at(world_center_of_mass);
        if gravity_direction != Vec3::ZERO {
            forces.apply_linear_acceleration(gravity_direction * PLANET_GRAVITY_ACCELERATION);
        }

        if !control.enabled {
            continue;
        }

        if lift > 0.0 && assembly.total_lift_strength > 0.0 {
            let lift_direction = planet_outward_at(world_center_of_mass);
            let lift_acceleration = PLANET_GRAVITY_ACCELERATION * lift;
            for point in &assembly.lift_points {
                let world_point = position + rotation * point.local_position;
                let share = point.strength / assembly.total_lift_strength;
                forces.apply_linear_acceleration_at_point(
                    lift_direction * lift_acceleration * share,
                    world_point,
                );
            }
        }

        if throttle.abs() > 0.001 {
            for point in &assembly.propellers {
                let thrust_direction = (rotation * point.local_direction)
                    .try_normalize()
                    .unwrap_or(Vec3::ZERO);
                if thrust_direction == Vec3::ZERO {
                    continue;
                }
                let world_point = position + rotation * point.local_position;
                forces.apply_linear_acceleration_at_point(
                    thrust_direction * point.strength * throttle,
                    world_point,
                );
            }
        }

        if control.dampeners {
            let velocity = forces.linear_velocity();
            if velocity.length_squared() > 0.0001 {
                let acceleration = (-velocity * BATTLE_SPACESHIP_LINEAR_DAMPENER)
                    .clamp_length_max(BATTLE_SPACESHIP_MAX_DAMPENER_ACCELERATION);
                forces.apply_linear_acceleration(acceleration);
            }
            let angular_velocity = forces.angular_velocity();
            if angular_velocity.length_squared() > 0.0001 {
                forces.apply_angular_acceleration(
                    -angular_velocity * BATTLE_SPACESHIP_ANGULAR_DAMPENER,
                );
            }
        }
    }
}

fn apply_battle_spaceship_propeller_airflow(
    control: Res<BattleSpaceshipControlState>,
    time: Res<Time>,
    ships: Query<
        (
            &Transform,
            &BattleSpaceshipPhysicsAssembly,
            Option<&BattleSpaceshipDocked>,
            Option<&BattleSpaceshipDisassemblyAlignment>,
        ),
        With<BattleSpaceshipPreviewRoot>,
    >,
    mut physics_voxels: Query<
        (&Transform, &mut LinearVelocity),
        (
            With<PhysicsVoxel>,
            Without<BattleSpaceshipPreviewRoot>,
            Without<HeldPhysicsVoxel>,
        ),
    >,
) {
    if !control.enabled || control.airflow <= 0.0 {
        return;
    }
    let throttle = control.throttle.clamp(-1.0, BATTLE_SPACESHIP_MAX_THROTTLE);
    if throttle.abs() <= 0.001 {
        return;
    }

    let airflow_scale = control.airflow.clamp(0.0, 2.5);
    let delta_seconds = time.delta_secs();
    for (target_transform, mut target_velocity) in &mut physics_voxels {
        let mut delta_velocity = Vec3::ZERO;
        for (ship_transform, assembly, docked, alignment) in &ships {
            if docked.is_some() || alignment.is_some() {
                continue;
            }
            for propeller in &assembly.propellers {
                let propeller_position =
                    ship_transform.translation + ship_transform.rotation * propeller.local_position;
                let airflow_direction = ship_transform.rotation * propeller.local_direction;
                delta_velocity += battle_spaceship_airflow_delta_velocity(
                    target_transform.translation,
                    target_velocity.0,
                    propeller_position,
                    airflow_direction,
                    propeller.airflow_speed,
                    propeller.airflow_radius,
                    throttle,
                    airflow_scale,
                    delta_seconds,
                );
            }
        }
        target_velocity.0 += delta_velocity;
    }
}

fn update_battle_spaceship_disassembly_alignment(
    mut commands: Commands,
    time: Res<Time>,
    mut ship_control: ResMut<BattleSpaceshipControlState>,
    mut ships: Query<
        (
            Entity,
            &mut Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
            &mut BattleSpaceshipDisassemblyAlignment,
        ),
        (
            With<BattleSpaceshipPreviewRoot>,
            Without<BattleSpaceshipDocked>,
        ),
    >,
) {
    let delta_seconds = time.delta_secs();
    let max_translation_step = BATTLE_SPACESHIP_DISASSEMBLY_ALIGN_SPEED * delta_seconds;
    let max_rotation_step = BATTLE_SPACESHIP_DISASSEMBLY_ALIGN_ROTATION_SPEED * delta_seconds;

    for (entity, mut transform, mut linear_velocity, mut angular_velocity, mut alignment) in
        &mut ships
    {
        alignment.elapsed_seconds += delta_seconds;
        linear_velocity.0 = Vec3::ZERO;
        angular_velocity.0 = Vec3::ZERO;
        transform.translation = battle_spaceship_move_towards(
            transform.translation,
            alignment.target_translation,
            max_translation_step,
        );
        transform.rotation = battle_spaceship_rotate_towards(
            transform.rotation,
            alignment.target_rotation,
            max_rotation_step,
        );

        if battle_spaceship_alignment_is_ready(&transform, &alignment) {
            alignment.ready_ticks = alignment.ready_ticks.saturating_add(1);
        } else {
            alignment.ready_ticks = 0;
        }

        if alignment.ready_ticks >= BATTLE_SPACESHIP_DISASSEMBLY_READY_TICKS {
            transform.translation = alignment.target_translation;
            transform.rotation = alignment.target_rotation;
            commands
                .entity(entity)
                .insert((
                    RigidBody::Kinematic,
                    BattleSpaceshipDocked,
                ))
                .remove::<BattleSpaceshipDisassemblyAlignment>();
            ship_control.enabled = false;
        } else if alignment.elapsed_seconds >= BATTLE_SPACESHIP_DISASSEMBLY_ALIGN_MAX_SECONDS {
            commands
                .entity(entity)
                .insert(RigidBody::Dynamic)
                .remove::<BattleSpaceshipDisassemblyAlignment>();
            ship_control.enabled = true;
        }
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
        (
            &mut Transform,
            Option<&BattleSpaceshipDisassemblyAlignment>,
        ),
        (
            With<BattleSpaceshipPreviewRoot>,
            Without<PhysicsVoxel>,
            Without<FreeCamera>,
        ),
    >,
    mut battle_spaceship_velocities: Query<
        (
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
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
        if let Ok((mut ship_transform, alignment)) = battle_spaceships.single_mut() {
            if alignment.is_some() {
                ship_grab_state.held = false;
            } else {
                ship_transform.translation = held_ship_position - ship_grab_state.grab_local_offset;
            }
            if let Ok((mut linear_velocity, mut angular_velocity)) =
                battle_spaceship_velocities.single_mut()
            {
                linear_velocity.0 = Vec3::ZERO;
                angular_velocity.0 = Vec3::ZERO;
            }
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
        if let (Ok((ship_transform, alignment)), Some(store)) = (
            battle_spaceships.single(),
            store.as_deref_mut(),
        ) {
            if alignment.is_none() {
                store.battle_spaceship_translation = ship_transform.translation.to_array();
                if let Err(err) = store.persist() {
                    eprintln!("failed to persist battle spaceship transform: {err}");
                }
            }
        }
        if let Ok((mut linear_velocity, mut angular_velocity)) =
            battle_spaceship_velocities.single_mut()
        {
            linear_velocity.0 = Vec3::ZERO;
            angular_velocity.0 = Vec3::ZERO;
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

    if let Ok((ship_transform, alignment)) = battle_spaceships.single() {
        if alignment.is_none() {
            if let Some(hit_distance) =
                battle_spaceship_ray_intersection(ray, ship_transform.translation)
            {
                let hit_position = ray.origin + *ray.direction * hit_distance;
                ship_grab_state.grab_local_offset = hit_position - ship_transform.translation;
                ship_grab_state.held = true;
                if let Ok((mut linear_velocity, mut angular_velocity)) =
                    battle_spaceship_velocities.single_mut()
                {
                    linear_velocity.0 = Vec3::ZERO;
                    angular_velocity.0 = Vec3::ZERO;
                }
                return;
            }
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
    let min = BATTLE_SPACESHIP_UNSCALED_MIN.as_vec3() * scale + translation;
    let max = (BATTLE_SPACESHIP_UNSCALED_MAX + IVec3::ONE).as_vec3() * scale + translation;
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

fn draw_legacy_area_marker_gizmos(
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    store: Option<Res<Persistent<VoxelSceneStore>>>,
    capture_state: Res<SceneCaptureState>,
    player_view_state: Res<ScenePlayerVoxelViewState>,
    mut gizmos: Gizmos,
) {
    let Some(store) = store else {
        return;
    };
    let access = scene_overlay_access(
        manager.as_deref(),
        &capture_state,
        &player_view_state,
    );
    for marker in &store.legacy_area_markers {
        if !legacy_area_marker_visible_for_access(marker, access.as_ref()) {
            continue;
        }
        draw_legacy_area_marker_gizmo(&mut gizmos, marker);
    }
}

fn draw_unit_scene_token_gizmos(
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    store: Option<Res<Persistent<VoxelSceneStore>>>,
    capture_state: Res<SceneCaptureState>,
    player_view_state: Res<ScenePlayerVoxelViewState>,
    mut gizmos: Gizmos,
) {
    let Some(store) = store else {
        return;
    };
    let access = scene_overlay_access(
        manager.as_deref(),
        &capture_state,
        &player_view_state,
    );
    for token in &store.unit_scene_tokens {
        if !unit_scene_token_visible_for_access(token, access.as_ref()) {
            continue;
        }
        draw_unit_scene_token_gizmo(&mut gizmos, token);
    }
}

fn scene_overlay_access(
    manager: Option<&Persistent<NapcatMessageManager>>,
    capture_state: &SceneCaptureState,
    player_view_state: &ScenePlayerVoxelViewState,
) -> Option<PlayerAccess> {
    capture_state
        .pending_captures
        .iter()
        .find(|pending| pending.started_preparing)
        .map(|pending| pending.user_id)
        .or(player_view_state.active_user_id)
        .map(|user_id| scene_capture_player_access(manager, user_id))
}

fn unit_scene_token_visible_for_access(
    token: &PersistedUnitSceneToken,
    access: Option<&PlayerAccess>,
) -> bool {
    access
        .map(|access| token.visibility.can_read_for_access(access))
        .unwrap_or(true)
}

fn legacy_area_marker_visible_for_access(
    marker: &PersistedLegacyAreaMarker,
    access: Option<&PlayerAccess>,
) -> bool {
    access
        .map(|access| marker.visibility.can_read_for_access(access))
        .unwrap_or(true)
}

fn draw_legacy_area_marker_gizmo(gizmos: &mut Gizmos, marker: &PersistedLegacyAreaMarker) {
    let color = legacy_area_marker_color(marker);
    let corners = legacy_area_marker_corners(marker);
    for (start, end) in [
        (corners[0], corners[1]),
        (corners[1], corners[2]),
        (corners[2], corners[3]),
        (corners[3], corners[0]),
    ] {
        gizmos.line(start, end, color);
    }

    let center = legacy_area_marker_center(marker);
    let pin_top = center + Vec3::Y * 1.2;
    gizmos.line(center, pin_top, color);
    gizmos.sphere(
        Isometry3d::from_translation(pin_top),
        0.16,
        color,
    );

    if marker.combat {
        gizmos.line(corners[0], corners[2], color);
        gizmos.line(corners[1], corners[3], color);
    }
}

fn draw_unit_scene_token_gizmo(gizmos: &mut Gizmos, token: &PersistedUnitSceneToken) {
    let color = unit_scene_token_color(token);
    let center = Vec3::from(token.translation);
    gizmos.sphere(
        Isometry3d::from_translation(center),
        0.28,
        color,
    );
    gizmos.line(
        center - Vec3::X * 0.45,
        center + Vec3::X * 0.45,
        color,
    );
    gizmos.line(
        center - Vec3::Z * 0.45,
        center + Vec3::Z * 0.45,
        color,
    );
    gizmos.line(center, center + Vec3::Y * 0.9, color);
}

fn unit_scene_token_color(token: &PersistedUnitSceneToken) -> Color {
    match &token.visibility {
        SceneVisibility::Public => Color::srgb(0.2, 0.92, 1.0),
        SceneVisibility::Party(_) => Color::srgb(1.0, 0.72, 0.18),
        SceneVisibility::Player(_) => Color::srgb(0.22, 0.88, 0.34),
        SceneVisibility::Gm => Color::srgb(0.95, 0.45, 1.0),
    }
}

fn legacy_area_marker_color(marker: &PersistedLegacyAreaMarker) -> Color {
    match (&marker.visibility, marker.combat) {
        (SceneVisibility::Gm, _) => Color::srgb(0.95, 0.45, 1.0),
        (_, true) => Color::srgb(1.0, 0.32, 0.18),
        _ => Color::srgb(0.15, 0.82, 1.0),
    }
}

fn legacy_area_marker_center(marker: &PersistedLegacyAreaMarker) -> Vec3 {
    Vec3::new(
        (marker.x + marker.width.max(1.0) * 0.5) * LEGACY_AREA_MARKER_SCALE,
        LEGACY_AREA_MARKER_Y,
        (marker.y + marker.height.max(1.0) * 0.5) * LEGACY_AREA_MARKER_SCALE,
    )
}

fn legacy_area_marker_corners(marker: &PersistedLegacyAreaMarker) -> [Vec3; 4] {
    let center = legacy_area_marker_center(marker);
    let half_width = marker.width.max(1.0) * LEGACY_AREA_MARKER_SCALE * 0.5;
    let half_depth = marker.height.max(1.0) * LEGACY_AREA_MARKER_SCALE * 0.5;
    [
        center + Vec3::new(-half_width, 0.0, -half_depth),
        center + Vec3::new(half_width, 0.0, -half_depth),
        center + Vec3::new(half_width, 0.0, half_depth),
        center + Vec3::new(-half_width, 0.0, half_depth),
    ]
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

fn draw_battle_spaceship_airflow_gizmos(
    control: Res<BattleSpaceshipControlState>,
    ships: Query<
        (
            &Transform,
            &BattleSpaceshipPhysicsAssembly,
            Option<&BattleSpaceshipDocked>,
            Option<&BattleSpaceshipDisassemblyAlignment>,
        ),
        With<BattleSpaceshipPreviewRoot>,
    >,
    mut gizmos: Gizmos,
) {
    if !control.enabled || control.airflow <= 0.0 {
        return;
    }
    let throttle = control.throttle.clamp(-1.0, BATTLE_SPACESHIP_MAX_THROTTLE);
    if throttle.abs() <= 0.001 {
        return;
    }
    let airflow_scale = control.airflow.clamp(0.0, 2.5);
    for (ship_transform, assembly, docked, alignment) in &ships {
        if docked.is_some() || alignment.is_some() {
            continue;
        }
        for propeller in &assembly.propellers {
            let signed_speed = propeller.airflow_speed * throttle * airflow_scale;
            let range = battle_spaceship_airflow_range(signed_speed);
            if range.abs() <= 0.001 {
                continue;
            }
            let origin =
                ship_transform.translation + ship_transform.rotation * propeller.local_position;
            let direction = (ship_transform.rotation * propeller.local_direction)
                .try_normalize()
                .unwrap_or(Vec3::ZERO);
            if direction == Vec3::ZERO {
                continue;
            }
            let end = origin + direction * range;
            gizmos.arrow(origin, end, Color::srgb(0.0, 0.72, 1.0));
        }
    }
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
                editor.edit_visibility.clone(),
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
        editor.edit_visibility.clone(),
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
    mut standee_visibility_query: Query<(&CharacterStandee, &mut Visibility)>,
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
            standee_visibility_changes: Vec::new(),
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
        current.standee_visibility_changes =
            apply_scene_capture_standee_visibility(&mut standee_visibility_query, &access);
        current.started_preparing = true;
        return;
    }

    let access = scene_capture_player_access(manager.as_deref(), current.user_id);
    apply_scene_player_standee_visibility(
        &mut standee_visibility_query,
        Some(&access),
    );

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
                      mut cameras: Query<&mut Camera, With<PlayerCaptureCamera>>,
                      mut standee_visibility_query: Query<(&CharacterStandee, &mut Visibility)>| {
                    if let Ok(mut camera) = cameras.get_mut(pending.camera_entity) {
                        camera.is_active = false;
                    }
                    apply_scene_capture_voxel_view(
                        &mut voxel_world,
                        &pending.voxel_view_changes,
                        SceneCaptureVoxelView::Restore,
                    );
                    restore_scene_standee_visibility(
                        &mut standee_visibility_query,
                        &pending.standee_visibility_changes,
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
                        apply_scene_player_standee_visibility(
                            &mut standee_visibility_query,
                            Some(&access),
                        );
                    } else {
                        apply_scene_player_standee_visibility(
                            &mut standee_visibility_query,
                            None,
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

fn scene_standee_visibility_for_access(
    visibility: &SceneVisibility,
    access: Option<&PlayerAccess>,
) -> Visibility {
    match access {
        Some(access) if !visibility.can_read_for_access(access) => Visibility::Hidden,
        _ => Visibility::Visible,
    }
}

fn apply_scene_player_standee_visibility(
    standees: &mut Query<(&CharacterStandee, &mut Visibility)>,
    access: Option<&PlayerAccess>,
) {
    for (standee, mut bevy_visibility) in standees.iter_mut() {
        *bevy_visibility = scene_standee_visibility_for_access(&standee.visibility, access);
    }
}

fn apply_scene_capture_standee_visibility(
    standees: &mut Query<(&CharacterStandee, &mut Visibility)>,
    access: &PlayerAccess,
) -> Vec<SceneStandeeVisibilityChange> {
    let mut changes = Vec::new();
    for (standee, mut bevy_visibility) in standees.iter_mut() {
        let next_visibility =
            scene_standee_visibility_for_access(&standee.visibility, Some(access));
        if *bevy_visibility == next_visibility {
            continue;
        }
        changes.push(SceneStandeeVisibilityChange {
            target_id: standee.target_id.clone(),
            restore_visibility: *bevy_visibility,
        });
        *bevy_visibility = next_visibility;
    }
    changes.sort_by(|left, right| left.target_id.cmp(&right.target_id));
    changes
}

fn restore_scene_standee_visibility(
    standees: &mut Query<(&CharacterStandee, &mut Visibility)>,
    changes: &[SceneStandeeVisibilityChange],
) {
    for change in changes {
        if let Some((_, mut bevy_visibility)) = standees
            .iter_mut()
            .find(|(standee, _)| standee.target_id == change.target_id)
        {
            *bevy_visibility = change.restore_visibility;
        }
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

    let mut active_targets = manager
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
    active_targets.extend(active_unit_template_standee_targets(
        &manager, store,
    ));
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
                if existing_standee.image_source == image_source
                    && existing_standee.visibility == persisted.visibility
                {
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
                        Visibility::Visible,
                        CharacterStandee {
                            target_id: target_id.clone(),
                            image_source,
                            visibility: persisted.visibility.clone(),
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

fn active_unit_template_standee_targets(
    manager: &NapcatMessageManager,
    store: &VoxelSceneStore,
) -> Vec<(String, String)> {
    store
        .character_standees
        .iter()
        .filter_map(|standee| {
            let unit_id = standee
                .target_id
                .strip_prefix(UNIT_TEMPLATE_STANDEE_PREFIX)?;
            let image_source = manager.unit_pool.get(unit_id)?.character.image.trim();
            (!image_source.is_empty()).then(|| {
                (
                    standee.target_id.clone(),
                    image_source.to_owned(),
                )
            })
        })
        .collect()
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
        visibility: SceneVisibility::Public,
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
    visibility: SceneVisibility,
) {
    let stroke = voxel_edit_stroke(runtime, positions, after, visibility);
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
    visibility: SceneVisibility,
) -> VoxelEditStroke {
    let mut by_position: HashMap<IVec3, VoxelEditChange> = HashMap::new();
    for position in positions {
        let before = runtime.edit_index.get(&position).cloned();
        let after = Some(PersistedVoxelState {
            voxel: after,
            visibility: visibility.clone(),
        });
        if before == after {
            continue;
        }
        by_position
            .entry(position)
            .and_modify(|change| change.after = after.clone())
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
        let next = if undo { change.before.clone() } else { change.after.clone() };
        set_runtime_voxel_state(runtime, change.position, next.clone());
        voxel_world.set_voxel(
            change.position,
            persisted_state_to_world_voxel(change.position, next.as_ref()),
        );
    }
}

fn set_runtime_voxel_state(
    runtime: &mut VoxelMapRuntimeState,
    position: IVec3,
    state: Option<PersistedVoxelState>,
) {
    match state {
        Some(state) => {
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
    state: Option<&PersistedVoxelState>,
) -> WorldVoxel<u8> {
    state
        .map(|state| WorldVoxel::from(state.voxel))
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

    fn empty_manager() -> NapcatMessageManager {
        NapcatMessageManager {
            messages: HashMap::default(),
            chat_targets: HashMap::default(),
            player_characters: HashMap::default(),
            trpg_groups: HashMap::default(),
            current_trpg_group: None,
            groups: HashMap::default(),
            read_message_counts: HashMap::default(),
            summarized_message_counts: HashMap::default(),
            open_chat_targets: HashSet::default(),
            pending_chat_targets: HashSet::default(),
            rejected_chat_targets: HashSet::default(),
            random_pools: HashMap::default(),
            skill_pool: Vec::new(),
            unit_pool: HashMap::default(),
        }
    }

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
    fn voxel_collision_boxes_merge_rectangular_prisms() {
        let positions = (0..3)
            .flat_map(|x| (0..2).flat_map(move |y| (0..2).map(move |z| IVec3::new(x, y, z))))
            .collect::<Vec<_>>();

        let boxes = voxel_collision_boxes_from_positions(positions, 1, 16);

        assert_eq!(boxes, vec![VoxelCollisionBox {
            origin: IVec3::ZERO,
            size: IVec3::new(3, 2, 2),
        }]);
    }

    #[test]
    fn voxel_collision_boxes_keep_l_shapes_conservative() {
        let boxes = voxel_collision_boxes_from_positions(
            [
                IVec3::new(0, 0, 0),
                IVec3::new(1, 0, 0),
                IVec3::new(0, 1, 0),
            ],
            1,
            16,
        );

        assert_eq!(boxes.len(), 2);
        assert!(boxes.contains(&VoxelCollisionBox {
            origin: IVec3::ZERO,
            size: IVec3::new(2, 1, 1),
        }));
        assert!(boxes.contains(&VoxelCollisionBox {
            origin: IVec3::new(0, 1, 0),
            size: IVec3::ONE,
        }));
    }

    #[test]
    fn static_voxel_collision_inputs_stay_bounded() {
        let decor_positions = space_hifi_decor_voxel_edits()
            .into_iter()
            .filter_map(|edit| {
                let PersistedVoxel::Solid(material) = edit.voxel else {
                    return None;
                };
                (material != MAT_STAR).then_some(IVec3::new(
                    edit.position[0],
                    edit.position[1],
                    edit.position[2],
                ))
            })
            .collect::<Vec<_>>();
        let decor_boxes = voxel_collision_boxes_from_positions(
            decor_positions.clone(),
            1,
            MAX_STATIC_VOXEL_COLLIDER_BOXES,
        );
        let detail_positions = voxel_planet_detail_preview_blocks()
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let detail_boxes = voxel_collision_boxes_from_positions(
            detail_positions.clone(),
            VOXEL_PLANET_DETAIL_PREVIEW_BLOCK,
            MAX_STATIC_VOXEL_COLLIDER_BOXES,
        );

        assert!(!decor_boxes.is_empty());
        assert!(decor_boxes.len() < decor_positions.len());
        assert!(!detail_boxes.is_empty());
        assert!(detail_boxes.len() <= MAX_STATIC_VOXEL_COLLIDER_BOXES);
        assert!(detail_boxes.len() <= detail_positions.len());
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
    fn procedural_battle_spaceship_generates_physics_assembly() {
        let voxels = procedural_battle_spaceship_voxels();
        let assembly = battle_spaceship_physics_assembly(&voxels).unwrap();

        assert!(voxels.len() > 2_000);
        assert!(voxels.len() < 50_000);
        assert!(voxels.values().any(|material| *material == MAT_ENGINE_RED));
        assert!(assembly.mass > 30_000.0);
        assert!(assembly.local_center_of_mass.is_finite());
        assert!(assembly.angular_inertia.cmpgt(Vec3::ZERO).all());
        assert!(assembly.propellers.len() >= 3);
        assert!(assembly
            .propellers
            .iter()
            .all(|propeller| propeller.airflow_speed > 0.0 && propeller.airflow_radius > 0.0));
        assert!(assembly.lift_points.len() >= 4);
        assert!(assembly.total_lift_strength > 0.0);
        assert!(assembly.bounds_min.x <= -200.0);
        assert!(assembly.bounds_max.z >= 450.0);
    }

    #[test]
    fn battle_spaceship_propeller_acceleration_uses_sail_power_curve() {
        let one_sail = battle_spaceship_propeller_acceleration(1.0, 1.0);
        let four_sails = battle_spaceship_propeller_acceleration(4.0, 1.0);
        let reverse = battle_spaceship_propeller_acceleration(4.0, -1.0);

        assert!(one_sail > 0.0);
        assert!((four_sails / one_sail - 8.0).abs() < 0.001);
        assert!((reverse + four_sails).abs() < 0.001);
    }

    #[test]
    fn battle_spaceship_airflow_range_grows_logarithmically() {
        let low = battle_spaceship_airflow_range(20.0);
        let high = battle_spaceship_airflow_range(80.0);
        let reverse = battle_spaceship_airflow_range(-80.0);

        assert!(low > 0.0);
        assert!(high > low);
        assert!(high < low * 3.0);
        assert!((reverse + high).abs() < 0.001);
    }

    #[test]
    fn battle_spaceship_airflow_pushes_only_inside_propeller_column() {
        let pushed = battle_spaceship_airflow_delta_velocity(
            Vec3::new(0.0, 0.0, 60.0),
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::Z,
            60.0,
            25.0,
            1.0,
            1.0,
            1.0 / 60.0,
        );
        let outside_radius = battle_spaceship_airflow_delta_velocity(
            Vec3::new(30.0, 0.0, 60.0),
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::Z,
            60.0,
            25.0,
            1.0,
            1.0,
            1.0 / 60.0,
        );
        let behind = battle_spaceship_airflow_delta_velocity(
            Vec3::new(0.0, 0.0, -4.0),
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::Z,
            60.0,
            25.0,
            1.0,
            1.0,
            1.0 / 60.0,
        );
        let reverse = battle_spaceship_airflow_delta_velocity(
            Vec3::new(0.0, 0.0, -60.0),
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::Z,
            60.0,
            25.0,
            -1.0,
            1.0,
            1.0 / 60.0,
        );

        assert!(pushed.z > 0.0);
        assert_eq!(outside_radius, Vec3::ZERO);
        assert_eq!(behind, Vec3::ZERO);
        assert!(reverse.z < 0.0);
    }

    #[test]
    fn battle_spaceship_disassembly_requires_slow_aligned_ship() {
        assert_eq!(
            battle_spaceship_disassembly_status(Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            BattleSpaceshipDisassemblyStatus::Ready
        );
        assert_eq!(
            battle_spaceship_disassembly_status(
                Quat::IDENTITY,
                Vec3::X * (BATTLE_SPACESHIP_DISASSEMBLE_MAX_SPEED + 0.1),
                Vec3::ZERO,
            ),
            BattleSpaceshipDisassemblyStatus::TooFast
        );
        assert_eq!(
            battle_spaceship_disassembly_status(
                Quat::IDENTITY,
                Vec3::ZERO,
                Vec3::Y * (BATTLE_SPACESHIP_DISASSEMBLE_MAX_ANGULAR_SPEED + 0.1),
            ),
            BattleSpaceshipDisassemblyStatus::TooFast
        );
        assert_eq!(
            battle_spaceship_disassembly_status(
                Quat::from_rotation_x(
                    (BATTLE_SPACESHIP_DISASSEMBLE_MAX_ROTATION_DEGREES + 2.0).to_radians(),
                ),
                Vec3::ZERO,
                Vec3::ZERO,
            ),
            BattleSpaceshipDisassemblyStatus::TooTilted
        );
    }

    #[test]
    fn battle_spaceship_disassembly_target_snaps_to_grid_and_cardinal_yaw() {
        let rotation = Quat::from_euler(
            EulerRot::YXZ,
            100.0_f32.to_radians(),
            12.0_f32.to_radians(),
            -8.0_f32.to_radians(),
        );
        let target_rotation = battle_spaceship_disassembly_target_rotation(rotation);
        let (target_yaw, target_pitch, target_roll) = target_rotation.to_euler(EulerRot::YXZ);

        assert!((target_yaw.to_degrees() - 90.0).abs() < 0.001);
        assert!(target_pitch.abs() < 0.001);
        assert!(target_roll.abs() < 0.001);

        let translation = Vec3::new(12.0, 24.0, -9.0);
        let local_center = Vec3::new(3.25, 4.5, -7.75);
        let target_translation = battle_spaceship_disassembly_target_translation(
            translation,
            rotation,
            target_rotation,
            local_center,
        );
        let current_center = translation + rotation * local_center;
        let target_center = target_translation + target_rotation * local_center;
        let grid = BATTLE_SPACESHIP_SCALE as f32;

        assert!(
            (target_center.x - battle_spaceship_nearest_grid_center(current_center.x, grid)).abs()
                < 0.001
        );
        assert!(
            (target_center.y - battle_spaceship_nearest_grid_center(current_center.y, grid)).abs()
                < 0.001
        );
        assert!(
            (target_center.z - battle_spaceship_nearest_grid_center(current_center.z, grid)).abs()
                < 0.001
        );
    }

    #[test]
    fn battle_spaceship_alignment_steps_are_bounded_and_settle() {
        let target_translation = Vec3::new(10.0, 0.0, 0.0);
        assert_eq!(
            battle_spaceship_move_towards(Vec3::ZERO, target_translation, 3.0),
            Vec3::new(3.0, 0.0, 0.0)
        );
        assert_eq!(
            battle_spaceship_move_towards(
                Vec3::new(9.0, 0.0, 0.0),
                target_translation,
                3.0
            ),
            target_translation
        );

        let target_rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let half_step = battle_spaceship_rotate_towards(
            Quat::IDENTITY,
            target_rotation,
            std::f32::consts::FRAC_PI_4,
        );
        assert!(
            (half_step.angle_between(Quat::from_rotation_y(
                std::f32::consts::FRAC_PI_4
            )))
            .to_degrees()
            .abs()
                < 0.001
        );
        assert_eq!(
            battle_spaceship_rotate_towards(
                half_step,
                target_rotation,
                std::f32::consts::FRAC_PI_2,
            ),
            target_rotation
        );
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
                visibility: SceneVisibility::Player(2),
            }],
            unit_scene_tokens: vec![PersistedUnitSceneToken {
                token_id: "unit-token:unit-a".to_owned(),
                unit_id: "unit-a".to_owned(),
                label: "巡逻兵".to_owned(),
                translation: [6.0, UNIT_SCENE_TOKEN_Y, -3.0],
                visibility: SceneVisibility::Party("red".to_owned()),
            }],
            legacy_area_markers: vec![PersistedLegacyAreaMarker {
                marker_id: "legacy-area:旧团:world-a:area-a".to_owned(),
                group_name: "旧团".to_owned(),
                world_id: "world-a".to_owned(),
                world_name: "旧世界".to_owned(),
                area_id: "area-a".to_owned(),
                area_name: "密谈区".to_owned(),
                combat: true,
                members: vec!["2".to_owned()],
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
                visibility: SceneVisibility::Party("red".to_owned()),
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
        assert_eq!(
            export.store.character_standees[0].visibility,
            SceneVisibility::Player(2)
        );
        assert_eq!(
            export.store.unit_scene_tokens[0].token_id,
            "unit-token:unit-a"
        );
        assert_eq!(
            export.store.unit_scene_tokens[0].visibility,
            SceneVisibility::Party("red".to_owned())
        );
        assert_eq!(
            export.store.legacy_area_markers[0].marker_id,
            "legacy-area:旧团:world-a:area-a"
        );
        assert_eq!(
            export.store.legacy_area_markers[0].visibility,
            SceneVisibility::Party("red".to_owned())
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
                visibility: SceneVisibility::Party("red".to_owned()),
            }],
            unit_scene_tokens: vec![PersistedUnitSceneToken {
                token_id: "unit-token:unit-a".to_owned(),
                unit_id: "unit-a".to_owned(),
                label: "导入巡逻兵".to_owned(),
                translation: [6.0, UNIT_SCENE_TOKEN_Y, -3.0],
                visibility: SceneVisibility::Gm,
            }],
            legacy_area_markers: vec![PersistedLegacyAreaMarker {
                marker_id: "legacy-area:旧团:world-a:area-a".to_owned(),
                group_name: "旧团".to_owned(),
                world_id: "world-a".to_owned(),
                world_name: "导入世界".to_owned(),
                area_id: "area-a".to_owned(),
                area_name: "导入密谈区".to_owned(),
                combat: true,
                members: vec!["2".to_owned(), "3".to_owned()],
                x: 11.0,
                y: 12.0,
                width: 13.0,
                height: 14.0,
                visibility: SceneVisibility::Gm,
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
                    visibility: SceneVisibility::Public,
                },
                PersistedCharacterStandee {
                    target_id: "9".to_owned(),
                    image_source: "local.png".to_owned(),
                    translation: [1.0, 1.0, 1.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    visibility: SceneVisibility::Gm,
                },
            ],
            unit_scene_tokens: vec![
                PersistedUnitSceneToken {
                    token_id: "unit-token:unit-a".to_owned(),
                    unit_id: "unit-a".to_owned(),
                    label: "本地巡逻兵".to_owned(),
                    translation: [0.0, UNIT_SCENE_TOKEN_Y, -3.0],
                    visibility: SceneVisibility::Public,
                },
                PersistedUnitSceneToken {
                    token_id: "unit-token:unit-b".to_owned(),
                    unit_id: "unit-b".to_owned(),
                    label: "本地守卫".to_owned(),
                    translation: [1.0, UNIT_SCENE_TOKEN_Y, -3.0],
                    visibility: SceneVisibility::Public,
                },
            ],
            legacy_area_markers: vec![
                PersistedLegacyAreaMarker {
                    marker_id: "legacy-area:旧团:world-a:area-a".to_owned(),
                    group_name: "旧团".to_owned(),
                    world_id: "world-a".to_owned(),
                    world_name: "本地世界".to_owned(),
                    area_id: "area-a".to_owned(),
                    area_name: "本地密谈区".to_owned(),
                    combat: false,
                    members: vec!["9".to_owned()],
                    x: 1.0,
                    y: 1.0,
                    width: 1.0,
                    height: 1.0,
                    visibility: SceneVisibility::Public,
                },
                PersistedLegacyAreaMarker {
                    marker_id: "legacy-area:旧团:world-b:area-b".to_owned(),
                    group_name: "旧团".to_owned(),
                    world_id: "world-b".to_owned(),
                    world_name: "本地世界B".to_owned(),
                    area_id: "area-b".to_owned(),
                    area_name: "本地区域B".to_owned(),
                    combat: false,
                    members: Vec::new(),
                    x: 2.0,
                    y: 2.0,
                    width: 2.0,
                    height: 2.0,
                    visibility: SceneVisibility::Public,
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
        assert_eq!(
            store
                .character_standees
                .iter()
                .find(|standee| standee.target_id == "2")
                .unwrap()
                .visibility,
            SceneVisibility::Party("red".to_owned())
        );
        assert!(store
            .character_standees
            .iter()
            .any(|standee| standee.target_id == "9"));
        let imported_token = store
            .unit_scene_tokens
            .iter()
            .find(|token| token.token_id == "unit-token:unit-a")
            .unwrap();
        assert_eq!(imported_token.label, "导入巡逻兵");
        assert_eq!(
            imported_token.visibility,
            SceneVisibility::Gm
        );
        assert!(store
            .unit_scene_tokens
            .iter()
            .any(|token| token.token_id == "unit-token:unit-b"));
        let imported_marker = store
            .legacy_area_markers
            .iter()
            .find(|marker| marker.marker_id == "legacy-area:旧团:world-a:area-a")
            .unwrap();
        assert_eq!(imported_marker.world_name, "导入世界");
        assert_eq!(imported_marker.area_name, "导入密谈区");
        assert_eq!(imported_marker.members, vec![
            "2".to_owned(),
            "3".to_owned()
        ]);
        assert_eq!(
            imported_marker.visibility,
            SceneVisibility::Gm
        );
        assert!(store
            .legacy_area_markers
            .iter()
            .any(|marker| marker.marker_id == "legacy-area:旧团:world-b:area-b"));
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
    fn unit_template_standee_helpers_place_update_and_remove_scene_standee() {
        let mut store = VoxelSceneStore::default();

        assert!(place_unit_template_standee(
            &mut store,
            "unit-alpha",
            "unit-alpha.png"
        )
        .unwrap());
        assert!(has_unit_template_standee(
            &store,
            "unit-alpha"
        ));
        assert_eq!(store.character_standees.len(), 1);
        assert_eq!(
            store.character_standees[0].target_id,
            unit_template_standee_target_id("unit-alpha")
        );
        assert_eq!(
            store.character_standees[0].image_source,
            "unit-alpha.png"
        );
        assert_eq!(
            store.character_standees[0].visibility,
            SceneVisibility::Public
        );

        assert!(!place_unit_template_standee(
            &mut store,
            "unit-alpha",
            "unit-alpha.png"
        )
        .unwrap());
        assert_eq!(store.character_standees.len(), 1);
        assert!(place_unit_template_standee(
            &mut store,
            "unit-alpha",
            "unit-alpha-v2.png"
        )
        .unwrap());
        assert_eq!(
            store.character_standees[0].image_source,
            "unit-alpha-v2.png"
        );

        assert!(remove_unit_template_standee(
            &mut store,
            "unit-alpha"
        ));
        assert!(!has_unit_template_standee(
            &store,
            "unit-alpha"
        ));
        assert!(store.character_standees.is_empty());
    }

    #[test]
    fn unit_template_token_helpers_place_update_and_remove_scene_token() {
        let mut store = VoxelSceneStore::default();

        assert!(place_unit_template_token(&mut store, " unit-alpha ", "巡逻兵").unwrap());
        assert!(has_unit_template_token(
            &store,
            "unit-alpha"
        ));
        assert_eq!(store.unit_scene_tokens.len(), 1);
        assert_eq!(
            store.unit_scene_tokens[0].token_id,
            unit_template_token_id("unit-alpha")
        );
        assert_eq!(
            store.unit_scene_tokens[0].unit_id,
            "unit-alpha"
        );
        assert_eq!(
            store.unit_scene_tokens[0].label,
            "巡逻兵"
        );
        assert_eq!(
            store.unit_scene_tokens[0].translation,
            [0.0, UNIT_SCENE_TOKEN_Y, -3.0]
        );
        assert_eq!(
            store.unit_scene_tokens[0].visibility,
            SceneVisibility::Public
        );

        assert!(!place_unit_template_token(&mut store, "unit-alpha", "巡逻兵").unwrap());
        let original_translation = store.unit_scene_tokens[0].translation;
        store.unit_scene_tokens[0].visibility = SceneVisibility::Gm;

        assert!(place_unit_template_token(&mut store, "unit-alpha", "精英巡逻兵").unwrap());
        assert_eq!(store.unit_scene_tokens.len(), 1);
        assert_eq!(
            store.unit_scene_tokens[0].label,
            "精英巡逻兵"
        );
        assert_eq!(
            store.unit_scene_tokens[0].translation,
            original_translation
        );
        assert_eq!(
            store.unit_scene_tokens[0].visibility,
            SceneVisibility::Gm
        );

        assert!(remove_unit_template_token(
            &mut store,
            "unit-alpha"
        ));
        assert!(!has_unit_template_token(
            &store,
            "unit-alpha"
        ));
        assert!(store.unit_scene_tokens.is_empty());
    }

    #[test]
    fn legacy_world_and_area_unit_tokens_use_scoped_ids_and_visibility() {
        let mut store = VoxelSceneStore::default();

        assert!(place_legacy_world_unit_token(
            &mut store,
            "旧团",
            "world-a",
            "旧世界",
            "unit-old",
            "行尸",
            false,
        )
        .unwrap());
        assert!(place_legacy_area_unit_token(
            &mut store,
            "旧团",
            "world-a",
            "area-a",
            "密谈区",
            "unit-old",
            "行尸",
            10.0,
            20.0,
            30.0,
            40.0,
            true,
            1,
        )
        .unwrap());

        assert_eq!(store.unit_scene_tokens.len(), 2);
        let world_token = store
            .unit_scene_tokens
            .iter()
            .find(|token| {
                token.token_id == legacy_world_unit_token_id("旧团", "world-a", "unit-old")
            })
            .unwrap();
        assert_eq!(world_token.unit_id, "unit-old");
        assert_eq!(world_token.label, "旧世界/行尸");
        assert_eq!(
            world_token.visibility,
            SceneVisibility::Gm
        );

        let area_token = store
            .unit_scene_tokens
            .iter()
            .find(|token| {
                token.token_id == legacy_area_unit_token_id("旧团", "world-a", "area-a", "unit-old")
            })
            .unwrap();
        assert_eq!(area_token.unit_id, "unit-old");
        assert_eq!(area_token.label, "密谈区/行尸");
        assert_eq!(
            area_token.visibility,
            SceneVisibility::Public
        );
        assert_eq!(area_token.translation, [
            2.5 - UNIT_SCENE_TOKEN_SPACING * 0.25,
            UNIT_SCENE_TOKEN_Y,
            4.0,
        ]);

        let original_translation = store
            .unit_scene_tokens
            .iter()
            .find(|token| {
                token.token_id == legacy_area_unit_token_id("旧团", "world-a", "area-a", "unit-old")
            })
            .unwrap()
            .translation;
        assert!(place_legacy_area_unit_token(
            &mut store,
            "旧团",
            "world-a",
            "area-a",
            "密谈区",
            "unit-old",
            "精英行尸",
            10.0,
            20.0,
            30.0,
            40.0,
            false,
            3,
        )
        .unwrap());
        let updated_area_token = store
            .unit_scene_tokens
            .iter()
            .find(|token| {
                token.token_id == legacy_area_unit_token_id("旧团", "world-a", "area-a", "unit-old")
            })
            .unwrap();
        assert_eq!(
            updated_area_token.label,
            "密谈区/精英行尸"
        );
        assert_eq!(
            updated_area_token.translation,
            original_translation
        );
        assert_eq!(
            updated_area_token.visibility,
            SceneVisibility::Public
        );

        assert!(place_legacy_world_unit_token(
            &mut store,
            "旧团",
            "world-a",
            "旧世界",
            "stale-world",
            "旧世界多余单位",
            true,
        )
        .unwrap());
        assert!(place_legacy_area_unit_token(
            &mut store,
            "旧团",
            "world-a",
            "area-a",
            "密谈区",
            "stale-area",
            "密谈区多余单位",
            10.0,
            20.0,
            30.0,
            40.0,
            true,
            2,
        )
        .unwrap());
        assert!(place_legacy_world_unit_token(
            &mut store,
            "旧团",
            "world-b",
            "另一个世界",
            "other-world",
            "其他世界单位",
            true,
        )
        .unwrap());
        assert!(place_unit_template_token(&mut store, "generic", "通用标记").unwrap());

        assert_eq!(
            prune_legacy_world_unit_tokens(&mut store, "旧团", "world-a", &[
                "unit-old".to_owned()
            ],),
            1
        );
        assert!(!store
            .unit_scene_tokens
            .iter()
            .any(|token| token.token_id
                == legacy_world_unit_token_id("旧团", "world-a", "stale-world")));
        assert!(store
            .unit_scene_tokens
            .iter()
            .any(|token| token.token_id
                == legacy_world_unit_token_id("旧团", "world-b", "other-world")));
        assert!(store
            .unit_scene_tokens
            .iter()
            .any(|token| token.token_id == unit_template_token_id("generic")));

        assert_eq!(
            prune_legacy_area_unit_tokens(
                &mut store,
                "旧团",
                "world-a",
                "area-a",
                &["unit-old".to_owned()],
            ),
            1
        );
        assert!(
            !store.unit_scene_tokens.iter().any(|token| token.token_id
                == legacy_area_unit_token_id(
                    "旧团",
                    "world-a",
                    "area-a",
                    "stale-area"
                ))
        );

        assert_eq!(
            remove_legacy_area_unit_tokens(&mut store, "旧团", "world-a", "area-a"),
            1
        );
        assert!(
            !store.unit_scene_tokens.iter().any(|token| token.token_id
                == legacy_area_unit_token_id("旧团", "world-a", "area-a", "unit-old"))
        );
        assert_eq!(
            remove_legacy_world_unit_tokens(&mut store, "旧团", "world-a"),
            1
        );
        assert!(!store.unit_scene_tokens.iter().any(
            |token| token.token_id == legacy_world_unit_token_id("旧团", "world-a", "unit-old")
        ));
        assert_eq!(
            remove_legacy_world_unit_tokens(&mut store, "旧团", "world-a"),
            0
        );
    }

    #[test]
    fn unit_scene_token_state_update_changes_position_and_visibility() {
        let mut store = VoxelSceneStore::default();
        place_unit_template_token(&mut store, "unit-alpha", "巡逻兵").unwrap();

        assert!(update_unit_scene_token_state(
            &mut store,
            &unit_template_token_id("unit-alpha"),
            [1.0, 2.0, 3.0],
            SceneVisibility::Party("red".to_owned()),
        ));
        assert_eq!(
            store.unit_scene_tokens[0].translation,
            [1.0, 2.0, 3.0]
        );
        assert_eq!(
            store.unit_scene_tokens[0].visibility,
            SceneVisibility::Party("red".to_owned())
        );

        assert!(!update_unit_scene_token_state(
            &mut store,
            &unit_template_token_id("unit-alpha"),
            [1.0, 2.0, 3.0],
            SceneVisibility::Party("red".to_owned()),
        ));
        assert!(!update_unit_scene_token_state(
            &mut store,
            "unit-token:missing",
            [4.0, 5.0, 6.0],
            SceneVisibility::Gm,
        ));
    }

    #[test]
    fn legacy_area_marker_helpers_place_update_and_remove_scene_marker() {
        let mut store = VoxelSceneStore::default();
        let members = vec!["3".to_owned(), "2".to_owned(), "2".to_owned()];

        assert!(place_legacy_area_marker(
            &mut store,
            "旧团",
            "world-a",
            "旧世界",
            "area-a",
            "密谈区",
            true,
            &members,
            1.0,
            2.0,
            3.0,
            4.0,
            false,
        )
        .unwrap());
        assert!(has_legacy_area_marker(
            &store, "旧团", "world-a", "area-a"
        ));
        assert_eq!(store.legacy_area_markers.len(), 1);
        assert_eq!(
            store.legacy_area_markers[0].marker_id,
            legacy_area_marker_id("旧团", "world-a", "area-a")
        );
        assert_eq!(
            store.legacy_area_markers[0].members,
            vec!["2".to_owned(), "3".to_owned()]
        );
        assert_eq!(
            store.legacy_area_markers[0].visibility,
            SceneVisibility::Gm
        );

        assert!(!place_legacy_area_marker(
            &mut store,
            "旧团",
            "world-a",
            "旧世界",
            "area-a",
            "密谈区",
            true,
            &members,
            1.0,
            2.0,
            3.0,
            4.0,
            false,
        )
        .unwrap());

        assert!(place_legacy_area_marker(
            &mut store,
            "旧团",
            "world-a",
            "旧世界",
            "area-a",
            "公开区",
            false,
            &members,
            5.0,
            6.0,
            7.0,
            8.0,
            true,
        )
        .unwrap());
        assert_eq!(store.legacy_area_markers.len(), 1);
        assert_eq!(
            store.legacy_area_markers[0].area_name,
            "公开区"
        );
        assert_eq!(
            store.legacy_area_markers[0].visibility,
            SceneVisibility::Public
        );

        assert!(remove_legacy_area_marker(
            &mut store, "旧团", "world-a", "area-a"
        ));
        assert!(!has_legacy_area_marker(
            &store, "旧团", "world-a", "area-a"
        ));
    }

    #[test]
    fn legacy_area_marker_voxel_outline_writes_visible_border_to_active_map() {
        let mut store = VoxelSceneStore {
            active_map_id: Some("legacy-map".to_owned()),
            maps: vec![PersistedVoxelMap {
                id: "legacy-map".to_owned(),
                name: "旧区域地图".to_owned(),
                edits: Vec::new(),
            }],
            ..Default::default()
        };
        place_legacy_area_marker(
            &mut store,
            "旧团",
            "world-a",
            "旧世界",
            "area-a",
            "战斗区",
            true,
            &[],
            10.0,
            20.0,
            30.0,
            40.0,
            false,
        )
        .unwrap();

        let count = stamp_legacy_area_marker_voxel_outline(&mut store, "旧团", "world-a", "area-a")
            .unwrap();

        assert_eq!(count, 14);
        let map = active_voxel_map(&store).unwrap();
        assert_eq!(map.id, "legacy-map");
        assert_eq!(map.edits.len(), 14);
        assert!(map
            .edits
            .iter()
            .any(|edit| edit.position == [1, LEGACY_AREA_MARKER_VOXEL_Y, 2]));
        assert!(map
            .edits
            .iter()
            .any(|edit| edit.position == [4, LEGACY_AREA_MARKER_VOXEL_Y, 6]));
        assert!(!map
            .edits
            .iter()
            .any(|edit| edit.position == [2, LEGACY_AREA_MARKER_VOXEL_Y, 3]));
        assert!(map
            .edits
            .iter()
            .all(|edit| edit.voxel == PersistedVoxel::Solid(MAT_ENGINE_RED)));
        assert!(map
            .edits
            .iter()
            .all(|edit| edit.visibility == SceneVisibility::Gm));
    }

    #[test]
    fn legacy_area_marker_voxel_fill_writes_visible_area_to_active_map() {
        let mut store = VoxelSceneStore {
            active_map_id: Some("legacy-map".to_owned()),
            maps: vec![PersistedVoxelMap {
                id: "legacy-map".to_owned(),
                name: "旧区域地图".to_owned(),
                edits: Vec::new(),
            }],
            ..Default::default()
        };
        place_legacy_area_marker(
            &mut store,
            "旧团",
            "world-a",
            "旧世界",
            "area-a",
            "讨论区",
            false,
            &[],
            10.0,
            20.0,
            30.0,
            40.0,
            true,
        )
        .unwrap();

        let count =
            stamp_legacy_area_marker_voxel_fill(&mut store, "旧团", "world-a", "area-a").unwrap();

        assert_eq!(count, 20);
        let map = active_voxel_map(&store).unwrap();
        assert_eq!(map.id, "legacy-map");
        assert_eq!(map.edits.len(), 20);
        assert!(map
            .edits
            .iter()
            .any(|edit| edit.position == [1, LEGACY_AREA_MARKER_VOXEL_Y, 2]));
        assert!(map
            .edits
            .iter()
            .any(|edit| edit.position == [2, LEGACY_AREA_MARKER_VOXEL_Y, 3]));
        assert!(map
            .edits
            .iter()
            .any(|edit| edit.position == [4, LEGACY_AREA_MARKER_VOXEL_Y, 6]));
        assert!(map
            .edits
            .iter()
            .all(|edit| edit.voxel == PersistedVoxel::Solid(MAT_WINDOW_CYAN)));
        assert!(map
            .edits
            .iter()
            .all(|edit| edit.visibility == SceneVisibility::Public));
    }

    #[test]
    fn legacy_area_marker_voxel_fill_rejects_oversized_old_area() {
        let mut store = VoxelSceneStore {
            active_map_id: Some("legacy-map".to_owned()),
            maps: vec![PersistedVoxelMap {
                id: "legacy-map".to_owned(),
                name: "旧区域地图".to_owned(),
                edits: Vec::new(),
            }],
            ..Default::default()
        };
        place_legacy_area_marker(
            &mut store,
            "旧团",
            "world-a",
            "旧世界",
            "area-a",
            "超大区域",
            true,
            &[],
            0.0,
            0.0,
            10_000.0,
            10_000.0,
            false,
        )
        .unwrap();

        let err = stamp_legacy_area_marker_voxel_fill(&mut store, "旧团", "world-a", "area-a")
            .unwrap_err();

        assert!(err.contains("超过上限"));
        assert!(active_voxel_map(&store).unwrap().edits.is_empty());
    }

    #[test]
    fn legacy_area_marker_visibility_uses_player_access_scope() {
        let public_marker = PersistedLegacyAreaMarker {
            marker_id: "legacy-area:旧团:world-a:public".to_owned(),
            group_name: "旧团".to_owned(),
            world_id: "world-a".to_owned(),
            world_name: "旧世界".to_owned(),
            area_id: "public".to_owned(),
            area_name: "公开区".to_owned(),
            visibility: SceneVisibility::Public,
            ..legacy_area_marker_for_test()
        };
        let red_marker = PersistedLegacyAreaMarker {
            marker_id: "legacy-area:旧团:world-a:red".to_owned(),
            area_id: "red".to_owned(),
            area_name: "红队区".to_owned(),
            visibility: SceneVisibility::Party("red".to_owned()),
            ..legacy_area_marker_for_test()
        };
        let gm_marker = PersistedLegacyAreaMarker {
            marker_id: "legacy-area:旧团:world-a:gm".to_owned(),
            area_id: "gm".to_owned(),
            area_name: "GM区".to_owned(),
            visibility: SceneVisibility::Gm,
            ..legacy_area_marker_for_test()
        };
        let red_access = PlayerAccess {
            player_id: 2,
            party_id: Some("red".to_owned()),
            character_id: None,
            is_gm: false,
        };
        let blue_access = PlayerAccess {
            player_id: 3,
            party_id: Some("blue".to_owned()),
            character_id: None,
            is_gm: false,
        };
        let gm_access = PlayerAccess {
            player_id: 9,
            party_id: None,
            character_id: None,
            is_gm: true,
        };

        assert!(legacy_area_marker_visible_for_access(
            &public_marker,
            Some(&blue_access)
        ));
        assert!(legacy_area_marker_visible_for_access(
            &red_marker,
            Some(&red_access)
        ));
        assert!(!legacy_area_marker_visible_for_access(
            &red_marker,
            Some(&blue_access)
        ));
        assert!(!legacy_area_marker_visible_for_access(
            &gm_marker,
            Some(&red_access)
        ));
        assert!(legacy_area_marker_visible_for_access(
            &gm_marker,
            Some(&gm_access)
        ));
        assert!(legacy_area_marker_visible_for_access(
            &gm_marker, None
        ));
    }

    #[test]
    fn unit_scene_token_visibility_uses_player_access_scope() {
        let public_token = PersistedUnitSceneToken {
            token_id: "unit-token:public".to_owned(),
            unit_id: "public".to_owned(),
            label: "公开单位".to_owned(),
            translation: [0.0, UNIT_SCENE_TOKEN_Y, -3.0],
            visibility: SceneVisibility::Public,
        };
        let red_token = PersistedUnitSceneToken {
            token_id: "unit-token:red".to_owned(),
            unit_id: "red".to_owned(),
            label: "红队单位".to_owned(),
            translation: [0.0, UNIT_SCENE_TOKEN_Y, -3.0],
            visibility: SceneVisibility::Party("red".to_owned()),
        };
        let player_token = PersistedUnitSceneToken {
            token_id: "unit-token:player".to_owned(),
            unit_id: "player".to_owned(),
            label: "私有单位".to_owned(),
            translation: [0.0, UNIT_SCENE_TOKEN_Y, -3.0],
            visibility: SceneVisibility::Player(2),
        };
        let gm_token = PersistedUnitSceneToken {
            token_id: "unit-token:gm".to_owned(),
            unit_id: "gm".to_owned(),
            label: "GM单位".to_owned(),
            translation: [0.0, UNIT_SCENE_TOKEN_Y, -3.0],
            visibility: SceneVisibility::Gm,
        };
        let red_access = PlayerAccess {
            player_id: 2,
            party_id: Some("red".to_owned()),
            character_id: None,
            is_gm: false,
        };
        let blue_access = PlayerAccess {
            player_id: 3,
            party_id: Some("blue".to_owned()),
            character_id: None,
            is_gm: false,
        };
        let gm_access = PlayerAccess {
            player_id: 9,
            party_id: None,
            character_id: None,
            is_gm: true,
        };

        assert!(unit_scene_token_visible_for_access(
            &public_token,
            Some(&blue_access)
        ));
        assert!(unit_scene_token_visible_for_access(
            &red_token,
            Some(&red_access)
        ));
        assert!(!unit_scene_token_visible_for_access(
            &red_token,
            Some(&blue_access)
        ));
        assert!(unit_scene_token_visible_for_access(
            &player_token,
            Some(&red_access)
        ));
        assert!(!unit_scene_token_visible_for_access(
            &player_token,
            Some(&blue_access)
        ));
        assert!(!unit_scene_token_visible_for_access(
            &gm_token,
            Some(&red_access)
        ));
        assert!(unit_scene_token_visible_for_access(
            &gm_token,
            Some(&gm_access)
        ));
        assert!(unit_scene_token_visible_for_access(
            &gm_token, None
        ));
    }

    #[test]
    fn legacy_area_marker_corners_map_old_rect_to_scene_xz_plane() {
        let marker = PersistedLegacyAreaMarker {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
            ..legacy_area_marker_for_test()
        };

        assert_eq!(
            legacy_area_marker_center(&marker),
            Vec3::new(2.5, LEGACY_AREA_MARKER_Y, 4.0)
        );
        assert_eq!(legacy_area_marker_corners(&marker), [
            Vec3::new(1.0, LEGACY_AREA_MARKER_Y, 2.0),
            Vec3::new(4.0, LEGACY_AREA_MARKER_Y, 2.0),
            Vec3::new(4.0, LEGACY_AREA_MARKER_Y, 6.0),
            Vec3::new(1.0, LEGACY_AREA_MARKER_Y, 6.0),
        ]);
    }

    fn legacy_area_marker_for_test() -> PersistedLegacyAreaMarker {
        PersistedLegacyAreaMarker {
            marker_id: "legacy-area:旧团:world-a:area-a".to_owned(),
            group_name: "旧团".to_owned(),
            world_id: "world-a".to_owned(),
            world_name: "旧世界".to_owned(),
            area_id: "area-a".to_owned(),
            area_name: "密谈区".to_owned(),
            combat: false,
            members: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
            visibility: SceneVisibility::Public,
        }
    }

    #[test]
    fn active_unit_template_standees_require_explicit_scene_placement() {
        let mut manager = empty_manager();
        let mut unit = crate::napcat::UnitPoolEntry::default();
        unit.character.image = "unit.png".to_owned();
        manager.unit_pool.insert("unit-a".to_owned(), unit);

        let mut store = VoxelSceneStore::default();
        assert!(active_unit_template_standee_targets(&manager, &store).is_empty());

        assert!(place_unit_template_standee(&mut store, "unit-a", "unit.png").unwrap());
        assert_eq!(
            active_unit_template_standee_targets(&manager, &store),
            vec![(
                unit_template_standee_target_id("unit-a"),
                "unit.png".to_owned()
            )]
        );

        manager.unit_pool.get_mut("unit-a").unwrap().character.image = "unit-v2.png".to_owned();
        assert_eq!(
            active_unit_template_standee_targets(&manager, &store),
            vec![(
                unit_template_standee_target_id("unit-a"),
                "unit-v2.png".to_owned()
            )]
        );

        manager
            .unit_pool
            .get_mut("unit-a")
            .unwrap()
            .character
            .image
            .clear();
        assert!(active_unit_template_standee_targets(&manager, &store).is_empty());
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
    fn legacy_character_standee_deserializes_as_public_visibility() {
        let standee = serde_json::from_value::<PersistedCharacterStandee>(serde_json::json!({
            "target_id": "2",
            "image_source": "portrait.png",
            "translation": [1.0, 2.0, 3.0],
            "rotation": [0.0, 0.0, 0.0, 1.0]
        }))
        .expect("legacy persisted character standee should deserialize");

        assert_eq!(
            standee.visibility,
            SceneVisibility::Public
        );
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
    fn scene_standee_visibility_uses_player_access() {
        let red_access = PlayerAccess {
            player_id: 2,
            party_id: Some("red".to_owned()),
            ..Default::default()
        };
        let blue_access = PlayerAccess {
            player_id: 3,
            party_id: Some("blue".to_owned()),
            ..Default::default()
        };
        let gm_access = PlayerAccess {
            player_id: 9,
            is_gm: true,
            ..Default::default()
        };

        assert!(matches!(
            scene_standee_visibility_for_access(
                &SceneVisibility::Public,
                Some(&red_access)
            ),
            Visibility::Visible
        ));
        assert!(matches!(
            scene_standee_visibility_for_access(
                &SceneVisibility::Party("red".to_owned()),
                Some(&red_access)
            ),
            Visibility::Visible
        ));
        assert!(matches!(
            scene_standee_visibility_for_access(
                &SceneVisibility::Party("red".to_owned()),
                Some(&blue_access)
            ),
            Visibility::Hidden
        ));
        assert!(matches!(
            scene_standee_visibility_for_access(
                &SceneVisibility::Player(2),
                Some(&blue_access)
            ),
            Visibility::Hidden
        ));
        assert!(matches!(
            scene_standee_visibility_for_access(&SceneVisibility::Gm, Some(&red_access)),
            Visibility::Hidden
        ));
        assert!(matches!(
            scene_standee_visibility_for_access(&SceneVisibility::Gm, Some(&gm_access)),
            Visibility::Visible
        ));
        assert!(matches!(
            scene_standee_visibility_for_access(&SceneVisibility::Gm, None),
            Visibility::Visible
        ));
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
    fn scene_player_view_request_distinguishes_filter_and_capture_modes() {
        let mut request = ScenePlayerViewRequest::default();

        request.filter_current_view(2);
        assert_eq!(request.user_id, Some(2));
        assert!(!request.use_capture_camera);

        request.view_with_capture_camera(3);
        assert_eq!(request.user_id, Some(3));
        assert!(request.use_capture_camera);

        request.restore_gm_view();
        assert!(request.restore_gm_view);
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
            SceneVisibility::Party("red".to_owned()),
        );

        assert_eq!(stroke.changes.len(), 2);
        let zero_change = stroke
            .changes
            .iter()
            .find(|change| change.position == IVec3::ZERO)
            .unwrap();
        assert_eq!(
            zero_change.before,
            Some(PersistedVoxelState::public(
                PersistedVoxel::Solid(MAT_HULL_LIGHT)
            ))
        );
        assert_eq!(
            zero_change.after,
            Some(PersistedVoxelState {
                voxel: PersistedVoxel::Solid(MAT_WINDOW_CYAN),
                visibility: SceneVisibility::Party("red".to_owned()),
            })
        );
    }

    #[test]
    fn voxel_edit_stroke_records_visibility_changes() {
        let mut runtime = VoxelMapRuntimeState::default();
        runtime.edit_index.insert(
            IVec3::ZERO,
            PersistedVoxelState::public(PersistedVoxel::Solid(MAT_HULL_LIGHT)),
        );

        let stroke = voxel_edit_stroke(
            &runtime,
            vec![IVec3::ZERO],
            PersistedVoxel::Solid(MAT_HULL_LIGHT),
            SceneVisibility::Player(2),
        );

        assert_eq!(stroke.changes.len(), 1);
        assert_eq!(
            stroke.changes[0].before,
            Some(PersistedVoxelState::public(
                PersistedVoxel::Solid(MAT_HULL_LIGHT)
            ))
        );
        assert_eq!(
            stroke.changes[0].after,
            Some(PersistedVoxelState {
                voxel: PersistedVoxel::Solid(MAT_HULL_LIGHT),
                visibility: SceneVisibility::Player(2),
            })
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
