use std::{
    collections::{
        hash_map::DefaultHasher,
        HashMap,
        HashSet,
        VecDeque,
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
};

use ab_glyph::{
    point,
    Font,
    FontRef,
    PxScale,
};
use avian3d::prelude::*;
use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    asset::RenderAssetUsages,
    camera::{
        primitives::Aabb,
        visibility::RenderLayers,
        RenderTarget,
    },
    core_pipeline::prepass::DepthPrepass,
    ecs::system::SystemParam,
    image::{
        ImageAddressMode,
        ImageFilterMode,
        ImageLoaderSettings,
        ImageSampler,
        ImageSamplerDescriptor,
    },
    input::mouse::{
        MouseMotion,
        MouseWheel,
    },
    math::{
        Affine2,
        Affine3A,
    },
    mesh::{
        Indices,
        PrimitiveTopology,
        VertexAttributeValues,
    },
    pbr::{
        ContactShadows,
        ScreenSpaceAmbientOcclusion,
        ScreenSpaceAmbientOcclusionQualityLevel,
    },
    post_process::bloom::Bloom,
    prelude::*,
    render::{
        render_resource::{
            Extent3d,
            Face,
            TextureDimension,
            TextureFormat,
            TextureUsages,
        },
        view::screenshot::{
            Screenshot,
            ScreenshotCaptured,
        },
    },
    window::{
        CursorGrabMode,
        CursorOptions,
        PrimaryWindow,
    },
};
use bevy_egui::{
    egui,
    input::EguiWantsInput,
    EguiContexts,
    EguiPrimaryContextPass,
    PrimaryEguiContext,
};
use bevy_persistent::{
    Persistent,
    StorageFormat,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::json;
use tokio_tungstenite::tungstenite::protocol::Message;
use voxxelmaxx::prelude::*;

use crate::{
    napcat::{
        CharacterHotbarSlot,
        CharacterSkillMetadata,
        NapcatIOSender,
        NapcatMessageManager,
        NapcatOutboundMessage,
        PlayerCharacter,
        Visibility as AccessVisibility,
    },
    rule_engine::{
        parse_rule,
        Action,
        ActorRef,
        BuffField,
        BuffValue,
    },
    scene::{
        encode_and_send_scene_capture_video,
        scene_capture_video_rotation,
        SceneCaptureKind,
        SceneCaptureRequests,
        SceneCharacterPositions,
        SCENE_CAPTURE_VIDEO_FRAMES,
    },
    voxel_radiance::{
        VoxelRadianceCascade,
        VoxelRadianceCascadePlugin,
        VoxelRadianceCascadeUniform,
    },
};

// Compact, geometry-only campaign layouts decoded from the group's workbook.
mod map_design;

#[cfg(test)]
mod spaceship_tests;

use map_design::{
    DecodedWorkbookMap,
    WorkbookMapDesign,
    ABANDONED,
    ARBITRATOR,
    ARROGANCE,
    KYO,
    NIFFY,
    XY_PLANET,
};

pub(crate) const VOXEL_SIZE: f32 = 0.25;
/// Horizontal physics-body chunk radius around the DM and every player camera.
const VOXEL_PHYSICS_CHUNK_LOAD_RADIUS: i32 = 8;
const VOXEL_DM_GIZMO_RENDER_LAYER: usize = 1;
const PLAYER_STANDEE_PLANE_NORMAL: Vec3 = Vec3::Z;
pub(crate) const MAX_VOXEL_BRUSH_RADIUS: i32 = 50;
const MAX_RAY_DISTANCE: f32 = 200.0;
const PLANET_MAX_RAY_DISTANCE: f32 = 600.0;
const EDIT_REPEAT_INTERVAL: f32 = 0.09;
const TRPG_PHYSICS_SUBSTEPS: u32 = 1;
const ORBITAL_PLANET_RADIUS: f32 = 12.125 * 10.0;
// Keep the old upper surface at the same scene height after increasing radius.
const ORBITAL_PLANET_CENTER: Vec3 = Vec3::new(0.0, -251.125, 0.0);
const ORBITAL_PLANET_VOXEL_RADIUS: i32 = 485;
const ORBITAL_PLANET_CAP_RADIUS: i32 = 128;
const ORBITAL_PLANET_SHELL_THICKNESS: f32 = 2.25;
const ORBITAL_PLANET_GRAVITY_ACCELERATION: f32 = 9.81;
const ORBITAL_PLANET_GRAVITY_MAX_ALTITUDE: f32 = 32.0;
const MAX_SCENE_SNAPSHOTS: usize = 20;
const VOXEL_SCENE_AUTOSAVE_SECONDS: f32 = 30.0;
const VOXEL_SCENE_LAYOUT_REVISION: u32 = 4;
const MAX_EXPLOSION_NEW_PHYSICS_BODIES: usize = 60;
pub(crate) const VOXEL_GLASS_MATERIAL: u8 = 11;
const VOXEL_GLASS_OPACITY: f32 = 0.05;
const VOXEL_MATERIAL_COUNT: usize = VOXEL_GLASS_MATERIAL as usize;
const MICRO_TILE_SUBDIVISIONS: u32 = 16;
const WORKBOOK_FEATURE_HOVER_MIN_Y_CELLS: f32 = 1.0;
const WORKBOOK_FEATURE_HOVER_MAX_Y_CELLS: f32 = 4.0;
const WORKBOOK_FEATURE_LABEL_Y_CELLS: f32 = 5.25;
const WORKBOOK_FEATURE_LABEL_VISIBILITY_RADIUS_METERS: f32 = 20.0;
const VOXEL_EMISSIVE_SCALE: f32 = 0.3;
const VOXEL_RADIANCE_VOLUME_DIMENSION: i32 = 96;
const VOXEL_RADIANCE_REBUILD_STEP: i32 = 16;
const VOXEL_RADIANCE_SKYLIGHT: [u8; 3] = [44, 62, 92];
const VOXEL_RADIANCE_TRANSMISSION: u16 = 238;
const VOXEL_RADIANCE_CUTOFF: u8 = 3;
const DEFAULT_AMBIENT_BRIGHTNESS: f32 = 72.0;
const DEFAULT_KEY_LIGHT_ILLUMINANCE: f32 = 8_500.0;
const DEFAULT_FILL_LIGHT_ILLUMINANCE: f32 = 1_600.0;
const DEFAULT_RADIANCE_INTENSITY: f32 = 0.55;
const FIRST_PERSON_RADIUS: f32 = VOXEL_SIZE * 0.5;
const FIRST_PERSON_BODY_LENGTH: f32 = VOXEL_SIZE;
const FIRST_PERSON_EYE_OFFSET: f32 = VOXEL_SIZE;
const FIRST_PERSON_SPEED: f32 = 2.8;
const FIRST_PERSON_JUMP_SPEED: f32 = 3.4;
const FIRST_PERSON_FLY_SPEED: f32 = 3.5;
const FIRST_PERSON_FOV_RADIANS: f32 = 70.0_f32.to_radians();
const FIRST_PERSON_DOUBLE_TAP_SECONDS: f32 = 0.32;
const DEFAULT_POSSESSION_MOVEMENT_BONUS: f32 = 10.0;
const VOXEL_SPACESHIP_SAVE_SECONDS: f32 = 1.0;
const VOXEL_SPACESHIP_LAYOUT_REVISION: u32 = 4;
const COMBAT_SPACESHIP_ID: &str = "usi-arrogance";
const MEDIUM_SPACESHIP_ID: &str = "medium-ship-01";
const SMALL_SPACESHIP_COUNT: usize = 6;
const ARROGANCE_SCALE: i32 = 3;
const HANGAR_MIN_X: i32 = -104;
const HANGAR_MAX_X: i32 = 112;
const HANGAR_REAR_Z: i32 = -48;
const HANGAR_MOUTH_Z: i32 = 60;
const HANGAR_CEILING_Y: i32 = WORKBOOK_ROOM_HEIGHT * ARROGANCE_SCALE;
const HANGAR_PARKING_Y: i32 = ARROGANCE_SCALE;
const HANGAR_PARKING_Z: i32 = -20;
const HANGAR_DOCK_CLEARANCE_CELLS: f32 = 6.0;
const HANGAR_DOCK_MAX_RELATIVE_SPEED: f32 = 1.0;
const HANGAR_DOCK_MAX_RELATIVE_ANGULAR_SPEED: f32 = 0.35;
const ARROGANCE_CAB_FLOOR_Y: i32 = 3;
const ARROGANCE_CAB_CEILING_Y: i32 = 12;
const ARROGANCE_CAB_FRONT_Z: i32 = HANGAR_REAR_Z - 24;
const ARROGANCE_CAB_REAR_Z: i32 = HANGAR_REAR_Z - 4;
const ARROGANCE_CAB_MAX_HALF_WIDTH: i32 = 12;
const DEFAULT_COLLISION_LAYER_BITS: u32 = 1 << 0;
const CARRIER_COLLISION_LAYER_BITS: u32 = 1 << 1;
const DOCKED_SPACESHIP_COLLISION_LAYER_BITS: u32 = 1 << 2;
const SMALL_SPACESHIP_BERTH_X: [i32; SMALL_SPACESHIP_COUNT] =
    [-88, -60, -32, 36, 64, 92];
const ORBITAL_LAYOUT_SCALE: i32 = 5;
const RESEARCH_STATION_CENTER: IVec3 =
    IVec3::new(100 * ORBITAL_LAYOUT_SCALE, 0, 100 * ORBITAL_LAYOUT_SCALE);
const SENSOR_STATION_CENTER: IVec3 =
    IVec3::new(100 * ORBITAL_LAYOUT_SCALE, 0, -100 * ORBITAL_LAYOUT_SCALE);
const CANNON_STATION_CENTER: IVec3 =
    IVec3::new(-100 * ORBITAL_LAYOUT_SCALE, 0, -100 * ORBITAL_LAYOUT_SCALE);
const COMBAT_SPACESHIP_CENTER: IVec3 =
    IVec3::new(-100 * ORBITAL_LAYOUT_SCALE, 0, 100 * ORBITAL_LAYOUT_SCALE);
const ABANDONED_STATION_CENTER: IVec3 =
    IVec3::new(0, 0, 200 * ORBITAL_LAYOUT_SCALE);
const WORKBOOK_ROOM_HEIGHT: i32 = 7;
const STATION_DOCK_PAD_HALF_WIDTH: i32 = 22;
const STATION_DOCK_PAD_HALF_LENGTH: i32 = 25;
const STATION_DOCK_HULL_GAP: i32 = 8;
const STATION_DOCK_BRIDGE_HALF_WIDTH: i32 = 2;
const STATION_DOCK_CLEAR_HEIGHT: i32 = 10;
const STATION_DOCK_MIN_SEPARATION: i32 = 64;
const FIRST_PERSON_START: Vec3 = Vec3::new(
    (COMBAT_SPACESHIP_CENTER.x + ARROGANCE.spawn[0] * ARROGANCE_SCALE) as f32
        * VOXEL_SIZE,
    (HANGAR_PARKING_Y as f32 + 1.0) * VOXEL_SIZE,
    (COMBAT_SPACESHIP_CENTER.z + ARROGANCE.spawn[1] * ARROGANCE_SCALE) as f32
        * VOXEL_SIZE,
);
const DEFAULT_SCENE_CAMERA_FOCUS: Vec3 = Vec3::new(
    COMBAT_SPACESHIP_CENTER.x as f32 * VOXEL_SIZE,
    1.0,
    COMBAT_SPACESHIP_CENTER.z as f32 * VOXEL_SIZE,
);
const DEFAULT_SCENE_CAMERA_DISTANCE: f32 = 50.0;
const PLAYER_CAPTURE_WIDTH: u32 = 1024;
const PLAYER_CAPTURE_HEIGHT: u32 = 768;
const PLAYER_CAPTURE_PREPARE_FRAMES: u8 = 3;
const PLAYER_STANDEE_HEIGHT: f32 = FIRST_PERSON_BODY_LENGTH + FIRST_PERSON_RADIUS * 2.0;
const TOOL_GUN_DRAG_RESPONSE: f32 = 12.0;
const TOOL_GUN_DRAG_MAX_SPEED: f32 = 24.0;
const PLANET_CLOUD_ALTITUDE: f32 = 3.5;
const PLANET_CLOUD_PUFF_COUNT: usize = 24;
const PLANET_SCIENCE_LAB_CENTER: IVec2 = IVec2::new(-45, 20);
const PLANET_SCIENCE_LAB_FLOOR_Y: i32 = 485;
const VOXEL_MINIMAP_RESOLUTION: usize = 64;
pub(crate) const DEFAULT_VOXEL_OCCLUSION_CAST_WIDTH_CELLS: f32 = 2.0;
pub(crate) const DEFAULT_VOXEL_OCCLUSION_CAST_HEIGHT_CELLS: f32 = 2.0;
pub(crate) const DEFAULT_VOXEL_OCCLUSION_CAST_END_WIDTH_CELLS: f32 = 1.0;
pub(crate) const DEFAULT_VOXEL_OCCLUSION_CAST_END_HEIGHT_CELLS: f32 = 2.0;
pub(crate) const MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS: f32 = 1.0;
pub(crate) const MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS: f32 = 16.0;

pub struct TrpgVoxelPlugin;

pub struct TrpgVoxelConnector;

#[derive(Resource, Debug, Clone)]
pub(crate) struct VoxelReplayOcclusionFade {
    pub(crate) active: bool,
    pub(crate) camera: Vec3,
    pub(crate) targets: Vec<Vec3>,
    pub(crate) opacity: f32,
    pub(crate) cast_width_cells: f32,
    pub(crate) cast_height_cells: f32,
    pub(crate) cast_end_width_cells: f32,
    pub(crate) cast_end_height_cells: f32,
    pub(crate) debug_gizmo: bool,
}

impl Default for VoxelReplayOcclusionFade {
    fn default() -> Self {
        Self {
            active: false,
            camera: Vec3::ZERO,
            targets: Vec::new(),
            opacity: 0.0,
            cast_width_cells: DEFAULT_VOXEL_OCCLUSION_CAST_WIDTH_CELLS,
            cast_height_cells: DEFAULT_VOXEL_OCCLUSION_CAST_HEIGHT_CELLS,
            cast_end_width_cells: DEFAULT_VOXEL_OCCLUSION_CAST_END_WIDTH_CELLS,
            cast_end_height_cells: DEFAULT_VOXEL_OCCLUSION_CAST_END_HEIGHT_CELLS,
            debug_gizmo: false,
        }
    }
}

#[derive(Component)]
struct VoxelOcclusionMesh {
    original_mesh: Handle<Mesh>,
    opaque_mesh: Handle<Mesh>,
    transparent_mesh: Handle<Mesh>,
    transparent_entity: Entity,
}

#[derive(Component)]
struct VoxelOcclusionTransparentMesh;

fn voxel_emissive(red: f32, green: f32, blue: f32) -> LinearRgba {
    LinearRgba::rgb(
        red * VOXEL_EMISSIVE_SCALE,
        green * VOXEL_EMISSIVE_SCALE,
        blue * VOXEL_EMISSIVE_SCALE,
    )
}

impl Connector for TrpgVoxelConnector {
    type Item = u8;

    fn solid(voxel: &Self::Item) -> bool {
        matches!(*voxel, 1..=3 | 6..=VOXEL_GLASS_MATERIAL)
    }
}

#[derive(Component)]
pub struct TrpgVoxelGrid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VoxelMinimapTile {
    pub(crate) top_cell: IVec3,
    pub(crate) material: u8,
    pub(crate) voxel_count: u32,
}

#[derive(Resource, Default)]
pub(crate) struct VoxelMinimapSnapshot {
    pub(crate) min: IVec2,
    pub(crate) max: IVec2,
    pub(crate) resolution: usize,
    pub(crate) tiles: Vec<Option<VoxelMinimapTile>>,
}

impl VoxelMinimapSnapshot {
    pub(crate) fn is_empty(&self) -> bool { self.tiles.is_empty() }

    pub(crate) fn tile(&self, x: usize, z: usize) -> Option<VoxelMinimapTile> {
        (x < self.resolution && z < self.resolution)
            .then(|| self.tiles[z * self.resolution + x])
            .flatten()
    }

    pub(crate) fn nearest_cell(&self, x: usize, z: usize) -> Option<IVec3> {
        if let Some(tile) = self.tile(x, z) {
            return Some(tile.top_cell);
        }
        for radius in 1..self.resolution {
            let min_x = x.saturating_sub(radius);
            let max_x = (x + radius).min(self.resolution.saturating_sub(1));
            let min_z = z.saturating_sub(radius);
            let max_z = (z + radius).min(self.resolution.saturating_sub(1));
            for tile_x in min_x..=max_x {
                for tile_z in [min_z, max_z] {
                    if let Some(tile) = self.tile(tile_x, tile_z) {
                        return Some(tile.top_cell);
                    }
                }
            }
            for tile_z in min_z.saturating_add(1)..max_z {
                for tile_x in [min_x, max_x] {
                    if let Some(tile) = self.tile(tile_x, tile_z) {
                        return Some(tile.top_cell);
                    }
                }
            }
        }
        None
    }

    pub(crate) fn world_fraction(&self, position: Vec3) -> Vec2 {
        let cell = position / VOXEL_SIZE;
        let span = (self.max - self.min).max(IVec2::ONE).as_vec2();
        (Vec2::new(cell.x, cell.z) - self.min.as_vec2()) / span
    }

    pub(crate) fn tile_indices(&self, fraction: Vec2) -> (usize, usize) {
        let last = self.resolution.saturating_sub(1) as f32;
        let clamped = fraction.clamp(Vec2::ZERO, Vec2::ONE) * last;
        (
            clamped.x.round() as usize,
            clamped.y.round() as usize,
        )
    }
}

#[derive(Component)]
pub(crate) struct VoxelViewportCamera;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct VoxelPlayerStandeeSynced;

#[derive(Component)]
struct VoxelPlayerCaptureCamera {
    user_id: u64,
}

#[derive(Component)]
pub(crate) struct VoxelPlayerStandee {
    pub(crate) user_id: u64,
    image_source: String,
    half_size: Vec2,
}

#[derive(Component, Clone)]
enum VoxelStandeeAccess {
    Player(u64),
    Unit(AccessVisibility),
}

#[derive(Component)]
struct VoxelUnitStandee {
    target_id: String,
    unit_id: String,
    image_source: String,
    access_visibility: AccessVisibility,
}

#[cfg(test)]
impl VoxelPlayerStandee {
    pub(crate) fn replay_test(user_id: u64) -> Self {
        Self {
            user_id,
            image_source: String::new(),
            half_size: Vec2::splat(VOXEL_SIZE),
        }
    }
}

#[derive(Resource, Default)]
struct VoxelPlayerStandeeAssets {
    entities: HashMap<u64, Entity>,
    textures: HashMap<String, Handle<Image>>,
    back_label_texture: Option<Handle<Image>>,
    failed_sources: HashSet<String>,
}

#[derive(Resource, Default)]
struct VoxelUnitStandeeAssets {
    entities: HashMap<String, Entity>,
    textures: HashMap<String, Handle<Image>>,
    back_label_texture: Option<Handle<Image>>,
    failed_sources: HashSet<String>,
}

#[derive(Clone)]
struct VoxelPlayerCameraRuntime {
    entity: Entity,
    target: Handle<Image>,
}

#[derive(Resource, Default)]
struct VoxelPlayerCameraRuntimes {
    cameras: HashMap<u64, VoxelPlayerCameraRuntime>,
}

#[derive(Clone, Serialize, Deserialize)]
struct PersistedVoxelPlayerCamera {
    user_id: u64,
    translation: [f32; 3],
    rotation: [f32; 4],
}

#[derive(Resource, Default, Serialize, Deserialize)]
pub(crate) struct VoxelPlayerCameraStore {
    cameras: Vec<PersistedVoxelPlayerCamera>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PersistedVoxelUnitStandee {
    unit_id: String,
    translation: [f32; 3],
    rotation: [f32; 4],
    #[serde(default)]
    visibility: AccessVisibility,
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct VoxelUnitStandeeStore {
    #[serde(default)]
    standees: Vec<PersistedVoxelUnitStandee>,
}

#[derive(Clone, Serialize, Deserialize)]
struct PersistedVoxelPossessionMovement {
    campaign_id: String,
    user_id: u64,
    turn: u32,
    movement_used: f32,
    completed: bool,
    turn_start_position_cells: [f32; 3],
}

#[derive(Resource, Default, Serialize, Deserialize)]
pub(crate) struct VoxelPossessionMovementStore {
    records: Vec<PersistedVoxelPossessionMovement>,
}

#[derive(Resource, Default)]
struct VoxelPlayerCameraEditor {
    selected_user_id: Option<u64>,
    new_user_id: String,
}

#[derive(Resource, Default)]
struct VoxelPlayerCaptureState {
    next_request_id: u64,
    pending: Vec<PendingVoxelPlayerCapture>,
}

struct PendingVoxelPlayerCapture {
    request_id: u64,
    user_id: u64,
    campaign_id: String,
    camera_entity: Entity,
    target: Handle<Image>,
    output_path: std::path::PathBuf,
    kind: SceneCaptureKind,
    prepare_frames_remaining: u8,
    activated: bool,
    original_camera_transform: Option<Transform>,
    video_frame_index: u32,
    video_frame_prepared: bool,
    screenshot_in_flight: bool,
    video_frames_dir: Option<PathBuf>,
    failure: Option<String>,
    hidden_standees: Vec<(Entity, Visibility)>,
}

#[derive(Component)]
struct VoxelGeometry {
    chunk: IVec3,
}

#[derive(Component)]
struct VoxelMicroDecoration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VoxelMicroTileKind {
    Hull,
    Fixture,
}

/// A render-only box measured in sixteenths of one canonical voxel.
///
/// `owner` is always a canonical gameplay cell. Removing that cell removes the
/// detail on the next chunk rebuild; micro tiles never create a second editing,
/// collision, persistence, or raycast grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoxelMicroTile {
    owner: IVec3,
    cell: IVec3,
    min: UVec3,
    max: UVec3,
    material: u8,
    kind: VoxelMicroTileKind,
}

#[derive(Resource, Clone, Debug, Default)]
struct VoxelMicroDecorations {
    tiles: Vec<VoxelMicroTile>,
}

#[derive(Resource, Default)]
struct VoxelGeometryDirtyChunks {
    chunks: HashSet<IVec3>,
}

#[derive(SystemParam)]
struct VoxelEditRuntime<'w> {
    editor: ResMut<'w, VoxelEditorState>,
    dirty_chunks: ResMut<'w, VoxelGeometryDirtyChunks>,
}

impl VoxelGeometryDirtyChunks {
    fn mark_cell_and_neighbors(&mut self, cell: IVec3) {
        let chunk = cell.div_euclid(DIMS);
        self.chunks.insert(chunk);
        let local = cell.rem_euclid(DIMS);
        for (axis, negative, positive) in [
            (local.x, IVec3::NEG_X, IVec3::X),
            (local.y, IVec3::NEG_Y, IVec3::Y),
            (local.z, IVec3::NEG_Z, IVec3::Z),
        ] {
            if axis == 0 {
                self.chunks.insert(chunk + negative);
            }
            if axis == DIMS.x - 1 {
                self.chunks.insert(chunk + positive);
            }
        }
    }
}

#[derive(Component, Clone)]
struct VoxelPhysicsBody {
    local_center: Vec3,
    cells: Vec<(IVec3, u8)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VoxelSpaceshipClass {
    Cruiser,
    Corvette,
    Shuttle,
    Interceptor,
    Scout,
}

impl VoxelSpaceshipClass {
    fn label(self) -> &'static str {
        match self {
            Self::Cruiser => "战斗巡洋舰",
            Self::Corvette => "中型护卫舰",
            Self::Shuttle => "穿梭艇",
            Self::Interceptor => "截击艇",
            Self::Scout => "侦察艇",
        }
    }
}

#[derive(Component, Clone)]
struct VoxelSpaceship {
    id: String,
    name: String,
    class: VoxelSpaceshipClass,
    cockpit_eye_local: Vec3,
    thrust_acceleration: f32,
    vertical_acceleration: f32,
    turn_speed: f32,
    max_speed: f32,
}

#[derive(Clone)]
struct VoxelSpaceshipSpec {
    ship: VoxelSpaceship,
    cells: Vec<(IVec3, u8)>,
    micro_tiles: Vec<VoxelMicroTile>,
    workbook_features: Option<VoxelWorkbookFeatureAnnotations>,
    transform: Transform,
    docking: Option<VoxelSpaceshipDocked>,
}

#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct VoxelSpaceshipDocked {
    carrier_id: String,
    local_translation: [f32; 3],
    local_rotation: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PersistedVoxelSpaceship {
    id: String,
    #[serde(default)]
    pilot_user_id: Option<u64>,
    translation: [f32; 3],
    rotation: [f32; 4],
    linear_velocity: [f32; 3],
    angular_velocity: [f32; 3],
    #[serde(default)]
    docking: Option<VoxelSpaceshipDocked>,
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct VoxelSpaceshipStore {
    ships: Vec<PersistedVoxelSpaceship>,
    #[serde(default)]
    layout_revision: u32,
}

impl Default for VoxelSpaceshipStore {
    fn default() -> Self {
        Self {
            ships: Vec::new(),
            layout_revision: VOXEL_SPACESHIP_LAYOUT_REVISION,
        }
    }
}

#[derive(Resource)]
struct VoxelSpaceshipControlState {
    selected_ship_id: Option<String>,
    driving_ship_id: Option<String>,
    cockpit_eye: Option<Vec3>,
    cockpit_rotation: Quat,
    thrust_input: f32,
    vertical_input: f32,
    boost_active: bool,
    brake_active: bool,
    emergency_stop_requested: bool,
    exit_pending: bool,
}

impl Default for VoxelSpaceshipControlState {
    fn default() -> Self {
        Self {
            selected_ship_id: Some(COMBAT_SPACESHIP_ID.to_owned()),
            driving_ship_id: None,
            cockpit_eye: None,
            cockpit_rotation: Quat::IDENTITY,
            thrust_input: 0.0,
            vertical_input: 0.0,
            boost_active: false,
            brake_active: false,
            emergency_stop_requested: false,
            exit_pending: false,
        }
    }
}

impl VoxelSpaceshipControlState {
    fn stop_driving(&mut self) {
        self.exit_pending |= self.driving_ship_id.is_some();
        self.driving_ship_id = None;
        self.cockpit_eye = None;
        self.thrust_input = 0.0;
        self.vertical_input = 0.0;
        self.boost_active = false;
        self.brake_active = false;
    }
}

#[derive(Resource, Default)]
struct VoxelSpaceshipPersistenceState {
    elapsed_seconds: f32,
}

#[derive(Resource, Default)]
struct VoxelPhysicsChunkLoader {
    columns: HashSet<IVec2>,
    unloaded_bodies: Vec<VoxelPhysicsBodySnapshot>,
}

impl VoxelPhysicsChunkLoader {
    fn contains_world_position(&self, position: Vec3) -> bool {
        self.columns.contains(&voxel_chunk_column(position))
    }
}

#[derive(Clone)]
struct VoxelPhysicsBodySnapshot {
    body: VoxelPhysicsBody,
    transform: Transform,
    linear_velocity: LinearVelocity,
    angular_velocity: AngularVelocity,
}

#[derive(Resource, Default)]
struct VoxelToolGunDragState {
    target: Option<Entity>,
    distance: f32,
    body_offset: Vec3,
}

#[derive(Clone)]
struct VoxelSceneSnapshot {
    name: String,
    voxels: Vec<(IVec3, u8)>,
    physics_bodies: Vec<VoxelPhysicsBodySnapshot>,
    placed_lights: Vec<VoxelPlacedLight>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PersistedVoxelCell {
    position: [i32; 3],
    material: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PersistedVoxelPhysicsBody {
    cells: Vec<PersistedVoxelCell>,
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
    linear_velocity: [f32; 3],
    angular_velocity: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PersistedVoxelPlacedLight {
    kind: VoxelLightTool,
    cell: [i32; 3],
    color: [f32; 3],
    intensity: f32,
    range: f32,
    direction: [f32; 3],
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
    linear_velocity: [f32; 3],
    angular_velocity: [f32; 3],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct PersistedVoxelScene {
    voxels: Vec<PersistedVoxelCell>,
    planet: Option<PersistedVoxelPlanet>,
    physics_bodies: Vec<PersistedVoxelPhysicsBody>,
    placed_lights: Vec<PersistedVoxelPlacedLight>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct PersistedVoxelPlanet {
    cells: Vec<PersistedVoxelCell>,
    removed: Vec<[i32; 3]>,
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct VoxelSceneStore {
    scene: Option<PersistedVoxelScene>,
    #[serde(default)]
    layout_revision: u32,
}

impl Default for VoxelSceneStore {
    fn default() -> Self {
        Self {
            scene: None,
            layout_revision: VOXEL_SCENE_LAYOUT_REVISION,
        }
    }
}

#[derive(Resource, Default)]
struct VoxelScenePersistenceState {
    elapsed_seconds: f32,
    force_save: bool,
}

#[derive(Component)]
struct VoxelOrbitalPlanet {
    cells: HashMap<IVec3, u8>,
    cell_bounds: Option<VoxelCellBounds>,
    removed: HashSet<IVec3>,
    collider_entity: Entity,
    mesh_entities: Vec<Entity>,
    mesh_handles: Vec<Handle<Mesh>>,
    voxel_size: f32,
    dirty: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoxelCellBounds {
    min: IVec3,
    max: IVec3,
}

impl VoxelCellBounds {
    fn from_cell(cell: IVec3) -> Self {
        Self {
            min: cell,
            max: cell,
        }
    }

    fn from_cells(cells: impl IntoIterator<Item = IVec3>) -> Option<Self> {
        let mut cells = cells.into_iter();
        let mut bounds = Self::from_cell(cells.next()?);
        for cell in cells {
            bounds.include(cell);
        }
        Some(bounds)
    }

    fn include(&mut self, cell: IVec3) {
        self.min = self.min.min(cell);
        self.max = self.max.max(cell);
    }

    fn local_aabb(self, voxel_size: f32) -> (Vec3, Vec3) {
        let half_voxel = Vec3::splat(voxel_size * 0.5);
        (
            self.min.as_vec3() * voxel_size - half_voxel,
            self.max.as_vec3() * voxel_size + half_voxel,
        )
    }
}

impl VoxelOrbitalPlanet {
    fn include_cell_in_bounds(&mut self, cell: IVec3) {
        if let Some(bounds) = &mut self.cell_bounds {
            bounds.include(cell);
        } else {
            self.cell_bounds = Some(VoxelCellBounds::from_cell(cell));
        }
    }

    fn refresh_cell_bounds(&mut self) {
        self.cell_bounds = VoxelCellBounds::from_cells(self.cells.keys().copied());
    }
}

#[derive(Component)]
struct VoxelPlanetCollider;

#[derive(Component)]
struct VoxelPlanetCloudLayer;

#[derive(Clone, Copy)]
struct VoxelPlanetRayHit {
    occupied: IVec3,
    normal: IVec3,
    distance: f32,
}

#[derive(Component)]
struct VoxelKeyLight;

#[derive(Component)]
struct VoxelFillLight;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum VoxelLightTool {
    Point,
    DarkPoint,
    Cube,
    Spot,
    Physics,
    Edit,
    Remove,
}

impl VoxelLightTool {
    pub(crate) const ALL: [Self; 7] = [
        Self::Point,
        Self::DarkPoint,
        Self::Cube,
        Self::Spot,
        Self::Physics,
        Self::Edit,
        Self::Remove,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Point => "点光源",
            Self::DarkPoint => "暗色点光",
            Self::Cube => "方块灯",
            Self::Spot => "聚光灯",
            Self::Physics => "物理灯",
            Self::Edit => "灯光编辑器",
            Self::Remove => "移除灯光",
        }
    }

    fn preset(self) -> Option<([f32; 3], f32, f32)> {
        match self {
            Self::Point => Some(([1.0, 0.78, 0.48], 1_800.0, 8.0)),
            Self::DarkPoint => Some(([0.18, 0.08, 0.32], 420.0, 4.0)),
            Self::Cube => Some(([0.2, 0.82, 1.0], 2_200.0, 8.0)),
            Self::Spot => Some(([1.0, 0.9, 0.72], 2_500.0, 10.0)),
            Self::Physics => Some(([0.28, 0.72, 1.0], 1_600.0, 7.0)),
            Self::Edit | Self::Remove => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum VoxelCreativeItem {
    Material(u8),
    Light(VoxelLightTool),
    Mode(VoxelEditMode),
    ToolGun,
    PlayerPossessionTool,
    SpaceshipPossessionTool,
    TeleportTool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VoxelTeleportDestination {
    ResearchStation,
    SensorStation,
    CannonStation,
    CombatSpaceship,
    AbandonedStation,
    PlanetScienceLab,
    PlayerStandee(u64),
    MapCell(IVec3),
}

impl VoxelTeleportDestination {
    pub(crate) const ALL: [Self; 6] = [
        Self::ResearchStation,
        Self::SensorStation,
        Self::CannonStation,
        Self::CombatSpaceship,
        Self::AbandonedStation,
        Self::PlanetScienceLab,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ResearchStation => "U.S.I Niffy女皇号科研空间站",
            Self::SensorStation => "U.S.I 女仲裁者号探测空间站",
            Self::CannonStation => "U.S.I Kyo空间防御炮台",
            Self::CombatSpaceship => "U.S.I 狂妄号战斗巡洋舰",
            Self::AbandonedStation => "废弃空间站",
            Self::PlanetScienceLab => "XY星基地",
            Self::PlayerStandee(_) => "玩家立牌",
            Self::MapCell(_) => "地图位置",
        }
    }

    fn player_position(self) -> Option<Vec3> {
        let floor_offset =
            Vec3::Y * (VOXEL_SIZE + FIRST_PERSON_RADIUS + FIRST_PERSON_BODY_LENGTH * 0.5);
        Some(match self {
            Self::ResearchStation => {
                (RESEARCH_STATION_CENTER
                    + IVec3::new(NIFFY.spawn[0], 0, NIFFY.spawn[1]))
                .as_vec3()
                    * VOXEL_SIZE
                    + floor_offset
            },
            Self::SensorStation => {
                (SENSOR_STATION_CENTER
                    + IVec3::new(ARBITRATOR.spawn[0], 0, ARBITRATOR.spawn[1]))
                .as_vec3()
                    * VOXEL_SIZE
                    + floor_offset
            },
            Self::CannonStation => {
                (CANNON_STATION_CENTER + IVec3::new(KYO.spawn[0], 0, KYO.spawn[1]))
                    .as_vec3()
                    * VOXEL_SIZE
                    + floor_offset
            },
            Self::CombatSpaceship => {
                (COMBAT_SPACESHIP_CENTER
                    + IVec3::new(ARROGANCE.spawn[0], 0, ARROGANCE.spawn[1]))
                .as_vec3()
                    * VOXEL_SIZE
                    + floor_offset
            },
            Self::AbandonedStation => {
                (ABANDONED_STATION_CENTER
                    + IVec3::new(ABANDONED.spawn[0], 0, ABANDONED.spawn[1]))
                .as_vec3()
                    * VOXEL_SIZE
                    + floor_offset
            },
            Self::PlanetScienceLab => {
                ORBITAL_PLANET_CENTER
                    + Vec3::new(
                        (PLANET_SCIENCE_LAB_CENTER.x + XY_PLANET.spawn[0]) as f32,
                        PLANET_SCIENCE_LAB_FLOOR_Y as f32,
                        (PLANET_SCIENCE_LAB_CENTER.y + XY_PLANET.spawn[1]) as f32,
                    ) * VOXEL_SIZE
                    + floor_offset
            },
            Self::PlayerStandee(_) => return None,
            Self::MapCell(cell) => cell.as_vec3() * VOXEL_SIZE + floor_offset,
        })
    }
}

#[derive(Component, Clone)]
struct VoxelPlacedLight {
    kind: VoxelLightTool,
    cell: IVec3,
    color: [f32; 3],
    intensity: f32,
    range: f32,
    direction: Vec3,
}

#[derive(Component)]
struct VoxelFirstPersonPlayer;

#[derive(Component)]
struct VoxelPlanetGravityBody;

#[derive(Resource)]
pub(crate) struct VoxelPossessionState {
    pub active_user_id: Option<u64>,
    applied_user_id: Option<u64>,
    pub selected_hotbar_slot: usize,
    pub player_inventory_open: bool,
    pub movement_used: f32,
    pub movement_limit: f32,
    movement_completed: bool,
    movement_happened: bool,
    movement_turn: u32,
    turn_start_position: Option<Vec3>,
    last_player_position: Option<Vec3>,
    reset_movement_requested: bool,
    movement_limit_bypassed: bool,
    movement_bypass_confirmation_pending: bool,
    persist_elapsed: f32,
    movement_persist_elapsed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum VoxelSkillTargeting {
    Area { radius: f32 },
    Person { range: f32 },
}

#[derive(Resource, Default)]
pub(crate) struct VoxelTargetingPreview {
    pub(crate) skill_name: String,
    pub(crate) affected_user_ids: Vec<u64>,
    pub(crate) show_affected_players: bool,
}

impl Default for VoxelPossessionState {
    fn default() -> Self {
        Self {
            active_user_id: None,
            applied_user_id: None,
            selected_hotbar_slot: 0,
            player_inventory_open: false,
            movement_used: 0.0,
            movement_limit: 0.0,
            movement_completed: false,
            movement_happened: false,
            movement_turn: 0,
            turn_start_position: None,
            last_player_position: None,
            reset_movement_requested: false,
            movement_limit_bypassed: false,
            movement_bypass_confirmation_pending: false,
            persist_elapsed: 0.0,
            movement_persist_elapsed: 0.0,
        }
    }
}

impl VoxelPossessionState {
    pub(crate) fn possess(&mut self, user_id: u64) {
        self.active_user_id = Some(user_id);
        self.reset_turn_overrides();
    }

    pub(crate) fn release(&mut self) {
        self.active_user_id = None;
        self.reset_turn_overrides();
    }

    pub(crate) fn movement_remaining(&self) -> f32 {
        (self.movement_limit - self.movement_used).max(0.0)
    }

    pub(crate) fn movement_is_completed(&self) -> bool { self.movement_completed }

    fn reset_turn_overrides(&mut self) {
        self.player_inventory_open = false;
        self.reset_movement_requested = false;
        self.movement_limit_bypassed = false;
        self.movement_bypass_confirmation_pending = false;
    }
}

#[derive(Component, Clone)]
struct VoxelAutoDoor {
    cells: Vec<IVec3>,
    trigger_center: Vec3,
    trigger_radius: f32,
    trigger_half_height: f32,
    width_axis: IVec3,
    material: u8,
    closed_translation: Vec3,
    open_translation: Vec3,
    open: bool,
}

#[derive(Resource)]
struct VoxelMaterials {
    handles: [Handle<StandardMaterial>; VOXEL_MATERIAL_COUNT],
    planet_ocean: Handle<StandardMaterial>,
}

#[derive(Resource)]
struct VoxelReplayFadeMaterials {
    handles: [Handle<StandardMaterial>; VOXEL_MATERIAL_COUNT],
    planet_ocean: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
struct VoxelRadianceVolume {
    image: Handle<Image>,
    volume_min: Vec3,
    voxel_world_size: f32,
    volume_dimensions: Vec3,
}

impl VoxelRadianceVolume {
    fn uniform(&self, intensity: f32) -> VoxelRadianceCascadeUniform {
        VoxelRadianceCascadeUniform {
            volume_min: self.volume_min,
            voxel_world_size: self.voxel_world_size,
            volume_dimensions: self.volume_dimensions,
            intensity,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum VoxelEditMode {
    #[default]
    Add,
    Remove,
    Paint,
    Physics,
    Drag,
    Push,
    Pull,
    Explode,
}

impl VoxelEditMode {
    pub(crate) const ALL: [Self; 8] = [
        Self::Add,
        Self::Remove,
        Self::Paint,
        Self::Physics,
        Self::Drag,
        Self::Push,
        Self::Pull,
        Self::Explode,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Add => "添加",
            Self::Remove => "删除",
            Self::Paint => "涂色",
            Self::Physics => "物理选区",
            Self::Drag => "拖拽",
            Self::Push => "推开",
            Self::Pull => "拉近",
            Self::Explode => "爆炸",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VoxelPhysicsAction {
    Push,
    Pull,
    Explode,
}

#[derive(Clone, Copy)]
struct VoxelPhysicsRequest {
    action: VoxelPhysicsAction,
    target: Option<Entity>,
    origin: Vec3,
}

#[derive(Clone)]
struct VoxelChange {
    position: IVec3,
    before: u8,
    after: u8,
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct VoxelInventoryStore {
    hotbar: [Option<VoxelCreativeItem>; 10],
    selected_hotbar_slot: usize,
    tool_gun_mode: VoxelEditMode,
}

impl Default for VoxelInventoryStore {
    fn default() -> Self {
        Self {
            hotbar: default_creative_hotbar(),
            selected_hotbar_slot: 0,
            tool_gun_mode: VoxelEditMode::Physics,
        }
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct VoxelToolbarSettingsStore {
    brush_radius: i32,
    first_person_speed: f32,
    placed_light_color: [f32; 3],
    placed_light_intensity: f32,
    placed_light_range: f32,
    physics_push_pull_impulse: f32,
    physics_explosion_impulse: f32,
    physics_explosion_radius: f32,
    ambient_brightness: f32,
    key_light_illuminance: f32,
    key_light_color: [f32; 3],
    fill_light_illuminance: f32,
    fill_light_color: [f32; 3],
    radiance_intensity: f32,
}

impl Default for VoxelToolbarSettingsStore {
    fn default() -> Self {
        Self {
            brush_radius: 0,
            first_person_speed: FIRST_PERSON_SPEED,
            placed_light_color: [1.0, 0.78, 0.48],
            placed_light_intensity: 1_800.0,
            placed_light_range: 8.0,
            physics_push_pull_impulse: 4.0,
            physics_explosion_impulse: 14.0,
            physics_explosion_radius: 6.0,
            ambient_brightness: DEFAULT_AMBIENT_BRIGHTNESS,
            key_light_illuminance: DEFAULT_KEY_LIGHT_ILLUMINANCE,
            key_light_color: [1.0, 1.0, 1.0],
            fill_light_illuminance: DEFAULT_FILL_LIGHT_ILLUMINANCE,
            fill_light_color: [0.5, 0.65, 1.0],
            radiance_intensity: DEFAULT_RADIANCE_INTENSITY,
        }
    }
}

impl VoxelToolbarSettingsStore {
    fn from_editor(editor: &VoxelEditorState) -> Self {
        Self {
            brush_radius: editor.brush_radius,
            first_person_speed: editor.first_person_speed,
            placed_light_color: editor.placed_light_color,
            placed_light_intensity: editor.placed_light_intensity,
            placed_light_range: editor.placed_light_range,
            physics_push_pull_impulse: editor.physics_push_pull_impulse,
            physics_explosion_impulse: editor.physics_explosion_impulse,
            physics_explosion_radius: editor.physics_explosion_radius,
            ambient_brightness: editor.ambient_brightness,
            key_light_illuminance: editor.key_light_illuminance,
            key_light_color: editor.key_light_color,
            fill_light_illuminance: editor.fill_light_illuminance,
            fill_light_color: editor.fill_light_color,
            radiance_intensity: editor.radiance_intensity,
        }
    }

    fn apply_to(&self, editor: &mut VoxelEditorState) {
        let defaults = Self::default();
        editor.brush_radius = self.brush_radius.clamp(0, MAX_VOXEL_BRUSH_RADIUS);
        editor.first_person_speed = finite_clamp(
            self.first_person_speed,
            0.25,
            50.0,
            defaults.first_person_speed,
        );
        editor.placed_light_color = finite_color(
            self.placed_light_color,
            defaults.placed_light_color,
        );
        editor.placed_light_intensity = finite_clamp(
            self.placed_light_intensity,
            0.0,
            100_000.0,
            defaults.placed_light_intensity,
        );
        editor.placed_light_range = finite_clamp(
            self.placed_light_range,
            0.25,
            100.0,
            defaults.placed_light_range,
        );
        editor.physics_push_pull_impulse = finite_clamp(
            self.physics_push_pull_impulse,
            0.1,
            200.0,
            defaults.physics_push_pull_impulse,
        );
        editor.physics_explosion_impulse = finite_clamp(
            self.physics_explosion_impulse,
            0.1,
            500.0,
            defaults.physics_explosion_impulse,
        );
        editor.physics_explosion_radius = finite_clamp(
            self.physics_explosion_radius,
            VOXEL_SIZE,
            100.0,
            defaults.physics_explosion_radius,
        );
        editor.ambient_brightness = finite_clamp(
            self.ambient_brightness,
            0.0,
            500.0,
            defaults.ambient_brightness,
        );
        editor.key_light_illuminance = finite_clamp(
            self.key_light_illuminance,
            0.0,
            50_000.0,
            defaults.key_light_illuminance,
        );
        editor.key_light_color = finite_color(
            self.key_light_color,
            defaults.key_light_color,
        );
        editor.fill_light_illuminance = finite_clamp(
            self.fill_light_illuminance,
            0.0,
            50_000.0,
            defaults.fill_light_illuminance,
        );
        editor.fill_light_color = finite_color(
            self.fill_light_color,
            defaults.fill_light_color,
        );
        editor.radiance_intensity = finite_clamp(
            self.radiance_intensity,
            0.0,
            3.0,
            defaults.radiance_intensity,
        );
    }
}

fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn finite_color(color: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|index| finite_clamp(color[index], 0.0, 1.0, fallback[index]))
}

fn default_creative_hotbar() -> [Option<VoxelCreativeItem>; 10] {
    std::array::from_fn(|index| {
        Some(VoxelCreativeItem::Material(
            index as u8 + 1,
        ))
    })
}

#[derive(Resource)]
pub(crate) struct VoxelEditorState {
    pub mode: VoxelEditMode,
    pub material: u8,
    pub brush_radius: i32,
    pub viewport_min: Vec2,
    pub viewport_max: Vec2,
    pub undo_requested: bool,
    pub redo_requested: bool,
    pub reset_requested: bool,
    pub view_reset_requested: bool,
    pub first_person_enabled: bool,
    pub first_person_flying: bool,
    pub first_person_speed: f32,
    pub creative_inventory_open: bool,
    pub teleport_menu_open: bool,
    pub creative_hotbar: [Option<VoxelCreativeItem>; 10],
    pub selected_hotbar_slot: usize,
    equipped_item: Option<VoxelCreativeItem>,
    tool_gun_mode: VoxelEditMode,
    pub light_tool: Option<VoxelLightTool>,
    pub placed_light_color: [f32; 3],
    pub placed_light_intensity: f32,
    pub placed_light_range: f32,
    selected_light: Option<Entity>,
    pub physics_requested: bool,
    physics_action_requested: Option<VoxelPhysicsRequest>,
    pub physics_push_pull_impulse: f32,
    pub physics_explosion_impulse: f32,
    pub physics_explosion_radius: f32,
    pub ambient_brightness: f32,
    pub key_light_illuminance: f32,
    pub key_light_color: [f32; 3],
    pub fill_light_illuminance: f32,
    pub fill_light_color: [f32; 3],
    pub radiance_intensity: f32,
    undo: Vec<Vec<VoxelChange>>,
    redo: Vec<Vec<VoxelChange>>,
    stroke_positions: HashSet<IVec3>,
    active_stroke: Vec<VoxelChange>,
    edit_repeat_seconds: f32,
    camera_focus: Vec3,
    camera_distance: f32,
    camera_yaw: f32,
    camera_pitch: f32,
    camera_drag_started_in_viewport: bool,
    left_started_over_ui: bool,
    right_started_over_ui: bool,
    selection_anchor: Option<IVec3>,
    selection_end: Option<IVec3>,
    selection_is_planet: bool,
    physics_status: Option<String>,
    scene_snapshots: Vec<VoxelSceneSnapshot>,
    next_scene_snapshot_number: u64,
    save_scene_requested: bool,
    restore_scene_requested: Option<usize>,
    reset_scene_confirmation: bool,
    first_person_space_tap_elapsed: f32,
    first_person_was_enabled: bool,
    first_person_cursor_released: bool,
    teleport_requested: Option<VoxelTeleportDestination>,
}

impl Default for VoxelEditorState {
    fn default() -> Self {
        Self {
            mode: VoxelEditMode::Add,
            material: 1,
            brush_radius: 0,
            viewport_min: Vec2::ZERO,
            viewport_max: Vec2::ZERO,
            undo_requested: false,
            redo_requested: false,
            reset_requested: false,
            view_reset_requested: false,
            first_person_enabled: true,
            first_person_flying: true,
            first_person_speed: FIRST_PERSON_SPEED,
            creative_inventory_open: false,
            teleport_menu_open: false,
            creative_hotbar: default_creative_hotbar(),
            selected_hotbar_slot: 0,
            equipped_item: Some(VoxelCreativeItem::Material(1)),
            tool_gun_mode: VoxelEditMode::Physics,
            light_tool: None,
            placed_light_color: [1.0, 0.78, 0.48],
            placed_light_intensity: 1_800.0,
            placed_light_range: 8.0,
            selected_light: None,
            physics_requested: false,
            physics_action_requested: None,
            physics_push_pull_impulse: 4.0,
            physics_explosion_impulse: 14.0,
            physics_explosion_radius: 6.0,
            ambient_brightness: DEFAULT_AMBIENT_BRIGHTNESS,
            key_light_illuminance: DEFAULT_KEY_LIGHT_ILLUMINANCE,
            key_light_color: [1.0, 1.0, 1.0],
            fill_light_illuminance: DEFAULT_FILL_LIGHT_ILLUMINANCE,
            fill_light_color: [0.5, 0.65, 1.0],
            radiance_intensity: DEFAULT_RADIANCE_INTENSITY,
            undo: Vec::new(),
            redo: Vec::new(),
            stroke_positions: HashSet::new(),
            active_stroke: Vec::new(),
            edit_repeat_seconds: 0.0,
            camera_focus: DEFAULT_SCENE_CAMERA_FOCUS,
            camera_distance: DEFAULT_SCENE_CAMERA_DISTANCE,
            camera_yaw: 0.7,
            camera_pitch: -0.45,
            camera_drag_started_in_viewport: false,
            left_started_over_ui: false,
            right_started_over_ui: false,
            selection_anchor: None,
            selection_end: None,
            selection_is_planet: false,
            physics_status: None,
            scene_snapshots: Vec::new(),
            next_scene_snapshot_number: 1,
            save_scene_requested: false,
            restore_scene_requested: None,
            reset_scene_confirmation: false,
            first_person_space_tap_elapsed: f32::INFINITY,
            first_person_was_enabled: false,
            // Keep the OS cursor free until the user explicitly clicks the 3D viewport.
            first_person_cursor_released: true,
            teleport_requested: None,
        }
    }
}

impl VoxelEditorState {
    pub(crate) fn set_viewport_bounds(
        &mut self,
        viewport_min: Vec2,
        viewport_max: Vec2,
        toolbar_bottom: f32,
    ) {
        self.viewport_min = Vec2::new(
            viewport_min.x,
            viewport_min.y.max(toolbar_bottom),
        );
        self.viewport_max = viewport_max;
    }

    fn contains_cursor(&self, cursor: Vec2) -> bool {
        cursor.cmpge(self.viewport_min).all() && cursor.cmple(self.viewport_max).all()
    }

    pub(crate) fn has_physics_selection(&self) -> bool { self.selection_bounds().is_some() }

    pub(crate) fn physics_selection_hint(&self) -> &str {
        if self.selection_anchor.is_none() {
            "依次右键点击两个方块，框选物理区域"
        } else if self.selection_end.is_none() {
            "再右键点击一个方块，确定选区另一角"
        } else {
            "选区已确定；可重新选择起点或生成物理体"
        }
    }

    pub(crate) fn physics_status(&self) -> Option<&str> { self.physics_status.as_deref() }

    pub(crate) fn has_selected_light(&self) -> bool { self.selected_light.is_some() }

    pub(crate) fn select_material(&mut self, material: u8) {
        self.equipped_item = Some(VoxelCreativeItem::Material(material));
        self.teleport_menu_open = false;
        self.material = material;
        self.mode = VoxelEditMode::Add;
        self.light_tool = None;
        self.selected_light = None;
    }

    pub(crate) fn equip_creative_item(&mut self, item: VoxelCreativeItem) {
        self.equipped_item = Some(item);
        match item {
            VoxelCreativeItem::Material(material) => self.select_material(material),
            VoxelCreativeItem::Light(tool) => {
                self.light_tool = Some(tool);
                self.selected_light = None;
                if let Some((color, intensity, range)) = tool.preset() {
                    self.placed_light_color = color;
                    self.placed_light_intensity = intensity;
                    self.placed_light_range = range;
                }
            },
            VoxelCreativeItem::Mode(mode) => {
                self.mode = mode;
                self.light_tool = None;
                self.selected_light = None;
            },
            VoxelCreativeItem::ToolGun => {
                self.mode = self.tool_gun_mode;
                self.light_tool = None;
                self.selected_light = None;
            },
            VoxelCreativeItem::PlayerPossessionTool
            | VoxelCreativeItem::SpaceshipPossessionTool => {
                self.light_tool = None;
                self.selected_light = None;
            },
            VoxelCreativeItem::TeleportTool => {
                self.light_tool = None;
                self.selected_light = None;
            },
        }
        if item != VoxelCreativeItem::TeleportTool {
            self.teleport_menu_open = false;
        }
    }

    pub(crate) fn select_hotbar_slot(&mut self, slot: usize) {
        if slot >= self.creative_hotbar.len() {
            return;
        }
        self.selected_hotbar_slot = slot;
        if let Some(item) = self.creative_hotbar[slot] {
            self.equip_creative_item(item);
        } else {
            self.equipped_item = None;
            self.light_tool = None;
            self.selected_light = None;
            self.teleport_menu_open = false;
        }
    }

    pub(crate) fn put_in_selected_hotbar(&mut self, item: VoxelCreativeItem) {
        self.creative_hotbar[self.selected_hotbar_slot] = Some(item);
        self.equip_creative_item(item);
    }

    pub(crate) fn delete_hotbar_slot(&mut self, slot: usize) {
        if slot < self.creative_hotbar.len() {
            self.creative_hotbar[slot] = None;
            if slot == self.selected_hotbar_slot {
                self.equipped_item = None;
                self.light_tool = None;
                self.selected_light = None;
                self.teleport_menu_open = false;
            }
        }
    }

    pub(crate) fn swap_hotbar_slots(&mut self, source: usize, destination: usize) {
        if source >= self.creative_hotbar.len() || destination >= self.creative_hotbar.len() {
            return;
        }
        self.creative_hotbar.swap(source, destination);
        if source == self.selected_hotbar_slot || destination == self.selected_hotbar_slot {
            if let Some(item) = self.creative_hotbar[self.selected_hotbar_slot] {
                self.equip_creative_item(item);
            } else {
                self.equipped_item = None;
                self.light_tool = None;
                self.selected_light = None;
                self.teleport_menu_open = false;
            }
        }
    }

    pub(crate) fn select_mode(&mut self, mode: VoxelEditMode) {
        self.equip_creative_item(VoxelCreativeItem::Mode(mode));
    }

    pub(crate) fn active_tool_label(&self) -> String {
        if self.equipped_item.is_none() {
            return "空手".to_owned();
        }
        if self.is_tool_gun_equipped() {
            return format!(
                "工具枪 · {}",
                self.tool_gun_mode.label()
            );
        }
        if self.is_player_possession_tool_equipped() {
            return "PL接管器".to_owned();
        }
        if self.is_spaceship_possession_tool_equipped() {
            return "舰船接管器".to_owned();
        }
        if self.is_teleport_tool_equipped() {
            return "传送器".to_owned();
        }
        self.light_tool
            .map_or_else(
                || self.mode.label(),
                VoxelLightTool::label,
            )
            .to_owned()
    }

    pub(crate) fn is_tool_gun_equipped(&self) -> bool {
        self.equipped_item == Some(VoxelCreativeItem::ToolGun)
    }

    pub(crate) fn is_player_possession_tool_equipped(&self) -> bool {
        self.equipped_item == Some(VoxelCreativeItem::PlayerPossessionTool)
    }

    pub(crate) fn is_spaceship_possession_tool_equipped(&self) -> bool {
        self.equipped_item == Some(VoxelCreativeItem::SpaceshipPossessionTool)
    }

    pub(crate) fn is_teleport_tool_equipped(&self) -> bool {
        self.equipped_item == Some(VoxelCreativeItem::TeleportTool)
    }

    pub(crate) fn request_teleport(&mut self, destination: VoxelTeleportDestination) {
        self.teleport_requested = Some(destination);
        self.teleport_menu_open = false;
    }

    pub(crate) fn cycle_tool_gun_mode(&mut self) {
        const MODES: [VoxelEditMode; 5] = [
            VoxelEditMode::Physics,
            VoxelEditMode::Drag,
            VoxelEditMode::Push,
            VoxelEditMode::Pull,
            VoxelEditMode::Explode,
        ];
        let current = MODES
            .iter()
            .position(|mode| *mode == self.tool_gun_mode)
            .unwrap_or(0);
        self.tool_gun_mode = MODES[(current + 1) % MODES.len()];
        if self.is_tool_gun_equipped() {
            self.mode = self.tool_gun_mode;
            self.selection_anchor = None;
            self.selection_end = None;
            self.selection_is_planet = false;
            self.physics_status = Some(format!(
                "工具枪模式：{}",
                self.tool_gun_mode.label()
            ));
        }
    }

    pub(crate) fn request_scene_snapshot(&mut self) { self.save_scene_requested = true; }

    pub(crate) fn scene_snapshot_labels(&self) -> Vec<String> {
        self.scene_snapshots
            .iter()
            .map(|snapshot| {
                format!(
                    "{}（{} 方块 / {} 物理体 / {} 灯光）",
                    snapshot.name,
                    snapshot.voxels.len(),
                    snapshot.physics_bodies.len(),
                    snapshot.placed_lights.len()
                )
            })
            .collect()
    }

    pub(crate) fn request_scene_restore(&mut self, index: usize) {
        if index < self.scene_snapshots.len() {
            self.restore_scene_requested = Some(index);
        }
    }

    pub(crate) fn reset_scene_confirmation(&self) -> bool { self.reset_scene_confirmation }

    pub(crate) fn request_reset_scene_confirmation(&mut self) {
        self.reset_scene_confirmation = true;
    }

    pub(crate) fn cancel_reset_scene_confirmation(&mut self) {
        self.reset_scene_confirmation = false;
    }

    pub(crate) fn confirm_reset_scene(&mut self) {
        self.reset_scene_confirmation = false;
        self.reset_requested = true;
    }

    fn selection_bounds(&self) -> Option<(IVec3, IVec3)> {
        let start = self.selection_anchor?;
        let end = self.selection_end?;
        Some((start.min(end), start.max(end)))
    }

    fn select_physics_corner(&mut self, cell: IVec3, is_planet: bool) {
        if self.selection_anchor.is_none()
            || self.selection_end.is_some()
            || self.selection_is_planet != is_planet
        {
            self.selection_anchor = Some(cell);
            self.selection_end = None;
            self.selection_is_planet = is_planet;
        } else {
            self.selection_end = Some(cell);
        }
        self.physics_status = None;
    }

    pub(crate) fn inspect_radiance_lighting(&mut self) {
        self.ambient_brightness = 0.0;
        self.key_light_illuminance = 0.0;
        self.fill_light_illuminance = 0.0;
        self.radiance_intensity = 1.2;
    }

    pub(crate) fn reset_lighting(&mut self) {
        self.ambient_brightness = DEFAULT_AMBIENT_BRIGHTNESS;
        self.key_light_illuminance = DEFAULT_KEY_LIGHT_ILLUMINANCE;
        self.key_light_color = [1.0, 1.0, 1.0];
        self.fill_light_illuminance = DEFAULT_FILL_LIGHT_ILLUMINANCE;
        self.fill_light_color = [0.5, 0.65, 1.0];
        self.radiance_intensity = DEFAULT_RADIANCE_INTENSITY;
    }
}

impl Plugin for TrpgVoxelPlugin {
    fn build(&self, app: &mut App) {
        let player_camera_store = Persistent::<VoxelPlayerCameraStore>::builder()
            .name("voxel_player_cameras")
            .format(StorageFormat::Toml)
            .path(
                Path::new(".data")
                    .join("willowblossom")
                    .join("voxel_player_cameras.toml"),
            )
            .default(VoxelPlayerCameraStore::default())
            .build()
            .expect("failed to initialize voxel player camera store");
        let unit_standee_store = Persistent::<VoxelUnitStandeeStore>::builder()
            .name("voxel_unit_standees")
            .format(StorageFormat::Toml)
            .path(
                Path::new(".data")
                    .join("willowblossom")
                    .join("voxel_unit_standees.toml"),
            )
            .default(VoxelUnitStandeeStore::default())
            .revertible(true)
            .revert_to_default_on_deserialization_errors(true)
            .build()
            .expect("failed to initialize voxel unit standee store");
        let possession_movement_store = Persistent::<VoxelPossessionMovementStore>::builder()
            .name("voxel_possession_movement")
            .format(StorageFormat::Toml)
            .path(
                Path::new(".data")
                    .join("willowblossom")
                    .join("voxel_possession_movement.toml"),
            )
            .default(VoxelPossessionMovementStore::default())
            .build()
            .expect("failed to initialize voxel possession movement store");
        let inventory_store = Persistent::<VoxelInventoryStore>::builder()
            .name("voxel_creative_inventory")
            .format(StorageFormat::Toml)
            .path(
                Path::new(".data")
                    .join("willowblossom")
                    .join("voxel_creative_inventory.toml"),
            )
            .default(VoxelInventoryStore::default())
            .revertible(true)
            .revert_to_default_on_deserialization_errors(true)
            .build()
            .expect("failed to initialize voxel creative inventory store");
        let toolbar_settings_store = Persistent::<VoxelToolbarSettingsStore>::builder()
            .name("voxel_toolbar_settings")
            .format(StorageFormat::Toml)
            .path(
                Path::new(".data")
                    .join("willowblossom")
                    .join("voxel_toolbar_settings.toml"),
            )
            .default(VoxelToolbarSettingsStore::default())
            .revertible(true)
            .revert_to_default_on_deserialization_errors(true)
            .build()
            .expect("failed to initialize voxel toolbar settings");
        let scene_store = Persistent::<VoxelSceneStore>::builder()
            .name("voxel_scene")
            .format(StorageFormat::Bincode)
            .path(
                Path::new(".data")
                    .join("willowblossom")
                    .join("voxel_scene.bin"),
            )
            .default(VoxelSceneStore::default())
            .revertible(true)
            .revert_to_default_on_deserialization_errors(true)
            .build()
            .expect("failed to initialize voxel scene store");
        let spaceship_store = Persistent::<VoxelSpaceshipStore>::builder()
            .name("voxel_spaceships")
            .format(StorageFormat::Toml)
            .path(
                Path::new(".data")
                    .join("willowblossom")
                    .join("voxel_spaceships.toml"),
            )
            .default(VoxelSpaceshipStore::default())
            .revertible(true)
            .revert_to_default_on_deserialization_errors(true)
            .build()
            .expect("failed to initialize voxel spaceship store");
        app.add_plugins((
            PhysicsPlugins::default(),
            VoxelPlugin::<u8>::default(),
            ConnectivityPlugin::<TrpgVoxelConnector>::default(),
            VoxelRadianceCascadePlugin,
        ))
        // Player observation is a prepared screenshot, so one inexpensive physics substep is
        // sufficient and avoids repeating the solver when explosions create many fragments.
        .insert_resource(SubstepCount(TRPG_PHYSICS_SUBSTEPS))
        .insert_resource(Gravity::ZERO)
        .insert_resource(static_workbook_micro_decorations())
        .insert_resource(static_workbook_feature_annotations())
        .init_resource::<VoxelEditorState>()
        .init_resource::<VoxelPossessionState>()
        .init_resource::<VoxelTargetingPreview>()
        .init_resource::<VoxelRadianceVolume>()
        .init_resource::<SceneCaptureRequests>()
        .init_resource::<SceneCharacterPositions>()
        .init_resource::<VoxelPlayerCameraRuntimes>()
        .init_resource::<VoxelPlayerCameraEditor>()
        .init_resource::<VoxelPlayerCaptureState>()
        .init_resource::<VoxelPlayerStandeeAssets>()
        .init_resource::<VoxelUnitStandeeAssets>()
        .init_resource::<VoxelToolGunDragState>()
        .init_resource::<VoxelPhysicsChunkLoader>()
        .init_resource::<VoxelGeometryDirtyChunks>()
        .init_resource::<VoxelScenePersistenceState>()
        .init_resource::<VoxelSpaceshipControlState>()
        .init_resource::<VoxelSpaceshipPersistenceState>()
        .init_resource::<VoxelMinimapSnapshot>()
        .init_resource::<VoxelReplayOcclusionFade>()
        .insert_resource(player_camera_store)
        .insert_resource(unit_standee_store)
        .insert_resource(possession_movement_store)
        .insert_resource(inventory_store)
        .insert_resource(toolbar_settings_store)
        .insert_resource(scene_store)
        .insert_resource(spaceship_store)
        .add_systems(
            Startup,
            (
                load_voxel_inventory,
                load_voxel_toolbar_settings,
                setup_voxel_materials,
                setup_voxel_grid,
                populate_voxel_grid,
                setup_voxel_auto_doors,
                setup_voxel_interior_lights,
                setup_voxel_sample_props,
                setup_voxel_radiance_volume,
                setup_voxel_view,
                load_persisted_voxel_scene,
                setup_voxel_spaceships,
                setup_voxel_player_cameras,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                (
                    voxel_editor_shortcuts,
                    handle_editor_requests,
                    use_player_possession_tool
                        .run_if(crate::replay::replay_mouse_interaction_inactive),
                    use_spaceship_possession_tool
                        .run_if(crate::replay::replay_mouse_interaction_inactive),
                    place_creative_light
                        .run_if(crate::replay::replay_mouse_interaction_inactive),
                    sync_selected_voxel_light,
                    edit_voxel_grid.run_if(crate::replay::replay_mouse_interaction_inactive),
                    despawn_unsupported_voxel_auto_doors,
                    use_voxel_teleport_tool
                        .run_if(crate::replay::replay_mouse_interaction_inactive),
                    drag_voxel_physics_body
                        .run_if(crate::replay::replay_mouse_interaction_inactive),
                    make_selection_physical,
                    rebuild_voxel_orbital_planet,
                    apply_voxel_physics_action,
                    process_voxel_scene_history,
                    refresh_voxel_minimap_snapshot,
                )
                    .chain(),
                (
                    (
                        update_loaded_voxel_physics_chunks,
                        stream_voxel_physics_bodies,
                        animate_voxel_auto_doors,
                        rebuild_voxel_geometry,
                        sync_voxel_radiance_volume,
                        sync_voxel_lighting,
                        apply_voxel_teleport,
                        release_controlled_docked_spaceship,
                        control_voxel_spaceships,
                        dock_idle_voxel_spaceships,
                        sync_docked_voxel_spaceships,
                        control_first_person_player,
                        control_voxel_camera
                            .run_if(crate::replay::replay_mouse_interaction_inactive),
                    )
                        .chain(),
                    (
                        sync_possessed_player_camera,
                        sync_voxel_player_cameras,
                        sync_voxel_player_standees.in_set(VoxelPlayerStandeeSynced),
                        sync_voxel_unit_standees,
                        sync_voxel_scene_character_positions,
                        capture_voxel_player_view,
                        draw_voxel_target.run_if(crate::replay::replay_video_capture_inactive),
                        animate_planet_clouds,
                        animate_voxel_materials,
                        persist_voxel_inventory,
                        persist_voxel_toolbar_settings,
                        persist_voxel_scene,
                        persist_voxel_spaceships,
                    )
                        .chain(),
                )
                    .chain(),
            )
                .chain(),
        )
        .add_systems(
            Update,
            draw_possessed_player_targeting
                .after(VoxelPlayerStandeeSynced)
                .run_if(crate::replay::replay_video_capture_inactive),
        )
        .add_systems(
            PhysicsSchedule,
            apply_voxel_planet_gravity.before(PhysicsStepSystems::First),
        )
        .add_systems(
            PostUpdate,
            (
                sync_voxel_occlusion_fade,
                draw_voxel_occlusion_cast_gizmos,
            )
                .chain()
                .after(crate::replay::ReplayCameraApplied)
                .before(TransformSystems::Propagate),
        )
        .add_systems(
            EguiPrimaryContextPass,
            (
                voxel_workbook_feature_overlay,
                voxel_player_camera_panel,
                voxel_spaceship_panel,
            )
                .chain()
                .after(crate::ui::ui_system)
                .run_if(crate::replay::replay_video_capture_inactive),
        );
    }
}

fn load_voxel_inventory(
    store: Res<Persistent<VoxelInventoryStore>>,
    mut editor: ResMut<VoxelEditorState>,
) {
    editor.creative_hotbar = store.hotbar;
    editor.selected_hotbar_slot = store
        .selected_hotbar_slot
        .min(editor.creative_hotbar.len() - 1);
    editor.tool_gun_mode = store.tool_gun_mode;
    let selected_hotbar_slot = editor.selected_hotbar_slot;
    editor.select_hotbar_slot(selected_hotbar_slot);
}

fn persist_voxel_inventory(
    editor: Res<VoxelEditorState>,
    mut store: ResMut<Persistent<VoxelInventoryStore>>,
) {
    let snapshot = VoxelInventoryStore {
        hotbar: editor.creative_hotbar,
        selected_hotbar_slot: editor.selected_hotbar_slot,
        tool_gun_mode: editor.tool_gun_mode,
    };
    if **store == snapshot {
        return;
    }
    **store = snapshot;
    if let Err(err) = store.persist() {
        eprintln!("failed to persist voxel creative inventory: {err}");
    }
}

fn load_voxel_toolbar_settings(
    store: Res<Persistent<VoxelToolbarSettingsStore>>,
    mut editor: ResMut<VoxelEditorState>,
) {
    store.apply_to(&mut editor);
}

fn persist_voxel_toolbar_settings(
    editor: Res<VoxelEditorState>,
    mut store: ResMut<Persistent<VoxelToolbarSettingsStore>>,
) {
    let snapshot = VoxelToolbarSettingsStore::from_editor(&editor);
    if **store == snapshot {
        return;
    }
    **store = snapshot;
    if let Err(err) = store.persist() {
        eprintln!("failed to persist voxel toolbar settings: {err}");
    }
}

fn persisted_voxel_cell(cell: IVec3, material: u8) -> PersistedVoxelCell {
    PersistedVoxelCell {
        position: cell.to_array(),
        material,
    }
}

fn persisted_voxel_physics_body(
    body: &VoxelPhysicsBody,
    transform: &Transform,
    linear_velocity: &LinearVelocity,
    angular_velocity: &AngularVelocity,
) -> PersistedVoxelPhysicsBody {
    PersistedVoxelPhysicsBody {
        cells: body
            .cells
            .iter()
            .map(|(cell, material)| persisted_voxel_cell(*cell, *material))
            .collect(),
        translation: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
        scale: transform.scale.to_array(),
        linear_velocity: linear_velocity.0.to_array(),
        angular_velocity: angular_velocity.0.to_array(),
    }
}

fn persisted_voxel_light(
    light: &VoxelPlacedLight,
    transform: &Transform,
    linear_velocity: Option<&LinearVelocity>,
    angular_velocity: Option<&AngularVelocity>,
) -> PersistedVoxelPlacedLight {
    PersistedVoxelPlacedLight {
        kind: light.kind,
        cell: light.cell.to_array(),
        color: light.color,
        intensity: light.intensity,
        range: light.range,
        direction: light.direction.to_array(),
        translation: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
        scale: transform.scale.to_array(),
        linear_velocity: linear_velocity
            .unwrap_or(&LinearVelocity::ZERO)
            .0
            .to_array(),
        angular_velocity: angular_velocity
            .unwrap_or(&AngularVelocity::ZERO)
            .0
            .to_array(),
    }
}

fn runtime_voxel_physics_body(
    body: &PersistedVoxelPhysicsBody,
) -> (
    Vec<(IVec3, u8)>,
    Transform,
    LinearVelocity,
    AngularVelocity,
) {
    (
        body.cells
            .iter()
            .map(|cell| {
                (
                    IVec3::from_array(cell.position),
                    cell.material,
                )
            })
            .collect(),
        Transform {
            translation: Vec3::from_array(body.translation),
            rotation: Quat::from_array(body.rotation).normalize(),
            scale: Vec3::from_array(body.scale),
        },
        LinearVelocity(Vec3::from_array(body.linear_velocity)),
        AngularVelocity(Vec3::from_array(body.angular_velocity)),
    )
}

fn runtime_voxel_light(light: &PersistedVoxelPlacedLight) -> VoxelPlacedLight {
    VoxelPlacedLight {
        kind: light.kind,
        cell: IVec3::from_array(light.cell),
        color: finite_color(light.color, [1.0, 0.78, 0.48]),
        intensity: finite_clamp(light.intensity, 0.0, 100_000.0, 1_800.0),
        range: finite_clamp(light.range, VOXEL_SIZE, 100.0, 8.0),
        direction: Vec3::from_array(light.direction),
    }
}

fn load_persisted_voxel_scene(
    mut commands: Commands,
    store: Res<Persistent<VoxelSceneStore>>,
    mut physics_loader: ResMut<VoxelPhysicsChunkLoader>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    physics_bodies: Query<Entity, With<VoxelPhysicsBody>>,
    placed_lights: Query<Entity, With<VoxelPlacedLight>>,
    mut planets: Query<&mut VoxelOrbitalPlanet>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    if store.layout_revision != VOXEL_SCENE_LAYOUT_REVISION {
        return;
    }
    let Some(scene) = store.scene.as_ref() else {
        return;
    };
    let Ok(mut grid) = grids.single_mut() else {
        return;
    };
    for cell in occupied_cells(&grid) {
        grid.set(cell, 0);
    }
    for cell in &scene.voxels {
        if cell.material != 0 {
            grid.set(
                IVec3::from_array(cell.position),
                cell.material,
            );
        }
    }
    if let (Some(saved_planet), Ok(mut planet)) = (
        scene.planet.as_ref(),
        planets.single_mut(),
    ) {
        planet.cells = saved_planet
            .cells
            .iter()
            .filter(|cell| cell.material != 0)
            .map(|cell| {
                (
                    IVec3::from_array(cell.position),
                    cell.material,
                )
            })
            .collect();
        planet.removed = saved_planet
            .removed
            .iter()
            .copied()
            .map(IVec3::from_array)
            .collect();
        planet.refresh_cell_bounds();
        planet.dirty = true;
    }
    for entity in &physics_bodies {
        commands.entity(entity).despawn();
    }
    for entity in &placed_lights {
        commands.entity(entity).despawn();
    }
    physics_loader.unloaded_bodies.clear();
    for body in &scene.physics_bodies {
        let (cells, transform, linear_velocity, angular_velocity) =
            runtime_voxel_physics_body(body);
        if cells.is_empty() {
            continue;
        }
        spawn_voxel_physics_body_at(
            &mut commands,
            &mut meshes,
            &materials,
            cells,
            transform,
            linear_velocity,
            angular_velocity,
        );
    }
    for light in &scene.placed_lights {
        let entity = spawn_voxel_placed_light(
            &mut commands,
            &mut meshes,
            &materials,
            runtime_voxel_light(light),
        );
        if light.kind == VoxelLightTool::Physics {
            commands.entity(entity).insert((
                Transform {
                    translation: Vec3::from_array(light.translation),
                    rotation: Quat::from_array(light.rotation).normalize(),
                    scale: Vec3::from_array(light.scale),
                },
                LinearVelocity(Vec3::from_array(light.linear_velocity)),
                AngularVelocity(Vec3::from_array(light.angular_velocity)),
            ));
        }
    }
}

fn persist_voxel_scene(
    time: Res<Time>,
    mut app_exit: MessageReader<AppExit>,
    mut persistence: ResMut<VoxelScenePersistenceState>,
    grids: Query<&Grid<u8>, With<TrpgVoxelGrid>>,
    planets: Query<&VoxelOrbitalPlanet>,
    physics_loader: Res<VoxelPhysicsChunkLoader>,
    physics_bodies: Query<(
        &VoxelPhysicsBody,
        &Transform,
        &LinearVelocity,
        &AngularVelocity,
    ), Without<VoxelSpaceship>>,
    placed_lights: Query<(
        &VoxelPlacedLight,
        &Transform,
        Option<&LinearVelocity>,
        Option<&AngularVelocity>,
    )>,
    mut store: ResMut<Persistent<VoxelSceneStore>>,
) {
    persistence.elapsed_seconds += time.delta_secs();
    let exiting = app_exit.read().next().is_some();
    if !exiting
        && !persistence.force_save
        && persistence.elapsed_seconds < VOXEL_SCENE_AUTOSAVE_SECONDS
    {
        return;
    }
    persistence.elapsed_seconds = 0.0;
    persistence.force_save = false;
    let Ok(grid) = grids.single() else {
        return;
    };
    let mut voxels = voxel_cells(grid)
        .into_iter()
        .map(|(cell, material)| persisted_voxel_cell(cell, material))
        .collect::<Vec<_>>();
    voxels.sort_unstable_by_key(|cell| cell.position);
    let planet = planets.single().ok().map(|planet| {
        let mut cells = planet
            .cells
            .iter()
            .map(|(cell, material)| persisted_voxel_cell(*cell, *material))
            .collect::<Vec<_>>();
        cells.sort_unstable_by_key(|cell| cell.position);
        let mut removed = planet
            .removed
            .iter()
            .map(|cell| cell.to_array())
            .collect::<Vec<_>>();
        removed.sort_unstable();
        PersistedVoxelPlanet { cells, removed }
    });
    let mut physics_bodies = physics_bodies
        .iter()
        .map(
            |(body, transform, linear_velocity, angular_velocity)| {
                persisted_voxel_physics_body(
                    body,
                    transform,
                    linear_velocity,
                    angular_velocity,
                )
            },
        )
        .collect::<Vec<_>>();
    physics_bodies.extend(
        physics_loader.unloaded_bodies.iter().map(|snapshot| {
            persisted_voxel_physics_body(
                &snapshot.body,
                &snapshot.transform,
                &snapshot.linear_velocity,
                &snapshot.angular_velocity,
            )
        }),
    );
    physics_bodies.sort_by_key(|body| {
        (
            body.translation.map(f32::to_bits),
            body.rotation.map(f32::to_bits),
            body.cells.first().map_or([0; 3], |cell| cell.position),
            body.cells.len(),
        )
    });
    let mut placed_lights = placed_lights
        .iter()
        .map(
            |(light, transform, linear_velocity, angular_velocity)| {
                persisted_voxel_light(
                    light,
                    transform,
                    linear_velocity,
                    angular_velocity,
                )
            },
        )
        .collect::<Vec<_>>();
    placed_lights.sort_unstable_by_key(|light| light.cell);
    let snapshot = VoxelSceneStore {
        scene: Some(PersistedVoxelScene {
            voxels,
            planet,
            physics_bodies,
            placed_lights,
        }),
        layout_revision: VOXEL_SCENE_LAYOUT_REVISION,
    };
    if **store == snapshot {
        return;
    }
    **store = snapshot;
    if let Err(err) = store.persist() {
        eprintln!("failed to persist voxel scene: {err}");
    }
}

fn voxel_editor_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    spaceship_control: Option<Res<VoxelSpaceshipControlState>>,
    mut editor: ResMut<VoxelEditorState>,
    mut possession: ResMut<VoxelPossessionState>,
) {
    if egui_input.wants_any_keyboard_input() {
        return;
    }
    let driving_spaceship = spaceship_control
        .as_deref()
        .is_some_and(|control| control.driving_ship_id.is_some());
    if keyboard.just_pressed(KeyCode::KeyE) && !driving_spaceship {
        editor.teleport_menu_open = false;
        if possession.active_user_id.is_some() {
            possession.player_inventory_open = !possession.player_inventory_open;
            editor.creative_inventory_open = false;
        } else {
            editor.creative_inventory_open = !editor.creative_inventory_open;
        }
    }
    if keyboard.just_pressed(KeyCode::KeyR)
        && possession.active_user_id.is_none()
        && !editor.creative_inventory_open
        && editor.is_tool_gun_equipped()
    {
        editor.cycle_tool_gun_mode();
    }
    for (key, slot) in [
        (KeyCode::Digit1, 0),
        (KeyCode::Digit2, 1),
        (KeyCode::Digit3, 2),
        (KeyCode::Digit4, 3),
        (KeyCode::Digit5, 4),
        (KeyCode::Digit6, 5),
        (KeyCode::Digit7, 6),
        (KeyCode::Digit8, 7),
        (KeyCode::Digit9, 8),
        (KeyCode::Digit0, 9),
    ] {
        if keyboard.just_pressed(key) {
            if let Some(active_user_id) = possession.active_user_id {
                if slot >= 9 {
                    continue;
                }
                let character = manager
                    .as_deref()
                    .and_then(|manager| manager.player_characters.get(&active_user_id.to_string()));
                activate_player_hotbar_slot(&mut possession, character, slot);
            } else {
                editor.select_hotbar_slot(slot);
            }
        }
    }
    if possession.active_user_id.is_some() {
        return;
    }
    if !keyboard.just_pressed(KeyCode::KeyZ) {
        return;
    }
    let control = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !control {
        return;
    }
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if shift {
        editor.redo_requested = true;
    } else {
        editor.undo_requested = true;
    }
}

fn activate_player_hotbar_slot(
    possession: &mut VoxelPossessionState,
    character: Option<&PlayerCharacter>,
    slot: usize,
) {
    if slot >= 9 {
        return;
    }
    possession.selected_hotbar_slot = slot;
    let releases_control = character.and_then(|character| character.inventory.hotbar.get(slot))
        == Some(&CharacterHotbarSlot::ReleaseControl);
    if releases_control {
        possession.release();
    }
}

fn voxel_glass_material() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgba(0.48, 0.92, 1.0, VOXEL_GLASS_OPACITY),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.08,
        reflectance: 0.8,
        cull_mode: None,
        ..default()
    }
}

fn setup_voxel_materials(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let paths = [
        "textures/voxel_grass.png",
        "textures/voxel_dirt.png",
        "textures/voxel_sand.png",
        "textures/voxel_water.png",
        "textures/voxel_lava.png",
    ];
    let textures = paths.map(|path| {
        asset_server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    mag_filter: ImageFilterMode::Nearest,
                    min_filter: ImageFilterMode::Nearest,
                    mipmap_filter: ImageFilterMode::Nearest,
                    ..default()
                });
            })
            .load(path)
    });
    let hifi_texture = asset_server
        .load_builder()
        .with_settings(|settings: &mut ImageLoaderSettings| {
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::ClampToEdge,
                address_mode_v: ImageAddressMode::ClampToEdge,
                mag_filter: ImageFilterMode::Nearest,
                min_filter: ImageFilterMode::Nearest,
                mipmap_filter: ImageFilterMode::Nearest,
                ..default()
            });
        })
        .load("textures/voxel_space_hifi.png");
    let mut fade_handles = std::array::from_fn(|_| Handle::default());
    let handles = std::array::from_fn(|index| {
        let mut material = if index < textures.len() {
            StandardMaterial {
                base_color_texture: Some(textures[index].clone()),
                base_color: [
                    Color::srgb(0.2, 0.6, 0.3),
                    Color::srgb(0.35, 0.18, 0.08),
                    Color::srgb(0.85, 0.72, 0.4),
                    Color::srgb(0.1, 0.4, 0.85),
                    Color::srgb(1.0, 0.15, 0.01),
                ][index],
                perceptual_roughness: 0.9,
                ..default()
            }
        } else if index < 8 {
            let atlas_row = [2, 0, 6][index - textures.len()];
            StandardMaterial {
                base_color_texture: Some(hifi_texture.clone()),
                base_color: Color::WHITE,
                uv_transform: hifi_voxel_tile_transform(atlas_row),
                metallic: 0.72,
                perceptual_roughness: 0.34,
                ..default()
            }
        } else if index == 8 {
            StandardMaterial {
                base_color: Color::srgb(0.28, 0.008, 0.014),
                metallic: 0.72,
                perceptual_roughness: 0.44,
                ..default()
            }
        } else if index == 9 {
            StandardMaterial {
                base_color: Color::srgb(0.46, 0.12, 0.018),
                emissive: voxel_emissive(0.42, 0.075, 0.008),
                metallic: 0.62,
                perceptual_roughness: 0.32,
                ..default()
            }
        } else {
            voxel_glass_material()
        };
        match index {
            0 => {
                material.emissive = voxel_emissive(0.04, 0.1, 0.045);
            },
            1 => {
                material.emissive = voxel_emissive(0.045, 0.022, 0.01);
            },
            2 => {
                material.emissive = voxel_emissive(0.11, 0.085, 0.035);
            },
            3 => {
                material.base_color = Color::srgba(0.72, 0.9, 1.0, 0.72);
                material.alpha_mode = AlphaMode::Blend;
                material.perceptual_roughness = 0.18;
                material.reflectance = 0.65;
                material.emissive = voxel_emissive(0.025, 0.08, 0.16);
            },
            4 => {
                material.emissive_texture = Some(textures[index].clone());
                material.emissive = voxel_emissive(5.0, 0.55, 0.02);
                material.perceptual_roughness = 0.55;
            },
            5 => {
                material.emissive = voxel_emissive(0.11, 0.14, 0.19);
                material.perceptual_roughness = 0.4;
            },
            6 => {
                material.emissive = voxel_emissive(0.055, 0.07, 0.09);
                material.perceptual_roughness = 0.48;
            },
            7 => {
                material.base_color = Color::srgb(0.48, 0.92, 1.0);
                material.emissive_texture = Some(hifi_texture.clone());
                material.emissive = voxel_emissive(0.2, 3.2, 4.4);
                material.metallic = 0.25;
            },
            8 => {
                material.emissive = voxel_emissive(0.085, 0.002, 0.004);
            },
            _ => {},
        }
        let mut fade_material = material.clone();
        fade_material.base_color = fade_material.base_color.with_alpha(0.0);
        fade_material.alpha_mode = AlphaMode::Blend;
        fade_handles[index] = materials.add(fade_material);
        materials.add(material)
    });
    let planet_ocean_material = opaque_planet_ocean_material(textures[3].clone());
    let mut fade_planet_ocean_material = planet_ocean_material.clone();
    fade_planet_ocean_material.base_color =
        fade_planet_ocean_material.base_color.with_alpha(0.0);
    fade_planet_ocean_material.alpha_mode = AlphaMode::Blend;
    let fade_planet_ocean = materials.add(fade_planet_ocean_material);
    let planet_ocean = materials.add(planet_ocean_material);
    commands.insert_resource(VoxelMaterials {
        handles,
        planet_ocean,
    });
    commands.insert_resource(VoxelReplayFadeMaterials {
        handles: fade_handles,
        planet_ocean: fade_planet_ocean,
    });
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.48, 0.56, 0.68),
        brightness: DEFAULT_AMBIENT_BRIGHTNESS,
        ..default()
    });
}

fn opaque_planet_ocean_material(texture: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(texture),
        base_color: Color::srgb(0.08, 0.38, 0.72),
        alpha_mode: AlphaMode::Opaque,
        perceptual_roughness: 0.32,
        reflectance: 0.55,
        emissive: voxel_emissive(0.025, 0.08, 0.16),
        ..default()
    }
}

fn hifi_voxel_tile_transform(row: usize) -> Affine2 {
    const ATLAS_WIDTH: f32 = 16.0;
    const ATLAS_HEIGHT: f32 = 192.0;
    const TILE_SIZE: f32 = 16.0;
    Affine2::from_scale_angle_translation(
        Vec2::new(
            (TILE_SIZE - 1.0) / ATLAS_WIDTH,
            (TILE_SIZE - 1.0) / ATLAS_HEIGHT,
        ),
        0.0,
        Vec2::new(
            0.5 / ATLAS_WIDTH,
            (row as f32 * TILE_SIZE + 0.5) / ATLAS_HEIGHT,
        ),
    )
}

fn radiance_voxel_color(material: u8) -> [u8; 4] {
    match material {
        // Alpha stores occupancy. RGB seeds the colored block-light volume.
        5 => [255, 72, 8, 255],
        8 => [34, 176, 220, 255],
        10 => [196, 78, 18, 255],
        VOXEL_GLASS_MATERIAL => [0, 0, 0, 0],
        _ if TrpgVoxelConnector::solid(&material) => [0, 0, 0, 255],
        _ => [0, 0, 0, 0],
    }
}

fn voxel_radiance_volume_origin(focus: Vec3) -> IVec3 {
    let focus = if focus.is_finite() { focus } else { Vec3::ZERO };
    let focus_cell = (focus / VOXEL_SIZE).floor().as_ivec3();
    let snapped_focus = IVec3::new(
        focus_cell.x.div_euclid(VOXEL_RADIANCE_REBUILD_STEP) * VOXEL_RADIANCE_REBUILD_STEP,
        focus_cell.y.div_euclid(VOXEL_RADIANCE_REBUILD_STEP) * VOXEL_RADIANCE_REBUILD_STEP,
        focus_cell.z.div_euclid(VOXEL_RADIANCE_REBUILD_STEP) * VOXEL_RADIANCE_REBUILD_STEP,
    );
    snapped_focus - IVec3::splat(VOXEL_RADIANCE_VOLUME_DIMENSION / 2)
}

fn voxel_radiance_index(local: IVec3) -> usize {
    let dimension = VOXEL_RADIANCE_VOLUME_DIMENSION;
    (local.x + dimension * (local.y + dimension * local.z)) as usize
}

fn propagate_voxel_radiance(data: &mut [u8], queue: &mut VecDeque<usize>) {
    let dimension = VOXEL_RADIANCE_VOLUME_DIMENSION as usize;
    let layer_len = dimension * dimension;
    const NEIGHBORS: [IVec3; 6] = [
        IVec3::X,
        IVec3::NEG_X,
        IVec3::Y,
        IVec3::NEG_Y,
        IVec3::Z,
        IVec3::NEG_Z,
    ];

    while let Some(index) = queue.pop_front() {
        let byte_index = index * 4;
        let propagated = [
            (data[byte_index] as u16 * VOXEL_RADIANCE_TRANSMISSION / 255) as u8,
            (data[byte_index + 1] as u16 * VOXEL_RADIANCE_TRANSMISSION / 255) as u8,
            (data[byte_index + 2] as u16 * VOXEL_RADIANCE_TRANSMISSION / 255) as u8,
        ];
        if propagated.iter().copied().max().unwrap_or_default() <= VOXEL_RADIANCE_CUTOFF {
            continue;
        }

        let local = IVec3::new(
            (index % dimension) as i32,
            ((index / dimension) % dimension) as i32,
            (index / layer_len) as i32,
        );
        for offset in NEIGHBORS {
            let neighbor = local + offset;
            if neighbor.min_element() < 0
                || neighbor.max_element() >= VOXEL_RADIANCE_VOLUME_DIMENSION
            {
                continue;
            }
            let neighbor_index = voxel_radiance_index(neighbor);
            let neighbor_byte = neighbor_index * 4;
            if data[neighbor_byte + 3] != 0 {
                continue;
            }
            let mut changed = false;
            for channel in 0..3 {
                if propagated[channel] > data[neighbor_byte + channel] {
                    data[neighbor_byte + channel] = propagated[channel];
                    changed = true;
                }
            }
            if changed {
                queue.push_back(neighbor_index);
            }
        }
    }
}

fn build_voxel_radiance_image(grid: &Grid<u8>, focus: Vec3) -> (Image, Vec3, f32, Vec3) {
    let origin = voxel_radiance_volume_origin(focus);
    let dimensions = IVec3::splat(VOXEL_RADIANCE_VOLUME_DIMENSION);
    let texel_count = VOXEL_RADIANCE_VOLUME_DIMENSION as usize
        * VOXEL_RADIANCE_VOLUME_DIMENSION as usize
        * VOXEL_RADIANCE_VOLUME_DIMENSION as usize;
    let mut data = vec![0; texel_count * 4];
    let mut queue = VecDeque::new();

    for (chunk_position, chunk) in grid.iter() {
        for chunk_local in prism(IVec3::ZERO, DIMS) {
            let material = chunk[chunk_local];
            if material == 0 {
                continue;
            }
            let local = *chunk_position * DIMS + chunk_local - origin;
            if local.min_element() < 0 || local.max_element() >= VOXEL_RADIANCE_VOLUME_DIMENSION {
                continue;
            }
            let color = radiance_voxel_color(material);
            let index = voxel_radiance_index(local);
            let byte_index = index * 4;
            data[byte_index..byte_index + 4].copy_from_slice(&color);
            if color[..3].iter().any(|channel| *channel != 0) {
                queue.push_back(index);
            }
        }
    }

    // Minecraft keeps a separate skylight channel: full-strength light travels
    // down open columns, while a roof blocks it. The shared flood fill below
    // then carries that light sideways through windows and doorways with falloff.
    for z in 0..VOXEL_RADIANCE_VOLUME_DIMENSION {
        for x in 0..VOXEL_RADIANCE_VOLUME_DIMENSION {
            let mut sky_visible = true;
            for y in (0..VOXEL_RADIANCE_VOLUME_DIMENSION).rev() {
                let local = IVec3::new(x, y, z);
                let index = voxel_radiance_index(local);
                let byte_index = index * 4;
                if data[byte_index + 3] != 0 {
                    sky_visible = false;
                } else if sky_visible {
                    data[byte_index..byte_index + 3].copy_from_slice(&VOXEL_RADIANCE_SKYLIGHT);
                    queue.push_back(index);
                }
            }
        }
    }
    propagate_voxel_radiance(&mut data, &mut queue);

    let size = Extent3d {
        width: VOXEL_RADIANCE_VOLUME_DIMENSION as u32,
        height: VOXEL_RADIANCE_VOLUME_DIMENSION as u32,
        depth_or_array_layers: VOXEL_RADIANCE_VOLUME_DIMENSION as u32,
    };
    let mut image = Image::new(
        size,
        TextureDimension::D3,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    (
        image,
        origin.as_vec3() * VOXEL_SIZE,
        VOXEL_SIZE,
        dimensions.as_vec3(),
    )
}

fn setup_voxel_radiance_volume(
    grids: Query<&Grid<u8>, With<TrpgVoxelGrid>>,
    editor: Res<VoxelEditorState>,
    mut images: ResMut<Assets<Image>>,
    mut volume: ResMut<VoxelRadianceVolume>,
) {
    let Ok(grid) = grids.single() else {
        return;
    };
    let (image, volume_min, voxel_world_size, volume_dimensions) =
        build_voxel_radiance_image(grid, editor.camera_focus);
    volume.image = images.add(image);
    volume.volume_min = volume_min;
    volume.voxel_world_size = voxel_world_size;
    volume.volume_dimensions = volume_dimensions;
}

fn sync_voxel_radiance_volume(
    grids: Query<Ref<Grid<u8>>, With<TrpgVoxelGrid>>,
    players: Query<&Transform, With<VoxelFirstPersonPlayer>>,
    editor: Res<VoxelEditorState>,
    mut images: ResMut<Assets<Image>>,
    mut volume: ResMut<VoxelRadianceVolume>,
    mut cameras: Query<
        (
            &mut VoxelRadianceCascade,
            &mut VoxelRadianceCascadeUniform,
        ),
        With<VoxelViewportCamera>,
    >,
) {
    let Ok(grid) = grids.single() else {
        return;
    };
    let focus = if editor.first_person_enabled {
        players
            .single()
            .map(|transform| transform.translation)
            .unwrap_or(editor.camera_focus)
    } else {
        editor.camera_focus
    };
    let desired_min = voxel_radiance_volume_origin(focus).as_vec3() * VOXEL_SIZE;
    if !grid.is_changed() && desired_min == volume.volume_min && images.contains(&volume.image) {
        return;
    }

    let (image, volume_min, voxel_world_size, volume_dimensions) =
        build_voxel_radiance_image(&grid, focus);
    if images.contains(&volume.image) {
        *images.get_mut(&volume.image).unwrap() = image;
    } else {
        volume.image = images.add(image);
    }
    volume.volume_min = volume_min;
    volume.voxel_world_size = voxel_world_size;
    volume.volume_dimensions = volume_dimensions;
    let uniform = volume.uniform(editor.radiance_intensity);
    for (mut cascade, mut camera_uniform) in &mut cameras {
        cascade.volume = volume.image.clone();
        *camera_uniform = uniform;
    }
}

fn sync_voxel_lighting(
    editor: Res<VoxelEditorState>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut lights: Query<(
        &mut DirectionalLight,
        Option<&VoxelKeyLight>,
        Option<&VoxelFillLight>,
    )>,
    mut cameras: Query<&mut VoxelRadianceCascadeUniform, With<VoxelViewportCamera>>,
) {
    if !editor.is_changed() {
        return;
    }
    ambient.brightness = editor.ambient_brightness.max(0.0);
    for (mut light, key, fill) in &mut lights {
        if key.is_some() {
            light.illuminance = editor.key_light_illuminance.max(0.0);
            light.color = Color::srgb(
                editor.key_light_color[0],
                editor.key_light_color[1],
                editor.key_light_color[2],
            );
        } else if fill.is_some() {
            light.illuminance = editor.fill_light_illuminance.max(0.0);
            light.color = Color::srgb(
                editor.fill_light_color[0],
                editor.fill_light_color[1],
                editor.fill_light_color[2],
            );
        }
    }
    for mut uniform in &mut cameras {
        uniform.intensity = editor.radiance_intensity.max(0.0);
    }
}

fn setup_voxel_grid(mut commands: Commands) {
    commands.spawn((
        TrpgVoxelGrid,
        Grid::<u8>::new(),
        BodyTracker::<TrpgVoxelConnector>::new(),
        Boundary,
    ));
}

fn populate_voxel_grid(mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>) {
    let Ok(mut grid) = grids.single_mut() else {
        return;
    };
    populate_default_grid(&mut grid);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpaceStationDockEdge {
    West,
    East,
    North,
    South,
}

impl SpaceStationDockEdge {
    fn direction(self) -> IVec3 {
        match self {
            Self::West => IVec3::NEG_X,
            Self::East => IVec3::X,
            Self::North => IVec3::NEG_Z,
            Self::South => IVec3::Z,
        }
    }

    fn tangent(self) -> IVec3 {
        match self {
            Self::West | Self::East => IVec3::Z,
            Self::North | Self::South => IVec3::X,
        }
    }

    fn sheet_offset(self) -> (i32, i32) {
        match self {
            Self::West => (-1, 0),
            Self::East => (1, 0),
            Self::North => (0, -1),
            Self::South => (0, 1),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpaceStationDockSpec {
    edge: SpaceStationDockEdge,
    along: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpaceStationDockingPort {
    edge: SpaceStationDockEdge,
    entrance: IVec3,
    pad_center: IVec3,
}

const NIFFY_DOCKS: [SpaceStationDockSpec; 3] = [
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::West,
        along: -22,
    },
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::East,
        along: 21,
    },
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::South,
        along: 5,
    },
];
const ARBITRATOR_DOCKS: [SpaceStationDockSpec; 3] = [
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::North,
        along: -47,
    },
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::East,
        along: 2,
    },
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::South,
        along: 0,
    },
];
const KYO_DOCKS: [SpaceStationDockSpec; 3] = [
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::West,
        along: 19,
    },
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::North,
        along: 43,
    },
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::South,
        along: -42,
    },
];
const ABANDONED_DOCKS: [SpaceStationDockSpec; 3] = [
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::West,
        along: -31,
    },
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::East,
        along: 32,
    },
    SpaceStationDockSpec {
        edge: SpaceStationDockEdge::North,
        along: 3,
    },
];

fn space_station_docking_specs(design: WorkbookMapDesign) -> &'static [SpaceStationDockSpec] {
    match design.name {
        "U.S.I Niffy女皇号空间站" => &NIFFY_DOCKS,
        "U.S.I 女仲裁者号空间站" => &ARBITRATOR_DOCKS,
        "U.S.I Kyo空间站" => &KYO_DOCKS,
        "废弃太空站" => &ABANDONED_DOCKS,
        _ => &[],
    }
}

fn workbook_column_occupied(decoded: &DecodedWorkbookMap, index: usize) -> bool {
    decoded.enclosed[index] || matches!(decoded.styles[index], 11 | 15)
}

fn space_station_docking_ports(design: WorkbookMapDesign) -> Vec<SpaceStationDockingPort> {
    let decoded = design.decode();
    let ports = space_station_docking_specs(design)
        .iter()
        .filter_map(|spec| {
            let direction = spec.edge.direction();
            let tangent = spec.edge.tangent();
            let (sheet_dx, sheet_dz) = spec.edge.sheet_offset();
            let entrance = decoded
                .styles
                .iter()
                .enumerate()
                .filter_map(|(index, style)| {
                    if !matches!(*style, 11 | 15) {
                        return None;
                    }
                    let sheet_x = (index % design.width) as i32;
                    let sheet_z = (index / design.width) as i32;
                    if !workbook_cell_is_exterior(
                        design,
                        &decoded,
                        sheet_x + sheet_dx,
                        sheet_z + sheet_dz,
                    ) {
                        return None;
                    }
                    let [x, z] = design.centered_offset(index);
                    let cell = IVec3::new(x, 0, z);
                    let along_distance = (cell.dot(tangent) - spec.along).abs();
                    Some((
                        (along_distance, -cell.dot(direction)),
                        cell,
                    ))
                })
                .min_by_key(|(key, _)| *key)
                .map(|(_, cell)| cell)?;
            let entrance_along = entrance.dot(tangent);
            let outermost = decoded
                .styles
                .iter()
                .enumerate()
                .filter_map(|(index, _)| {
                    if !workbook_column_occupied(&decoded, index) {
                        return None;
                    }
                    let [x, z] = design.centered_offset(index);
                    let cell = IVec3::new(x, 0, z);
                    ((cell.dot(tangent) - entrance_along).abs() <= STATION_DOCK_PAD_HALF_WIDTH + 2)
                        .then_some(cell.dot(direction))
                })
                .max()
                .unwrap_or_else(|| entrance.dot(direction));
            let pad_center = entrance
                + direction
                    * (outermost - entrance.dot(direction)
                        + STATION_DOCK_HULL_GAP
                        + STATION_DOCK_PAD_HALF_LENGTH);
            Some(SpaceStationDockingPort {
                edge: spec.edge,
                entrance,
                pad_center,
            })
        })
        .collect::<Vec<_>>();
    for (index, port) in ports.iter().enumerate() {
        for other in &ports[index + 1..] {
            debug_assert!(
                (port.pad_center - other.pad_center).length_squared()
                    >= STATION_DOCK_MIN_SEPARATION.pow(2),
                "{} docking pads at {:?} and {:?} are too close",
                design.name,
                port.pad_center,
                other.pad_center
            );
        }
    }
    ports
}

fn add_space_station_docking_areas(
    grid: &mut Mut<Grid<u8>>,
    center: IVec3,
    design: WorkbookMapDesign,
) {
    for port in space_station_docking_ports(design) {
        let direction = port.edge.direction();
        let tangent = port.edge.tangent();
        let pad_inner_step =
            (port.pad_center - port.entrance).dot(direction) - STATION_DOCK_PAD_HALF_LENGTH;

        // Cut only a narrow personnel entrance through the existing hull and
        // connect it to the pad. The landing surface itself stays roofless and
        // wall-free so ships can approach from open space.
        for inward_step in -1..=1 {
            for lateral in -STATION_DOCK_BRIDGE_HALF_WIDTH..=STATION_DOCK_BRIDGE_HALF_WIDTH {
                let column = center + port.entrance + direction * inward_step + tangent * lateral;
                grid.set(column, 7);
                for y in 1..=WORKBOOK_ROOM_HEIGHT - 2 {
                    grid.set(column + IVec3::Y * y, 0);
                }
            }
        }
        for step in 0..pad_inner_step {
            for lateral in -STATION_DOCK_BRIDGE_HALF_WIDTH..=STATION_DOCK_BRIDGE_HALF_WIDTH {
                let column = center + port.entrance + direction * step + tangent * lateral;
                grid.set(
                    column,
                    if lateral == 0 { 10 } else { 7 },
                );
                for y in 1..=STATION_DOCK_CLEAR_HEIGHT {
                    grid.set(column + IVec3::Y * y, 0);
                }
            }
        }

        for radial in -STATION_DOCK_PAD_HALF_LENGTH..=STATION_DOCK_PAD_HALF_LENGTH {
            for lateral in -STATION_DOCK_PAD_HALF_WIDTH..=STATION_DOCK_PAD_HALF_WIDTH {
                let column = center + port.pad_center + direction * radial + tangent * lateral;
                let material = if radial.abs() == STATION_DOCK_PAD_HALF_LENGTH
                    || lateral.abs() == STATION_DOCK_PAD_HALF_WIDTH
                {
                    7
                } else if lateral.abs() <= 1 {
                    10
                } else if radial == 0 {
                    9
                } else {
                    6
                };
                grid.set(column, material);
                for y in 1..=STATION_DOCK_CLEAR_HEIGHT {
                    grid.set(column + IVec3::Y * y, 0);
                }
            }
        }
    }
}


fn populate_default_grid(grid: &mut Mut<Grid<u8>>) {
    for (center, design) in static_workbook_orbital_locations() {
        build_workbook_orbital_location(grid, center, design);
        add_space_station_docking_areas(grid, center, design);
    }
    for door in static_voxel_auto_doors() {
        for cell in door.cells {
            grid.set(cell, 0);
        }
    }
}

#[cfg(test)]
fn workbook_orbital_locations() -> [(IVec3, WorkbookMapDesign); 5] {
    [
        (RESEARCH_STATION_CENTER, NIFFY),
        (SENSOR_STATION_CENTER, ARBITRATOR),
        (CANNON_STATION_CENTER, KYO),
        (COMBAT_SPACESHIP_CENTER, ARROGANCE),
        (ABANDONED_STATION_CENTER, ABANDONED),
    ]
}

fn static_workbook_orbital_locations() -> [(IVec3, WorkbookMapDesign); 4] {
    [
        (RESEARCH_STATION_CENTER, NIFFY),
        (SENSOR_STATION_CENTER, ARBITRATOR),
        (CANNON_STATION_CENTER, KYO),
        (ABANDONED_STATION_CENTER, ABANDONED),
    ]
}

fn static_workbook_micro_decorations() -> VoxelMicroDecorations {
    let mut tiles = Vec::new();
    for (center, design) in static_workbook_orbital_locations() {
        let decoded = design.decode();
        tiles.extend(
            workbook_micro_tiles(design, &decoded)
                .into_iter()
                .map(|mut tile| {
                    tile.owner += center;
                    tile.cell += center;
                    tile
                }),
        );
    }
    VoxelMicroDecorations { tiles }
}

fn static_workbook_feature_annotations() -> StaticWorkbookFeatureAnnotations {
    let mut entries = Vec::new();
    for (center, design) in static_workbook_orbital_locations() {
        let decoded = design.decode();
        entries.extend(
            workbook_feature_regions(design, &decoded)
                .into_iter()
                .map(|region| StaticWorkbookFeatureAnnotation {
                    map_name: design.name,
                    center,
                    region,
                }),
        );
    }
    StaticWorkbookFeatureAnnotations { entries }
}

fn workbook_fixture(style: u8) -> Option<(u8, i32)> {
    Some(match style {
        13 => (10, 3),
        26 => (4, 1),
        33 => (6, WORKBOOK_ROOM_HEIGHT - 1),
        34 => (7, WORKBOOK_ROOM_HEIGHT - 1),
        35 | 36 => (6, WORKBOOK_ROOM_HEIGHT - 1),
        37 => (9, 4),
        38 => (5, 3),
        _ => return None,
    })
}

fn workbook_planet_fixture(style: u8) -> Option<(u8, i32)> {
    workbook_fixture(style).or_else(|| {
        Some(match style {
            16 => (9, 2),
            17 | 18 => (3, 2),
            25 => (8, 2),
            27 => (8, 1),
            32 => (1, 1),
            _ => return None,
        })
    })
}

fn workbook_exterior_fixture(style: u8) -> bool {
    matches!(style, 13 | 33..=38)
}

fn build_workbook_orbital_location(
    grid: &mut Mut<Grid<u8>>,
    center: IVec3,
    design: WorkbookMapDesign,
) {
    let decoded = design.decode();
    for (index, style) in decoded.styles.iter().copied().enumerate() {
        let [x, z] = design.centered_offset(index);
        let base = center + IVec3::new(x, 0, z);
        let wall = style == 11;
        let door = style == 15;
        let enclosed = decoded.enclosed[index];

        if wall || door || enclosed {
            grid.set(base, if style == 12 { 7 } else { 2 });
            grid.set(base + IVec3::Y * WORKBOOK_ROOM_HEIGHT, 6);
        }
        if wall {
            for y in 1..WORKBOOK_ROOM_HEIGHT {
                let material = if workbook_station_wall_is_glass(design, &decoded, index, y)
                {
                    VOXEL_GLASS_MATERIAL
                } else {
                    6
                };
                grid.set(base + IVec3::Y * y, material);
            }
            continue;
        }

        let Some((material, height)) = workbook_fixture(style) else {
            continue;
        };
        if !enclosed && !workbook_exterior_fixture(style) {
            continue;
        }
        grid.set(base, if enclosed { 2 } else { material });
        for y in 1..=height {
            grid.set(base + IVec3::Y * y, material);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkbookHullStyle {
    ResearchStation,
    SensorStation,
    CannonStation,
    CombatCruiser,
    AbandonedStation,
}

#[derive(Clone, Copy)]
struct WorkbookHullPalette {
    hull: u8,
    trim: u8,
    armor: u8,
    window: u8,
    rib_spacing: i32,
}

fn workbook_hull_style(design: WorkbookMapDesign) -> WorkbookHullStyle {
    match design.name {
        "U.S.I Niffy女皇号空间站" => WorkbookHullStyle::ResearchStation,
        "U.S.I 女仲裁者号空间站" => WorkbookHullStyle::SensorStation,
        "U.S.I Kyo空间站" => WorkbookHullStyle::CannonStation,
        "U.S.I 狂妄号" => WorkbookHullStyle::CombatCruiser,
        "废弃太空站" => WorkbookHullStyle::AbandonedStation,
        name => panic!("missing workbook hull style for {name}"),
    }
}

fn workbook_hull_palette(style: WorkbookHullStyle) -> WorkbookHullPalette {
    match style {
        WorkbookHullStyle::ResearchStation => WorkbookHullPalette {
            hull: 6,
            trim: 7,
            armor: 10,
            window: 8,
            rib_spacing: 12,
        },
        WorkbookHullStyle::SensorStation => WorkbookHullPalette {
            hull: 6,
            trim: 7,
            armor: 10,
            window: 8,
            rib_spacing: 10,
        },
        WorkbookHullStyle::CannonStation => WorkbookHullPalette {
            hull: 6,
            trim: 9,
            armor: 7,
            window: 8,
            rib_spacing: 9,
        },
        WorkbookHullStyle::CombatCruiser => WorkbookHullPalette {
            hull: 6,
            trim: 7,
            armor: 9,
            window: 8,
            rib_spacing: 11,
        },
        WorkbookHullStyle::AbandonedStation => WorkbookHullPalette {
            hull: 7,
            trim: 6,
            armor: 9,
            window: 10,
            rib_spacing: 14,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum WorkbookFeatureKind {
    ControlConsole = 1,
    EnergyPlatform = 2,
    SupplyRack = 3,
    ArmorLocker = 4,
    CargoRack = 5,
    OutpostTerminal = 6,
    EscapePod = 7,
    ThermiteFactory = 8,
    Teleporter = 9,
    MedicalAnalyzer = 10,
    Furniture = 11,
    ExperimentBench = 12,
    Crate = 13,
    RadarConsole = 14,
    InstrumentPanel = 15,
}

impl WorkbookFeatureKind {
    fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            1 => Self::ControlConsole,
            2 => Self::EnergyPlatform,
            3 => Self::SupplyRack,
            4 => Self::ArmorLocker,
            5 => Self::CargoRack,
            6 => Self::OutpostTerminal,
            7 => Self::EscapePod,
            8 => Self::ThermiteFactory,
            9 => Self::Teleporter,
            10 => Self::MedicalAnalyzer,
            11 => Self::Furniture,
            12 => Self::ExperimentBench,
            13 => Self::Crate,
            14 => Self::RadarConsole,
            15 => Self::InstrumentPanel,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Self::ControlConsole => "控制台",
            Self::EnergyPlatform => "能量台",
            Self::SupplyRack => "补给品",
            Self::ArmorLocker => "防护服",
            Self::CargoRack => "集装货物",
            Self::OutpostTerminal => "前哨站",
            Self::EscapePod => "逃生舱",
            Self::ThermiteFactory => "热熔炸弹工厂",
            Self::Teleporter => "传送门",
            Self::MedicalAnalyzer => "验血 / GIT",
            Self::Furniture => "桌椅",
            Self::ExperimentBench => "实验品",
            Self::Crate => "木箱",
            Self::RadarConsole => "雷达控制台",
            Self::InstrumentPanel => "仪表",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkbookFeatureRegion {
    kind: WorkbookFeatureKind,
    /// Canonical floor-owner cells in workbook-local coordinates.
    cells: Vec<IVec3>,
    /// One real member cell nearest the region centroid, used by the map label.
    anchor: IVec3,
}

#[derive(Clone, Debug)]
struct StaticWorkbookFeatureAnnotation {
    map_name: &'static str,
    center: IVec3,
    region: WorkbookFeatureRegion,
}

#[derive(Resource, Clone, Debug, Default)]
struct StaticWorkbookFeatureAnnotations {
    entries: Vec<StaticWorkbookFeatureAnnotation>,
}

#[derive(Component, Clone, Debug)]
struct VoxelWorkbookFeatureAnnotations {
    map_name: &'static str,
    regions: Vec<WorkbookFeatureRegion>,
}

fn push_micro_box(
    tiles: &mut Vec<VoxelMicroTile>,
    owner: IVec3,
    cell: IVec3,
    min: [u32; 3],
    max: [u32; 3],
    material: u8,
    kind: VoxelMicroTileKind,
) {
    let min = UVec3::from_array(min);
    let max = UVec3::from_array(max);
    debug_assert!(min.cmplt(max).all());
    debug_assert!(max.cmpge(UVec3::splat(1)).all());
    debug_assert!(max.cmple(UVec3::splat(MICRO_TILE_SUBDIVISIONS)).all());
    debug_assert!((1..=VOXEL_MATERIAL_COUNT as u8).contains(&material));
    tiles.push(VoxelMicroTile {
        owner,
        cell,
        min,
        max,
        material,
        kind,
    });
}

fn push_fixture_box(
    tiles: &mut Vec<VoxelMicroTile>,
    owner: IVec3,
    height_cell: i32,
    min: [u32; 3],
    max: [u32; 3],
    material: u8,
) {
    push_micro_box(
        tiles,
        owner,
        owner + IVec3::Y * height_cell,
        min,
        max,
        material,
        VoxelMicroTileKind::Fixture,
    );
}

fn workbook_feature_floor_material(kind: WorkbookFeatureKind) -> u8 {
    match kind {
        WorkbookFeatureKind::ControlConsole => 7,
        WorkbookFeatureKind::EnergyPlatform => 8,
        WorkbookFeatureKind::SupplyRack => 3,
        WorkbookFeatureKind::ArmorLocker => 6,
        WorkbookFeatureKind::CargoRack => 3,
        WorkbookFeatureKind::OutpostTerminal => 9,
        WorkbookFeatureKind::EscapePod => 7,
        WorkbookFeatureKind::ThermiteFactory => 5,
        WorkbookFeatureKind::Teleporter => 8,
        WorkbookFeatureKind::MedicalAnalyzer => 3,
        WorkbookFeatureKind::Furniture => 3,
        WorkbookFeatureKind::ExperimentBench => 1,
        WorkbookFeatureKind::Crate => 3,
        WorkbookFeatureKind::RadarConsole => 8,
        WorkbookFeatureKind::InstrumentPanel => 9,
    }
}

fn workbook_feature_fixture_spacing(kind: WorkbookFeatureKind) -> usize {
    match kind {
        WorkbookFeatureKind::EnergyPlatform
        | WorkbookFeatureKind::Teleporter => 4,
        WorkbookFeatureKind::ControlConsole
        | WorkbookFeatureKind::OutpostTerminal
        | WorkbookFeatureKind::ThermiteFactory
        | WorkbookFeatureKind::MedicalAnalyzer
        | WorkbookFeatureKind::Furniture
        | WorkbookFeatureKind::RadarConsole
        | WorkbookFeatureKind::InstrumentPanel => 3,
        WorkbookFeatureKind::SupplyRack
        | WorkbookFeatureKind::ArmorLocker
        | WorkbookFeatureKind::CargoRack
        | WorkbookFeatureKind::EscapePod
        | WorkbookFeatureKind::ExperimentBench
        | WorkbookFeatureKind::Crate => 2,
    }
}

fn add_workbook_feature_floor_micro_tile(
    tiles: &mut Vec<VoxelMicroTile>,
    owner: IVec3,
    kind: WorkbookFeatureKind,
) {
    push_fixture_box(
        tiles,
        owner,
        1,
        [1, 0, 1],
        [15, 1, 15],
        workbook_feature_floor_material(kind),
    );
}

fn add_workbook_fixture_micro_tiles(
    tiles: &mut Vec<VoxelMicroTile>,
    owner: IVec3,
    kind: WorkbookFeatureKind,
) {
    match kind {
        WorkbookFeatureKind::ControlConsole => {
            push_fixture_box(tiles, owner, 1, [1, 0, 2], [15, 3, 14], 7);
            push_fixture_box(tiles, owner, 1, [2, 3, 5], [14, 10, 14], 6);
            push_fixture_box(tiles, owner, 1, [3, 7, 2], [13, 14, 5], 8);
            push_fixture_box(tiles, owner, 1, [5, 11, 1], [7, 13, 2], 9);
            push_fixture_box(tiles, owner, 1, [9, 11, 1], [11, 13, 2], 5);
        },
        WorkbookFeatureKind::EnergyPlatform => {
            push_fixture_box(tiles, owner, 1, [0, 0, 0], [16, 2, 16], 7);
            push_fixture_box(tiles, owner, 1, [1, 2, 6], [15, 4, 10], 8);
            push_fixture_box(tiles, owner, 1, [6, 2, 1], [10, 4, 15], 8);
            push_fixture_box(tiles, owner, 1, [6, 4, 6], [10, 15, 10], 9);
        },
        WorkbookFeatureKind::SupplyRack => {
            push_fixture_box(tiles, owner, 1, [1, 0, 13], [15, 16, 15], 7);
            push_fixture_box(tiles, owner, 2, [1, 0, 13], [15, 14, 15], 7);
            for shelf_y in [2, 8, 14] {
                push_fixture_box(tiles, owner, 1, [1, shelf_y, 2], [15, shelf_y + 2, 15], 6);
            }
            push_fixture_box(tiles, owner, 1, [2, 4, 3], [7, 8, 12], 3);
            push_fixture_box(tiles, owner, 1, [9, 4, 3], [14, 8, 12], 9);
            push_fixture_box(tiles, owner, 1, [3, 10, 3], [13, 14, 12], 10);
        },
        WorkbookFeatureKind::ArmorLocker => {
            push_fixture_box(tiles, owner, 1, [2, 0, 2], [14, 16, 14], 6);
            push_fixture_box(tiles, owner, 2, [2, 0, 2], [14, 12, 14], 6);
            push_fixture_box(tiles, owner, 1, [4, 3, 1], [12, 14, 2], 7);
            push_fixture_box(tiles, owner, 2, [4, 1, 1], [12, 9, 2], 8);
            push_fixture_box(tiles, owner, 2, [11, 5, 0], [13, 7, 1], 9);
        },
        WorkbookFeatureKind::CargoRack => {
            push_fixture_box(tiles, owner, 1, [1, 0, 1], [15, 14, 15], 3);
            for x in [2, 7, 12] {
                push_fixture_box(tiles, owner, 1, [x, 1, 0], [x + 2, 13, 1], 7);
            }
            push_fixture_box(tiles, owner, 1, [1, 5, 0], [15, 7, 1], 7);
            push_fixture_box(tiles, owner, 1, [1, 11, 0], [15, 13, 1], 7);
        },
        WorkbookFeatureKind::OutpostTerminal => {
            push_fixture_box(tiles, owner, 1, [1, 0, 1], [15, 3, 15], 9);
            push_fixture_box(tiles, owner, 1, [3, 3, 5], [13, 12, 14], 6);
            push_fixture_box(tiles, owner, 1, [4, 7, 2], [12, 14, 5], 8);
            push_fixture_box(tiles, owner, 2, [7, 0, 7], [9, 12, 9], 7);
            push_fixture_box(tiles, owner, 2, [4, 10, 4], [12, 12, 12], 8);
        },
        WorkbookFeatureKind::EscapePod => {
            push_fixture_box(tiles, owner, 1, [2, 0, 3], [14, 16, 13], 7);
            push_fixture_box(tiles, owner, 2, [2, 0, 3], [14, 16, 13], 6);
            push_fixture_box(tiles, owner, 3, [4, 0, 5], [12, 8, 11], 7);
            push_fixture_box(tiles, owner, 2, [4, 3, 2], [12, 13, 3], 8);
            push_fixture_box(tiles, owner, 1, [5, 5, 2], [7, 8, 3], 5);
        },
        WorkbookFeatureKind::ThermiteFactory => {
            push_fixture_box(tiles, owner, 1, [1, 0, 1], [15, 5, 15], 7);
            push_fixture_box(tiles, owner, 1, [2, 5, 3], [7, 16, 13], 6);
            push_fixture_box(tiles, owner, 1, [9, 5, 3], [14, 16, 13], 6);
            push_fixture_box(tiles, owner, 2, [3, 0, 4], [6, 13, 12], 5);
            push_fixture_box(tiles, owner, 2, [10, 0, 4], [13, 13, 12], 5);
            push_fixture_box(tiles, owner, 1, [7, 8, 7], [9, 11, 9], 9);
        },
        WorkbookFeatureKind::Teleporter => {
            push_fixture_box(tiles, owner, 1, [0, 0, 0], [16, 2, 16], 7);
            push_fixture_box(tiles, owner, 1, [2, 2, 2], [14, 4, 14], 8);
            for (x, z) in [(1, 1), (12, 1), (1, 12), (12, 12)] {
                push_fixture_box(tiles, owner, 1, [x, 4, z], [x + 3, 16, z + 3], 9);
                push_fixture_box(tiles, owner, 2, [x, 0, z], [x + 3, 14, z + 3], 8);
            }
        },
        WorkbookFeatureKind::MedicalAnalyzer => {
            push_fixture_box(tiles, owner, 1, [0, 0, 1], [16, 3, 15], 6);
            push_fixture_box(tiles, owner, 1, [2, 3, 4], [14, 7, 14], 3);
            push_fixture_box(tiles, owner, 1, [2, 7, 12], [14, 16, 15], 7);
            push_fixture_box(tiles, owner, 2, [2, 0, 12], [14, 10, 15], 7);
            push_fixture_box(tiles, owner, 2, [3, 1, 11], [13, 8, 12], 8);
            for x in [4, 7, 10] {
                push_fixture_box(tiles, owner, 2, [x, 3, 10], [x + 1, 6, 11], 5);
            }
        },
        WorkbookFeatureKind::Furniture => {
            for (x, z) in [(2, 2), (11, 2), (2, 11), (11, 11)] {
                push_fixture_box(tiles, owner, 1, [x, 0, z], [x + 3, 10, z + 3], 3);
            }
            push_fixture_box(tiles, owner, 1, [1, 9, 1], [15, 12, 15], 3);
            push_fixture_box(tiles, owner, 1, [5, 3, 0], [11, 8, 3], 7);
        },
        WorkbookFeatureKind::ExperimentBench => {
            push_fixture_box(tiles, owner, 1, [1, 0, 2], [4, 10, 14], 7);
            push_fixture_box(tiles, owner, 1, [12, 0, 2], [15, 10, 14], 7);
            push_fixture_box(tiles, owner, 1, [1, 9, 1], [15, 12, 15], 6);
            push_fixture_box(tiles, owner, 1, [3, 12, 4], [7, 16, 9], 8);
            push_fixture_box(tiles, owner, 2, [9, 0, 5], [13, 8, 11], 1);
        },
        WorkbookFeatureKind::Crate => {
            push_fixture_box(tiles, owner, 1, [1, 0, 1], [15, 14, 15], 3);
            for x in [1, 7, 13] {
                push_fixture_box(tiles, owner, 1, [x, 1, 0], [x + 2, 13, 1], 7);
            }
            push_fixture_box(tiles, owner, 1, [1, 5, 0], [15, 7, 1], 7);
            push_fixture_box(tiles, owner, 1, [1, 11, 0], [15, 13, 1], 7);
        },
        WorkbookFeatureKind::RadarConsole => {
            push_fixture_box(tiles, owner, 1, [1, 0, 1], [15, 3, 15], 7);
            push_fixture_box(tiles, owner, 1, [3, 3, 6], [13, 11, 14], 6);
            push_fixture_box(tiles, owner, 1, [4, 7, 3], [12, 14, 6], 8);
            push_fixture_box(tiles, owner, 2, [7, 0, 7], [9, 12, 9], 9);
            push_fixture_box(tiles, owner, 2, [3, 9, 7], [13, 11, 9], 8);
        },
        WorkbookFeatureKind::InstrumentPanel => {
            push_fixture_box(tiles, owner, 1, [1, 0, 10], [15, 16, 15], 7);
            push_fixture_box(tiles, owner, 2, [1, 0, 10], [15, 10, 15], 6);
            push_fixture_box(tiles, owner, 1, [3, 5, 9], [6, 8, 10], 8);
            push_fixture_box(tiles, owner, 1, [8, 5, 9], [10, 8, 10], 5);
            push_fixture_box(tiles, owner, 1, [12, 5, 9], [14, 8, 10], 9);
        },
    }
}

fn workbook_cell_is_exterior(
    design: WorkbookMapDesign,
    decoded: &DecodedWorkbookMap,
    sheet_x: i32,
    sheet_z: i32,
) -> bool {
    sheet_x < 0
        || sheet_z < 0
        || sheet_x >= design.width as i32
        || sheet_z >= design.height as i32
        || decoded.exterior[sheet_z as usize * design.width + sheet_x as usize]
}

fn workbook_station_wall_is_glass(
    design: WorkbookMapDesign,
    decoded: &DecodedWorkbookMap,
    index: usize,
    y: i32,
) -> bool {
    if decoded.styles[index] != 11
        || !(2..WORKBOOK_ROOM_HEIGHT - 1).contains(&y)
    {
        return false;
    }

    let palette = workbook_hull_palette(workbook_hull_style(design));
    let sheet_x = (index % design.width) as i32;
    let sheet_z = (index / design.width) as i32;
    let [x, z] = design.centered_offset(index);
    [(-1, 0), (1, 0), (0, -1), (0, 1)]
        .into_iter()
        .filter(|(dx, dz)| {
            workbook_cell_is_exterior(design, decoded, sheet_x + dx, sheet_z + dz)
        })
        .any(|(dx, _)| {
            let longitudinal = if dx != 0 { z } else { x };
            let pattern = longitudinal.rem_euclid(palette.rib_spacing);
            (2..=palette.rib_spacing - 3).contains(&pattern)
        })
}

fn add_workbook_hull_micro_tiles(
    tiles: &mut Vec<VoxelMicroTile>,
    design: WorkbookMapDesign,
    decoded: &DecodedWorkbookMap,
) {
    let palette = workbook_hull_palette(workbook_hull_style(design));
    let directions = [
        (IVec3::NEG_X, -1, 0),
        (IVec3::X, 1, 0),
        (IVec3::NEG_Z, 0, -1),
        (IVec3::Z, 0, 1),
    ];
    for (index, style) in decoded.styles.iter().copied().enumerate() {
        let sheet_x = (index % design.width) as i32;
        let sheet_z = (index / design.width) as i32;
        let [x, z] = design.centered_offset(index);
        let base = IVec3::new(x, 0, z);

        if decoded.enclosed[index]
            && (x.rem_euclid(palette.rib_spacing) == 0
                || z.rem_euclid(palette.rib_spacing) == 0)
        {
            let (min, max) = if x.rem_euclid(palette.rib_spacing) == 0 {
                ([7, 0, 0], [9, 1, 16])
            } else {
                ([0, 0, 7], [16, 1, 9])
            };
            push_micro_box(
                tiles,
                base + IVec3::Y * WORKBOOK_ROOM_HEIGHT,
                base + IVec3::Y * (WORKBOOK_ROOM_HEIGHT + 1),
                min,
                max,
                palette.trim,
                VoxelMicroTileKind::Hull,
            );
        }

        if !matches!(style, 11 | 15) {
            continue;
        }
        for (direction, dx, dz) in directions {
            if !workbook_cell_is_exterior(
                design,
                decoded,
                sheet_x + dx,
                sheet_z + dz,
            ) {
                continue;
            }
            let longitudinal = if dx != 0 { z } else { x };
            let pattern = longitudinal.rem_euclid(palette.rib_spacing);
            let rib = pattern == 0;
            let thickness = if rib { 4 } else { 2 };
            let (min, max) = match direction {
                IVec3::NEG_X => ([16 - thickness, 0, 0], [16, 16, 16]),
                IVec3::X => ([0, 0, 0], [thickness, 16, 16]),
                IVec3::NEG_Z => ([0, 0, 16 - thickness], [16, 16, 16]),
                IVec3::Z => ([0, 0, 0], [16, 16, thickness]),
                _ => unreachable!(),
            };
            let outside = base + direction;
            if style == 11 {
                for y in 0..=WORKBOOK_ROOM_HEIGHT {
                    let material = if workbook_station_wall_is_glass(
                        design, decoded, index, y,
                    ) {
                        VOXEL_GLASS_MATERIAL
                    } else if matches!(y, 3 | 4) && matches!(pattern, 4 | 5) {
                        palette.window
                    } else if y == 1 || y == WORKBOOK_ROOM_HEIGHT - 1 {
                        palette.trim
                    } else if y == 2 && pattern == 7 {
                        palette.armor
                    } else if rib {
                        palette.trim
                    } else {
                        palette.hull
                    };
                    push_micro_box(
                        tiles,
                        base + IVec3::Y * y,
                        outside + IVec3::Y * y,
                        min,
                        max,
                        material,
                        VoxelMicroTileKind::Hull,
                    );
                }
            } else {
                // A thin sill and canopy preserve the automatic door opening.
                let (sill_min, sill_max) = match direction {
                    IVec3::NEG_X | IVec3::X => (
                        [min[0], 0, 1],
                        [max[0], 2, 15],
                    ),
                    _ => ([1, 0, min[2]], [15, 2, max[2]]),
                };
                push_micro_box(
                    tiles,
                    base,
                    outside,
                    sill_min,
                    sill_max,
                    palette.trim,
                    VoxelMicroTileKind::Hull,
                );
                push_micro_box(
                    tiles,
                    base + IVec3::Y * WORKBOOK_ROOM_HEIGHT,
                    outside + IVec3::Y * (WORKBOOK_ROOM_HEIGHT - 1),
                    min,
                    max,
                    palette.trim,
                    VoxelMicroTileKind::Hull,
                );
                push_micro_box(
                    tiles,
                    base + IVec3::Y * WORKBOOK_ROOM_HEIGHT,
                    outside + IVec3::Y * WORKBOOK_ROOM_HEIGHT,
                    min,
                    max,
                    palette.armor,
                    VoxelMicroTileKind::Hull,
                );
            }
            push_micro_box(
                tiles,
                base + IVec3::Y * WORKBOOK_ROOM_HEIGHT,
                base + IVec3::Y * (WORKBOOK_ROOM_HEIGHT + 1),
                [0, 0, 0],
                [16, 1, 16],
                if rib { palette.armor } else { palette.trim },
                VoxelMicroTileKind::Hull,
            );
        }
    }
}

fn workbook_feature_regions(
    design: WorkbookMapDesign,
    decoded: &DecodedWorkbookMap,
) -> Vec<WorkbookFeatureRegion> {
    let mut regions = Vec::new();
    let mut seen = vec![false; decoded.features.len()];
    for start in 0..decoded.features.len() {
        let feature = decoded.features[start];
        let Some(kind) = WorkbookFeatureKind::from_id(feature) else {
            continue;
        };
        if seen[start] || !decoded.enclosed[start] {
            continue;
        }
        let mut pending = vec![start];
        let mut component = Vec::new();
        seen[start] = true;
        while let Some(index) = pending.pop() {
            component.push(index);
            let x = index % design.width;
            let z = index / design.width;
            for neighbor in [
                x.checked_sub(1).map(|x| z * design.width + x),
                (x + 1 < design.width).then_some(z * design.width + x + 1),
                z.checked_sub(1).map(|z| z * design.width + x),
                (z + 1 < design.height).then_some((z + 1) * design.width + x),
            ]
            .into_iter()
            .flatten()
            {
                if !seen[neighbor]
                    && decoded.enclosed[neighbor]
                    && decoded.features[neighbor] == feature
                {
                    seen[neighbor] = true;
                    pending.push(neighbor);
                }
            }
        }

        let mut cells = component
            .into_iter()
            .map(|index| {
                let [x, z] = design.centered_offset(index);
                IVec3::new(x, 0, z)
            })
            .collect::<Vec<_>>();
        cells.sort_unstable_by_key(|cell| (cell.z, cell.x));
        let count = cells.len() as i64;
        let sum_x = cells.iter().map(|cell| i64::from(cell.x)).sum::<i64>();
        let sum_z = cells.iter().map(|cell| i64::from(cell.z)).sum::<i64>();
        let anchor = cells
            .iter()
            .copied()
            .min_by_key(|cell| {
                let dx = i64::from(cell.x) * count - sum_x;
                let dz = i64::from(cell.z) * count - sum_z;
                dx * dx + dz * dz
            })
            .expect("workbook feature components cannot be empty");
        regions.push(WorkbookFeatureRegion {
            kind,
            cells,
            anchor,
        });
    }
    regions
}

fn workbook_feature_annotations(
    design: WorkbookMapDesign,
    decoded: &DecodedWorkbookMap,
) -> VoxelWorkbookFeatureAnnotations {
    VoxelWorkbookFeatureAnnotations {
        map_name: design.name,
        regions: workbook_feature_regions(design, decoded),
    }
}

fn workbook_micro_tiles(
    design: WorkbookMapDesign,
    decoded: &DecodedWorkbookMap,
) -> Vec<VoxelMicroTile> {
    let mut tiles = Vec::new();
    for region in workbook_feature_regions(design, decoded) {
        for owner in region.cells.iter().copied() {
            add_workbook_feature_floor_micro_tile(
                &mut tiles,
                owner,
                region.kind,
            );
        }

        let min_x = region.cells.iter().map(|cell| cell.x).min().unwrap();
        let min_z = region.cells.iter().map(|cell| cell.z).min().unwrap();
        let spacing = workbook_feature_fixture_spacing(region.kind) as i32;
        let offset = spacing / 2;
        let mut fixture_cells = region
            .cells
            .iter()
            .copied()
            .filter(|cell| {
                (cell.x - min_x).rem_euclid(spacing) == offset
                    && (cell.z - min_z).rem_euclid(spacing) == offset
            })
            .collect::<Vec<_>>();
        if fixture_cells.is_empty() {
            fixture_cells.push(region.anchor);
        }
        for owner in fixture_cells {
            add_workbook_fixture_micro_tiles(&mut tiles, owner, region.kind);
        }
    }
    add_workbook_hull_micro_tiles(&mut tiles, design, decoded);
    tiles.sort_unstable_by_key(|tile| {
        (
            tile.cell.y,
            tile.cell.z,
            tile.cell.x,
            tile.material,
            tile.min.y,
            tile.min.z,
            tile.min.x,
        )
    });
    tiles.dedup();
    tiles
}

#[cfg(test)]
fn voxel_auto_doors() -> Vec<VoxelAutoDoor> {
    let mut doors = Vec::new();
    for (center, design) in workbook_orbital_locations() {
        doors.extend(workbook_auto_doors(center, design));
    }
    doors
}

fn static_voxel_auto_doors() -> Vec<VoxelAutoDoor> {
    let mut doors = Vec::new();
    for (center, design) in static_workbook_orbital_locations() {
        doors.extend(workbook_auto_doors(center, design));
    }
    doors
}

fn workbook_auto_doors(center: IVec3, design: WorkbookMapDesign) -> Vec<VoxelAutoDoor> {
    let decoded = design.decode();
    let mut seen = vec![false; decoded.styles.len()];
    let mut doors = Vec::new();
    for start in 0..decoded.styles.len() {
        if decoded.styles[start] != 15 || seen[start] {
            continue;
        }
        let mut pending = vec![start];
        seen[start] = true;
        let mut min_x = usize::MAX;
        let mut max_x = 0;
        let mut min_z = usize::MAX;
        let mut max_z = 0;
        while let Some(index) = pending.pop() {
            let x = index % design.width;
            let z = index / design.width;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_z = min_z.min(z);
            max_z = max_z.max(z);
            for neighbor in [
                x.checked_sub(1).map(|x| z * design.width + x),
                (x + 1 < design.width).then_some(z * design.width + x + 1),
                z.checked_sub(1).map(|z| z * design.width + x),
                (z + 1 < design.height).then_some((z + 1) * design.width + x),
            ]
            .into_iter()
            .flatten()
            {
                if decoded.styles[neighbor] == 15 && !seen[neighbor] {
                    seen[neighbor] = true;
                    pending.push(neighbor);
                }
            }
        }

        let width = max_x - min_x + 1;
        let depth = max_z - min_z + 1;
        let width_axis = if width >= depth { IVec3::X } else { IVec3::Z };
        let span = width.max(depth) as i32;
        let center_index =
            ((min_z + max_z) / 2) * design.width + (min_x + max_x) / 2;
        let [x, z] = design.centered_offset(center_index);
        doors.push(make_voxel_auto_door(
            center + IVec3::new(x, 0, z),
            width_axis,
            (span - 1) / 2,
            WORKBOOK_ROOM_HEIGHT - 2,
            1.75,
        ));
    }
    doors
}

fn make_voxel_auto_door(
    base: IVec3,
    width_axis: IVec3,
    half_width: i32,
    height: i32,
    trigger_radius: f32,
) -> VoxelAutoDoor {
    let cells = (-half_width..=half_width)
        .flat_map(|width| (1..=height).map(move |y| base + width_axis * width + IVec3::Y * y))
        .collect::<Vec<_>>();
    let trigger_center =
        (base.as_vec3() + Vec3::new(0.5, (height + 1) as f32 * 0.5, 0.5)) * VOXEL_SIZE;
    let (closed_translation, _) = voxel_door_transform_and_size(&cells);
    VoxelAutoDoor {
        cells,
        trigger_center,
        trigger_radius,
        trigger_half_height: (height as f32 * VOXEL_SIZE * 0.65).max(VOXEL_SIZE * 3.0),
        width_axis,
        material: 10,
        closed_translation,
        open_translation: closed_translation,
        open: false,
    }
}

fn voxel_door_transform_and_size(cells: &[IVec3]) -> (Vec3, Vec3) {
    let min = cells.iter().copied().reduce(IVec3::min).unwrap_or_default();
    let max = cells.iter().copied().reduce(IVec3::max).unwrap_or_default();
    let size = (max - min + IVec3::ONE).as_vec3() * VOXEL_SIZE;
    let translation = (min.as_vec3() + (max - min + IVec3::ONE).as_vec3() * 0.5) * VOXEL_SIZE;
    (translation, size)
}

fn voxel_auto_door_panel_size(panel: &VoxelAutoDoor) -> Vec3 {
    let (_, mut size) = voxel_door_transform_and_size(&panel.cells);
    let depth = VOXEL_SIZE * 0.45;
    if panel.width_axis == IVec3::X {
        size.z = depth;
    } else {
        size.x = depth;
    }
    size
}

fn setup_voxel_auto_doors(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    for door in static_voxel_auto_doors() {
        for panel in voxel_auto_door_panels(&door) {
            let size = voxel_auto_door_panel_size(&panel);
            let translation = panel.closed_translation;
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
                MeshMaterial3d(materials.handles[panel.material as usize - 1].clone()),
                Transform::from_translation(translation),
                RigidBody::Kinematic,
                Collider::cuboid(size.x, size.y, size.z),
                panel,
            ));
        }
    }
}

fn voxel_auto_door_panels(door: &VoxelAutoDoor) -> [VoxelAutoDoor; 2] {
    let axis = door.width_axis;
    let projection = |cell: IVec3| cell.dot(axis);
    let min = door
        .cells
        .iter()
        .copied()
        .map(projection)
        .min()
        .unwrap_or(0);
    let max = door
        .cells
        .iter()
        .copied()
        .map(projection)
        .max()
        .unwrap_or(0);
    let midpoint = (min + max) / 2;
    let left_cells = door
        .cells
        .iter()
        .copied()
        .filter(|cell| projection(*cell) <= midpoint)
        .collect::<Vec<_>>();
    let right_cells = door
        .cells
        .iter()
        .copied()
        .filter(|cell| projection(*cell) > midpoint)
        .collect::<Vec<_>>();
    let make_panel = |cells: Vec<IVec3>, direction: f32| {
        let (closed_translation, size) = voxel_door_transform_and_size(&cells);
        let panel_width = size.dot(axis.as_vec3().abs());
        VoxelAutoDoor {
            cells,
            trigger_center: door.trigger_center,
            trigger_radius: door.trigger_radius,
            trigger_half_height: door.trigger_half_height,
            width_axis: axis,
            material: door.material,
            closed_translation,
            open_translation: closed_translation
                + axis.as_vec3() * direction * (panel_width + VOXEL_SIZE * 0.5),
            open: false,
        }
    };
    [make_panel(left_cells, -1.0), make_panel(right_cells, 1.0)]
}

fn voxel_auto_door_should_open(door: &VoxelAutoDoor, player_position: Vec3) -> bool {
    let horizontal = Vec2::new(
        player_position.x - door.trigger_center.x,
        player_position.z - door.trigger_center.z,
    );
    horizontal.length() <= door.trigger_radius
        && (player_position.y - door.trigger_center.y).abs() <= door.trigger_half_height
}

fn voxel_auto_door_has_support(grid: &Grid<u8>, door_cells: &HashSet<IVec3>) -> bool {
    door_cells.iter().any(|cell| {
        VOXEL_NEIGHBORS.into_iter().any(|offset| {
            let neighbor = *cell + offset;
            !door_cells.contains(&neighbor)
                && grid.get(neighbor).is_some_and(TrpgVoxelConnector::solid)
        })
    })
}

fn despawn_unsupported_voxel_auto_doors(
    mut commands: Commands,
    grids: Query<&Grid<u8>, With<TrpgVoxelGrid>>,
    doors: Query<(Entity, &VoxelAutoDoor)>,
) {
    let Ok(grid) = grids.single() else { return };
    let mut groups = HashMap::<(u32, u32, u32), (Vec<Entity>, HashSet<IVec3>)>::new();
    for (entity, door) in &doors {
        let key = (
            door.trigger_center.x.to_bits(),
            door.trigger_center.y.to_bits(),
            door.trigger_center.z.to_bits(),
        );
        let (entities, cells) = groups.entry(key).or_default();
        entities.push(entity);
        cells.extend(door.cells.iter().copied());
    }
    for (_, (entities, cells)) in groups {
        if voxel_auto_door_has_support(grid, &cells) {
            continue;
        }
        for entity in entities {
            commands.entity(entity).despawn();
        }
    }
}

fn workbook_interior_lights() -> Vec<(Vec3, Color)> {
    let mut lights = Vec::new();
    for (center, design, color) in [
        (
            RESEARCH_STATION_CENTER,
            NIFFY,
            Color::srgb(0.55, 0.75, 1.0),
        ),
        (
            SENSOR_STATION_CENTER,
            ARBITRATOR,
            Color::srgb(0.48, 0.9, 1.0),
        ),
        (
            CANNON_STATION_CENTER,
            KYO,
            Color::srgb(1.0, 0.64, 0.32),
        ),
        (
            ABANDONED_STATION_CENTER,
            ABANDONED,
            Color::srgb(0.72, 0.24, 0.18),
        ),
    ] {
        let decoded = design.decode();
        let candidates = decoded
            .enclosed
            .iter()
            .enumerate()
            .filter_map(|(index, enclosed)| {
                (*enclosed
                    && decoded.features[index] == 0
                    && workbook_fixture(decoded.styles[index]).is_none())
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let mut selected = HashSet::new();
        for target_z in (8..design.height).step_by(22) {
            for target_x in (8..design.width).step_by(26) {
                let Some(index) = candidates.iter().copied().min_by_key(|index| {
                    let x = index % design.width;
                    let z = index / design.width;
                    x.abs_diff(target_x).pow(2) + z.abs_diff(target_z).pow(2)
                }) else {
                    continue;
                };
                selected.insert(index);
            }
        }
        for index in selected {
            let [x, z] = design.centered_offset(index);
            lights.push((
                (center.as_vec3()
                    + Vec3::new(
                        x as f32 + 0.5,
                        WORKBOOK_ROOM_HEIGHT as f32 - 1.0,
                        z as f32 + 0.5,
                    ))
                    * VOXEL_SIZE,
                color,
            ));
        }
    }
    lights
}

fn setup_voxel_interior_lights(mut commands: Commands) {
    for (position, color) in workbook_interior_lights() {
        commands.spawn((
            PointLight {
                color,
                intensity: 18_000.0,
                range: 6.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(position),
        ));
    }
}

fn voxel_prop_cells(size: IVec3, base_material: u8, accent_material: u8) -> Vec<(IVec3, u8)> {
    prism(IVec3::ZERO, size)
        .map(|cell| {
            let accent = cell.y == size.y - 1 && (cell.x + cell.z) % 2 == 0;
            (
                cell,
                if accent { accent_material } else { base_material },
            )
        })
        .collect()
}

fn voxel_physics_prop_specs() -> Vec<(Vec<(IVec3, u8)>, Transform)> {
    let mut specs = Vec::new();
    for (center, design) in static_workbook_orbital_locations() {
        let decoded = design.decode();
        let candidates = decoded
            .enclosed
            .iter()
            .enumerate()
            .filter_map(|(index, enclosed)| {
                (*enclosed
                    && decoded.features[index] == 0
                    && workbook_fixture(decoded.styles[index]).is_none())
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        for (ordinal, target) in [
            [design.spawn[0] - 5, design.spawn[1] + 4],
            [design.spawn[0] + 5, design.spawn[1] + 4],
        ]
        .into_iter()
        .enumerate()
        {
            let Some(index) = candidates.iter().copied().min_by_key(|index| {
                let [x, z] = design.centered_offset(*index);
                x.abs_diff(target[0]).pow(2) + z.abs_diff(target[1]).pow(2)
            }) else {
                continue;
            };
            let [x, z] = design.centered_offset(index);
            specs.push((
                voxel_prop_cells(
                    IVec3::new(2, 2, 2),
                    if ordinal == 0 { 6 } else { 7 },
                    if ordinal == 0 { 8 } else { 10 },
                ),
                Transform::from_translation(Vec3::new(
                    (center.x + x) as f32 * VOXEL_SIZE,
                    VOXEL_SIZE * 1.1,
                    (center.z + z) as f32 * VOXEL_SIZE,
                ))
                .with_rotation(Quat::from_rotation_y(ordinal as f32 * 0.35)),
            ));
        }
    }
    specs
}

fn setup_voxel_sample_props(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    spawn_default_voxel_physics_props(&mut commands, &mut meshes, &materials);
}

fn spawn_default_voxel_physics_props(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &VoxelMaterials,
) {
    for (cells, transform) in voxel_physics_prop_specs() {
        spawn_voxel_physics_body_at(
            commands,
            meshes,
            materials,
            cells,
            transform,
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
        );
    }
}

fn combat_spaceship_cab_half_width(z: i32) -> i32 {
    let taper_length = 7;
    if z < ARROGANCE_CAB_FRONT_Z + taper_length {
        6 + (z - ARROGANCE_CAB_FRONT_Z) * (ARROGANCE_CAB_MAX_HALF_WIDTH - 6) / taper_length
    } else {
        ARROGANCE_CAB_MAX_HALF_WIDTH
    }
}

fn combat_spaceship_cab_interior_contains(cell: IVec3) -> bool {
    let clear_height =
        (ARROGANCE_CAB_FLOOR_Y + 1..ARROGANCE_CAB_CEILING_Y).contains(&cell.y);
    if !clear_height {
        return false;
    }
    if (ARROGANCE_CAB_FRONT_Z + 1..=ARROGANCE_CAB_REAR_Z).contains(&cell.z) {
        return cell.x.abs() < combat_spaceship_cab_half_width(cell.z);
    }
    (ARROGANCE_CAB_REAR_Z + 1..HANGAR_REAR_Z).contains(&cell.z) && cell.x.abs() < 2
}

fn combat_spaceship_cab_cells() -> HashMap<IVec3, u8> {
    let mut cells = HashMap::new();
    for z in ARROGANCE_CAB_FRONT_Z..=ARROGANCE_CAB_REAR_Z {
        let half_width = combat_spaceship_cab_half_width(z);
        for x in -half_width..=half_width {
            let edge = x.abs() == half_width;
            cells.insert(
                IVec3::new(x, ARROGANCE_CAB_FLOOR_Y, z),
                if edge { 7 } else { 6 },
            );
            cells.insert(
                IVec3::new(x, ARROGANCE_CAB_CEILING_Y, z),
                if edge { 7 } else { 6 },
            );
        }

        for y in ARROGANCE_CAB_FLOOR_Y + 1..ARROGANCE_CAB_CEILING_Y {
            let window =
                (ARROGANCE_CAB_FLOOR_Y + 3..=ARROGANCE_CAB_CEILING_Y - 3).contains(&y);
            let material = if window { VOXEL_GLASS_MATERIAL } else { 7 };
            cells.insert(IVec3::new(-half_width, y, z), material);
            cells.insert(IVec3::new(half_width, y, z), material);
        }
    }

    // A broad forward windscreen, with a solid sill and header, closes the
    // tapered nose while retaining a clear seated sightline.
    let front_half_width = combat_spaceship_cab_half_width(ARROGANCE_CAB_FRONT_Z);
    for x in -front_half_width..=front_half_width {
        for y in ARROGANCE_CAB_FLOOR_Y + 1..ARROGANCE_CAB_CEILING_Y {
            let material = if (ARROGANCE_CAB_FLOOR_Y + 3..=ARROGANCE_CAB_CEILING_Y - 3).contains(&y)
            {
                VOXEL_GLASS_MATERIAL
            } else {
                7
            };
            cells.insert(
                IVec3::new(x, y, ARROGANCE_CAB_FRONT_Z),
                material,
            );
        }
    }

    // Close the rear around a three-cell-wide personnel door, then bridge the
    // short armored neck to the hangar without intruding into its flight path.
    for x in -ARROGANCE_CAB_MAX_HALF_WIDTH..=ARROGANCE_CAB_MAX_HALF_WIDTH {
        for y in ARROGANCE_CAB_FLOOR_Y + 1..ARROGANCE_CAB_CEILING_Y {
            if x.abs() > 1 || y >= ARROGANCE_CAB_CEILING_Y - 2 {
                cells.insert(
                    IVec3::new(x, y, ARROGANCE_CAB_REAR_Z),
                    7,
                );
            }
        }
    }
    for z in ARROGANCE_CAB_REAR_Z + 1..HANGAR_REAR_Z {
        for x in -2..=2 {
            cells.insert(
                IVec3::new(x, ARROGANCE_CAB_FLOOR_Y, z),
                7,
            );
            cells.insert(
                IVec3::new(x, ARROGANCE_CAB_CEILING_Y, z),
                7,
            );
        }
        for y in ARROGANCE_CAB_FLOOR_Y + 1..ARROGANCE_CAB_CEILING_Y {
            cells.insert(IVec3::new(-2, y, z), 7);
            cells.insert(IVec3::new(2, y, z), 7);
        }
    }

    // Flight console, two pilot chairs, side terminals, and inset ceiling
    // lamps make the enclosed volume read as a working bridge.
    for x in -4..=4 {
        cells.insert(
            IVec3::new(
                x,
                ARROGANCE_CAB_FLOOR_Y + 1,
                ARROGANCE_CAB_FRONT_Z + 3,
            ),
            10,
        );
    }
    for x in [-4, 4] {
        cells.insert(
            IVec3::new(
                x,
                ARROGANCE_CAB_FLOOR_Y + 1,
                ARROGANCE_CAB_FRONT_Z + 8,
            ),
            9,
        );
    }
    for x in [-10, 10] {
        for z in ARROGANCE_CAB_FRONT_Z + 8..=ARROGANCE_CAB_REAR_Z - 4 {
            cells.insert(
                IVec3::new(x, ARROGANCE_CAB_FLOOR_Y + 1, z),
                8,
            );
        }
    }
    for z in [ARROGANCE_CAB_FRONT_Z + 7, ARROGANCE_CAB_REAR_Z - 5] {
        for x in -2..=2 {
            cells.insert(
                IVec3::new(x, ARROGANCE_CAB_CEILING_Y, z),
                10,
            );
        }
    }
    cells
}

fn scale_combat_spaceship_cell(cell: IVec3) -> IVec3 {
    cell * ARROGANCE_SCALE
}

fn scale_combat_spaceship_cells(cells: &HashMap<IVec3, u8>) -> HashMap<IVec3, u8> {
    let mut scaled =
        HashMap::with_capacity(cells.len() * ARROGANCE_SCALE.pow(3) as usize);
    for (cell, material) in cells {
        let origin = scale_combat_spaceship_cell(*cell);
        for offset_x in 0..ARROGANCE_SCALE {
            for offset_y in 0..ARROGANCE_SCALE {
                for offset_z in 0..ARROGANCE_SCALE {
                    scaled.insert(
                        origin + IVec3::new(offset_x, offset_y, offset_z),
                        *material,
                    );
                }
            }
        }
    }
    scaled
}

fn combat_spaceship_hangar_contains(cell: IVec3) -> bool {
    (HANGAR_MIN_X..=HANGAR_MAX_X).contains(&cell.x)
        && (HANGAR_PARKING_Y..HANGAR_CEILING_Y).contains(&cell.y)
        && (HANGAR_REAR_Z..=HANGAR_MOUTH_Z).contains(&cell.z)
}

fn carve_combat_spaceship_hangar(cells: &mut HashMap<IVec3, u8>) {
    // This is a subtraction from the proportionally enlarged workbook hull.
    // It does not add a parking shell, room, divider, deck, roof, or outer box.
    cells.retain(|cell, _| !combat_spaceship_hangar_contains(*cell));
}

fn original_combat_spaceship_voxel_cells() -> HashMap<IVec3, u8> {
    let mut world = World::new();
    let grid_entity = world.spawn(Grid::<u8>::new()).id();
    {
        let mut entity = world.entity_mut(grid_entity);
        let mut grid = entity
            .get_mut::<Grid<u8>>()
            .expect("temporary spaceship grid must exist");
        build_workbook_orbital_location(
            &mut grid,
            IVec3::ZERO,
            ARROGANCE,
        );
        for door in workbook_auto_doors(IVec3::ZERO, ARROGANCE) {
            for cell in door.cells {
                grid.set(cell, 0);
            }
        }
    }
    let grid = world
        .entity(grid_entity)
        .get::<Grid<u8>>()
        .expect("temporary spaceship grid must still exist");
    voxel_cells(grid).into_iter().collect()
}

fn combat_spaceship_voxel_cells() -> Vec<(IVec3, u8)> {
    let original = original_combat_spaceship_voxel_cells();
    let mut occupied = scale_combat_spaceship_cells(&original);
    carve_combat_spaceship_hangar(&mut occupied);
    occupied.retain(|cell, _| !combat_spaceship_cab_interior_contains(*cell));
    occupied.extend(combat_spaceship_cab_cells());
    let mut cells = occupied.into_iter().collect::<Vec<_>>();
    cells.sort_unstable_by_key(|(cell, _)| (cell.y, cell.z, cell.x));
    cells
}

fn remove_static_combat_spaceship(grid: &mut Mut<Grid<u8>>) {
    // Old scene snapshots contain the original 1x workbook hull in the static
    // grid. The enlarged 3x carrier is a separate dynamic VoxelSpaceship, so
    // remove the legacy workbook cells rather than subtracting its new shape.
    for (cell, _) in original_combat_spaceship_voxel_cells() {
        grid.set(COMBAT_SPACESHIP_CENTER + cell, 0);
    }
}

fn small_spaceship_voxel_cells(variant: usize) -> Vec<(IVec3, u8)> {
    let half_width = 5 + (variant % 2) as i32;
    let nose = -13 - (variant % 3) as i32;
    let tail = 9 + (variant % 2) as i32;
    let wing_span = half_width + 4 + (variant % 3 == 2) as i32;
    let mut cells = HashMap::<IVec3, u8>::new();

    for z in nose..=tail {
        let section_half_width = if z < -5 {
            let run = (-5 - nose).max(1);
            1 + (z - nose) * (half_width - 1) / run
        } else if z > 6 {
            let run = (tail - 6).max(1);
            1 + (tail - z) * (half_width - 1) / run
        } else {
            half_width
        };

        for x in -section_half_width..=section_half_width {
            let edge = x.abs() == section_half_width;
            cells.insert(
                IVec3::new(x, 0, z),
                if edge { 7 } else { 6 },
            );
            let canopy_half_width = (section_half_width - 1).clamp(0, 3);
            let canopy = (nose + 1..=1).contains(&z)
                && x.abs() <= canopy_half_width;
            cells.insert(
                IVec3::new(x, 5, z),
                if canopy {
                    VOXEL_GLASS_MATERIAL
                } else if edge {
                    7
                } else {
                    6
                },
            );
        }

        for y in 1..=4 {
            let window = (nose + 1..=1).contains(&z) && (2..=4).contains(&y);
            let wall_material = if window {
                VOXEL_GLASS_MATERIAL
            } else if y == 1 || y == 4 {
                7
            } else {
                6
            };
            cells.insert(
                IVec3::new(-section_half_width, y, z),
                wall_material,
            );
            cells.insert(
                IVec3::new(section_half_width, y, z),
                wall_material,
            );
        }

        if z == nose || z == tail {
            for x in -section_half_width..=section_half_width {
                for y in 1..=4 {
                    cells.insert(
                        IVec3::new(x, y, z),
                        if z == nose && (2..=4).contains(&y) {
                            VOXEL_GLASS_MATERIAL
                        } else if y == 2 {
                            7
                        } else {
                            6
                        },
                    );
                }
            }
        }
    }

    // Swept wings preserve a recognizable silhouette without turning the ship
    // into a rectangular room floating in space.
    for z in -2_i32..=4 {
        let span = wing_span - (z - 1).abs() / 2;
        for x in -span..=span {
            let material = if x.abs() >= span - 1 { 9 } else { 7 };
            cells.entry(IVec3::new(x, 0, z)).or_insert(material);
        }
    }

    // The cabin is furnished entirely with canonical voxel cells: flight
    // console, paired seats, side terminals, cargo lockers, and ceiling lamps.
    cells.insert(IVec3::new(0, 1, -5), 10);
    cells.insert(IVec3::new(1, 2, -5), 8);
    for x in [-2, 2] {
        cells.insert(IVec3::new(x, 1, -1), 9);
        cells.insert(IVec3::new(x, 1, 2), 9);
    }
    for x in [-(half_width - 1), half_width - 1] {
        for z in -4..=0 {
            cells.insert(IVec3::new(x, 1, z), 8);
        }
        for z in 3..=5 {
            cells.insert(IVec3::new(x, 1, z), 10);
        }
    }
    for z in [-3, 1, 5] {
        cells.insert(IVec3::new(0, 5, z), 10);
    }

    // Twin engine pods and dorsal fins vary by hull while remaining connected
    // to the main voxel body.
    for x in [-2, 2] {
        for z in tail - 1..=tail + 1 {
            cells.insert(IVec3::new(x, 1, z), 9);
            cells.insert(IVec3::new(x, 2, z), 8);
        }
    }
    if variant % 2 == 1 {
        for z in 0..=4 {
            cells.insert(IVec3::new(-wing_span, 1, z), 10);
            cells.insert(IVec3::new(wing_span, 1, z), 10);
        }
    }

    // Keep the cockpit and central aisle genuinely hollow, then cut a broad
    // aft hatch and extend a short boarding ramp into space.
    for x in -1..=1 {
        for y in 1..=4 {
            for z in -3..=tail {
                cells.remove(&IVec3::new(x, y, z));
            }
        }
    }
    for step in 1_i32..=3 {
        let ramp_half_width = (3 - step).max(1);
        for x in -ramp_half_width..=ramp_half_width {
            cells.insert(
                IVec3::new(x, 0, tail + step),
                if x.abs() == ramp_half_width { 10 } else { 7 },
            );
        }
    }

    let mut cells = cells.into_iter().collect::<Vec<_>>();
    cells.sort_unstable_by_key(|(cell, _)| (cell.y, cell.z, cell.x));
    cells
}

fn medium_spaceship_voxel_cells() -> Vec<(IVec3, u8)> {
    const NOSE_Z: i32 = -21;
    const TAIL_Z: i32 = 14;
    const HULL_HALF_WIDTH: i32 = 14;
    const WING_SPAN: i32 = 18;
    const ROOF_Y: i32 = 8;

    let mut cells = HashMap::<IVec3, u8>::new();
    for z in NOSE_Z..=TAIL_Z {
        let section_half_width = if z < -9 {
            2 + (z - NOSE_Z) * (HULL_HALF_WIDTH - 2) / (-9 - NOSE_Z)
        } else if z > 8 {
            3 + (TAIL_Z - z) * (HULL_HALF_WIDTH - 3) / (TAIL_Z - 8)
        } else {
            HULL_HALF_WIDTH
        };

        for x in -section_half_width..=section_half_width {
            cells.insert(
                IVec3::new(x, 0, z),
                if x.abs() == section_half_width { 7 } else { 6 },
            );
            let canopy_half_width = (section_half_width - 2).clamp(1, 7);
            cells.insert(
                IVec3::new(x, ROOF_Y, z),
                if (NOSE_Z + 2..=1).contains(&z)
                    && x.abs() <= canopy_half_width
                {
                    VOXEL_GLASS_MATERIAL
                } else if x.abs() == section_half_width {
                    7
                } else {
                    6
                },
            );
        }

        for y in 1..ROOF_Y {
            let material = if (2..=6).contains(&y)
                && (NOSE_Z + 2..=2).contains(&z)
            {
                VOXEL_GLASS_MATERIAL
            } else if matches!(y, 1 | 7) {
                7
            } else {
                6
            };
            cells.insert(IVec3::new(-section_half_width, y, z), material);
            cells.insert(IVec3::new(section_half_width, y, z), material);
        }

        if z == NOSE_Z || z == TAIL_Z {
            for x in -section_half_width..=section_half_width {
                for y in 1..ROOF_Y {
                    cells.insert(
                        IVec3::new(x, y, z),
                        if z == NOSE_Z && (2..=6).contains(&y) {
                            VOXEL_GLASS_MATERIAL
                        } else if z == TAIL_Z && matches!(y, 3 | 4) {
                            8
                        } else {
                            6
                        },
                    );
                }
            }
        }
    }

    // Broad swept wings and attached engine nacelles give the corvette a
    // clearly medium-class silhouette while remaining one connected body.
    for z in -3_i32..=8 {
        let span = WING_SPAN - (z - 2).abs() / 3;
        for x in -span..=span {
            cells.entry(IVec3::new(x, 0, z)).or_insert(if x.abs() >= span - 1 {
                10
            } else {
                7
            });
        }
    }
    for x in [-11, 11] {
        for z in 7..=TAIL_Z + 3 {
            for y in 1..=3 {
                cells.insert(
                    IVec3::new(x, y, z),
                    if y == 2 && z >= TAIL_Z { 8 } else { 9 },
                );
            }
        }
    }

    // The walkable cabin has a bridge, paired stations, a central aisle, cargo
    // racks, overhead lights, and an enterable aft ramp.
    cells.insert(IVec3::new(0, 1, -12), 10);
    cells.insert(IVec3::new(0, 2, -12), 8);
    for x in [-3, 3] {
        for z in [-8, -3, 3] {
            cells.insert(IVec3::new(x, 1, z), 9);
        }
    }
    for x in [-(HULL_HALF_WIDTH - 1), HULL_HALF_WIDTH - 1] {
        for z in -7..=1 {
            cells.insert(IVec3::new(x, 1, z), 8);
        }
        for z in 4..=9 {
            cells.insert(IVec3::new(x, 1, z), 10);
            cells.insert(IVec3::new(x, 2, z), 7);
        }
    }
    for z in [-9, -4, 1, 6, 11] {
        cells.insert(IVec3::new(0, ROOF_Y, z), 10);
    }

    for x in -2..=2 {
        for y in 1..ROOF_Y {
            for z in -7..=TAIL_Z {
                cells.remove(&IVec3::new(x, y, z));
            }
        }
    }
    for step in 1_i32..=5 {
        let ramp_half_width = (4 - step / 2).max(2);
        for x in -ramp_half_width..=ramp_half_width {
            cells.insert(
                IVec3::new(x, 0, TAIL_Z + step),
                if x.abs() == ramp_half_width { 10 } else { 7 },
            );
        }
    }

    let mut cells = cells.into_iter().collect::<Vec<_>>();
    cells.sort_unstable_by_key(|(cell, _)| (cell.y, cell.z, cell.x));
    cells
}

fn scale_combat_spaceship_micro_tiles(
    tiles: Vec<VoxelMicroTile>,
    occupied: &HashSet<IVec3>,
) -> Vec<VoxelMicroTile> {
    let subdivisions = MICRO_TILE_SUBDIVISIONS as i32;
    let scale = IVec3::splat(ARROGANCE_SCALE);
    let mut scaled = Vec::new();
    for tile in tiles {
        let source_min = tile.cell * subdivisions + tile.min.as_ivec3();
        let source_max = tile.cell * subdivisions + tile.max.as_ivec3();
        let scaled_min = IVec3::new(
            source_min.x * scale.x,
            source_min.y * scale.y,
            source_min.z * scale.z,
        );
        let scaled_max = IVec3::new(
            source_max.x * scale.x,
            source_max.y * scale.y,
            source_max.z * scale.z,
        );
        let min_cell = IVec3::new(
            scaled_min.x.div_euclid(subdivisions),
            scaled_min.y.div_euclid(subdivisions),
            scaled_min.z.div_euclid(subdivisions),
        );
        let max_cell = IVec3::new(
            (scaled_max.x - 1).div_euclid(subdivisions),
            (scaled_max.y - 1).div_euclid(subdivisions),
            (scaled_max.z - 1).div_euclid(subdivisions),
        );
        let owner_origin = scale_combat_spaceship_cell(tile.owner);
        let owner_max = owner_origin + scale - IVec3::ONE;

        for cell_x in min_cell.x..=max_cell.x {
            for cell_y in min_cell.y..=max_cell.y {
                for cell_z in min_cell.z..=max_cell.z {
                    let cell = IVec3::new(cell_x, cell_y, cell_z);
                    let owner = IVec3::new(
                        cell.x.clamp(owner_origin.x, owner_max.x),
                        cell.y.clamp(owner_origin.y, owner_max.y),
                        cell.z.clamp(owner_origin.z, owner_max.z),
                    );
                    if !occupied.contains(&owner) || combat_spaceship_hangar_contains(cell) {
                        continue;
                    }
                    let cell_min = cell * subdivisions;
                    let min = (scaled_min - cell_min)
                        .clamp(IVec3::ZERO, IVec3::splat(subdivisions));
                    let max = (scaled_max - cell_min)
                        .clamp(IVec3::ZERO, IVec3::splat(subdivisions));
                    scaled.push(VoxelMicroTile {
                        owner,
                        cell,
                        min: min.as_uvec3(),
                        max: max.as_uvec3(),
                        material: tile.material,
                        kind: tile.kind,
                    });
                }
            }
        }
    }
    scaled.sort_unstable_by_key(|tile| {
        (
            tile.cell.y,
            tile.cell.z,
            tile.cell.x,
            tile.material,
            tile.min.y,
            tile.min.z,
            tile.min.x,
        )
    });
    scaled.dedup();
    scaled
}

fn scale_combat_spaceship_feature_annotations(
    annotations: VoxelWorkbookFeatureAnnotations,
) -> VoxelWorkbookFeatureAnnotations {
    let regions = annotations
        .regions
        .into_iter()
        .filter_map(|region| {
            let target_anchor = scale_combat_spaceship_cell(region.anchor);
            let mut cells = region
                .cells
                .into_iter()
                .flat_map(|cell| {
                    let origin = scale_combat_spaceship_cell(cell);
                    (0..ARROGANCE_SCALE).flat_map(move |offset_x| {
                        (0..ARROGANCE_SCALE).map(move |offset_z| {
                            origin + IVec3::new(offset_x, 0, offset_z)
                        })
                    })
                })
                .filter(|cell| {
                    !(HANGAR_MIN_X..=HANGAR_MAX_X).contains(&cell.x)
                        || !(HANGAR_REAR_Z..=HANGAR_MOUTH_Z).contains(&cell.z)
                })
                .collect::<Vec<_>>();
            cells.sort_unstable_by_key(|cell| (cell.z, cell.x));
            let anchor = cells
                .iter()
                .copied()
                .min_by_key(|cell| cell.distance_squared(target_anchor))?;
            Some(WorkbookFeatureRegion {
                kind: region.kind,
                cells,
                anchor,
            })
        })
        .collect();
    VoxelWorkbookFeatureAnnotations {
        map_name: annotations.map_name,
        regions,
    }
}

fn default_voxel_spaceship_docking(berth: IVec3) -> VoxelSpaceshipDocked {
    VoxelSpaceshipDocked {
        carrier_id: COMBAT_SPACESHIP_ID.to_owned(),
        local_translation: (berth.as_vec3() * VOXEL_SIZE).to_array(),
        local_rotation: Quat::from_rotation_y(std::f32::consts::PI).to_array(),
    }
}

fn carrier_collision_layers() -> CollisionLayers {
    CollisionLayers::from_bits(CARRIER_COLLISION_LAYER_BITS, u32::MAX)
}

fn docked_voxel_spaceship_collision_layers() -> CollisionLayers {
    CollisionLayers::from_bits(
        DOCKED_SPACESHIP_COLLISION_LAYER_BITS,
        DEFAULT_COLLISION_LAYER_BITS,
    )
}

fn default_docked_voxel_spaceship_transform(berth: IVec3) -> Transform {
    Transform::from_translation(
        (COMBAT_SPACESHIP_CENTER + berth).as_vec3() * VOXEL_SIZE,
    )
    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
}

fn default_voxel_spaceship_specs() -> Vec<VoxelSpaceshipSpec> {
    let arrogance_decoded = ARROGANCE.decode();
    let arrogance_cells = combat_spaceship_voxel_cells();
    let arrogance_occupied = arrogance_cells
        .iter()
        .map(|(cell, _)| *cell)
        .collect::<HashSet<_>>();
    let arrogance_micro_tiles = scale_combat_spaceship_micro_tiles(
        workbook_micro_tiles(ARROGANCE, &arrogance_decoded),
        &arrogance_occupied,
    );
    let arrogance_features = scale_combat_spaceship_feature_annotations(
        workbook_feature_annotations(ARROGANCE, &arrogance_decoded),
    );
    let mut specs = vec![VoxelSpaceshipSpec {
        ship: VoxelSpaceship {
            id: COMBAT_SPACESHIP_ID.to_owned(),
            name: "U.S.I 狂妄号".to_owned(),
            class: VoxelSpaceshipClass::Cruiser,
            cockpit_eye_local: Vec3::new(
                0.0,
                ARROGANCE_CAB_FLOOR_Y as f32 + 3.5,
                ARROGANCE_CAB_FRONT_Z as f32 + 9.5,
            ) * VOXEL_SIZE,
            thrust_acceleration: 2.4,
            vertical_acceleration: 1.4,
            turn_speed: 0.32,
            max_speed: 14.0,
        },
        cells: arrogance_cells,
        micro_tiles: arrogance_micro_tiles,
        workbook_features: Some(arrogance_features),
        transform: Transform::from_translation(
            COMBAT_SPACESHIP_CENTER.as_vec3() * VOXEL_SIZE,
        ),
        docking: None,
    }];
    let medium_berth = IVec3::new(0, HANGAR_PARKING_Y, HANGAR_PARKING_Z);
    specs.push(VoxelSpaceshipSpec {
        ship: VoxelSpaceship {
            id: MEDIUM_SPACESHIP_ID.to_owned(),
            name: "WB-M1 苍鹭号".to_owned(),
            class: VoxelSpaceshipClass::Corvette,
            cockpit_eye_local: Vec3::new(0.5, 3.5, -9.5) * VOXEL_SIZE,
            thrust_acceleration: 5.5,
            vertical_acceleration: 3.8,
            turn_speed: 0.78,
            max_speed: 22.0,
        },
        cells: medium_spaceship_voxel_cells(),
        micro_tiles: Vec::new(),
        workbook_features: None,
        transform: default_docked_voxel_spaceship_transform(medium_berth),
        docking: Some(default_voxel_spaceship_docking(medium_berth)),
    });
    let names = [
        "WB-01 雨燕号",
        "WB-02 萤火号",
        "WB-03 云雀号",
        "WB-04 信风号",
        "WB-05 渡鸦号",
        "WB-06 星槎号",
    ];
    let classes = [
        VoxelSpaceshipClass::Shuttle,
        VoxelSpaceshipClass::Interceptor,
        VoxelSpaceshipClass::Scout,
        VoxelSpaceshipClass::Shuttle,
        VoxelSpaceshipClass::Interceptor,
        VoxelSpaceshipClass::Scout,
    ];
    for index in 0..SMALL_SPACESHIP_COUNT {
        let berth = IVec3::new(
            SMALL_SPACESHIP_BERTH_X[index],
            HANGAR_PARKING_Y,
            HANGAR_PARKING_Z,
        );
        let class = classes[index];
        let (thrust_acceleration, vertical_acceleration, turn_speed, max_speed) = match class {
            VoxelSpaceshipClass::Shuttle => (8.0, 6.0, 1.35, 26.0),
            VoxelSpaceshipClass::Interceptor => (12.0, 8.0, 1.8, 36.0),
            VoxelSpaceshipClass::Scout => (10.0, 7.0, 1.6, 32.0),
            VoxelSpaceshipClass::Cruiser | VoxelSpaceshipClass::Corvette => unreachable!(),
        };
        specs.push(VoxelSpaceshipSpec {
            ship: VoxelSpaceship {
                id: format!("small-ship-{:02}", index + 1),
                name: names[index].to_owned(),
                class,
                cockpit_eye_local: Vec3::new(0.5, 2.8, -1.5) * VOXEL_SIZE,
                thrust_acceleration,
                vertical_acceleration,
                turn_speed,
                max_speed,
            },
            cells: small_spaceship_voxel_cells(index),
            micro_tiles: Vec::new(),
            workbook_features: None,
            transform: default_docked_voxel_spaceship_transform(berth),
            docking: Some(default_voxel_spaceship_docking(berth)),
        });
    }
    specs
}

fn persisted_voxel_spaceship_from_spec(
    spec: &VoxelSpaceshipSpec,
    pilot_user_id: Option<u64>,
) -> PersistedVoxelSpaceship {
    PersistedVoxelSpaceship {
        id: spec.ship.id.clone(),
        pilot_user_id,
        translation: spec.transform.translation.to_array(),
        rotation: spec.transform.rotation.to_array(),
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
        docking: spec.docking.clone(),
    }
}

fn finite_array<const N: usize>(values: [f32; N]) -> bool {
    values.into_iter().all(f32::is_finite)
}

impl VoxelSpaceshipDocked {
    fn local_transform(&self) -> Option<Transform> {
        if !finite_array(self.local_translation) || !finite_array(self.local_rotation) {
            return None;
        }
        let rotation = Quat::from_array(self.local_rotation);
        (rotation.length_squared() > f32::EPSILON).then(|| Transform {
            translation: Vec3::from_array(self.local_translation),
            rotation: rotation.normalize(),
            scale: Vec3::ONE,
        })
    }
}

fn docked_voxel_spaceship_world_transform(
    carrier_transform: &Transform,
    docking: &VoxelSpaceshipDocked,
) -> Option<Transform> {
    let local_transform = docking.local_transform()?;
    Some(Transform {
        translation: carrier_transform
            .compute_affine()
            .transform_point3(local_transform.translation),
        rotation: (carrier_transform.rotation * local_transform.rotation).normalize(),
        scale: Vec3::ONE,
    })
}

fn docked_voxel_spaceship_point_velocity(
    carrier_transform: &Transform,
    carrier_linear_velocity: Vec3,
    carrier_angular_velocity: Vec3,
    docked_translation: Vec3,
) -> Vec3 {
    carrier_linear_velocity
        + carrier_angular_velocity.cross(docked_translation - carrier_transform.translation)
}

fn voxel_spaceship_runtime_pose(
    persisted: &PersistedVoxelSpaceship,
    fallback: &VoxelSpaceshipSpec,
) -> (Transform, LinearVelocity, AngularVelocity) {
    let translation = finite_array(persisted.translation)
        .then(|| Vec3::from_array(persisted.translation))
        .unwrap_or(fallback.transform.translation);
    let saved_rotation = Quat::from_array(persisted.rotation);
    let rotation = if finite_array(persisted.rotation)
        && saved_rotation.length_squared() > f32::EPSILON
    {
        saved_rotation.normalize()
    } else {
        fallback.transform.rotation
    };
    let linear_velocity = finite_array(persisted.linear_velocity)
        .then(|| Vec3::from_array(persisted.linear_velocity))
        .unwrap_or(Vec3::ZERO);
    let angular_velocity = finite_array(persisted.angular_velocity)
        .then(|| Vec3::from_array(persisted.angular_velocity))
        .unwrap_or(Vec3::ZERO);
    (
        Transform {
            translation,
            rotation,
            scale: Vec3::ONE,
        },
        LinearVelocity(linear_velocity),
        AngularVelocity(angular_velocity),
    )
}

fn spawn_voxel_spaceship(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &VoxelMaterials,
    spec: &VoxelSpaceshipSpec,
    transform: Transform,
    linear_velocity: LinearVelocity,
    angular_velocity: AngularVelocity,
    docking: Option<VoxelSpaceshipDocked>,
) -> Entity {
    let collider_cells = spec
        .cells
        .iter()
        .filter_map(|(cell, material)| TrpgVoxelConnector::solid(material).then_some(*cell))
        .collect::<Vec<_>>();
    let min = collider_cells
        .iter()
        .copied()
        .reduce(IVec3::min)
        .unwrap_or(IVec3::ZERO);
    let max = collider_cells
        .iter()
        .copied()
        .reduce(IVec3::max)
        .unwrap_or(IVec3::ZERO);
    let local_center =
        (min.as_vec3() + (max - min + IVec3::ONE).as_vec3() * 0.5) * VOXEL_SIZE;
    let (material_meshes, _) = build_voxel_meshes_from_cells(&spec.cells);
    let micro_meshes = build_micro_tile_meshes(&spec.micro_tiles);
    let rigid_body = if docking.is_some() {
        RigidBody::Kinematic
    } else {
        RigidBody::Dynamic
    };
    let entity = commands
        .spawn((
            Name::new(spec.ship.name.clone()),
            spec.ship.clone(),
            VoxelPhysicsBody {
                local_center,
                cells: spec.cells.clone(),
            },
            rigid_body,
            canonical_voxel_collider(&collider_cells),
            GravityScale(0.0),
            Friction::new(0.25),
            Restitution::new(0.05),
            linear_velocity,
            angular_velocity,
            LinearDamping(0.05),
            AngularDamping(1.1),
            transform,
            Visibility::Visible,
        ))
        .with_children(|parent| {
            for (material_id, mesh) in material_meshes {
                parent.spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.handles[material_id as usize - 1].clone()),
                ));
            }
            for (material_id, mesh) in micro_meshes {
                parent.spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.handles[material_id as usize - 1].clone()),
                    VoxelMicroDecoration,
                ));
            }
        })
        .id();
    if let Some(docking) = docking {
        // The docking layer remains visible to spatial ray queries and collides
        // with default-layer players, but not with the carrier or other berths.
        commands
            .entity(entity)
            .insert((docking, docked_voxel_spaceship_collision_layers()));
    } else if spec.ship.id == COMBAT_SPACESHIP_ID {
        commands
            .entity(entity)
            .insert(carrier_collision_layers());
    }
    if let Some(features) = &spec.workbook_features {
        commands.entity(entity).insert(features.clone());
    }
    entity
}

fn spawn_default_voxel_spaceships(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &VoxelMaterials,
    store: &mut Persistent<VoxelSpaceshipStore>,
    reset_poses: bool,
) {
    let specs = default_voxel_spaceship_specs();
    let layout_changed = store.layout_revision != VOXEL_SPACESHIP_LAYOUT_REVISION;
    let reset_poses = reset_poses || layout_changed;
    let mut changed = layout_changed;
    if layout_changed {
        store.layout_revision = VOXEL_SPACESHIP_LAYOUT_REVISION;
    }
    for spec in &specs {
        let persisted = if let Some(existing) = store
            .ships
            .iter_mut()
            .find(|ship| ship.id == spec.ship.id)
        {
            if reset_poses {
                let pilot_user_id = existing.pilot_user_id;
                *existing = persisted_voxel_spaceship_from_spec(spec, pilot_user_id);
                changed = true;
            }
            existing.clone()
        } else {
            let persisted = persisted_voxel_spaceship_from_spec(spec, None);
            store.ships.push(persisted.clone());
            changed = true;
            persisted
        };
        let (transform, linear_velocity, angular_velocity) =
            voxel_spaceship_runtime_pose(&persisted, spec);
        spawn_voxel_spaceship(
            commands,
            meshes,
            materials,
            spec,
            transform,
            linear_velocity,
            angular_velocity,
            persisted.docking.clone(),
        );
    }
    if changed {
        if let Err(err) = store.persist() {
            eprintln!("failed to persist default voxel spaceships: {err}");
        }
    }
}

fn setup_voxel_spaceships(
    mut commands: Commands,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
    mut store: ResMut<Persistent<VoxelSpaceshipStore>>,
) {
    if let Ok(mut grid) = grids.single_mut() {
        remove_static_combat_spaceship(&mut grid);
    }
    spawn_default_voxel_spaceships(
        &mut commands,
        &mut meshes,
        &materials,
        &mut store,
        false,
    );
}

fn voxel_spaceship_driver_authorized(
    pilot_user_id: Option<u64>,
    active_user_id: Option<u64>,
) -> bool {
    match pilot_user_id {
        Some(pilot_user_id) => active_user_id == Some(pilot_user_id),
        None => active_user_id.is_none(),
    }
}

fn begin_voxel_spaceship_takeover(
    ship_id: &str,
    pilot_user_id: Option<u64>,
    editor: &mut VoxelEditorState,
    possession: &mut VoxelPossessionState,
    control: &mut VoxelSpaceshipControlState,
) {
    control.driving_ship_id = Some(ship_id.to_owned());
    control.selected_ship_id = Some(ship_id.to_owned());
    control.exit_pending = false;
    if let Some(user_id) = pilot_user_id {
        possession.possess(user_id);
    } else {
        possession.release();
    }
    editor.camera_yaw = 0.0;
    editor.camera_pitch = 0.0;
    editor.first_person_enabled = true;
    editor.creative_inventory_open = false;
    editor.teleport_menu_open = false;
    editor.first_person_cursor_released = true;
}

fn release_controlled_docked_spaceship(
    mut commands: Commands,
    control: Res<VoxelSpaceshipControlState>,
    carriers: Query<
        (
            &VoxelSpaceship,
            &Transform,
            &LinearVelocity,
            &AngularVelocity,
        ),
        Without<VoxelSpaceshipDocked>,
    >,
    mut docked_spaceships: Query<
        (
            Entity,
            &VoxelSpaceship,
            &mut Transform,
            &VoxelSpaceshipDocked,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        With<VoxelSpaceshipDocked>,
    >,
) {
    let Some(driving_ship_id) = control.driving_ship_id.as_deref() else {
        return;
    };
    let Some((entity, _, mut transform, docking, mut linear, mut angular)) =
        docked_spaceships
            .iter_mut()
            .find(|(_, ship, ..)| ship.id == driving_ship_id)
    else {
        return;
    };
    let Some((_, carrier_transform, carrier_linear, carrier_angular)) = carriers
        .iter()
        .find(|(carrier, ..)| carrier.id == docking.carrier_id)
    else {
        return;
    };
    let Some(world_transform) =
        docked_voxel_spaceship_world_transform(carrier_transform, docking)
    else {
        return;
    };

    linear.0 = docked_voxel_spaceship_point_velocity(
        carrier_transform,
        carrier_linear.0,
        carrier_angular.0,
        world_transform.translation,
    );
    angular.0 = carrier_angular.0;
    *transform = world_transform;
    commands
        .entity(entity)
        .insert(RigidBody::Dynamic)
        .remove::<VoxelSpaceshipDocked>()
        .remove::<CollisionLayers>();
}

fn sync_docked_voxel_spaceships(
    control: Res<VoxelSpaceshipControlState>,
    carriers: Query<
        (
            &VoxelSpaceship,
            &Transform,
            &LinearVelocity,
            &AngularVelocity,
        ),
        Without<VoxelSpaceshipDocked>,
    >,
    mut docked_spaceships: Query<
        (
            &VoxelSpaceship,
            &mut Transform,
            &VoxelSpaceshipDocked,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        With<VoxelSpaceshipDocked>,
    >,
) {
    for (ship, mut transform, docking, mut linear, mut angular) in &mut docked_spaceships {
        // Removal is deferred until the release system finishes. Do not snap a
        // craft selected for launch back into its berth in that same frame.
        if control.driving_ship_id.as_deref() == Some(ship.id.as_str()) {
            continue;
        }
        let Some((_, carrier_transform, carrier_linear, carrier_angular)) = carriers
            .iter()
            .find(|(carrier, ..)| carrier.id == docking.carrier_id)
        else {
            continue;
        };
        let Some(world_transform) =
            docked_voxel_spaceship_world_transform(carrier_transform, docking)
        else {
            continue;
        };
        linear.0 = docked_voxel_spaceship_point_velocity(
            carrier_transform,
            carrier_linear.0,
            carrier_angular.0,
            world_transform.translation,
        );
        angular.0 = carrier_angular.0;
        *transform = world_transform;
    }
}

fn voxel_spaceship_inside_hangar_docking_zone(local_translation: Vec3) -> bool {
    if !local_translation.is_finite() {
        return false;
    }
    let local_cell = local_translation / VOXEL_SIZE;
    local_cell.x >= HANGAR_MIN_X as f32 + HANGAR_DOCK_CLEARANCE_CELLS
        && local_cell.x <= HANGAR_MAX_X as f32 - HANGAR_DOCK_CLEARANCE_CELLS
        && (HANGAR_PARKING_Y as f32 - 0.5..=HANGAR_PARKING_Y as f32 + 3.0)
            .contains(&local_cell.y)
        && local_cell.z >= HANGAR_REAR_Z as f32 + HANGAR_DOCK_CLEARANCE_CELLS
        && local_cell.z <= HANGAR_MOUTH_Z as f32 - HANGAR_DOCK_CLEARANCE_CELLS
}

fn dock_idle_voxel_spaceships(
    mut commands: Commands,
    control: Res<VoxelSpaceshipControlState>,
    mut spaceships: ParamSet<(
        Query<
            (
                &VoxelSpaceship,
                &Transform,
                &LinearVelocity,
                &AngularVelocity,
            ),
            Without<VoxelSpaceshipDocked>,
        >,
        Query<
            (
                Entity,
                &VoxelSpaceship,
                &Transform,
                &mut LinearVelocity,
                &mut AngularVelocity,
            ),
            Without<VoxelSpaceshipDocked>,
        >,
    )>,
) {
    let carrier_state = spaceships
        .p0()
        .iter()
        .find(|(ship, ..)| ship.id == COMBAT_SPACESHIP_ID)
        .map(|(_, transform, linear, angular)| (*transform, linear.0, angular.0));
    let Some((carrier_transform, carrier_linear, carrier_angular)) = carrier_state else {
        return;
    };
    let inverse_carrier = carrier_transform.compute_affine().inverse();

    for (entity, ship, transform, mut linear, mut angular) in spaceships.p1().iter_mut() {
        if ship.id == COMBAT_SPACESHIP_ID
            || control.driving_ship_id.as_deref() == Some(ship.id.as_str())
        {
            continue;
        }
        let local_translation = inverse_carrier.transform_point3(transform.translation);
        if !voxel_spaceship_inside_hangar_docking_zone(local_translation) {
            continue;
        }
        let carrier_point_velocity = docked_voxel_spaceship_point_velocity(
            &carrier_transform,
            carrier_linear,
            carrier_angular,
            transform.translation,
        );
        if (linear.0 - carrier_point_velocity).length_squared()
            > HANGAR_DOCK_MAX_RELATIVE_SPEED.powi(2)
            || (angular.0 - carrier_angular).length_squared()
                > HANGAR_DOCK_MAX_RELATIVE_ANGULAR_SPEED.powi(2)
        {
            continue;
        }
        let local_rotation =
            (carrier_transform.rotation.inverse() * transform.rotation).normalize();
        let docking = VoxelSpaceshipDocked {
            carrier_id: COMBAT_SPACESHIP_ID.to_owned(),
            local_translation: local_translation.to_array(),
            local_rotation: local_rotation.to_array(),
        };
        linear.0 = carrier_point_velocity;
        angular.0 = carrier_angular;
        commands
            .entity(entity)
            .insert((
                RigidBody::Kinematic,
                docking,
                docked_voxel_spaceship_collision_layers(),
            ));
    }
}

fn control_voxel_spaceships(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    mut editor: ResMut<VoxelEditorState>,
    mut possession: ResMut<VoxelPossessionState>,
    store: Res<Persistent<VoxelSpaceshipStore>>,
    mut control: ResMut<VoxelSpaceshipControlState>,
    mut spaceships: Query<
        (
            &VoxelSpaceship,
            &Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        (
            With<VoxelSpaceship>,
            Without<VoxelFirstPersonPlayer>,
            Without<VoxelViewportCamera>,
        ),
    >,
) {
    control.cockpit_eye = None;
    control.thrust_input = 0.0;
    control.vertical_input = 0.0;
    control.boost_active = false;
    control.brake_active = false;
    let Some(driving_ship_id) = control.driving_ship_id.clone() else {
        if control.exit_pending {
            possession.applied_user_id = None;
            editor.first_person_enabled = true;
            editor.first_person_flying = possession.active_user_id.is_none();
            control.exit_pending = false;
        }
        return;
    };
    let pilot_user_id = store
        .ships
        .iter()
        .find(|ship| ship.id == driving_ship_id)
        .and_then(|ship| ship.pilot_user_id);
    if !voxel_spaceship_driver_authorized(pilot_user_id, possession.active_user_id) {
        control.stop_driving();
        possession.applied_user_id = None;
        return;
    }
    let Some((ship, transform, mut linear_velocity, mut angular_velocity)) = spaceships
        .iter_mut()
        .find(|(ship, ..)| ship.id == driving_ship_id)
    else {
        control.stop_driving();
        possession.applied_user_id = None;
        return;
    };
    if keyboard.just_pressed(KeyCode::KeyF) && !egui_input.wants_any_keyboard_input() {
        control.stop_driving();
        possession.applied_user_id = None;
        return;
    }

    possession.applied_user_id = None;
    control.cockpit_eye = Some(
        transform
            .compute_affine()
            .transform_point3(ship.cockpit_eye_local),
    );
    control.cockpit_rotation = transform.rotation;
    if control.emergency_stop_requested {
        linear_velocity.0 = Vec3::ZERO;
        angular_velocity.0 = Vec3::ZERO;
        control.emergency_stop_requested = false;
    }

    let input_blocked = egui_input.wants_any_keyboard_input()
        || editor.creative_inventory_open
        || editor.teleport_menu_open
        || possession.player_inventory_open
        || editor.first_person_cursor_released;
    if input_blocked {
        return;
    }
    let thrust_input =
        keyboard.pressed(KeyCode::KeyW) as i8 - keyboard.pressed(KeyCode::KeyS) as i8;
    let vertical_input = keyboard.pressed(KeyCode::Space) as i8
        - (keyboard.pressed(KeyCode::ControlLeft)
            || keyboard.pressed(KeyCode::ControlRight)) as i8;
    let yaw_input =
        keyboard.pressed(KeyCode::KeyA) as i8 - keyboard.pressed(KeyCode::KeyD) as i8;
    let pitch_input = keyboard.pressed(KeyCode::ArrowDown) as i8
        - keyboard.pressed(KeyCode::ArrowUp) as i8;
    let roll_input =
        keyboard.pressed(KeyCode::KeyQ) as i8 - keyboard.pressed(KeyCode::KeyE) as i8;
    let boost = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let braking = keyboard.pressed(KeyCode::KeyX);
    let delta_seconds = time.delta_secs().clamp(0.0, 0.05);
    let boost_scale = if boost { 1.75 } else { 1.0 };
    let forward = transform.rotation * Vec3::NEG_Z;
    let up = transform.rotation * Vec3::Y;
    linear_velocity.0 += (
        forward * thrust_input as f32 * ship.thrust_acceleration
            + up * vertical_input as f32 * ship.vertical_acceleration
    ) * boost_scale
        * delta_seconds;
    if braking {
        let response = (-7.0 * delta_seconds).exp();
        linear_velocity.0 *= response;
        angular_velocity.0 *= response;
    }
    linear_velocity.0 = linear_velocity
        .0
        .clamp_length_max(ship.max_speed * boost_scale);

    let local_angular_target = Vec3::new(
        pitch_input as f32,
        yaw_input as f32,
        roll_input as f32,
    ) * ship.turn_speed
        * boost_scale;
    let world_angular_target = transform.rotation * local_angular_target;
    let angular_response = 1.0 - (-5.0 * delta_seconds).exp();
    angular_velocity.0 = angular_velocity
        .0
        .lerp(world_angular_target, angular_response);

    control.thrust_input = thrust_input as f32;
    control.vertical_input = vertical_input as f32;
    control.boost_active = boost;
    control.brake_active = braking;
}

fn persist_voxel_spaceships(
    time: Res<Time>,
    mut app_exit: MessageReader<AppExit>,
    mut persistence: ResMut<VoxelSpaceshipPersistenceState>,
    spaceships: Query<(
        &VoxelSpaceship,
        &Transform,
        &LinearVelocity,
        &AngularVelocity,
        Option<&VoxelSpaceshipDocked>,
    )>,
    mut store: ResMut<Persistent<VoxelSpaceshipStore>>,
) {
    persistence.elapsed_seconds += time.delta_secs();
    let exiting = app_exit.read().next().is_some();
    if !exiting && persistence.elapsed_seconds < VOXEL_SPACESHIP_SAVE_SECONDS {
        return;
    }
    persistence.elapsed_seconds = 0.0;
    let mut changed = false;
    for (ship, transform, linear_velocity, angular_velocity, docking) in &spaceships {
        let Some(record) = store.ships.iter_mut().find(|record| record.id == ship.id) else {
            continue;
        };
        let snapshot = PersistedVoxelSpaceship {
            id: ship.id.clone(),
            pilot_user_id: record.pilot_user_id,
            translation: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
            linear_velocity: linear_velocity.0.to_array(),
            angular_velocity: angular_velocity.0.to_array(),
            docking: docking.cloned(),
        };
        if *record != snapshot {
            *record = snapshot;
            changed = true;
        }
    }
    if changed {
        if let Err(err) = store.persist() {
            eprintln!("failed to persist voxel spaceship state: {err}");
        }
    }
}

fn animate_voxel_auto_doors(
    time: Res<Time>,
    editor: Res<VoxelEditorState>,
    players: Query<
        &Transform,
        (
            With<VoxelFirstPersonPlayer>,
            Without<VoxelAutoDoor>,
        ),
    >,
    mut doors: Query<
        (&mut VoxelAutoDoor, &mut Transform),
        (
            With<VoxelAutoDoor>,
            Without<VoxelFirstPersonPlayer>,
        ),
    >,
) {
    let Ok(player) = players.single() else {
        return;
    };
    let response = 1.0 - (-14.0 * time.delta_secs()).exp();
    for (mut door, mut transform) in &mut doors {
        let should_open =
            editor.first_person_enabled && voxel_auto_door_should_open(&door, player.translation);
        let target = if should_open { door.open_translation } else { door.closed_translation };
        transform.translation = transform.translation.lerp(target, response);
        if transform.translation.distance_squared(target) < 0.000_001 {
            transform.translation = target;
        }
        door.open = should_open;
    }
}

fn setup_voxel_view(
    mut commands: Commands,
    mut gizmo_config: ResMut<GizmoConfigStore>,
    editor: Res<VoxelEditorState>,
    radiance_volume: Res<VoxelRadianceVolume>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
    voxel_materials: Res<VoxelMaterials>,
) {
    for (_, config, _) in gizmo_config.iter_mut() {
        config.render_layers = RenderLayers::layer(VOXEL_DM_GIZMO_RENDER_LAYER);
    }
    commands.spawn((
        DirectionalLight {
            illuminance: DEFAULT_KEY_LIGHT_ILLUMINANCE,
            shadow_maps_enabled: true,
            contact_shadows_enabled: true,
            ..default()
        },
        VoxelKeyLight,
        Transform::from_xyz(8.0, 16.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.5, 0.65, 1.0),
            illuminance: DEFAULT_FILL_LIGHT_ILLUMINANCE,
            shadow_maps_enabled: false,
            ..default()
        },
        VoxelFillLight,
        Transform::from_xyz(-12.0, 8.0, -18.0).looking_at(Vec3::new(0.0, 2.0, 30.0), Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        DepthPrepass,
        // Voxel GI and screen-space contact lighting sample the viewport depth.
        // Keep this camera single-sampled so that depth is available as a normal
        // texture instead of an unresolved multisampled attachment.
        Msaa::Off,
        TemporalAntiAliasing::default(),
        ScreenSpaceAmbientOcclusion {
            quality_level: ScreenSpaceAmbientOcclusionQualityLevel::High,
            constant_object_thickness: VOXEL_SIZE,
        },
        ContactShadows {
            linear_steps: 16,
            thickness: VOXEL_SIZE * 0.4,
            length: VOXEL_SIZE * 6.0,
        },
        Bloom {
            intensity: 0.08,
            ..Bloom::OLD_SCHOOL
        },
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.055, 0.065, 0.075)),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            far: 2_500.0,
            ..default()
        }),
        editor_camera_transform(&editor),
        RenderLayers::from_layers(&[0, VOXEL_DM_GIZMO_RENDER_LAYER]),
        VoxelViewportCamera,
        // Render egui through this camera's target so the 3D clear pass resets
        // the UI every frame before egui is composited after post-processing.
        PrimaryEguiContext,
        VoxelRadianceCascade {
            volume: radiance_volume.image.clone(),
        },
        radiance_volume.uniform(editor.radiance_intensity),
    ));
    spawn_voxel_orbital_planet(
        &mut commands,
        &mut meshes,
        &voxel_materials,
    );
    spawn_planet_clouds(
        &mut commands,
        &mut meshes,
        &mut standard_materials,
    );
    let player_collider = Collider::capsule(
        FIRST_PERSON_RADIUS,
        FIRST_PERSON_BODY_LENGTH,
    );
    let mut ground_shape = player_collider.clone();
    ground_shape.set_scale(Vec3::splat(0.99), 10);
    commands.spawn((
        VoxelFirstPersonPlayer,
        RigidBody::Dynamic,
        player_collider,
        ShapeCaster::new(
            ground_shape,
            Vec3::ZERO,
            Quat::IDENTITY,
            Dir3::NEG_Y,
        )
        .with_max_distance(0.015),
        LockedAxes::ROTATION_LOCKED,
        Friction::ZERO.with_combine_rule(CoefficientCombine::Min),
        Restitution::ZERO.with_combine_rule(CoefficientCombine::Min),
        ConstantLinearAcceleration::new(0.0, -9.81, 0.0),
        Transform::from_translation(FIRST_PERSON_START),
    ));
}

fn setup_voxel_player_cameras(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    store: Res<Persistent<VoxelPlayerCameraStore>>,
    mut runtimes: ResMut<VoxelPlayerCameraRuntimes>,
) {
    for camera in &store.cameras {
        spawn_voxel_player_camera(
            &mut commands,
            &mut images,
            &mut runtimes,
            camera.user_id,
            voxel_player_camera_transform(camera),
        );
    }
}

fn sync_voxel_player_cameras(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    requests: Res<SceneCaptureRequests>,
    viewport_camera: Query<
        &Transform,
        (
            With<VoxelViewportCamera>,
            Without<VoxelPlayerCaptureCamera>,
        ),
    >,
    mut runtimes: ResMut<VoxelPlayerCameraRuntimes>,
    mut store: ResMut<Persistent<VoxelPlayerCameraStore>>,
) {
    let mut user_ids = requests
        .requests
        .iter()
        .filter(|request| voxel_observation_player_allowed(manager.as_deref(), request.user_id))
        .map(|request| request.user_id)
        .collect::<HashSet<_>>();
    if let Some(manager) = manager.as_deref() {
        user_ids.extend(completed_voxel_player_user_ids(
            &manager.player_characters,
        ));
        if let Some(group) = manager.current_group() {
            user_ids.extend(
                group
                    .players
                    .iter()
                    .filter_map(|target_id| target_id.parse::<u64>().ok()),
            );
        }
    }
    let default_transform = viewport_camera
        .single()
        .copied()
        .unwrap_or_else(|_| Transform::from_translation(FIRST_PERSON_START));
    let mut changed = false;
    for user_id in user_ids {
        if runtimes.cameras.contains_key(&user_id) {
            continue;
        }
        let transform = initial_voxel_player_camera_transform(&store, user_id, default_transform);
        spawn_voxel_player_camera(
            &mut commands,
            &mut images,
            &mut runtimes,
            user_id,
            transform,
        );
        if persisted_voxel_player_camera(&store, user_id).is_none() {
            upsert_voxel_player_camera(&mut store, user_id, &transform);
            changed = true;
        }
    }
    if changed {
        if let Err(err) = store.persist() {
            eprintln!("failed to persist voxel player cameras: {err}");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HoveredWorkbookFeature {
    kind: WorkbookFeatureKind,
    map_name: &'static str,
    distance: f32,
}

#[derive(Clone, Copy)]
struct ProjectedWorkbookFeatureLabel {
    kind: WorkbookFeatureKind,
    position: egui::Pos2,
    depth: f32,
}

fn workbook_feature_anchor_local(region: &WorkbookFeatureRegion) -> Vec3 {
    (region.anchor.as_vec3()
        + Vec3::new(
            0.5,
            WORKBOOK_FEATURE_LABEL_Y_CELLS,
            0.5,
        ))
        * VOXEL_SIZE
}

fn raycast_workbook_feature_region(
    ray: Ray3d,
    local_to_world: Affine3A,
    region: &WorkbookFeatureRegion,
) -> Option<f32> {
    let world_to_local = local_to_world.inverse();
    let local_origin = world_to_local.transform_point3(ray.origin);
    let local_direction = world_to_local.transform_vector3(*ray.direction);
    if !local_origin.is_finite()
        || !local_direction.is_finite()
        || local_direction.length_squared() <= f32::EPSILON
    {
        return None;
    }
    let local_direction = local_direction.normalize();
    region
        .cells
        .iter()
        .filter_map(|cell| {
            let min = (cell.as_vec3()
                + Vec3::new(
                    0.0,
                    WORKBOOK_FEATURE_HOVER_MIN_Y_CELLS,
                    0.0,
                ))
                * VOXEL_SIZE;
            let max = (cell.as_vec3()
                + Vec3::new(
                    1.0,
                    WORKBOOK_FEATURE_HOVER_MAX_Y_CELLS,
                    1.0,
                ))
                * VOXEL_SIZE;
            let (near, _) = ray_aabb_distance_range(
                local_origin,
                local_direction,
                min,
                max,
                MAX_RAY_DISTANCE,
            )?;
            let local_hit = local_origin + local_direction * near;
            let world_hit = local_to_world.transform_point3(local_hit);
            Some(ray.origin.distance(world_hit))
        })
        .min_by(f32::total_cmp)
}

fn workbook_feature_pointer_position(
    window: &Window,
    editor: &VoxelEditorState,
    pixels_per_point: f32,
) -> Option<Vec2> {
    let pixels_per_point = pixels_per_point.max(f32::EPSILON);
    let viewport_min = editor.viewport_min / pixels_per_point;
    let viewport_max = editor.viewport_max / pixels_per_point;
    let contains = |position: Vec2| {
        position.cmpge(viewport_min).all() && position.cmple(viewport_max).all()
    };
    if editor.first_person_enabled && !editor.first_person_cursor_released {
        Some((viewport_min + viewport_max) * 0.5)
    } else {
        window
            .cursor_position()
            .filter(|position| contains(*position))
    }
}

fn workbook_feature_label_color(kind: WorkbookFeatureKind) -> egui::Color32 {
    match kind {
        WorkbookFeatureKind::EnergyPlatform
        | WorkbookFeatureKind::Teleporter
        | WorkbookFeatureKind::RadarConsole => egui::Color32::from_rgb(96, 225, 255),
        WorkbookFeatureKind::MedicalAnalyzer
        | WorkbookFeatureKind::ExperimentBench => egui::Color32::from_rgb(145, 240, 190),
        WorkbookFeatureKind::ThermiteFactory => egui::Color32::from_rgb(255, 156, 92),
        WorkbookFeatureKind::EscapePod
        | WorkbookFeatureKind::ArmorLocker
        | WorkbookFeatureKind::CargoRack => egui::Color32::from_rgb(255, 208, 112),
        _ => egui::Color32::from_rgb(210, 224, 244),
    }
}

fn workbook_feature_label_in_range(camera_position: Vec3, world_anchor: Vec3) -> bool {
    let distance_squared = camera_position.distance_squared(world_anchor);
    distance_squared.is_finite()
        && distance_squared <= WORKBOOK_FEATURE_LABEL_VISIBILITY_RADIUS_METERS.powi(2)
}

fn project_workbook_feature_label(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    local_to_world: Affine3A,
    region: &WorkbookFeatureRegion,
    editor: &VoxelEditorState,
    pixels_per_point: f32,
) -> Option<ProjectedWorkbookFeatureLabel> {
    let world_anchor = local_to_world.transform_point3(workbook_feature_anchor_local(region));
    if !workbook_feature_label_in_range(camera_transform.translation(), world_anchor) {
        return None;
    }
    let projected = camera
        .world_to_viewport_with_depth(camera_transform, world_anchor)
        .ok()?;
    let screen = projected.truncate();
    let pixels_per_point = pixels_per_point.max(f32::EPSILON);
    let viewport_min = editor.viewport_min / pixels_per_point;
    let viewport_max = editor.viewport_max / pixels_per_point;
    if !screen.cmpge(viewport_min).all() || !screen.cmple(viewport_max).all() {
        return None;
    }
    Some(ProjectedWorkbookFeatureLabel {
        kind: region.kind,
        position: egui::pos2(screen.x, screen.y),
        depth: projected.z,
    })
}

fn paint_workbook_map_label(
    painter: &egui::Painter,
    label: ProjectedWorkbookFeatureLabel,
) {
    let color = workbook_feature_label_color(label.kind);
    let galley = painter.layout_no_wrap(
        label.kind.label().to_owned(),
        egui::FontId::proportional(13.0),
        color,
    );
    let padding = egui::vec2(6.0, 3.0);
    let label_center = label.position - egui::vec2(0.0, galley.size().y * 0.5 + 8.0);
    let rect = egui::Rect::from_center_size(label_center, galley.size() + padding * 2.0);
    painter.line_segment(
        [label.position, rect.center_bottom()],
        egui::Stroke::new(1.0, color.gamma_multiply(0.8)),
    );
    painter.circle_filled(label.position, 2.0, color);
    painter.rect_filled(
        rect,
        4.0,
        egui::Color32::from_rgba_unmultiplied(5, 12, 20, 210),
    );
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, color.gamma_multiply(0.72)),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + padding, galley, color);
}

fn draw_workbook_feature_hover_hud(
    ctx: &egui::Context,
    pointer: Vec2,
    hovered: HoveredWorkbookFeature,
) {
    let screen = ctx.content_rect();
    let size = egui::vec2(205.0, 54.0);
    let mut position = egui::pos2(pointer.x + 16.0, pointer.y + 18.0);
    if position.x + size.x > screen.right() {
        position.x = pointer.x - size.x - 16.0;
    }
    if position.y + size.y > screen.bottom() {
        position.y = pointer.y - size.y - 18.0;
    }
    let accent = workbook_feature_label_color(hovered.kind);
    egui::Area::new(egui::Id::new("voxel_workbook_feature_hover"))
        .fixed_pos(position)
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(4, 11, 18, 238))
                .stroke(egui::Stroke::new(1.5, accent))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.colored_label(accent, hovered.kind.label());
                    ui.small(format!("位置：{}", hovered.map_name));
                });
        });
}

fn voxel_workbook_feature_overlay(
    mut contexts: EguiContexts,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<
        (&Camera, &GlobalTransform),
        (
            With<VoxelViewportCamera>,
            Without<VoxelPlayerCaptureCamera>,
        ),
    >,
    editor: Res<VoxelEditorState>,
    static_annotations: Res<StaticWorkbookFeatureAnnotations>,
    moving_annotations: Query<(&GlobalTransform, &VoxelWorkbookFeatureAnnotations)>,
    egui_input: Res<EguiWantsInput>,
) {
    let (Ok(ctx), Ok(window), Ok((camera, camera_transform))) = (
        contexts.ctx_mut(),
        windows.single(),
        cameras.single(),
    ) else {
        return;
    };

    let mut labels = Vec::new();
    let pixels_per_point = ctx.pixels_per_point();
    for entry in &static_annotations.entries {
        let local_to_world = Affine3A::from_translation(entry.center.as_vec3() * VOXEL_SIZE);
        if let Some(label) = project_workbook_feature_label(
            camera,
            camera_transform,
            local_to_world,
            &entry.region,
            &editor,
            pixels_per_point,
        ) {
            labels.push(label);
        }
    }
    for (transform, annotations) in &moving_annotations {
        for region in &annotations.regions {
            if let Some(label) = project_workbook_feature_label(
                camera,
                camera_transform,
                transform.affine(),
                region,
                &editor,
                pixels_per_point,
            ) {
                labels.push(label);
            }
        }
    }
    // Paint distant annotations first so a nearby room label remains readable.
    labels.sort_by(|left, right| right.depth.total_cmp(&left.depth));
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("voxel_workbook_map_labels"),
    ));
    for label in labels {
        paint_workbook_map_label(&painter, label);
    }

    if egui_input.wants_any_pointer_input() {
        return;
    }
    let Some(pointer) = workbook_feature_pointer_position(window, &editor, pixels_per_point) else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, pointer) else {
        return;
    };
    let mut hovered = None::<HoveredWorkbookFeature>;
    let mut consider = |kind, map_name, distance| {
        if hovered.is_none_or(|current| distance < current.distance) {
            hovered = Some(HoveredWorkbookFeature {
                kind,
                map_name,
                distance,
            });
        }
    };
    for entry in &static_annotations.entries {
        let local_to_world = Affine3A::from_translation(entry.center.as_vec3() * VOXEL_SIZE);
        if let Some(distance) = raycast_workbook_feature_region(
            ray,
            local_to_world,
            &entry.region,
        ) {
            consider(entry.region.kind, entry.map_name, distance);
        }
    }
    for (transform, annotations) in &moving_annotations {
        for region in &annotations.regions {
            if let Some(distance) = raycast_workbook_feature_region(
                ray,
                transform.affine(),
                region,
            ) {
                consider(region.kind, annotations.map_name, distance);
            }
        }
    }
    if let Some(hovered) = hovered {
        draw_workbook_feature_hover_hud(ctx, pointer, hovered);
    }
}

fn voxel_player_camera_panel(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut images: ResMut<Assets<Image>>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    mut editor: ResMut<VoxelPlayerCameraEditor>,
    mut voxel_editor: ResMut<VoxelEditorState>,
    mut possession: ResMut<VoxelPossessionState>,
    mut runtimes: ResMut<VoxelPlayerCameraRuntimes>,
    mut store: ResMut<Persistent<VoxelPlayerCameraStore>>,
    viewport_camera: Query<
        &Transform,
        (
            With<VoxelViewportCamera>,
            Without<VoxelPlayerCaptureCamera>,
        ),
    >,
    mut capture_cameras: Query<
        (
            &VoxelPlayerCaptureCamera,
            &mut Transform,
        ),
        (
            With<VoxelPlayerCaptureCamera>,
            Without<VoxelViewportCamera>,
        ),
    >,
    mut first_person_players: Query<
        (&mut Transform, &mut LinearVelocity),
        (
            With<VoxelFirstPersonPlayer>,
            Without<VoxelViewportCamera>,
            Without<VoxelPlayerCaptureCamera>,
        ),
    >,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let mut user_ids = runtimes.cameras.keys().copied().collect::<Vec<_>>();
    user_ids.sort_unstable();
    if editor
        .selected_user_id
        .is_none_or(|selected| !runtimes.cameras.contains_key(&selected))
    {
        editor.selected_user_id = user_ids.first().copied();
    }

    egui::Window::new("玩家观察相机")
        .default_pos(egui::pos2(12.0, 270.0))
        .default_width(300.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.small("玩家发送 .观察 后，将私聊收到此相机的第一人称画面。");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut editor.new_user_id).hint_text("玩家QQ号"));
                let new_user_id = editor.new_user_id.trim().parse::<u64>().ok();
                if ui.button("创建").clicked() {
                    let Some(user_id) = new_user_id else { return };
                    if runtimes.cameras.contains_key(&user_id) {
                        return;
                    }
                    let transform = viewport_camera
                        .single()
                        .copied()
                        .unwrap_or_else(|_| Transform::from_translation(FIRST_PERSON_START));
                    spawn_voxel_player_camera(
                        &mut commands,
                        &mut images,
                        &mut runtimes,
                        user_id,
                        transform,
                    );
                    upsert_voxel_player_camera(&mut store, user_id, &transform);
                    if let Err(err) = store.persist() {
                        eprintln!("failed to persist voxel player camera: {err}");
                    }
                    editor.selected_user_id = Some(user_id);
                    editor.new_user_id.clear();
                }
            });
            if user_ids.is_empty() {
                ui.label("还没有玩家观察相机");
                return;
            }

            let mut selected = editor.selected_user_id.unwrap_or(user_ids[0]);
            egui::ComboBox::from_label("玩家")
                .selected_text(voxel_player_display_name(
                    manager.as_deref(),
                    selected,
                ))
                .show_ui(ui, |ui| {
                    for user_id in &user_ids {
                        ui.selectable_value(
                            &mut selected,
                            *user_id,
                            voxel_player_display_name(manager.as_deref(), *user_id),
                        );
                    }
                });
            editor.selected_user_id = Some(selected);

            ui.separator();
            let possessing_selected = possession.active_user_id == Some(selected);
            ui.horizontal(|ui| {
                if possessing_selected {
                    if ui.button("解除接管").clicked() {
                        possession.release();
                        if let Err(err) = store.persist() {
                            eprintln!("failed to persist released player camera: {err}");
                        }
                    }
                } else if ui.button("GM接管PL").clicked() {
                    if possession.active_user_id.is_some() {
                        if let Err(err) = store.persist() {
                            eprintln!("failed to persist previous possessed player camera: {err}");
                        }
                    }
                    possession.possess(selected);
                }
                if let Some(active) = possession.active_user_id {
                    ui.small(format!(
                        "正在接管：{}",
                        voxel_player_display_name(manager.as_deref(), active)
                    ));
                } else {
                    ui.small("GM创造模式");
                }
            });
            if possessing_selected {
                ui.small(format!(
                    "本轮移动 {:.2}/{:.2}，剩余 {:.2}{}",
                    possession.movement_used,
                    possession.movement_limit,
                    possession.movement_remaining(),
                    if possession.movement_completed {
                        "（已完成并锁定；可由 GM 撤销）"
                    } else if possession.movement_limit_bypassed {
                        "（已允许超限）"
                    } else {
                        ""
                    },
                ));
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            possession.turn_start_position.is_some(),
                            egui::Button::new("撤销本轮移动"),
                        )
                        .on_hover_text("返回本轮开始位置，并把本轮已用移动距离归零")
                        .clicked()
                    {
                        possession.reset_movement_requested = true;
                    }
                    if possession.movement_limit_bypassed {
                        if ui.button("恢复移动上限").clicked() {
                            possession.movement_limit_bypassed = false;
                        }
                    } else if possession.movement_bypass_confirmation_pending {
                        if ui
                            .add(
                                egui::Button::new("再次确认：允许超限移动")
                                    .fill(egui::Color32::from_rgb(150, 35, 35)),
                            )
                            .clicked()
                        {
                            possession.movement_limit_bypassed = true;
                            possession.movement_bypass_confirmation_pending = false;
                        }
                        if ui.button("取消").clicked() {
                            possession.movement_bypass_confirmation_pending = false;
                        }
                    } else if ui
                        .add(
                            egui::Button::new("危险：突破移动上限")
                                .fill(egui::Color32::from_rgb(100, 30, 30)),
                        )
                        .on_hover_text("需要再次确认；只对当前接管和当前回合有效")
                        .clicked()
                    {
                        possession.movement_bypass_confirmation_pending = true;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if let Some(character) = manager
                        .as_deref()
                        .and_then(|manager| manager.player_characters.get(&selected.to_string()))
                    {
                        for (index, slot) in character.inventory.hotbar.iter().enumerate() {
                            let label = voxel_player_hotbar_slot_label(*slot, character);
                            ui.selectable_value(
                                &mut possession.selected_hotbar_slot,
                                index,
                                format!("{} {label}", index + 1),
                            );
                        }
                    }
                });
                ui.small("生存模式：不可飞行或编辑方块；数字键1-9切换物品/主动技能。");
            }

            let Some((_, mut transform)) = capture_cameras
                .iter_mut()
                .find(|(camera, _)| camera.user_id == selected)
            else {
                return;
            };
            let mut changed = false;
            if ui.button("使用当前GM视角").clicked() {
                if let Ok(viewport_transform) = viewport_camera.single() {
                    *transform = *viewport_transform;
                    changed = true;
                }
            }
            if ui.button("使用当前PL视角").clicked() {
                let player_view = *transform;
                if let Ok((mut player_transform, mut velocity)) = first_person_players.single_mut()
                {
                    apply_voxel_player_view_to_dm(
                        &mut voxel_editor,
                        &mut player_transform,
                        &mut velocity,
                        &player_view,
                    );
                }
            }
            ui.label("位置");
            ui.horizontal(|ui| {
                changed |= ui
                    .add(egui::DragValue::new(&mut transform.translation.x).prefix("X "))
                    .changed();
                changed |= ui
                    .add(egui::DragValue::new(&mut transform.translation.y).prefix("Y "))
                    .changed();
                changed |= ui
                    .add(egui::DragValue::new(&mut transform.translation.z).prefix("Z "))
                    .changed();
            });
            let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);
            let (mut yaw, mut pitch, mut roll) = (
                yaw.to_degrees(),
                pitch.to_degrees(),
                roll.to_degrees(),
            );
            ui.label("朝向");
            let rotation_changed = ui
                .horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut yaw).prefix("Y "))
                        .changed()
                        | ui.add(egui::DragValue::new(&mut pitch).prefix("P "))
                            .changed()
                        | ui.add(egui::DragValue::new(&mut roll).prefix("R "))
                            .changed()
                })
                .inner;
            if rotation_changed {
                transform.rotation = Quat::from_euler(
                    EulerRot::YXZ,
                    yaw.to_radians(),
                    pitch.to_radians(),
                    roll.to_radians(),
                );
                changed = true;
            }
            if changed {
                upsert_voxel_player_camera(&mut store, selected, &transform);
                if let Err(err) = store.persist() {
                    eprintln!("failed to persist voxel player camera: {err}");
                }
            }
        });
}

#[derive(Clone)]
struct VoxelSpaceshipTelemetry {
    id: String,
    name: String,
    class: VoxelSpaceshipClass,
    max_speed: f32,
    speed: f32,
    forward_speed: f32,
    angular_speed: f32,
    translation: Vec3,
    rotation: Quat,
}

fn voxel_spaceship_pilot_user_ids(
    manager: Option<&Persistent<NapcatMessageManager>>,
) -> Vec<u64> {
    let mut user_ids = manager
        .and_then(|manager| manager.current_group())
        .into_iter()
        .flat_map(|group| &group.players)
        .filter_map(|target_id| target_id.parse::<u64>().ok())
        .collect::<Vec<_>>();
    user_ids.sort_unstable();
    user_ids.dedup();
    user_ids
}

fn draw_voxel_spaceship_hud(
    ctx: &egui::Context,
    telemetry: &VoxelSpaceshipTelemetry,
    pilot_label: &str,
    control: &VoxelSpaceshipControlState,
) {
    egui::Area::new(egui::Id::new("voxel_spaceship_cockpit_hud"))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 76.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(5, 18, 30, 232))
                .stroke(egui::Stroke::new(
                    1.5,
                    egui::Color32::from_rgb(55, 205, 235),
                ))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(460.0);
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(105, 235, 255),
                            format!("◈ {}", telemetry.name),
                        );
                        ui.separator();
                        ui.small(telemetry.class.label());
                        ui.separator();
                        ui.small(format!("驾驶：{pilot_label}"));
                        if control.boost_active {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 190, 70),
                                "加力",
                            );
                        }
                        if control.brake_active {
                            ui.colored_label(
                                egui::Color32::from_rgb(120, 225, 170),
                                "制动",
                            );
                        }
                    });
                    let displayed_max_speed = telemetry.max_speed
                        * if control.boost_active { 1.75 } else { 1.0 };
                    let speed_fraction =
                        (telemetry.speed / displayed_max_speed.max(f32::EPSILON)).clamp(0.0, 1.0);
                    ui.add(
                        egui::ProgressBar::new(speed_fraction)
                            .desired_width(460.0)
                            .fill(egui::Color32::from_rgb(32, 176, 220))
                            .text(format!(
                                "速度 {:05.1} / {:05.1}  ·  前向 {:+05.1}",
                                telemetry.speed,
                                displayed_max_speed,
                                telemetry.forward_speed,
                            )),
                    );

                    let (yaw, pitch, roll) = telemetry.rotation.to_euler(EulerRot::YXZ);
                    let (horizon_rect, _) = ui.allocate_exact_size(
                        egui::vec2(460.0, 38.0),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter_at(horizon_rect);
                    let center = horizon_rect.center();
                    let half_width = 112.0;
                    let horizon_offset = pitch.sin() * 18.0;
                    let horizon_slope = roll.sin() * 28.0;
                    painter.line_segment(
                        [
                            center + egui::vec2(-half_width, horizon_offset - horizon_slope),
                            center + egui::vec2(half_width, horizon_offset + horizon_slope),
                        ],
                        egui::Stroke::new(
                            1.5,
                            egui::Color32::from_rgb(80, 220, 245),
                        ),
                    );
                    painter.circle_stroke(
                        center,
                        6.0,
                        egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgb(255, 215, 100),
                        ),
                    );
                    painter.text(
                        horizon_rect.left_center() + egui::vec2(4.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("艏向 {:03.0}°", yaw.to_degrees().rem_euclid(360.0)),
                        egui::FontId::monospace(12.0),
                        egui::Color32::LIGHT_GRAY,
                    );
                    painter.text(
                        horizon_rect.right_center() - egui::vec2(4.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        format!("角速 {:.2}", telemetry.angular_speed),
                        egui::FontId::monospace(12.0),
                        egui::Color32::LIGHT_GRAY,
                    );
                    ui.centered_and_justified(|ui| {
                        ui.small(
                            "W/S 推进 · A/D 偏航 · ↑/↓ 俯仰 · Q/E 翻滚 · 空格/Ctrl 升降 · Shift 加力 · X 制动 · F 离舰",
                        );
                    });
                });
        });
}

fn voxel_spaceship_panel(
    mut contexts: EguiContexts,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    mut voxel_editor: ResMut<VoxelEditorState>,
    mut possession: ResMut<VoxelPossessionState>,
    mut control: ResMut<VoxelSpaceshipControlState>,
    mut store: ResMut<Persistent<VoxelSpaceshipStore>>,
    spaceships: Query<(
        &VoxelSpaceship,
        &Transform,
        &LinearVelocity,
        &AngularVelocity,
    )>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let mut telemetry = spaceships
        .iter()
        .map(|(ship, transform, linear_velocity, angular_velocity)| {
            VoxelSpaceshipTelemetry {
                id: ship.id.clone(),
                name: ship.name.clone(),
                class: ship.class,
                max_speed: ship.max_speed,
                speed: linear_velocity.length(),
                forward_speed: linear_velocity.dot(transform.rotation * Vec3::NEG_Z),
                angular_speed: angular_velocity.length(),
                translation: transform.translation,
                rotation: transform.rotation,
            }
        })
        .collect::<Vec<_>>();
    telemetry.sort_by(|left, right| left.id.cmp(&right.id));
    if telemetry.is_empty() {
        control.stop_driving();
        return;
    }
    if control
        .selected_ship_id
        .as_ref()
        .is_none_or(|selected| !telemetry.iter().any(|ship| &ship.id == selected))
    {
        control.selected_ship_id = Some(telemetry[0].id.clone());
    }
    let pilot_user_ids = voxel_spaceship_pilot_user_ids(manager.as_deref());
    let mut assignment_changed = false;

    egui::Window::new("舰船调度台")
        .id(egui::Id::new("voxel_spaceship_control_window"))
        .default_pos(egui::pos2(324.0, 270.0))
        .default_width(310.0)
        .resizable(false)
        .show(ctx, |ui| {
            let mut selected_id = control
                .selected_ship_id
                .clone()
                .unwrap_or_else(|| telemetry[0].id.clone());
            let selected_name = telemetry
                .iter()
                .find(|ship| ship.id == selected_id)
                .map(|ship| ship.name.as_str())
                .unwrap_or("舰船");
            egui::ComboBox::from_label("舰船")
                .selected_text(selected_name)
                .show_ui(ui, |ui| {
                    for ship in &telemetry {
                        ui.selectable_value(
                            &mut selected_id,
                            ship.id.clone(),
                            format!("{} · {}", ship.name, ship.class.label()),
                        );
                    }
                });
            control.selected_ship_id = Some(selected_id.clone());
            let Some(selected) = telemetry.iter().find(|ship| ship.id == selected_id) else {
                return;
            };
            ui.horizontal(|ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(90, 225, 155),
                    "● 动态物理",
                );
                ui.colored_label(
                    egui::Color32::from_rgb(95, 205, 245),
                    "● 零重力",
                );
                ui.small(format!("{} 方块舰体", selected.class.label()));
            });
            ui.small(format!(
                "位置 X {:.1}  Y {:.1}  Z {:.1} · 速度 {:.1}",
                selected.translation.x,
                selected.translation.y,
                selected.translation.z,
                selected.speed,
            ));
            ui.separator();

            let mut pilot_user_id = store
                .ships
                .iter()
                .find(|ship| ship.id == selected.id)
                .and_then(|ship| ship.pilot_user_id);
            let previous_pilot = pilot_user_id;
            let pilot_text = pilot_user_id.map_or_else(
                || "GM / 未分配".to_owned(),
                |user_id| voxel_player_display_name(manager.as_deref(), user_id),
            );
            egui::ComboBox::from_label("驾驶权限")
                .selected_text(pilot_text)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut pilot_user_id,
                        None,
                        "GM / 未分配",
                    );
                    for user_id in &pilot_user_ids {
                        ui.selectable_value(
                            &mut pilot_user_id,
                            Some(*user_id),
                            voxel_player_display_name(manager.as_deref(), *user_id),
                        );
                    }
                });
            if pilot_user_id != previous_pilot {
                if let Some(record) = store
                    .ships
                    .iter_mut()
                    .find(|ship| ship.id == selected.id)
                {
                    record.pilot_user_id = pilot_user_id;
                    assignment_changed = true;
                }
                if control.driving_ship_id.as_deref() == Some(selected.id.as_str())
                    && !voxel_spaceship_driver_authorized(
                        pilot_user_id,
                        possession.active_user_id,
                    )
                {
                    control.stop_driving();
                }
            }
            ui.small("只有 GM 可在此分配；玩家身份按 QQ 数字 ID 校验。");

            let driving_selected =
                control.driving_ship_id.as_deref() == Some(selected.id.as_str());
            ui.horizontal(|ui| {
                if driving_selected {
                    if ui.button("停止驾驶").clicked() {
                        control.stop_driving();
                    }
                } else {
                    let label = if pilot_user_id.is_some() {
                        "以该玩家驾驶"
                    } else {
                        "GM 开始驾驶"
                    };
                    if ui
                        .add(
                            egui::Button::new(label)
                                .fill(egui::Color32::from_rgb(18, 104, 132)),
                        )
                        .clicked()
                    {
                        begin_voxel_spaceship_takeover(
                            &selected.id,
                            pilot_user_id,
                            &mut voxel_editor,
                            &mut possession,
                            &mut control,
                        );
                    }
                }
                if ui
                    .button("紧急制动")
                    .on_hover_text("下一物理帧把线速度和角速度归零")
                    .clicked()
                {
                    control.emergency_stop_requested = true;
                }
            });
            ui.small("开始后点击 3D 视口锁定鼠标；离舰不会改动玩家的角色归属。");
        });

    if assignment_changed {
        if let Err(err) = store.persist() {
            eprintln!("failed to persist voxel spaceship pilot assignment: {err}");
        }
    }
    if let Some(active_ship_id) = control.driving_ship_id.as_deref() {
        if let Some(active) = telemetry.iter().find(|ship| ship.id == active_ship_id) {
            let pilot_user_id = store
                .ships
                .iter()
                .find(|ship| ship.id == active_ship_id)
                .and_then(|ship| ship.pilot_user_id);
            let pilot_label = pilot_user_id.map_or_else(
                || "GM".to_owned(),
                |user_id| voxel_player_display_name(manager.as_deref(), user_id),
            );
            draw_voxel_spaceship_hud(ctx, active, &pilot_label, &control);
        }
    }
}

fn voxel_player_hotbar_slot_label(
    slot: CharacterHotbarSlot,
    character: &crate::napcat::PlayerCharacter,
) -> String {
    match slot {
        CharacterHotbarSlot::Empty => "空".to_owned(),
        CharacterHotbarSlot::ReleaseControl => "解除控制".to_owned(),
        CharacterHotbarSlot::Item(index) => character
            .inventory
            .items
            .get(index)
            .map(|item| {
                let name = item.name.trim();
                if name.is_empty() {
                    "未命名物品".to_owned()
                } else {
                    name.to_owned()
                }
            })
            .unwrap_or_else(|| "空".to_owned()),
        CharacterHotbarSlot::Skill(index) => character
            .skill_names
            .get(index)
            .map(|name| {
                let name = name.trim();
                if name.is_empty() {
                    "未命名技能".to_owned()
                } else {
                    name.to_owned()
                }
            })
            .unwrap_or_else(|| "空".to_owned()),
    }
}

fn apply_voxel_player_view_to_editor(editor: &mut VoxelEditorState, transform: &Transform) {
    let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
    editor.first_person_enabled = true;
    editor.first_person_flying = true;
    editor.camera_yaw = yaw;
    editor.camera_pitch = pitch;
    editor.camera_focus = orbit_focus_preserving_camera_position(
        transform.translation,
        yaw,
        pitch,
        editor.camera_distance,
    );
}

fn apply_voxel_player_view_to_dm(
    editor: &mut VoxelEditorState,
    player_transform: &mut Transform,
    velocity: &mut LinearVelocity,
    player_view: &Transform,
) {
    apply_voxel_player_view_to_editor(editor, player_view);
    player_transform.translation = first_person_player_position(player_view.translation);
    velocity.0 = Vec3::ZERO;
}

fn sync_voxel_player_standees(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: ResMut<VoxelPlayerStandeeAssets>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    existing: Query<(Entity, &VoxelPlayerStandee)>,
    capture_cameras: Query<
        (&Transform, &VoxelPlayerCaptureCamera),
        (
            With<VoxelPlayerCaptureCamera>,
            Without<VoxelPlayerStandee>,
        ),
    >,
    mut standee_transforms: Query<
        &mut Transform,
        (
            With<VoxelPlayerStandee>,
            Without<VoxelPlayerCaptureCamera>,
        ),
    >,
) {
    let Some(manager) = manager else { return };
    assets.entities.clear();
    for (entity, standee) in &existing {
        assets.entities.insert(standee.user_id, entity);
    }
    let camera_transforms = capture_cameras
        .iter()
        .map(|(transform, camera)| (camera.user_id, *transform))
        .collect::<HashMap<_, _>>();
    let active = manager
        .player_characters
        .iter()
        .filter_map(|(target_id, character)| {
            let user_id = target_id.parse::<u64>().ok()?;
            let image_source = character.image.trim();
            (character.inited
                && !image_source.is_empty()
                && camera_transforms.contains_key(&user_id))
            .then(|| (user_id, image_source.to_owned()))
        })
        .collect::<HashMap<_, _>>();

    for (entity, standee) in &existing {
        if active.contains_key(&standee.user_id) {
            continue;
        }
        commands.entity(entity).despawn();
        assets.entities.remove(&standee.user_id);
    }

    for (user_id, image_source) in active {
        let camera_transform = camera_transforms[&user_id];
        if let Some(entity) = assets.entities.get(&user_id).copied() {
            if let Ok((_, standee)) = existing.get(entity) {
                if standee.image_source == image_source {
                    if let Ok(mut transform) = standee_transforms.get_mut(entity) {
                        *transform = voxel_player_standee_transform(&camera_transform);
                    }
                    continue;
                }
                assets.failed_sources.remove(&standee.image_source);
            }
            commands.entity(entity).despawn();
            assets.entities.remove(&user_id);
        }
        if assets.failed_sources.contains(&image_source) {
            continue;
        }

        match load_voxel_player_standee_texture(
            &image_source,
            &mut images,
            &mut assets.textures,
        ) {
            Ok((texture, image_size)) => {
                let size = voxel_player_standee_size(image_size);
                let back_label_texture = assets
                    .back_label_texture
                    .get_or_insert_with(|| images.add(voxel_player_standee_back_label_image()))
                    .clone();
                let back_label_material = materials.add(StandardMaterial {
                    base_color_texture: Some(back_label_texture),
                    alpha_mode: AlphaMode::Opaque,
                    cull_mode: Some(Face::Back),
                    unlit: true,
                    ..default()
                });
                let portrait_material = materials.add(voxel_player_standee_material(texture));
                let mut entity_commands = commands.spawn((
                    Mesh3d(
                        meshes.add(Plane3d::new(PLAYER_STANDEE_PLANE_NORMAL, size * 0.5).mesh()),
                    ),
                    MeshMaterial3d(portrait_material.clone()),
                    voxel_player_standee_transform(&camera_transform),
                    Visibility::Visible,
                    VoxelPlayerStandee {
                        user_id,
                        image_source,
                        half_size: size * 0.5,
                    },
                    VoxelStandeeAccess::Player(user_id),
                ));
                entity_commands.with_children(|parent| {
                    parent.spawn((
                        Mesh3d(
                            meshes
                                .add(Plane3d::new(PLAYER_STANDEE_PLANE_NORMAL, size * 0.5).mesh()),
                        ),
                        MeshMaterial3d(portrait_material),
                        voxel_player_standee_back_transform(),
                    ));
                    parent.spawn((
                        Mesh3d(
                            meshes.add(
                                Plane3d::new(
                                    PLAYER_STANDEE_PLANE_NORMAL,
                                    Vec2::splat(VOXEL_SIZE * 0.42),
                                )
                                .mesh(),
                            ),
                        ),
                        MeshMaterial3d(back_label_material),
                        voxel_player_standee_back_label_transform(),
                    ));
                });
                let entity = entity_commands.id();
                assets.entities.insert(user_id, entity);
            },
            Err(err) => {
                assets.failed_sources.insert(image_source);
                eprintln!("failed to load voxel player standee for {user_id}: {err}");
            },
        }
    }
}

#[derive(Clone)]
struct ActiveVoxelUnitStandee {
    target_id: String,
    image_source: String,
    transform: Transform,
    access_visibility: AccessVisibility,
}

fn active_voxel_unit_standees(
    manager: &NapcatMessageManager,
    store: &VoxelUnitStandeeStore,
) -> HashMap<String, ActiveVoxelUnitStandee> {
    store
        .standees
        .iter()
        .filter_map(|persisted| {
            let unit = manager.unit_pool.get(&persisted.unit_id)?;
            let image_source = unit.character.image.trim();
            if image_source.is_empty() {
                return None;
            }
            Some((
                persisted.unit_id.clone(),
                ActiveVoxelUnitStandee {
                    target_id: voxel_unit_standee_target_id(&persisted.unit_id),
                    image_source: image_source.to_owned(),
                    transform: Transform {
                        translation: Vec3::from_array(persisted.translation),
                        rotation: Quat::from_array(persisted.rotation).normalize(),
                        scale: Vec3::ONE,
                    },
                    access_visibility: persisted.visibility.clone(),
                },
            ))
        })
        .collect()
}

fn sync_voxel_unit_standees(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: ResMut<VoxelUnitStandeeAssets>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    store: Res<Persistent<VoxelUnitStandeeStore>>,
    existing: Query<(Entity, &VoxelUnitStandee), Without<VoxelPlayerStandee>>,
    mut standee_transforms: Query<
        &mut Transform,
        (
            With<VoxelUnitStandee>,
            Without<VoxelPlayerStandee>,
        ),
    >,
) {
    let Some(manager) = manager else { return };
    assets.entities.clear();
    for (entity, standee) in &existing {
        assets.entities.insert(standee.unit_id.clone(), entity);
    }
    let active = active_voxel_unit_standees(&manager, &store);

    for (entity, standee) in &existing {
        if active.contains_key(&standee.unit_id) {
            continue;
        }
        commands.entity(entity).despawn();
        assets.entities.remove(&standee.unit_id);
    }

    for (unit_id, active_standee) in active {
        if let Some(entity) = assets.entities.get(&unit_id).copied() {
            if let Ok((_, standee)) = existing.get(entity) {
                if standee.image_source == active_standee.image_source
                    && standee.access_visibility == active_standee.access_visibility
                {
                    if let Ok(mut transform) = standee_transforms.get_mut(entity) {
                        *transform = active_standee.transform;
                    }
                    continue;
                }
                assets.failed_sources.remove(&standee.image_source);
            }
            commands.entity(entity).despawn();
            assets.entities.remove(&unit_id);
        }
        if assets.failed_sources.contains(&active_standee.image_source) {
            continue;
        }

        match load_voxel_player_standee_texture(
            &active_standee.image_source,
            &mut images,
            &mut assets.textures,
        ) {
            Ok((texture, image_size)) => {
                let size = voxel_player_standee_size(image_size);
                let back_label_texture = assets
                    .back_label_texture
                    .get_or_insert_with(|| images.add(voxel_player_standee_back_label_image()))
                    .clone();
                let back_label_material = materials.add(StandardMaterial {
                    base_color_texture: Some(back_label_texture),
                    alpha_mode: AlphaMode::Opaque,
                    cull_mode: Some(Face::Back),
                    unlit: true,
                    ..default()
                });
                let portrait_material = materials.add(voxel_player_standee_material(texture));
                let mut entity_commands = commands.spawn((
                    Mesh3d(
                        meshes.add(Plane3d::new(PLAYER_STANDEE_PLANE_NORMAL, size * 0.5).mesh()),
                    ),
                    MeshMaterial3d(portrait_material.clone()),
                    active_standee.transform,
                    Visibility::Visible,
                    VoxelUnitStandee {
                        target_id: active_standee.target_id,
                        unit_id: unit_id.clone(),
                        image_source: active_standee.image_source.clone(),
                        access_visibility: active_standee.access_visibility.clone(),
                    },
                    VoxelStandeeAccess::Unit(active_standee.access_visibility),
                ));
                entity_commands.with_children(|parent| {
                    parent.spawn((
                        Mesh3d(
                            meshes
                                .add(Plane3d::new(PLAYER_STANDEE_PLANE_NORMAL, size * 0.5).mesh()),
                        ),
                        MeshMaterial3d(portrait_material),
                        voxel_player_standee_back_transform(),
                    ));
                    parent.spawn((
                        Mesh3d(
                            meshes.add(
                                Plane3d::new(
                                    PLAYER_STANDEE_PLANE_NORMAL,
                                    Vec2::splat(VOXEL_SIZE * 0.42),
                                )
                                .mesh(),
                            ),
                        ),
                        MeshMaterial3d(back_label_material),
                        voxel_player_standee_back_label_transform(),
                    ));
                });
                let entity = entity_commands.id();
                assets.entities.insert(unit_id, entity);
            },
            Err(err) => {
                assets.failed_sources.insert(active_standee.image_source);
                eprintln!("failed to load voxel unit standee for {unit_id}: {err}");
            },
        }
    }
}

fn sync_voxel_scene_character_positions(
    mut positions: ResMut<SceneCharacterPositions>,
    player_standees: Query<(&VoxelPlayerStandee, &Transform), Without<VoxelUnitStandee>>,
    unit_standees: Query<(&VoxelUnitStandee, &Transform), Without<VoxelPlayerStandee>>,
) {
    positions.positions.clear();
    positions.positions.extend(
        player_standees.iter().map(|(standee, transform)| {
            (
                standee.user_id.to_string(),
                transform.translation,
            )
        }),
    );
    positions.positions.extend(
        unit_standees.iter().map(|(standee, transform)| {
            (
                standee.target_id.clone(),
                transform.translation,
            )
        }),
    );
}

fn voxel_player_standee_transform(camera_transform: &Transform) -> Transform { *camera_transform }

fn voxel_player_standee_back_transform() -> Transform {
    Transform::from_xyz(0.0, 0.0, -0.005).with_rotation(Quat::from_rotation_y(
        std::f32::consts::PI,
    ))
}

fn voxel_player_standee_back_label_transform() -> Transform { Transform::from_xyz(0.0, 0.0, 0.01) }

fn voxel_player_standee_back_label_image() -> Image {
    const SIZE: u32 = 128;
    const BACKGROUND: [u8; 3] = [166, 13, 13];
    let mut pixels = vec![0; (SIZE * SIZE * 4) as usize];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], u8::MAX]);
    }

    let font = FontRef::try_from_slice(include_bytes!(
        "../assets/fonts/AlibabaHealthFont.ttf"
    ))
    .expect("bundled Chinese font should be valid");
    let scale = PxScale::from(96.0);
    let glyph_id = font.glyph_id('背');
    let unpositioned = glyph_id.with_scale_and_position(scale, point(0.0, 0.0));
    let bounds = font
        .outline_glyph(unpositioned)
        .expect("bundled Chinese font should contain 背")
        .px_bounds();
    let position = point(
        (SIZE as f32 - bounds.width()) * 0.5 - bounds.min.x,
        (SIZE as f32 - bounds.height()) * 0.5 - bounds.min.y,
    );
    let glyph = glyph_id.with_scale_and_position(scale, position);
    font.outline_glyph(glyph)
        .expect("bundled Chinese font should contain 背")
        .draw(|x, y, coverage| {
            if x >= SIZE || y >= SIZE {
                return;
            }
            let index = ((y * SIZE + x) * 4) as usize;
            for channel in 0..3 {
                pixels[index + channel] = (BACKGROUND[channel] as f32 * (1.0 - coverage)
                    + u8::MAX as f32 * coverage)
                    .round() as u8;
            }
        });

    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    image
}

fn voxel_player_standee_material(texture: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(texture),
        alpha_mode: AlphaMode::Opaque,
        cull_mode: Some(Face::Back),
        unlit: true,
        ..default()
    }
}

fn voxel_player_standee_size(image_size: Vec2) -> Vec2 {
    let width = (image_size.x / image_size.y.max(1.0) * PLAYER_STANDEE_HEIGHT)
        .clamp(VOXEL_SIZE, VOXEL_SIZE * 3.0);
    Vec2::new(width, PLAYER_STANDEE_HEIGHT)
}

fn load_voxel_player_standee_texture(
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
    let path = cached_or_local_voxel_standee_path(source)?;
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

pub(crate) fn cached_or_local_voxel_standee_path(source: &str) -> Result<PathBuf, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("empty image source".to_owned());
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        return cache_remote_voxel_standee_image(source);
    }
    if let Ok(url) = url::Url::parse(source) {
        if url.scheme() == "file" {
            return url
                .to_file_path()
                .map_err(|_| format!("file URI is not a local path: {source}"));
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

fn cache_remote_voxel_standee_image(url: &str) -> Result<PathBuf, String> {
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
    let path = cache_dir.join(cache_name).with_extension(extension);
    fs::write(&path, &bytes).map_err(|err| err.to_string())?;
    Ok(path)
}

fn capture_voxel_player_view(
    mut commands: Commands,
    mut requests: ResMut<SceneCaptureRequests>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    napcat_sender: Option<Res<NapcatIOSender>>,
    runtimes: Res<VoxelPlayerCameraRuntimes>,
    mut state: ResMut<VoxelPlayerCaptureState>,
    mut cameras: Query<
        (&mut Camera, &mut Transform),
        (
            With<VoxelPlayerCaptureCamera>,
            Without<VoxelStandeeAccess>,
        ),
    >,
    mut standees: Query<
        (
            Entity,
            &VoxelStandeeAccess,
            &mut Visibility,
        ),
        Without<VoxelPlayerCaptureCamera>,
    >,
) {
    let incoming = requests.requests.drain(..).collect::<Vec<_>>();
    for request in incoming {
        let Some(camera) = runtimes.cameras.get(&request.user_id) else {
            if voxel_observation_player_allowed(manager.as_deref(), request.user_id) {
                requests.requests.push(request);
            } else {
                eprintln!(
                    "ignored voxel observation request from unconfigured user {}",
                    request.user_id
                );
            }
            continue;
        };
        let output_dir = Path::new(".data")
            .join("willowblossom")
            .join("scene_captures");
        if let Err(err) = std::fs::create_dir_all(&output_dir) {
            eprintln!("failed to create scene capture directory: {err}");
            continue;
        }
        let request_id = state.next_request_id;
        state.next_request_id += 1;
        let (output_path, video_frames_dir) = voxel_capture_output_paths(
            &output_dir,
            request.kind,
            request_id,
            request.user_id,
        );
        if let Some(frames_dir) = &video_frames_dir {
            if frames_dir.exists() {
                if let Err(err) = std::fs::remove_dir_all(frames_dir) {
                    eprintln!("failed to clear voxel observation video frames: {err}");
                    continue;
                }
            }
            if let Err(err) = std::fs::create_dir_all(frames_dir) {
                eprintln!("failed to create voxel observation video frames: {err}");
                continue;
            }
        }
        state.pending.push(PendingVoxelPlayerCapture {
            request_id,
            user_id: request.user_id,
            campaign_id: request.campaign_id,
            camera_entity: camera.entity,
            target: camera.target.clone(),
            output_path,
            kind: request.kind,
            prepare_frames_remaining: PLAYER_CAPTURE_PREPARE_FRAMES,
            activated: false,
            original_camera_transform: None,
            video_frame_index: 0,
            video_frame_prepared: false,
            screenshot_in_flight: false,
            video_frames_dir,
            failure: None,
            hidden_standees: Vec::new(),
        });
    }

    let Some(current) = state.pending.first_mut() else { return };
    if current.kind == SceneCaptureKind::PanoramaVideo
        && !manager.as_deref().is_some_and(|manager| {
            manager.can_serve_scene_capture(current.user_id, &current.campaign_id)
        })
    {
        current.failure = Some("player no longer belongs to the active campaign".to_owned());
    }
    if !current.activated {
        if let Ok((mut camera, transform)) = cameras.get_mut(current.camera_entity) {
            camera.is_active = true;
            current.original_camera_transform = Some(*transform);
        }
        for (entity, standee_access, mut visibility) in &mut standees {
            if voxel_standee_visible_to(
                manager.as_deref(),
                current.user_id,
                standee_access,
            ) {
                continue;
            }
            current.hidden_standees.push((entity, visibility.clone()));
            *visibility = Visibility::Hidden;
        }
        current.activated = true;
        return;
    }
    if current.prepare_frames_remaining > 0 {
        current.prepare_frames_remaining -= 1;
        return;
    }

    if current.kind == SceneCaptureKind::PanoramaVideo {
        let capture_finished = current.failure.is_some()
            || (current.video_frame_index >= SCENE_CAPTURE_VIDEO_FRAMES
                && !current.screenshot_in_flight);
        if capture_finished {
            let pending = state.pending.remove(0);
            if let Ok((mut camera, mut transform)) = cameras.get_mut(pending.camera_entity) {
                camera.is_active = false;
                if let Some(original) = pending.original_camera_transform {
                    *transform = original;
                }
            }
            for (entity, visibility) in &pending.hidden_standees {
                if let Ok((_, _, mut current_visibility)) = standees.get_mut(*entity) {
                    *current_visibility = visibility.clone();
                }
            }
            if let Some(err) = pending.failure {
                eprintln!("failed to capture voxel observation video: {err}");
                if let Some(frames_dir) = pending.video_frames_dir {
                    let _ = std::fs::remove_dir_all(frames_dir);
                }
            } else if let (Some(sender), Some(frames_dir)) = (
                napcat_sender.as_deref(),
                pending.video_frames_dir.clone(),
            ) {
                encode_and_send_scene_capture_video(
                    sender.0.clone(),
                    pending.request_id,
                    pending.user_id,
                    frames_dir,
                    pending.output_path,
                );
            } else if let Some(frames_dir) = pending.video_frames_dir {
                let _ = std::fs::remove_dir_all(frames_dir);
            }
            return;
        }

        if current.screenshot_in_flight {
            return;
        }
        if !current.video_frame_prepared {
            let Some(original) = current.original_camera_transform else {
                current.failure = Some("player camera transform is unavailable".to_owned());
                return;
            };
            if let Ok((_, mut transform)) = cameras.get_mut(current.camera_entity) {
                transform.rotation = scene_capture_video_rotation(
                    original.rotation,
                    current.video_frame_index,
                );
                current.video_frame_prepared = true;
            } else {
                current.failure = Some("player camera no longer exists".to_owned());
            }
            return;
        }

        let Some(frames_dir) = current.video_frames_dir.clone() else {
            current.failure = Some("video frame directory is unavailable".to_owned());
            return;
        };
        let request_id = current.request_id;
        let frame_path = frames_dir.join(format!(
            "frame_{:04}.png",
            current.video_frame_index
        ));
        current.screenshot_in_flight = true;
        commands
            .spawn(Screenshot::image(
                current.target.clone(),
            ))
            .observe(
                move |screenshot: On<ScreenshotCaptured>,
                      mut state: ResMut<VoxelPlayerCaptureState>| {
                    let Some(pending) = state
                        .pending
                        .iter_mut()
                        .find(|pending| pending.request_id == request_id)
                    else {
                        return;
                    };
                    let save_result = screenshot
                        .image
                        .clone()
                        .try_into_dynamic()
                        .map_err(|err| err.to_string())
                        .and_then(|image| {
                            image
                                .to_rgb8()
                                .save(&frame_path)
                                .map_err(|err| err.to_string())
                        });
                    pending.screenshot_in_flight = false;
                    pending.video_frame_prepared = false;
                    match save_result {
                        Ok(()) => pending.video_frame_index += 1,
                        Err(err) => pending.failure = Some(err),
                    }
                },
            );
        return;
    }

    let pending = state.pending.remove(0);
    commands
        .spawn(Screenshot::image(
            pending.target.clone(),
        ))
        .observe(
            move |screenshot: On<ScreenshotCaptured>,
                  napcat_sender: Option<Res<NapcatIOSender>>,
                  mut cameras: Query<
                &mut Camera,
                (
                    With<VoxelPlayerCaptureCamera>,
                    Without<VoxelStandeeAccess>,
                ),
            >,
                  mut standees: Query<
                &mut Visibility,
                (
                    With<VoxelStandeeAccess>,
                    Without<VoxelPlayerCaptureCamera>,
                ),
            >| {
                if let Ok(mut camera) = cameras.get_mut(pending.camera_entity) {
                    camera.is_active = false;
                }
                for (entity, visibility) in &pending.hidden_standees {
                    if let Ok(mut current_visibility) = standees.get_mut(*entity) {
                        *current_visibility = visibility.clone();
                    }
                }
                let save_result = screenshot
                    .image
                    .clone()
                    .try_into_dynamic()
                    .map_err(|err| err.to_string())
                    .and_then(|image| {
                        image
                            .to_rgb8()
                            .save(&pending.output_path)
                            .map_err(|err| err.to_string())
                    });
                if let Err(err) = save_result {
                    eprintln!("failed to save voxel player capture: {err}");
                    return;
                }
                let Some(napcat_sender) = napcat_sender else { return };
                let file = match voxel_capture_file_uri(&pending.output_path) {
                    Ok(file) => file,
                    Err(err) => {
                        eprintln!("failed to build voxel capture file URI: {err}");
                        return;
                    },
                };
                let message = Message::Text(
                    json!({
                        "action": "send_private_msg",
                        "params": {
                            "user_id": pending.user_id,
                            "message": [{
                                "type": "image",
                                "data": { "file": file, "summary": "场景观察" }
                            }]
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
                    eprintln!("failed to queue voxel player capture: {err}");
                }
            },
        );
}

fn spawn_voxel_player_camera(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    runtimes: &mut VoxelPlayerCameraRuntimes,
    user_id: u64,
    transform: Transform,
) {
    let target = images.add(voxel_player_capture_image());
    let entity = commands
        .spawn((
            Camera3d::default(),
            Msaa::Off,
            Camera {
                is_active: false,
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.055, 0.065, 0.075)),
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: FIRST_PERSON_FOV_RADIANS,
                far: 2_500.0,
                ..default()
            }),
            RenderTarget::Image(target.clone().into()),
            RenderLayers::layer(0),
            transform,
            VoxelPlayerCaptureCamera { user_id },
        ))
        .id();
    runtimes.cameras.insert(user_id, VoxelPlayerCameraRuntime {
        entity,
        target,
    });
}

fn voxel_player_capture_image() -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: PLAYER_CAPTURE_WIDTH,
            height: PLAYER_CAPTURE_HEIGHT,
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

fn persisted_voxel_player_camera(
    store: &VoxelPlayerCameraStore,
    user_id: u64,
) -> Option<&PersistedVoxelPlayerCamera> {
    store
        .cameras
        .iter()
        .find(|camera| camera.user_id == user_id)
}

fn completed_voxel_player_user_ids(
    characters: &HashMap<String, PlayerCharacter>,
) -> impl Iterator<Item = u64> + '_ {
    characters
        .iter()
        .filter(|(_, character)| character.inited)
        .filter_map(|(target_id, _)| target_id.parse::<u64>().ok())
}

fn initial_voxel_player_camera_transform(
    store: &VoxelPlayerCameraStore,
    user_id: u64,
    dm_transform: Transform,
) -> Transform {
    persisted_voxel_player_camera(store, user_id)
        .map(voxel_player_camera_transform)
        .unwrap_or(dm_transform)
}

fn voxel_player_camera_transform(camera: &PersistedVoxelPlayerCamera) -> Transform {
    Transform {
        translation: Vec3::from(camera.translation),
        rotation: Quat::from_array(camera.rotation),
        scale: Vec3::ONE,
    }
}

fn upsert_voxel_player_camera(
    store: &mut VoxelPlayerCameraStore,
    user_id: u64,
    transform: &Transform,
) {
    let persisted = PersistedVoxelPlayerCamera {
        user_id,
        translation: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
    };
    if let Some(camera) = store
        .cameras
        .iter_mut()
        .find(|camera| camera.user_id == user_id)
    {
        *camera = persisted;
    } else {
        store.cameras.push(persisted);
    }
}

pub(crate) fn voxel_unit_standee_target_id(unit_id: &str) -> String {
    format!("unit:{}", unit_id.trim())
}

pub(crate) fn has_voxel_unit_standee(store: &VoxelUnitStandeeStore, unit_id: &str) -> bool {
    let unit_id = unit_id.trim();
    !unit_id.is_empty()
        && store
            .standees
            .iter()
            .any(|standee| standee.unit_id == unit_id)
}

pub(crate) fn place_voxel_unit_standee(
    store: &mut VoxelUnitStandeeStore,
    unit_id: &str,
    image_source: &str,
    editor: &VoxelEditorState,
) -> Result<bool, String> {
    let unit_id = unit_id.trim();
    if unit_id.is_empty() {
        return Err("单位ID为空".to_owned());
    }
    if image_source.trim().is_empty() {
        return Err("单位模板还没有立绘".to_owned());
    }
    if has_voxel_unit_standee(store, unit_id) {
        return Ok(false);
    }

    let transform = default_voxel_unit_standee_transform(editor, store.standees.len());
    store.standees.push(PersistedVoxelUnitStandee {
        unit_id: unit_id.to_owned(),
        translation: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
        visibility: AccessVisibility::Public,
    });
    Ok(true)
}

pub(crate) fn remove_voxel_unit_standee(store: &mut VoxelUnitStandeeStore, unit_id: &str) -> bool {
    let unit_id = unit_id.trim();
    let previous_len = store.standees.len();
    store.standees.retain(|standee| standee.unit_id != unit_id);
    store.standees.len() != previous_len
}

fn default_voxel_unit_standee_transform(
    editor: &VoxelEditorState,
    standee_index: usize,
) -> Transform {
    let camera = editor_camera_transform(editor);
    let mut translation = if editor.first_person_enabled {
        camera.translation + *camera.forward() * (VOXEL_SIZE * 4.0)
    } else {
        editor.camera_focus
    };
    let camera_right = *camera.right();
    let flat_right = Vec3::new(camera_right.x, 0.0, camera_right.z).normalize_or_zero();
    let flat_right = if flat_right == Vec3::ZERO { Vec3::X } else { flat_right };
    translation += flat_right * symmetric_standee_slot(standee_index) as f32 * (VOXEL_SIZE * 3.0);

    let facing_target = Vec3::new(
        camera.translation.x,
        translation.y,
        camera.translation.z,
    );
    if facing_target.distance_squared(translation) <= f32::EPSILON {
        Transform::from_translation(translation).with_rotation(camera.rotation)
    } else {
        Transform::from_translation(translation).looking_at(facing_target, Vec3::Y)
    }
}

fn symmetric_standee_slot(index: usize) -> i32 {
    if index == 0 {
        0
    } else {
        let distance = index.div_ceil(2) as i32;
        if index % 2 == 1 {
            distance
        } else {
            -distance
        }
    }
}

fn voxel_player_display_name(
    manager: Option<&Persistent<NapcatMessageManager>>,
    user_id: u64,
) -> String {
    let target_id = user_id.to_string();
    let Some(manager) = manager else { return target_id };
    if let Some(character) = manager.player_characters.get(&target_id) {
        let name = if character.nickname.trim().is_empty() {
            character.name.trim()
        } else {
            character.nickname.trim()
        };
        if !name.is_empty() {
            return format!("{name} ({target_id})");
        }
    }
    target_id
}

fn voxel_observation_player_allowed(
    manager: Option<&Persistent<NapcatMessageManager>>,
    user_id: u64,
) -> bool {
    let Some(manager) = manager else { return false };
    let target_id = user_id.to_string();
    manager.player_characters.contains_key(&target_id)
        || manager
            .current_group()
            .is_some_and(|group| group.players.iter().any(|player| player == &target_id))
}

fn voxel_standee_visible_to(
    manager: Option<&Persistent<NapcatMessageManager>>,
    requester_id: u64,
    standee_access: &VoxelStandeeAccess,
) -> bool {
    match standee_access {
        VoxelStandeeAccess::Player(standee_user_id) => {
            voxel_player_standee_visible_to(manager, requester_id, *standee_user_id)
        },
        VoxelStandeeAccess::Unit(visibility) => manager.is_some_and(|manager| {
            voxel_unit_standee_visible_for_access(
                &manager.player_access_for_user(requester_id),
                visibility,
            )
        }),
    }
}

fn voxel_unit_standee_visible_for_access(
    requester: &crate::napcat::PlayerAccess,
    visibility: &AccessVisibility,
) -> bool {
    requester.can_read(visibility)
}

fn voxel_player_standee_visible_to(
    manager: Option<&Persistent<NapcatMessageManager>>,
    requester_id: u64,
    standee_user_id: u64,
) -> bool {
    let Some(manager) = manager else { return false };
    let requester = manager.player_access_for_user(requester_id);
    let standee = manager.player_access_for_user(standee_user_id);
    voxel_player_standee_visible_for_access(
        requester_id,
        standee_user_id,
        requester.is_gm,
        requester.party_id.as_deref(),
        standee.party_id.as_deref(),
    )
}

fn voxel_player_standee_visible_for_access(
    requester_id: u64,
    standee_user_id: u64,
    requester_is_gm: bool,
    requester_party_id: Option<&str>,
    standee_party_id: Option<&str>,
) -> bool {
    requester_id != standee_user_id && (requester_is_gm || requester_party_id == standee_party_id)
}

fn voxel_capture_file_uri(path: &Path) -> Result<String, String> {
    let path = std::fs::canonicalize(path).map_err(|err| err.to_string())?;
    url::Url::from_file_path(&path)
        .map(|url| url.to_string())
        .map_err(|_| {
            format!(
                "path cannot be represented as a file URI: {}",
                path.display()
            )
        })
}

fn voxel_capture_output_paths(
    output_dir: &Path,
    kind: SceneCaptureKind,
    request_id: u64,
    user_id: u64,
) -> (PathBuf, Option<PathBuf>) {
    match kind {
        SceneCaptureKind::Image => (
            output_dir.join(format!("player_{user_id}.png")),
            None,
        ),
        SceneCaptureKind::PanoramaVideo => (
            output_dir.join(format!(
                "player_{user_id}_360_{request_id}.mp4"
            )),
            Some(output_dir.join(format!(
                "voxel_video_{request_id}_frames"
            ))),
        ),
    }
}

fn voxel_orbital_planet_cells() -> Vec<(IVec3, u8)> {
    let radius = ORBITAL_PLANET_RADIUS / VOXEL_SIZE - 0.5;
    let inner_radius = radius - ORBITAL_PLANET_SHELL_THICKNESS;
    let inner_squared = inner_radius.powi(2);
    let mut cells = Vec::new();
    for x in -ORBITAL_PLANET_CAP_RADIUS..=ORBITAL_PLANET_CAP_RADIUS {
        for z in -ORBITAL_PLANET_CAP_RADIUS..=ORBITAL_PLANET_CAP_RADIUS {
            let Some(outer_y) = planet_surface_y(x, z) else { continue };
            let horizontal_squared = x * x + z * z;
            let inner_y = (inner_squared - horizontal_squared as f32).sqrt().ceil() as i32;
            for y in inner_y..=outer_y {
                let cell = IVec3::new(x, y, z);
                if let Some(material) = procedural_planet_material(cell) {
                    cells.push((cell, material));
                }
            }
        }
    }
    cells.extend(xy_planet_map_cells());
    let mut cells = cells
        .into_iter()
        .collect::<HashMap<_, _>>()
        .into_iter()
        .collect::<Vec<_>>();
    cells.sort_unstable_by_key(|(cell, _)| (cell.y, cell.z, cell.x));
    cells
}

fn planet_surface_y(x: i32, z: i32) -> Option<i32> {
    let horizontal_squared = x * x + z * z;
    if horizontal_squared > ORBITAL_PLANET_CAP_RADIUS.pow(2) {
        return None;
    }
    let radius = ORBITAL_PLANET_RADIUS / VOXEL_SIZE - 0.5;
    Some((radius.powi(2) - horizontal_squared as f32).sqrt().floor() as i32)
}

fn xy_planet_map_cells() -> Vec<(IVec3, u8)> {
    let decoded = XY_PLANET.decode();
    let mut cells = Vec::new();
    for (index, style) in decoded.styles.iter().copied().enumerate() {
        let [local_x, local_z] = XY_PLANET.centered_offset(index);
        let x = PLANET_SCIENCE_LAB_CENTER.x + local_x;
        let z = PLANET_SCIENCE_LAB_CENTER.y + local_z;
        let Some(surface_y) = planet_surface_y(x, z) else {
            continue;
        };
        if let Some(material) = match style {
            7 => Some(1),
            8 | 10 => Some(3),
            9 => Some(2),
            19 | 20 => Some(4),
            _ => None,
        } {
            cells.push((IVec3::new(x, surface_y, z), material));
        }

        let wall = style == 11;
        let door = style == 15;
        let enclosed = decoded.enclosed[index];
        if wall || door || enclosed {
            for y in surface_y + 1..=PLANET_SCIENCE_LAB_FLOOR_Y {
                cells.push((IVec3::new(x, y, z), 6));
            }
            cells.push((
                IVec3::new(x, PLANET_SCIENCE_LAB_FLOOR_Y, z),
                if style == 12 { 7 } else { 2 },
            ));
            cells.push((
                IVec3::new(
                    x,
                    PLANET_SCIENCE_LAB_FLOOR_Y + WORKBOOK_ROOM_HEIGHT,
                    z,
                ),
                6,
            ));
        }
        if wall {
            for y in 1..WORKBOOK_ROOM_HEIGHT {
                cells.push((
                    IVec3::new(x, PLANET_SCIENCE_LAB_FLOOR_Y + y, z),
                    6,
                ));
            }
        } else if enclosed {
            if let Some((material, height)) = workbook_planet_fixture(style) {
                for y in 1..=height {
                    cells.push((
                        IVec3::new(x, PLANET_SCIENCE_LAB_FLOOR_Y + y, z),
                        material,
                    ));
                }
            }
        }
    }
    cells
}

fn procedural_planet_material(cell: IVec3) -> Option<u8> {
    if cell.y < 0 || cell.x * cell.x + cell.z * cell.z > ORBITAL_PLANET_CAP_RADIUS.pow(2) {
        return None;
    }
    let local_position = cell.as_vec3() * VOXEL_SIZE;
    let distance = local_position.length();
    if distance > ORBITAL_PLANET_RADIUS - VOXEL_SIZE * 0.5 {
        return None;
    }
    let depth = ORBITAL_PLANET_RADIUS - distance;
    if depth > 5.0 * VOXEL_SIZE {
        return Some(6);
    }
    if depth > 1.5 * VOXEL_SIZE {
        return Some(2);
    }
    let continental = (cell.x as f32 * 0.095).sin()
        + (cell.z as f32 * 0.08).cos()
        + ((cell.x + cell.z) as f32 * 0.055).sin()
        + (cell.y as f32 * 0.115).cos() * 0.35;
    Some(
        if cell.y >= ORBITAL_PLANET_VOXEL_RADIUS - 4 {
            3
        } else if continental > 0.72 {
            1
        } else if continental > 0.52 {
            3
        } else {
            4
        },
    )
}

fn dig_planet_voxel(planet: &mut VoxelOrbitalPlanet, cell: IVec3) -> bool {
    if planet.cells.remove(&cell).is_none() {
        return false;
    }
    planet.removed.insert(cell);

    // Keep the planet sparse: only materialize the buried cells next to the
    // newly exposed cavity. Deleted cells stay in `removed`, so procedural
    // generation can never fill a tunnel back in behind the player.
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let neighbor = cell + IVec3::new(x, y, z);
                if neighbor == cell
                    || planet.removed.contains(&neighbor)
                    || planet.cells.contains_key(&neighbor)
                {
                    continue;
                }
                if let Some(material) = procedural_planet_material(neighbor) {
                    planet.cells.insert(neighbor, material);
                    planet.include_cell_in_bounds(neighbor);
                }
            }
        }
    }
    planet.dirty = true;
    true
}

fn set_planet_voxel(planet: &mut VoxelOrbitalPlanet, cell: IVec3, material: u8) -> bool {
    if planet.cells.get(&cell).copied() == Some(material) {
        return false;
    }
    planet.removed.remove(&cell);
    planet.cells.insert(cell, material);
    planet.include_cell_in_bounds(cell);
    planet.dirty = true;
    true
}

fn explode_planet_voxels(
    planet: &mut VoxelOrbitalPlanet,
    local_origin: Vec3,
    radius: f32,
) -> Vec<(IVec3, u8)> {
    let radius = radius.max(VOXEL_SIZE);
    let radius_squared = radius * radius;
    let mut selected = planet
        .cells
        .iter()
        .filter_map(|(cell, material)| {
            let center = cell.as_vec3() * VOXEL_SIZE;
            (center.distance_squared(local_origin) <= radius_squared).then_some((*cell, *material))
        })
        .collect::<Vec<_>>();
    selected.sort_unstable_by_key(|(cell, _)| (cell.y, cell.z, cell.x));

    // Use the exact same removal path as hand digging and physics extraction.
    // Each removed surface cell reveals its buried neighbours. Those newly
    // exposed cells stay in the planet instead of recursively joining this
    // explosion, keeping a useful digging surface and a bounded edit cost.
    let mut removed = Vec::with_capacity(selected.len());
    for (cell, material) in selected {
        if dig_planet_voxel(planet, cell) {
            removed.push((cell, material));
        }
    }
    removed
}

fn sorted_planet_cells(planet: &VoxelOrbitalPlanet) -> Vec<(IVec3, u8)> {
    let mut cells = planet
        .cells
        .iter()
        .map(|(cell, material)| (*cell, *material))
        .collect::<Vec<_>>();
    cells.sort_unstable_by_key(|(cell, _)| (cell.y, cell.z, cell.x));
    cells
}

fn planet_material_handle(materials: &VoxelMaterials, material_id: u8) -> Handle<StandardMaterial> {
    if material_id == 4 {
        materials.planet_ocean.clone()
    } else {
        materials.handles[material_id as usize - 1].clone()
    }
}

fn voxel_planet_gravity_acceleration(position: Vec3, planet_center: Vec3) -> Vec3 {
    let toward_center = planet_center - position;
    let distance_squared = toward_center.length_squared();
    let maximum_distance = ORBITAL_PLANET_RADIUS + ORBITAL_PLANET_GRAVITY_MAX_ALTITUDE;
    if !distance_squared.is_finite()
        || distance_squared <= f32::EPSILON
        || distance_squared > maximum_distance * maximum_distance
    {
        return Vec3::ZERO;
    }
    toward_center / distance_squared.sqrt() * ORBITAL_PLANET_GRAVITY_ACCELERATION
}

fn apply_voxel_planet_gravity(
    planets: Query<&Transform, With<VoxelOrbitalPlanet>>,
    mut bodies: Query<Forces, With<VoxelPlanetGravityBody>>,
) {
    let Ok(planet_transform) = planets.single() else {
        return;
    };

    for mut forces in &mut bodies {
        let acceleration = voxel_planet_gravity_acceleration(
            forces.position().0,
            planet_transform.translation,
        );
        if acceleration != Vec3::ZERO {
            forces.apply_linear_acceleration(acceleration);
        }
    }
}

fn spawn_voxel_orbital_planet(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &VoxelMaterials,
) -> Entity {
    let cells = voxel_orbital_planet_cells();
    let cell_bounds = VoxelCellBounds::from_cells(cells.iter().map(|(cell, _)| *cell));
    let (material_meshes, collider_cells) = build_voxel_meshes_from_cells(&cells);
    let center_offset = Vec3::splat(-0.5 * VOXEL_SIZE);
    let entity = commands
        .spawn((
            RigidBody::Static,
            Transform::from_translation(ORBITAL_PLANET_CENTER),
        ))
        .id();
    let collider_entity = commands
        .spawn((
            canonical_voxel_collider(&collider_cells),
            Friction::new(0.8),
            Transform::from_translation(center_offset),
            VoxelPlanetCollider,
        ))
        .id();
    commands.entity(entity).add_child(collider_entity);
    let mut mesh_entities = Vec::new();
    let mut mesh_handles = Vec::new();
    commands.entity(entity).with_children(|parent| {
        for (material_id, mesh) in material_meshes {
            let mesh_handle = meshes.add(mesh);
            mesh_handles.push(mesh_handle.clone());
            mesh_entities.push(
                parent
                    .spawn((
                        Mesh3d(mesh_handle),
                        MeshMaterial3d(planet_material_handle(
                            materials,
                            material_id,
                        )),
                        Transform::from_translation(center_offset),
                    ))
                    .id(),
            );
        }
    });
    commands.entity(entity).insert(VoxelOrbitalPlanet {
        cells: cells.into_iter().collect(),
        cell_bounds,
        removed: HashSet::new(),
        collider_entity,
        mesh_entities,
        mesh_handles,
        voxel_size: VOXEL_SIZE,
        dirty: false,
    });
    entity
}

fn spawn_planet_clouds(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let cloud_mesh = meshes.add(Sphere::new(1.0).mesh().uv(12, 8));
    let cloud_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.92, 0.96, 1.0, 0.42),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 1.0,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let layer = commands
        .spawn((
            VoxelPlanetCloudLayer,
            Transform::from_translation(ORBITAL_PLANET_CENTER),
        ))
        .id();
    commands.entity(layer).with_children(|parent| {
        let golden_angle = std::f32::consts::PI * (3.0 - 5.0_f32.sqrt());
        for index in 0..PLANET_CLOUD_PUFF_COUNT {
            let fraction = (index as f32 + 0.5) / PLANET_CLOUD_PUFF_COUNT as f32;
            let y = 0.965 + fraction * 0.035;
            let radial = (1.0 - y * y).sqrt();
            let angle = golden_angle * index as f32;
            let direction = Vec3::new(
                radial * angle.cos(),
                y,
                radial * angle.sin(),
            );
            let tangent = Quat::from_rotation_arc(Vec3::Y, direction);
            let width = 2.5 + ((index * 17 % 9) as f32) * 0.32;
            let depth = 1.7 + ((index * 11 % 7) as f32) * 0.24;
            parent.spawn((
                Mesh3d(cloud_mesh.clone()),
                MeshMaterial3d(cloud_material.clone()),
                Transform::from_translation(
                    direction * (ORBITAL_PLANET_RADIUS + PLANET_CLOUD_ALTITUDE),
                )
                .with_rotation(tangent)
                .with_scale(Vec3::new(width, 0.4, depth)),
            ));
        }
    });
    layer
}

fn animate_planet_clouds(
    time: Res<Time>,
    mut clouds: Query<&mut Transform, With<VoxelPlanetCloudLayer>>,
) {
    for mut transform in &mut clouds {
        transform.rotate_y(time.delta_secs() * 0.012);
    }
}

fn rebuild_voxel_orbital_planet(
    mut commands: Commands,
    mut planets: Query<(Entity, &mut VoxelOrbitalPlanet)>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    for (entity, mut planet) in &mut planets {
        if !planet.dirty {
            continue;
        }
        for mesh_entity in planet.mesh_entities.drain(..) {
            commands.entity(mesh_entity).despawn();
        }
        for mesh_handle in planet.mesh_handles.drain(..) {
            meshes.remove(mesh_handle.id());
        }
        let cells = sorted_planet_cells(&planet);
        planet.cell_bounds = VoxelCellBounds::from_cells(cells.iter().map(|(cell, _)| *cell));
        let (material_meshes, collider_cells) = build_voxel_meshes_from_cells(&cells);
        if collider_cells.is_empty() {
            commands
                .entity(planet.collider_entity)
                .insert(ColliderDisabled);
        } else {
            commands
                .entity(planet.collider_entity)
                .insert(canonical_voxel_collider(
                    &collider_cells,
                ))
                .remove::<ColliderDisabled>();
        }
        let mut mesh_entities = Vec::new();
        let mut mesh_handles = Vec::new();
        commands.entity(entity).with_children(|parent| {
            for (material_id, mesh) in material_meshes {
                let mesh_handle = meshes.add(mesh);
                mesh_handles.push(mesh_handle.clone());
                mesh_entities.push(
                    parent
                        .spawn((
                            Mesh3d(mesh_handle),
                            MeshMaterial3d(planet_material_handle(
                                &materials,
                                material_id,
                            )),
                            Transform::from_translation(Vec3::splat(-0.5 * VOXEL_SIZE)),
                        ))
                        .id(),
                );
            }
        });
        planet.mesh_entities = mesh_entities;
        planet.mesh_handles = mesh_handles;
        planet.dirty = false;
    }
}

fn editor_camera_transform(editor: &VoxelEditorState) -> Transform {
    let rotation = Quat::from_euler(
        EulerRot::YXZ,
        editor.camera_yaw,
        editor.camera_pitch,
        0.0,
    );
    let position = editor.camera_focus + rotation * Vec3::new(0.0, 0.0, editor.camera_distance);
    Transform::from_translation(position).looking_at(editor.camera_focus, Vec3::Y)
}

fn viewport_ray(
    window: &Window,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    editor: &VoxelEditorState,
) -> Option<Ray3d> {
    if editor.first_person_cursor_released {
        return None;
    }
    let screen_position = if editor.first_person_enabled {
        (editor.viewport_min + editor.viewport_max) * 0.5
    } else {
        window
            .cursor_position()
            .filter(|cursor| editor.contains_cursor(*cursor))?
    };
    camera
        .viewport_to_world(camera_transform, screen_position)
        .ok()
}

fn use_player_possession_tool(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    standees: Query<(
        &VoxelPlayerStandee,
        &GlobalTransform,
        &Visibility,
    )>,
    mut editor: ResMut<VoxelEditorState>,
    mut possession: ResMut<VoxelPossessionState>,
    mut camera_editor: ResMut<VoxelPlayerCameraEditor>,
    camera_store: ResMut<Persistent<VoxelPlayerCameraStore>>,
    egui_input: Res<EguiWantsInput>,
) {
    if !possession_tool_can_target(
        editor.is_player_possession_tool_equipped(),
        possession.active_user_id,
    ) || editor.creative_inventory_open
        || !mouse.just_pressed(MouseButton::Right)
        || egui_input.wants_any_pointer_input()
    {
        return;
    }
    let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single()) else {
        return;
    };
    let Some(ray) = viewport_ray(
        window,
        camera,
        camera_transform,
        &editor,
    ) else {
        return;
    };
    let selected = standees
        .iter()
        .filter_map(|(standee, transform, visibility)| {
            if *visibility == Visibility::Hidden {
                return None;
            }
            ray_intersects_player_standee(ray, transform, standee.half_size)
                .map(|distance| (standee.user_id, distance))
        })
        .filter(|(_, distance)| *distance <= MAX_RAY_DISTANCE)
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(user_id, _)| user_id);

    if let Err(err) = camera_store.persist() {
        eprintln!("failed to persist player camera before possession tool action: {err}");
    }
    match selected {
        Some(user_id) if possession.active_user_id == Some(user_id) => {
            possession.release();
            camera_editor.selected_user_id = Some(user_id);
            editor.physics_status = Some(format!("已解除PL {user_id}的接管"));
        },
        Some(user_id) => {
            possession.possess(user_id);
            camera_editor.selected_user_id = Some(user_id);
            editor.physics_status = Some(format!("已选择并接管PL {user_id}"));
        },
        None if possession.active_user_id.is_some() => {
            possession.release();
            editor.physics_status = Some("已解除PL接管".to_owned());
        },
        None => {
            editor.physics_status = Some("没有瞄准玩家立绘".to_owned());
        },
    }
}

fn use_spaceship_possession_tool(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    spaceships: Query<&VoxelSpaceship>,
    spatial_query: SpatialQuery,
    store: Res<Persistent<VoxelSpaceshipStore>>,
    mut editor: ResMut<VoxelEditorState>,
    mut possession: ResMut<VoxelPossessionState>,
    mut control: ResMut<VoxelSpaceshipControlState>,
    egui_input: Res<EguiWantsInput>,
) {
    if !spaceship_possession_tool_can_target(
        editor.is_spaceship_possession_tool_equipped(),
        possession.active_user_id,
    ) || editor.creative_inventory_open
        || !mouse.just_pressed(MouseButton::Right)
        || egui_input.wants_any_pointer_input()
    {
        return;
    }
    let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single()) else {
        return;
    };
    let Some(ray) = viewport_ray(
        window,
        camera,
        camera_transform,
        &editor,
    ) else {
        return;
    };
    let selected = spatial_query
        .cast_ray_predicate(
            ray.origin,
            ray.direction,
            MAX_RAY_DISTANCE,
            true,
            &SpatialQueryFilter::default(),
            &|entity| spaceships.contains(entity),
        )
        .and_then(|hit| spaceships.get(hit.entity).ok())
        .map(|ship| (ship.id.clone(), ship.name.clone()));

    match selected {
        Some((ship_id, ship_name))
            if control.driving_ship_id.as_deref() == Some(ship_id.as_str()) =>
        {
            control.stop_driving();
            editor.physics_status = Some(format!("已解除舰船 {ship_name} 的接管"));
        },
        Some((ship_id, ship_name)) => {
            let pilot_user_id = store
                .ships
                .iter()
                .find(|ship| ship.id == ship_id)
                .and_then(|ship| ship.pilot_user_id);
            begin_voxel_spaceship_takeover(
                &ship_id,
                pilot_user_id,
                &mut editor,
                &mut possession,
                &mut control,
            );
            editor.physics_status = Some(pilot_user_id.map_or_else(
                || format!("GM已接管舰船 {ship_name}"),
                |user_id| format!("已让PL {user_id}接管舰船 {ship_name}"),
            ));
        },
        None => {
            editor.physics_status = Some("没有瞄准可驾驶舰船".to_owned());
        },
    }
}

fn spaceship_possession_tool_can_target(tool_equipped: bool, active_user_id: Option<u64>) -> bool {
    tool_equipped && active_user_id.is_none()
}

fn possession_tool_can_target(tool_equipped: bool, active_user_id: Option<u64>) -> bool {
    tool_equipped && active_user_id.is_none()
}

fn ray_intersects_player_standee(
    ray: Ray3d,
    transform: &GlobalTransform,
    half_size: Vec2,
) -> Option<f32> {
    let inverse = transform.to_matrix().inverse();
    let local_origin = inverse.transform_point3(ray.origin);
    let local_direction = inverse.transform_vector3(*ray.direction);
    if local_direction.z.abs() <= f32::EPSILON {
        return None;
    }
    let local_distance = -local_origin.z / local_direction.z;
    if local_distance < 0.0 {
        return None;
    }
    let local_hit = local_origin + local_direction * local_distance;
    if local_hit.x.abs() > half_size.x || local_hit.y.abs() > half_size.y {
        return None;
    }
    let world_hit = transform.transform_point(local_hit);
    Some(ray.origin.distance(world_hit))
}

fn voxel_chunk_column(world_position: Vec3) -> IVec2 {
    let chunk_world_size = VOXEL_SIZE * DIMS.x as f32;
    IVec2::new(
        (world_position.x / chunk_world_size).floor() as i32,
        (world_position.z / chunk_world_size).floor() as i32,
    )
}

fn loaded_physics_chunk_columns(anchors: impl IntoIterator<Item = Vec3>) -> HashSet<IVec2> {
    let mut columns = HashSet::new();
    for anchor in anchors {
        let center = voxel_chunk_column(anchor);
        for z in -VOXEL_PHYSICS_CHUNK_LOAD_RADIUS..=VOXEL_PHYSICS_CHUNK_LOAD_RADIUS {
            for x in -VOXEL_PHYSICS_CHUNK_LOAD_RADIUS..=VOXEL_PHYSICS_CHUNK_LOAD_RADIUS {
                columns.insert(center + IVec2::new(x, z));
            }
        }
    }
    columns
}

fn update_loaded_voxel_physics_chunks(
    editor: Res<VoxelEditorState>,
    dm_camera: Query<&Transform, With<VoxelViewportCamera>>,
    player_cameras: Query<&Transform, With<VoxelPlayerCaptureCamera>>,
    mut loader: ResMut<VoxelPhysicsChunkLoader>,
) {
    let dm_position = if editor.first_person_enabled {
        dm_camera
            .single()
            .map(|transform| transform.translation)
            .unwrap_or(editor.camera_focus)
    } else {
        editor.camera_focus
    };
    let columns = loaded_physics_chunk_columns(
        std::iter::once(dm_position)
            .chain(player_cameras.iter().map(|transform| transform.translation)),
    );
    if loader.columns != columns {
        loader.columns = columns;
    }
}

fn voxel_physics_body_is_loaded(
    body: &VoxelPhysicsBody,
    transform: &Transform,
    loader: &VoxelPhysicsChunkLoader,
) -> bool {
    let affine = transform.compute_affine();
    body.cells.iter().any(|(cell, _)| {
        let local_center = (cell.as_vec3() + Vec3::splat(0.5)) * VOXEL_SIZE;
        loader.contains_world_position(affine.transform_point3(local_center))
    })
}

fn stream_voxel_physics_bodies(
    mut commands: Commands,
    mut loader: ResMut<VoxelPhysicsChunkLoader>,
    bodies: Query<(
        Entity,
        &VoxelPhysicsBody,
        &Transform,
        &LinearVelocity,
        &AngularVelocity,
    ), Without<VoxelSpaceship>>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    for (entity, body, transform, linear_velocity, angular_velocity) in &bodies {
        if voxel_physics_body_is_loaded(body, transform, &loader) {
            continue;
        }
        loader.unloaded_bodies.push(VoxelPhysicsBodySnapshot {
            body: body.clone(),
            transform: *transform,
            linear_velocity: *linear_velocity,
            angular_velocity: *angular_velocity,
        });
        commands.entity(entity).despawn();
    }

    let cached = std::mem::take(&mut loader.unloaded_bodies);
    for snapshot in cached {
        if !voxel_physics_body_is_loaded(
            &snapshot.body,
            &snapshot.transform,
            &loader,
        ) {
            loader.unloaded_bodies.push(snapshot);
            continue;
        }
        spawn_voxel_physics_body_at(
            &mut commands,
            &mut meshes,
            &materials,
            snapshot.body.cells,
            snapshot.transform,
            snapshot.linear_velocity,
            snapshot.angular_velocity,
        );
    }
}

fn rebuild_voxel_geometry(
    mut commands: Commands,
    grids: Query<&Grid<u8>, (With<TrpgVoxelGrid>, Changed<Grid<u8>>)>,
    old_geometry: Query<(Entity, &VoxelGeometry)>,
    mut dirty_chunks: ResMut<VoxelGeometryDirtyChunks>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
    micro_decorations: Res<VoxelMicroDecorations>,
) {
    let Ok(grid) = grids.single() else {
        return;
    };
    let rebuild_all = dirty_chunks.chunks.is_empty();
    let chunks = if rebuild_all {
        old_geometry
            .iter()
            .for_each(|(entity, _)| commands.entity(entity).despawn());
        grid.iter().map(|(chunk, _)| *chunk).collect::<HashSet<_>>()
    } else {
        let chunks = std::mem::take(&mut dirty_chunks.chunks);
        for (entity, geometry) in &old_geometry {
            if chunks.contains(&geometry.chunk) {
                commands.entity(entity).despawn();
            }
        }
        chunks
    };

    for chunk in chunks {
        let (material_meshes, collider_voxels) = build_voxel_chunk_meshes(grid, chunk);
        for (material_id, mesh) in material_meshes {
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.handles[material_id as usize - 1].clone()),
                VoxelGeometry { chunk },
            ));
        }
        let live_micro_tiles = micro_decorations
            .tiles
            .iter()
            .copied()
            .filter(|tile| {
                tile.owner.div_euclid(DIMS) == chunk
                    && grid.get(tile.owner).copied().unwrap_or(0) != 0
            })
            .collect::<Vec<_>>();
        for (material_id, mesh) in build_micro_tile_meshes(&live_micro_tiles) {
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.handles[material_id as usize - 1].clone()),
                VoxelGeometry { chunk },
                VoxelMicroDecoration,
            ));
        }
        if !collider_voxels.is_empty() {
            commands.spawn((
                RigidBody::Static,
                canonical_voxel_collider(&collider_voxels),
                VoxelGeometry { chunk },
            ));
        }
    }
}

fn build_voxel_chunk_meshes(
    grid: &Grid<u8>,
    chunk_position: IVec3,
) -> (Vec<(u8, Mesh)>, Vec<IVec3>) {
    let Some(chunk) = grid.get_chunk(chunk_position) else {
        return (Vec::new(), Vec::new());
    };
    let cells = prism(IVec3::ZERO, DIMS)
        .filter_map(|local| {
            let material = chunk[local];
            (material != 0).then_some((chunk_position * DIMS + local, material))
        })
        .collect::<Vec<_>>();
    if cells.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut occupied = HashMap::new();
    for neighbor in [
        IVec3::ZERO,
        IVec3::X,
        IVec3::NEG_X,
        IVec3::Y,
        IVec3::NEG_Y,
        IVec3::Z,
        IVec3::NEG_Z,
    ] {
        let neighbor_position = chunk_position + neighbor;
        let Some(neighbor_chunk) = grid.get_chunk(neighbor_position) else {
            continue;
        };
        for local in prism(IVec3::ZERO, DIMS) {
            let material = neighbor_chunk[local];
            if material != 0 {
                occupied.insert(
                    neighbor_position * DIMS + local,
                    material,
                );
            }
        }
    }
    build_voxel_meshes_from_cells_with_occupied(&cells, &occupied)
}

#[cfg(test)]
fn build_voxel_meshes(grid: &Grid<u8>) -> (Vec<(u8, Mesh)>, Vec<IVec3>) {
    let cells = grid
        .iter()
        .flat_map(|(chunk_position, chunk)| {
            prism(IVec3::ZERO, DIMS)
                .filter_map(|local| {
                    let material = chunk[local];
                    (material != 0).then_some((*chunk_position * DIMS + local, material))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    build_voxel_meshes_from_cells(&cells)
}

fn canonical_voxel_collider(cells: &[IVec3]) -> Collider {
    Collider::voxels(Vec3::splat(VOXEL_SIZE), cells)
}

fn build_voxel_meshes_from_cells(cells: &[(IVec3, u8)]) -> (Vec<(u8, Mesh)>, Vec<IVec3>) {
    let occupied = cells.iter().copied().collect::<HashMap<_, _>>();
    build_voxel_meshes_from_cells_with_occupied(cells, &occupied)
}

fn build_voxel_meshes_from_cells_with_occupied(
    cells: &[(IVec3, u8)],
    occupied: &HashMap<IVec3, u8>,
) -> (Vec<(u8, Mesh)>, Vec<IVec3>) {
    let mut material_meshes = Vec::new();
    let collider_voxels = cells
        .iter()
        .filter_map(|(cell, material)| TrpgVoxelConnector::solid(material).then_some(*cell))
        .collect::<Vec<_>>();

    for material in 1..=VOXEL_MATERIAL_COUNT as u8 {
        let mut positions = Vec::<[f32; 3]>::new();
        let mut normals = Vec::<[f32; 3]>::new();
        let mut uvs = Vec::<[f32; 2]>::new();
        let mut indices = Vec::<u32>::new();
        for &(cell, cell_material) in cells {
            if cell_material != material {
                continue;
            }
            append_voxel_faces(
                occupied,
                cell,
                &mut positions,
                &mut normals,
                &mut uvs,
                &mut indices,
            );
        }
        if positions.is_empty() {
            continue;
        }
        material_meshes.push((material, {
            let colors = vec![[1.0, 1.0, 1.0, 1.0]; positions.len()];
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_indices(Indices::U32(indices))
        }));
    }

    (material_meshes, collider_voxels)
}

fn build_micro_tile_meshes(tiles: &[VoxelMicroTile]) -> Vec<(u8, Mesh)> {
    let mut material_meshes = Vec::new();
    for material in 1..=VOXEL_MATERIAL_COUNT as u8 {
        let mut positions = Vec::<[f32; 3]>::new();
        let mut normals = Vec::<[f32; 3]>::new();
        let mut uvs = Vec::<[f32; 2]>::new();
        let mut indices = Vec::<u32>::new();
        for tile in tiles.iter().filter(|tile| tile.material == material) {
            append_micro_tile_faces(
                *tile,
                &mut positions,
                &mut normals,
                &mut uvs,
                &mut indices,
            );
        }
        if positions.is_empty() {
            continue;
        }
        material_meshes.push((material, {
            let colors = vec![[1.0, 1.0, 1.0, 1.0]; positions.len()];
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_indices(Indices::U32(indices))
        }));
    }
    material_meshes
}

fn append_micro_tile_faces(
    tile: VoxelMicroTile,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
) {
    let subdivisions = MICRO_TILE_SUBDIVISIONS as f32;
    let min = tile.cell.as_vec3() + tile.min.as_vec3() / subdivisions;
    let max = tile.cell.as_vec3() + tile.max.as_vec3() / subdivisions;
    let faces = [
        (IVec3::X, [
            [max.x, min.y, min.z],
            [max.x, max.y, min.z],
            [max.x, max.y, max.z],
            [max.x, min.y, max.z],
        ]),
        (IVec3::NEG_X, [
            [min.x, min.y, max.z],
            [min.x, max.y, max.z],
            [min.x, max.y, min.z],
            [min.x, min.y, min.z],
        ]),
        (IVec3::Y, [
            [min.x, max.y, max.z],
            [max.x, max.y, max.z],
            [max.x, max.y, min.z],
            [min.x, max.y, min.z],
        ]),
        (IVec3::NEG_Y, [
            [min.x, min.y, min.z],
            [max.x, min.y, min.z],
            [max.x, min.y, max.z],
            [min.x, min.y, max.z],
        ]),
        (IVec3::Z, [
            [max.x, min.y, max.z],
            [max.x, max.y, max.z],
            [min.x, max.y, max.z],
            [min.x, min.y, max.z],
        ]),
        (IVec3::NEG_Z, [
            [min.x, min.y, min.z],
            [min.x, max.y, min.z],
            [max.x, max.y, min.z],
            [max.x, min.y, min.z],
        ]),
    ];
    for (normal, corners) in faces {
        let base = positions.len() as u32;
        for (corner, uv) in corners
            .into_iter()
            .zip([[0., 1.], [0., 0.], [1., 0.], [1., 1.]])
        {
            positions.push((Vec3::from(corner) * VOXEL_SIZE).to_array());
            normals.push(normal.as_vec3().to_array());
            uvs.push(uv);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn append_voxel_faces(
    occupied: &HashMap<IVec3, u8>,
    cell: IVec3,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
) {
    const FACES: [(IVec3, [[f32; 3]; 4]); 6] = [
        (IVec3::X, [
            [1., 0., 0.],
            [1., 1., 0.],
            [1., 1., 1.],
            [1., 0., 1.],
        ]),
        (IVec3::NEG_X, [
            [0., 0., 1.],
            [0., 1., 1.],
            [0., 1., 0.],
            [0., 0., 0.],
        ]),
        (IVec3::Y, [
            [0., 1., 1.],
            [1., 1., 1.],
            [1., 1., 0.],
            [0., 1., 0.],
        ]),
        (IVec3::NEG_Y, [
            [0., 0., 0.],
            [1., 0., 0.],
            [1., 0., 1.],
            [0., 0., 1.],
        ]),
        (IVec3::Z, [
            [1., 0., 1.],
            [1., 1., 1.],
            [0., 1., 1.],
            [0., 0., 1.],
        ]),
        (IVec3::NEG_Z, [
            [0., 0., 0.],
            [0., 1., 0.],
            [1., 1., 0.],
            [1., 0., 0.],
        ]),
    ];
    for (normal, corners) in FACES {
        if occupied.get(&(cell + normal)).copied().unwrap_or(0) != 0 {
            continue;
        }
        let base = positions.len() as u32;
        for (corner, uv) in corners
            .into_iter()
            .zip([[0., 1.], [0., 0.], [1., 0.], [1., 1.]])
        {
            positions.push(((cell.as_vec3() + Vec3::from(corner)) * VOXEL_SIZE).to_array());
            normals.push(normal.as_vec3().to_array());
            uvs.push(uv);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn animate_voxel_materials(
    time: Res<Time>,
    voxel_materials: Res<VoxelMaterials>,
    fade_materials: Res<VoxelReplayFadeMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let seconds = time.elapsed_secs();
    let water_uv = Affine2::from_translation(Vec2::new(
        seconds * 0.035,
        (seconds * 0.021).sin() * 0.08,
    ));
    for handle in [&voxel_materials.handles[3], &voxel_materials.planet_ocean] {
        if let Some(mut water) = materials.get_mut(handle) {
            water.uv_transform = water_uv;
        }
    }
    for handle in [&fade_materials.handles[3], &fade_materials.planet_ocean] {
        if let Some(mut water) = materials.get_mut(handle) {
            water.uv_transform = water_uv;
        }
    }
    if let Some(mut lava) = materials.get_mut(&voxel_materials.handles[4]) {
        lava.uv_transform = Affine2::from_translation(Vec2::new(
            seconds * -0.018,
            seconds * 0.027,
        ));
        let pulse = 4.5 + (seconds * 2.4).sin() * 1.2;
        lava.emissive = voxel_emissive(pulse, pulse * 0.11, 0.015);
    }
    if let Some(mut lava) = materials.get_mut(&fade_materials.handles[4]) {
        lava.uv_transform = Affine2::from_translation(Vec2::new(
            seconds * -0.018,
            seconds * 0.027,
        ));
        let pulse = 4.5 + (seconds * 2.4).sin() * 1.2;
        lava.emissive = voxel_emissive(pulse, pulse * 0.11, 0.015);
    }
}

fn sync_voxel_occlusion_fade(
    mut commands: Commands,
    fade: Res<VoxelReplayOcclusionFade>,
    voxel_materials: Res<VoxelMaterials>,
    fade_materials: Res<VoxelReplayFadeMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    voxel_meshes: Query<
        (
            Entity,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            &Aabb,
            &GlobalTransform,
            Option<&VoxelOcclusionMesh>,
        ),
        Without<VoxelOcclusionTransparentMesh>,
    >,
) {
    let opacity = fade.opacity.clamp(0.0, 1.0);
    for handle in fade_materials
        .handles
        .iter()
        .chain(std::iter::once(&fade_materials.planet_ocean))
    {
        if let Some(mut material) = materials.get_mut(handle) {
            material.base_color = material.base_color.with_alpha(opacity);
        }
    }

    let fade_enabled = fade.active && opacity < 0.999 && !fade.targets.is_empty();
    let broadphase_half_extent = voxel_occlusion_cast_broadphase_half_extent(&fade);
    for (entity, current_mesh, material, aabb, transform, occlusion_mesh) in &voxel_meshes {
        let original_mesh = occlusion_mesh
            .map(|state| &state.original_mesh)
            .unwrap_or(&current_mesh.0);
        let Some(fade_handle) =
            replay_fade_handle(material, &voxel_materials, &fade_materials)
        else {
            continue;
        };

        if !fade_enabled
            || (occlusion_mesh.is_none()
                && !replay_sightline_intersects_any_aabb(
                    fade.camera,
                    &fade.targets,
                    broadphase_half_extent,
                    aabb,
                    transform,
                ))
        {
            if let Some(state) = occlusion_mesh {
                restore_voxel_occlusion_mesh(&mut commands, &mut meshes, entity, state);
            }
            continue;
        }

        let Some(source_mesh) = meshes.get(original_mesh).cloned() else {
            continue;
        };
        let Some((opaque_mesh, transparent_mesh)) =
            partition_voxel_mesh_for_replay_cast(&source_mesh, transform, &fade)
        else {
            if let Some(state) = occlusion_mesh {
                restore_voxel_occlusion_mesh(&mut commands, &mut meshes, entity, state);
            }
            continue;
        };

        if let Some(state) = occlusion_mesh {
            if let Some(mut mesh) = meshes.get_mut(&state.opaque_mesh) {
                *mesh = opaque_mesh;
            }
            if let Some(mut mesh) = meshes.get_mut(&state.transparent_mesh) {
                *mesh = transparent_mesh;
            }
        } else {
            let original_mesh = current_mesh.0.clone();
            let opaque_mesh = meshes.add(opaque_mesh);
            let transparent_mesh = meshes.add(transparent_mesh);
            let transparent_entity = commands
                .spawn((
                    Mesh3d(transparent_mesh.clone()),
                    MeshMaterial3d(fade_handle),
                    Transform::IDENTITY,
                    VoxelOcclusionTransparentMesh,
                ))
                .id();
            commands.entity(entity).add_child(transparent_entity).insert((
                Mesh3d(opaque_mesh.clone()),
                VoxelOcclusionMesh {
                    original_mesh,
                    opaque_mesh,
                    transparent_mesh,
                    transparent_entity,
                },
            ));
        }
    }
}

fn restore_voxel_occlusion_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    entity: Entity,
    state: &VoxelOcclusionMesh,
) {
    commands
        .entity(entity)
        .insert(Mesh3d(state.original_mesh.clone()))
        .remove::<VoxelOcclusionMesh>();
    commands.entity(state.transparent_entity).despawn();
    meshes.remove(state.opaque_mesh.id());
    meshes.remove(state.transparent_mesh.id());
}

fn partition_voxel_mesh_for_replay_cast(
    mesh: &Mesh,
    transform: &GlobalTransform,
    fade: &VoxelReplayOcclusionFade,
) -> Option<(Mesh, Mesh)> {
    let VertexAttributeValues::Float32x3(positions) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
    else {
        return None;
    };
    let VertexAttributeValues::Float32x3(normals) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)? else {
        return None;
    };
    let indices = match mesh.indices()? {
        Indices::U16(indices) => indices.iter().map(|index| *index as u32).collect::<Vec<_>>(),
        Indices::U32(indices) => indices.clone(),
    };
    let mut opaque_indices = Vec::with_capacity(indices.len());
    let mut transparent_indices = Vec::new();
    for triangle in indices.chunks_exact(3) {
        let first = triangle[0] as usize;
        let second = triangle[1] as usize;
        let third = triangle[2] as usize;
        let surface_center = (
            Vec3::from(positions[first])
                + Vec3::from(positions[second])
                + Vec3::from(positions[third])
        ) / 3.0;
        let surface_normal = Vec3::from(normals[first]);
        let inside_position = surface_center - surface_normal * VOXEL_SIZE * 0.01;
        let cell_center =
            (inside_position / VOXEL_SIZE).floor() * VOXEL_SIZE + Vec3::splat(VOXEL_SIZE * 0.5);
        let world_center = transform.transform_point(cell_center);
        let destination = if voxel_is_touched_by_replay_cast(world_center, transform, fade) {
            &mut transparent_indices
        } else {
            &mut opaque_indices
        };
        destination.extend_from_slice(triangle);
    }
    if transparent_indices.is_empty() {
        return None;
    }

    let mut opaque_mesh = mesh.clone();
    opaque_mesh.insert_indices(Indices::U32(opaque_indices));
    let mut transparent_mesh = mesh.clone();
    transparent_mesh.insert_indices(Indices::U32(transparent_indices));
    Some((opaque_mesh, transparent_mesh))
}

fn voxel_is_touched_by_replay_cast(
    world_center: Vec3,
    transform: &GlobalTransform,
    fade: &VoxelReplayOcclusionFade,
) -> bool {
    let affine = transform.affine();
    let cell_half_axes = [
        affine.transform_vector3(Vec3::X * VOXEL_SIZE * 0.5),
        affine.transform_vector3(Vec3::Y * VOXEL_SIZE * 0.5),
        affine.transform_vector3(Vec3::Z * VOXEL_SIZE * 0.5),
    ];
    fade.targets.iter().any(|target| {
        let Some((forward, right, up, length)) = replay_cast_frame(fade.camera, *target) else {
            return false;
        };
        let offset = world_center - fade.camera;
        let forward_radius = projected_voxel_half_extent(cell_half_axes, forward);
        let right_radius = projected_voxel_half_extent(cell_half_axes, right);
        let up_radius = projected_voxel_half_extent(cell_half_axes, up);
        let along = offset.dot(forward);
        let progress = (along / length).clamp(0.0, 1.0);
        let cast_size = voxel_occlusion_cast_size_at(fade, progress);
        along >= -forward_radius
            && along <= length + forward_radius
            && offset.dot(right).abs() <= cast_size.x * 0.5 + right_radius
            && offset.dot(up).abs() <= cast_size.y * 0.5 + up_radius
    })
}

fn projected_voxel_half_extent(cell_half_axes: [Vec3; 3], axis: Vec3) -> f32 {
    cell_half_axes
        .iter()
        .map(|half_axis| half_axis.dot(axis).abs())
        .sum()
}

fn replay_cast_frame(camera: Vec3, target: Vec3) -> Option<(Vec3, Vec3, Vec3, f32)> {
    let sightline = target - camera;
    let length = sightline.length();
    if length <= f32::EPSILON {
        return None;
    }
    let forward = sightline / length;
    let horizontal = forward.cross(Vec3::Y);
    let right = if horizontal.length_squared() > 1.0e-6 {
        horizontal.normalize()
    } else {
        Vec3::X
    };
    let up = right.cross(forward).normalize();
    Some((forward, right, up, length))
}

fn voxel_occlusion_cast_width(width_cells: f32) -> f32 {
    width_cells.clamp(
        MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
        MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
    ) * VOXEL_SIZE
}

fn voxel_occlusion_cast_height(height_cells: f32) -> f32 {
    height_cells.clamp(
        MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
        MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
    ) * VOXEL_SIZE
}

fn voxel_occlusion_cast_size_at(fade: &VoxelReplayOcclusionFade, progress: f32) -> Vec2 {
    let start = Vec2::new(
        voxel_occlusion_cast_width(fade.cast_width_cells),
        voxel_occlusion_cast_height(fade.cast_height_cells),
    );
    let end = Vec2::new(
        voxel_occlusion_cast_width(fade.cast_end_width_cells),
        voxel_occlusion_cast_height(fade.cast_end_height_cells),
    );
    start.lerp(end, progress.clamp(0.0, 1.0))
}

fn voxel_occlusion_cast_broadphase_half_extent(fade: &VoxelReplayOcclusionFade) -> f32 {
    voxel_occlusion_cast_width(fade.cast_width_cells)
        .max(voxel_occlusion_cast_height(fade.cast_height_cells))
        .max(voxel_occlusion_cast_width(fade.cast_end_width_cells))
        .max(voxel_occlusion_cast_height(fade.cast_end_height_cells))
        * 0.5
        + VOXEL_SIZE
}

fn draw_voxel_occlusion_cast_gizmos(
    mut gizmos: Gizmos,
    fade: Res<VoxelReplayOcclusionFade>,
) {
    if !fade.active || !fade.debug_gizmo {
        return;
    }
    let start_size = voxel_occlusion_cast_size_at(&fade, 0.0);
    let end_size = voxel_occlusion_cast_size_at(&fade, 1.0);
    for target in &fade.targets {
        let Some((_, right, up, _)) = replay_cast_frame(fade.camera, *target) else {
            continue;
        };
        let start_corners = replay_cast_corners(fade.camera, right, up, start_size);
        let end_corners = replay_cast_corners(*target, right, up, end_size);
        let color = Color::srgb(0.1, 0.95, 1.0);
        for index in 0..4 {
            let next = (index + 1) % 4;
            gizmos.line(start_corners[index], start_corners[next], color);
            gizmos.line(end_corners[index], end_corners[next], color);
            gizmos.line(start_corners[index], end_corners[index], color);
        }
        gizmos.line(fade.camera, *target, Color::srgb(1.0, 0.2, 0.8));
        gizmos.sphere(
            Isometry3d::from_translation(*target),
            VOXEL_SIZE * 0.35,
            Color::srgb(1.0, 0.85, 0.1),
        );
    }
}

fn replay_cast_corners(center: Vec3, right: Vec3, up: Vec3, size: Vec2) -> [Vec3; 4] {
    let right = right * size.x * 0.5;
    let up = up * size.y * 0.5;
    [
        center - right - up,
        center + right - up,
        center + right + up,
        center - right + up,
    ]
}

fn replay_sightline_intersects_any_aabb(
    camera: Vec3,
    targets: &[Vec3],
    touched_voxel_half_extent: f32,
    aabb: &Aabb,
    transform: &GlobalTransform,
) -> bool {
    targets.iter().any(|target| {
        replay_sightline_intersects_aabb(
            camera,
            *target,
            touched_voxel_half_extent,
            aabb,
            transform,
        )
    })
}

fn replay_sightline_intersects_aabb(
    camera: Vec3,
    focus: Vec3,
    half_extent: f32,
    aabb: &Aabb,
    transform: &GlobalTransform,
) -> bool {
    let sightline = focus - camera;
    let sightline_length = sightline.length();
    if sightline_length <= f32::EPSILON {
        return false;
    }
    let sightline_direction = sightline / sightline_length;
    let local_center = Vec3::from(aabb.center);
    let local_half_extents = Vec3::from(aabb.half_extents);
    let mut world_min = Vec3::splat(f32::INFINITY);
    let mut world_max = Vec3::splat(f32::NEG_INFINITY);
    let mut nearest_along_sightline = f32::INFINITY;
    let mut farthest_along_sightline = f32::NEG_INFINITY;
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                let local_corner = local_center + local_half_extents * Vec3::new(x, y, z);
                let world_corner = transform.transform_point(local_corner);
                world_min = world_min.min(world_corner);
                world_max = world_max.max(world_corner);
                let along_sightline = (world_corner - camera).dot(sightline_direction);
                nearest_along_sightline = nearest_along_sightline.min(along_sightline);
                farthest_along_sightline = farthest_along_sightline.max(along_sightline);
            }
        }
    }
    if nearest_along_sightline >= sightline_length || farthest_along_sightline <= 0.0 {
        return false;
    }
    world_min -= Vec3::splat(half_extent);
    world_max += Vec3::splat(half_extent);
    segment_intersects_bounds(camera, focus, world_min, world_max)
}

fn segment_intersects_bounds(camera: Vec3, focus: Vec3, min: Vec3, max: Vec3) -> bool {
    let direction = focus - camera;
    let mut near = 0.0_f32;
    let mut far = 1.0_f32;
    for axis in 0..3 {
        let origin = camera[axis];
        let delta = direction[axis];
        if delta.abs() <= f32::EPSILON {
            if origin < min[axis] || origin > max[axis] {
                return false;
            }
            continue;
        }
        let inverse = delta.recip();
        let first = (min[axis] - origin) * inverse;
        let second = (max[axis] - origin) * inverse;
        near = near.max(first.min(second));
        far = far.min(first.max(second));
        if near > far {
            return false;
        }
    }
    true
}

fn replay_fade_handle(
    material: &MeshMaterial3d<StandardMaterial>,
    voxel_materials: &VoxelMaterials,
    fade_materials: &VoxelReplayFadeMaterials,
) -> Option<Handle<StandardMaterial>> {
    voxel_materials
        .handles
        .iter()
        .zip(&fade_materials.handles)
        .find_map(|(normal, fade)| (material.0 == *normal).then(|| fade.clone()))
        .or_else(|| {
            (material.0 == voxel_materials.planet_ocean)
                .then(|| fade_materials.planet_ocean.clone())
        })
}

fn handle_editor_requests(
    mut commands: Commands,
    mut editor: ResMut<VoxelEditorState>,
    mut physics_loader: ResMut<VoxelPhysicsChunkLoader>,
    mut persistence: ResMut<VoxelScenePersistenceState>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    physics_bodies: Query<Entity, With<VoxelPhysicsBody>>,
    placed_lights: Query<Entity, With<VoxelPlacedLight>>,
    mut planets: Query<&mut VoxelOrbitalPlanet>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
    mut spaceship_store: Option<ResMut<Persistent<VoxelSpaceshipStore>>>,
    mut spaceship_control: Option<ResMut<VoxelSpaceshipControlState>>,
) {
    let Ok(mut grid) = grids.single_mut() else {
        return;
    };
    if editor.reset_requested {
        physics_loader.unloaded_bodies.clear();
        for entity in &physics_bodies {
            commands.entity(entity).despawn();
        }
        for entity in &placed_lights {
            commands.entity(entity).despawn();
        }
        let occupied = occupied_cells(&grid);
        for position in occupied {
            grid.set(position, 0);
        }
        populate_default_grid(&mut grid);
        remove_static_combat_spaceship(&mut grid);
        if let Ok(mut planet) = planets.single_mut() {
            planet.cells = voxel_orbital_planet_cells().into_iter().collect();
            planet.refresh_cell_bounds();
            planet.removed.clear();
            planet.dirty = true;
        }
        spawn_default_voxel_physics_props(&mut commands, &mut meshes, &materials);
        if let Some(store) = spaceship_store.as_deref_mut() {
            spawn_default_voxel_spaceships(
                &mut commands,
                &mut meshes,
                &materials,
                store,
                true,
            );
        }
        if let Some(control) = spaceship_control.as_deref_mut() {
            control.stop_driving();
            control.selected_ship_id = Some(COMBAT_SPACESHIP_ID.to_owned());
        }
        editor.undo.clear();
        editor.redo.clear();
        editor.selection_anchor = None;
        editor.selection_end = None;
        editor.selection_is_planet = false;
        editor.selected_light = None;
        editor.physics_status = Some("Scene reset to defaults and saved".to_owned());
        editor.reset_requested = false;
        persistence.force_save = true;
    }
    if editor.undo_requested {
        if let Some(stroke) = editor.undo.pop() {
            apply_stroke(&mut grid, &stroke, false);
            editor.redo.push(stroke);
        }
        editor.undo_requested = false;
    }
    if editor.redo_requested {
        if let Some(stroke) = editor.redo.pop() {
            apply_stroke(&mut grid, &stroke, true);
            editor.undo.push(stroke);
        }
        editor.redo_requested = false;
    }
}

fn make_selection_physical(
    mut commands: Commands,
    mut editor: ResMut<VoxelEditorState>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    mut planets: Query<(
        &mut VoxelOrbitalPlanet,
        &GlobalTransform,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    if !editor.physics_requested {
        return;
    }
    editor.physics_requested = false;
    let Some((min, max)) = editor.selection_bounds() else {
        editor.physics_status = Some("请先用两个角点框选区域".to_owned());
        return;
    };
    if editor.selection_is_planet {
        let Ok((mut planet, planet_transform)) = planets.single_mut() else {
            return;
        };
        let mut selected_voxels = planet
            .cells
            .iter()
            .filter_map(|(cell, material)| {
                (cell.cmpge(min).all()
                    && cell.cmple(max).all()
                    && TrpgVoxelConnector::solid(material))
                .then_some((*cell, *material))
            })
            .collect::<Vec<_>>();
        selected_voxels.sort_unstable_by_key(|(cell, _)| (cell.y, cell.z, cell.x));
        let voxel_count = selected_voxels.len();
        if voxel_count == 0 {
            editor.physics_status = Some("行星选区内没有可物理化的固体方块".to_owned());
            return;
        }
        for (cell, _) in &selected_voxels {
            dig_planet_voxel(&mut planet, *cell);
        }
        let origin = selected_voxels
            .iter()
            .map(|(cell, _)| *cell)
            .reduce(IVec3::min)
            .unwrap_or(IVec3::ZERO);
        let local_cells = selected_voxels
            .into_iter()
            .map(|(cell, material)| (cell - origin, material))
            .collect();
        let transform = Transform::from_matrix(
            planet_transform.to_matrix()
                * Mat4::from_translation(
                    origin.as_vec3() * VOXEL_SIZE - Vec3::splat(VOXEL_SIZE * 0.5),
                ),
        );
        spawn_voxel_physics_body_at(
            &mut commands,
            &mut meshes,
            &materials,
            local_cells,
            transform,
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
        );
        editor.selection_anchor = None;
        editor.selection_end = None;
        editor.selection_is_planet = false;
        editor.physics_status = Some(format!(
            "已将 {voxel_count} 个行星方块生成 1 个物理体"
        ));
        return;
    }
    let Ok(mut grid) = grids.single_mut() else {
        return;
    };

    let selected_voxels = selected_solid_voxels(&grid, min, max);
    let voxel_count = selected_voxels.len();
    if voxel_count == 0 {
        editor.physics_status = Some("选区内没有可物理化的固体方块".to_owned());
        return;
    }

    for (cell, _) in &selected_voxels {
        grid.set(*cell, 0);
    }
    spawn_voxel_physics_body(
        &mut commands,
        &mut meshes,
        &materials,
        selected_voxels,
    );

    editor.selection_anchor = None;
    editor.selection_end = None;
    editor.selection_is_planet = false;
    editor.physics_status = Some(format!(
        "已将 {voxel_count} 个方块生成 1 个物理体"
    ));
}

fn selected_solid_voxels(grid: &Grid<u8>, min: IVec3, max: IVec3) -> Vec<(IVec3, u8)> {
    prism(min, max + IVec3::ONE)
        .filter_map(|cell| {
            let material = grid.get(cell).copied()?;
            TrpgVoxelConnector::solid(&material).then_some((cell, material))
        })
        .collect()
}

fn selected_solid_voxels_in_radius(grid: &Grid<u8>, origin: Vec3, radius: f32) -> Vec<(IVec3, u8)> {
    let radius = radius.max(VOXEL_SIZE);
    let radius_squared = radius * radius;
    let mut selected = grid
        .iter()
        .flat_map(|(chunk_position, chunk)| {
            prism(IVec3::ZERO, DIMS).filter_map(move |local| {
                let material = chunk[local];
                if !TrpgVoxelConnector::solid(&material) {
                    return None;
                }
                let cell = *chunk_position * DIMS + local;
                let center = (cell.as_vec3() + Vec3::splat(0.5)) * VOXEL_SIZE;
                (center.distance_squared(origin) <= radius_squared).then_some((cell, material))
            })
        })
        .collect::<Vec<_>>();
    // Hash-map chunk iteration is unordered. Preserve the old prism traversal order so
    // fragment allocation remains deterministic while avoiding a radius-cubed scan.
    selected.sort_unstable_by_key(|(cell, _)| (cell.y, cell.z, cell.x));
    selected
}

fn physics_body_intersects_radius(
    body: &VoxelPhysicsBody,
    transform: &Transform,
    origin: Vec3,
    radius: f32,
) -> bool {
    let radius_squared = radius.max(VOXEL_SIZE).powi(2);
    let affine = transform.compute_affine();
    body.cells.iter().any(|(cell, _)| {
        let local_center = (cell.as_vec3() + Vec3::splat(0.5)) * VOXEL_SIZE;
        affine
            .transform_point3(local_center)
            .distance_squared(origin)
            <= radius_squared
    })
}

fn allocate_fragment_parts(source_sizes: &[usize], max_parts: usize) -> Vec<usize> {
    let mut counts = vec![0; source_sizes.len()];
    let initial_parts = source_sizes
        .iter()
        .filter(|size| **size > 0)
        .count()
        .min(max_parts);
    for (count, _) in counts
        .iter_mut()
        .zip(source_sizes.iter())
        .filter(|(_, size)| **size > 0)
        .take(initial_parts)
    {
        *count = 1;
    }

    let mut remaining = max_parts.saturating_sub(initial_parts);
    while remaining > 0 {
        let Some(index) = (0..source_sizes.len())
            .filter(|index| counts[*index] > 0 && counts[*index] < source_sizes[*index])
            .max_by(|left, right| {
                (source_sizes[*left] * counts[*right]).cmp(&(source_sizes[*right] * counts[*left]))
            })
        else {
            break;
        };
        counts[index] += 1;
        remaining -= 1;
    }
    counts
}

#[derive(Clone, Copy)]
struct FragmentRng(u64);

impl FragmentRng {
    fn new(seed: u64) -> Self { Self(seed.max(1)) }

    fn next(&mut self) -> u64 {
        // A small deterministic generator keeps fragmentation testable while each
        // explosion supplies a different seed.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn index(&mut self, len: usize) -> usize { (self.next() as usize) % len }
}

const VOXEL_NEIGHBORS: [IVec3; 6] = [
    IVec3::X,
    IVec3::NEG_X,
    IVec3::Y,
    IVec3::NEG_Y,
    IVec3::Z,
    IVec3::NEG_Z,
];

fn explosion_fragment_seed(origin: Vec3, sequence: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    origin.x.to_bits().hash(&mut hasher);
    origin.y.to_bits().hash(&mut hasher);
    origin.z.to_bits().hash(&mut hasher);
    sequence.hash(&mut hasher);
    hasher.finish()
}

fn split_voxel_cells_randomly(
    mut cells: Vec<(IVec3, u8)>,
    requested_parts: usize,
    seed: u64,
) -> Vec<Vec<(IVec3, u8)>> {
    let part_count = requested_parts.min(cells.len());
    if part_count == 0 {
        return Vec::new();
    }

    let mut rng = FragmentRng::new(seed);
    for index in (1..cells.len()).rev() {
        let other = rng.index(index + 1);
        cells.swap(index, other);
    }

    let mut unassigned = cells.iter().copied().collect::<HashMap<_, _>>();
    let mut parts = vec![Vec::new(); part_count];
    let mut frontiers = vec![Vec::new(); part_count];

    for (part_index, (cell, _)) in cells.iter().copied().take(part_count).enumerate() {
        let material = unassigned.remove(&cell).unwrap();
        parts[part_index].push((cell, material));
        frontiers[part_index].extend(VOXEL_NEIGHBORS.map(|offset| cell + offset));
    }

    while !unassigned.is_empty() {
        let smallest_size = parts
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                frontiers[*index]
                    .iter()
                    .any(|cell| unassigned.contains_key(cell))
            })
            .map(|(_, part)| part.len())
            .min();

        let Some(smallest_size) = smallest_size else {
            // Disconnected source components can exhaust every active frontier.
            // Keep the body budget fixed and attach a new island to a smallest part.
            let part_index = parts
                .iter()
                .enumerate()
                .min_by_key(|(_, part)| part.len())
                .map(|(index, _)| index)
                .unwrap_or(0);
            let remaining = cells
                .iter()
                .map(|(cell, _)| *cell)
                .filter(|cell| unassigned.contains_key(cell))
                .collect::<Vec<_>>();
            let cell = remaining[rng.index(remaining.len())];
            let material = unassigned.remove(&cell).unwrap();
            parts[part_index].push((cell, material));
            frontiers[part_index].extend(VOXEL_NEIGHBORS.map(|offset| cell + offset));
            continue;
        };

        // Let any nearly-smallest cluster grow. This balances mass without creating
        // the regular bands produced by sorted list slicing.
        let candidates = parts
            .iter()
            .enumerate()
            .filter(|(index, part)| {
                part.len() <= smallest_size + 1
                    && frontiers[*index]
                        .iter()
                        .any(|cell| unassigned.contains_key(cell))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let part_index = candidates[rng.index(candidates.len())];

        loop {
            let frontier_index = rng.index(frontiers[part_index].len());
            let cell = frontiers[part_index].swap_remove(frontier_index);
            let Some(material) = unassigned.remove(&cell) else {
                continue;
            };
            parts[part_index].push((cell, material));
            frontiers[part_index].extend(VOXEL_NEIGHBORS.map(|offset| cell + offset));
            break;
        }
    }
    parts
}

fn spawn_voxel_physics_body(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &VoxelMaterials,
    component: Vec<(IVec3, u8)>,
) -> Entity {
    let origin = component
        .iter()
        .map(|(cell, _)| *cell)
        .reduce(IVec3::min)
        .unwrap_or(IVec3::ZERO);
    let mut local_cells = Vec::with_capacity(component.len());
    for (cell, material) in component {
        let local = cell - origin;
        local_cells.push((local, material));
    }
    spawn_voxel_physics_body_at(
        commands,
        meshes,
        materials,
        local_cells,
        Transform::from_translation(origin.as_vec3() * VOXEL_SIZE),
        LinearVelocity::ZERO,
        AngularVelocity::ZERO,
    )
}

fn spawn_voxel_physics_body_at(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &VoxelMaterials,
    cells: Vec<(IVec3, u8)>,
    transform: Transform,
    linear_velocity: LinearVelocity,
    angular_velocity: AngularVelocity,
) -> Entity {
    let collider_voxels = cells.iter().map(|(cell, _)| *cell).collect::<Vec<_>>();
    let (material_meshes, _) = build_voxel_meshes_from_cells(&cells);
    let local_max = collider_voxels
        .iter()
        .copied()
        .reduce(IVec3::max)
        .unwrap_or(IVec3::ZERO);
    let local_center = (local_max + IVec3::ONE).as_vec3() * VOXEL_SIZE * 0.5;
    commands
        .spawn((
            VoxelPhysicsBody {
                local_center,
                cells,
            },
            VoxelPlanetGravityBody,
            RigidBody::Dynamic,
            canonical_voxel_collider(&collider_voxels),
            Friction::new(0.8),
            linear_velocity,
            angular_velocity,
            LinearDamping(0.15),
            AngularDamping(0.35),
            transform,
            Visibility::Visible,
        ))
        .with_children(|parent| {
            for (material_id, mesh) in material_meshes {
                parent.spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.handles[material_id as usize - 1].clone()),
                ));
            }
        })
        .id()
}

fn spawn_planet_explosion_fragments(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &VoxelMaterials,
    planet_transform: &GlobalTransform,
    removed: Vec<(IVec3, u8)>,
    fragment_seed: u64,
) -> Vec<Entity> {
    let part_count = removed.len().min(MAX_EXPLOSION_NEW_PHYSICS_BODIES);
    let mut fragments = Vec::with_capacity(part_count);
    for cells in split_voxel_cells_randomly(removed, part_count, fragment_seed) {
        let origin = cells
            .iter()
            .map(|(cell, _)| *cell)
            .reduce(IVec3::min)
            .unwrap_or(IVec3::ZERO);
        let local_cells = cells
            .into_iter()
            .map(|(cell, material)| (cell - origin, material))
            .collect::<Vec<_>>();
        let transform = Transform::from_matrix(
            planet_transform.to_matrix()
                * Mat4::from_translation(
                    origin.as_vec3() * VOXEL_SIZE - Vec3::splat(VOXEL_SIZE * 0.5),
                ),
        );
        fragments.push(spawn_voxel_physics_body_at(
            commands,
            meshes,
            materials,
            local_cells,
            transform,
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
        ));
    }
    fragments
}

fn process_voxel_scene_history(
    mut commands: Commands,
    mut editor: ResMut<VoxelEditorState>,
    mut physics_loader: ResMut<VoxelPhysicsChunkLoader>,
    mut persistence: ResMut<VoxelScenePersistenceState>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    physics_bodies: Query<(
        Entity,
        &VoxelPhysicsBody,
        &Transform,
        &LinearVelocity,
        &AngularVelocity,
    ), Without<VoxelSpaceship>>,
    placed_lights: Query<(Entity, &VoxelPlacedLight)>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    if editor.save_scene_requested {
        editor.save_scene_requested = false;
        let Ok(grid) = grids.single() else {
            return;
        };
        let voxels = voxel_cells(grid);
        let mut physics_bodies = physics_bodies
            .iter()
            .map(
                |(_, body, transform, linear_velocity, angular_velocity)| {
                    VoxelPhysicsBodySnapshot {
                        body: body.clone(),
                        transform: *transform,
                        linear_velocity: *linear_velocity,
                        angular_velocity: *angular_velocity,
                    }
                },
            )
            .collect::<Vec<_>>();
        physics_bodies.extend(physics_loader.unloaded_bodies.iter().cloned());
        let placed_lights = placed_lights
            .iter()
            .map(|(_, light)| light.clone())
            .collect::<Vec<_>>();
        let snapshot_number = editor.next_scene_snapshot_number;
        editor.next_scene_snapshot_number += 1;
        editor.scene_snapshots.push(VoxelSceneSnapshot {
            name: format!("场景快照 {snapshot_number}"),
            voxels,
            physics_bodies,
            placed_lights,
        });
        if editor.scene_snapshots.len() > MAX_SCENE_SNAPSHOTS {
            editor.scene_snapshots.remove(0);
        }
        let snapshot = editor.scene_snapshots.last().unwrap();
        editor.physics_status = Some(format!(
            "已保存 {}：{} 个方块，{} 个物理体，{} 盏灯",
            snapshot.name,
            snapshot.voxels.len(),
            snapshot.physics_bodies.len(),
            snapshot.placed_lights.len()
        ));
        persistence.force_save = true;
    }

    let Some(snapshot_index) = editor.restore_scene_requested.take() else {
        return;
    };
    let Some(snapshot) = editor.scene_snapshots.get(snapshot_index).cloned() else {
        return;
    };
    let Ok(mut grid) = grids.single_mut() else {
        return;
    };
    for cell in occupied_cells(&grid) {
        grid.set(cell, 0);
    }
    for (cell, material) in &snapshot.voxels {
        grid.set(*cell, *material);
    }
    for (entity, ..) in &physics_bodies {
        commands.entity(entity).despawn();
    }
    physics_loader.unloaded_bodies.clear();
    for (entity, _) in &placed_lights {
        commands.entity(entity).despawn();
    }
    for body in &snapshot.physics_bodies {
        spawn_voxel_physics_body_at(
            &mut commands,
            &mut meshes,
            &materials,
            body.body.cells.clone(),
            body.transform,
            body.linear_velocity,
            body.angular_velocity,
        );
    }
    for light in &snapshot.placed_lights {
        spawn_voxel_placed_light(
            &mut commands,
            &mut meshes,
            &materials,
            light.clone(),
        );
    }

    editor.undo.clear();
    editor.redo.clear();
    editor.active_stroke.clear();
    editor.stroke_positions.clear();
    editor.selection_anchor = None;
    editor.selection_end = None;
    editor.selection_is_planet = false;
    editor.selected_light = None;
    editor.physics_action_requested = None;
    editor.physics_status = Some(format!("已恢复 {}", snapshot.name));
    persistence.force_save = true;
}

fn voxel_cells(grid: &Grid<u8>) -> Vec<(IVec3, u8)> {
    grid.iter()
        .flat_map(|(chunk_position, chunk)| {
            prism(IVec3::ZERO, DIMS)
                .filter_map(|local| {
                    let material = chunk[local];
                    (material != 0).then_some((*chunk_position * DIMS + local, material))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn voxel_minimap_snapshot_from_cells(
    cells: &[(IVec3, u8)],
    resolution: usize,
) -> VoxelMinimapSnapshot {
    let resolution = resolution.max(1);
    let Some((first_cell, _)) = cells.first() else {
        return VoxelMinimapSnapshot::default();
    };
    let (mut min, mut max) = (first_cell.xz(), first_cell.xz());
    for (cell, _) in &cells[1..] {
        min = min.min(cell.xz());
        max = max.max(cell.xz());
    }

    let span = (max - min).max(IVec2::ONE);
    let last = resolution.saturating_sub(1) as i64;
    let tile_axis = |value: i32, axis_min: i32, axis_span: i32| {
        (((value - axis_min) as i64 * last) / i64::from(axis_span)) as usize
    };
    let mut snapshot = VoxelMinimapSnapshot {
        min,
        max,
        resolution,
        tiles: vec![None; resolution * resolution],
    };
    for (cell, material) in cells {
        let x = tile_axis(cell.x, min.x, span.x);
        let z = tile_axis(cell.z, min.y, span.y);
        let tile = &mut snapshot.tiles[z * resolution + x];
        match tile {
            Some(tile) => {
                tile.voxel_count = tile.voxel_count.saturating_add(1);
                if cell.y > tile.top_cell.y {
                    tile.top_cell = *cell;
                    tile.material = *material;
                }
            },
            None => {
                *tile = Some(VoxelMinimapTile {
                    top_cell: *cell,
                    material: *material,
                    voxel_count: 1,
                });
            },
        }
    }
    snapshot
}

fn refresh_voxel_minimap_snapshot(
    grids: Query<&Grid<u8>, (With<TrpgVoxelGrid>, Changed<Grid<u8>>)>,
    mut snapshot: ResMut<VoxelMinimapSnapshot>,
) {
    let Ok(grid) = grids.single() else {
        return;
    };
    *snapshot = voxel_minimap_snapshot_from_cells(
        &voxel_cells(grid),
        VOXEL_MINIMAP_RESOLUTION,
    );
}

fn apply_voxel_physics_action(
    mut editor: ResMut<VoxelEditorState>,
    cameras: Query<&GlobalTransform, With<VoxelViewportCamera>>,
    mut bodies: Query<(
        Entity,
        Forces,
        &Transform,
        &VoxelPhysicsBody,
    )>,
) {
    let Some(request) = editor.physics_action_requested else {
        return;
    };
    if request
        .target
        .is_some_and(|target| !bodies.contains(target))
    {
        return;
    }
    editor.physics_action_requested = None;
    let Ok(camera_transform) = cameras.single() else {
        return;
    };
    let camera_position = camera_transform.translation();
    let push_pull_impulse = editor.physics_push_pull_impulse.max(0.0);
    let explosion_impulse = editor.physics_explosion_impulse.max(0.0);
    let explosion_radius = editor.physics_explosion_radius.max(VOXEL_SIZE);
    let mut affected = 0;

    for (entity, mut forces, transform, body) in &mut bodies {
        if request.action != VoxelPhysicsAction::Explode && request.target != Some(entity) {
            continue;
        }
        let body_position = transform
            .compute_affine()
            .transform_point3(body.local_center);
        let Some(impulse) = physics_action_impulse(
            request.action,
            body_position,
            camera_position,
            request.origin,
            push_pull_impulse,
            explosion_impulse,
            explosion_radius,
        ) else {
            continue;
        };
        forces.apply_linear_impulse(impulse);
        affected += 1;
    }

    let action_name = match request.action {
        VoxelPhysicsAction::Push => "推开",
        VoxelPhysicsAction::Pull => "拉近",
        VoxelPhysicsAction::Explode => "爆炸",
    };
    editor.physics_status = Some(format!(
        "{action_name}已作用于 {affected} 个物理体"
    ));
}

fn physics_action_impulse(
    action: VoxelPhysicsAction,
    body_position: Vec3,
    camera_position: Vec3,
    explosion_origin: Vec3,
    push_pull_impulse: f32,
    explosion_impulse: f32,
    explosion_radius: f32,
) -> Option<Vec3> {
    match action {
        VoxelPhysicsAction::Push | VoxelPhysicsAction::Pull => {
            let away = (body_position - camera_position).try_normalize()?;
            let direction = if action == VoxelPhysicsAction::Push { away } else { -away };
            Some(direction * push_pull_impulse)
        },
        VoxelPhysicsAction::Explode => {
            let offset = body_position - explosion_origin;
            let distance = offset.length();
            if distance > explosion_radius {
                return None;
            }
            let direction = offset.try_normalize().unwrap_or(Vec3::Y);
            let falloff = 1.0 - distance / explosion_radius.max(f32::EPSILON);
            Some(direction * explosion_impulse * falloff)
        },
    }
}

fn occupied_cells(grid: &Grid<u8>) -> Vec<IVec3> {
    grid.iter()
        .flat_map(|(chunk_position, chunk)| {
            prism(IVec3::ZERO, DIMS)
                .filter(|local| chunk[*local] != 0)
                .map(|local| *chunk_position * DIMS + local)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn apply_stroke(grid: &mut Mut<Grid<u8>>, stroke: &[VoxelChange], forward: bool) {
    for change in stroke {
        grid.set(
            change.position,
            if forward { change.after } else { change.before },
        );
    }
}

fn spawn_voxel_placed_light(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &VoxelMaterials,
    light: VoxelPlacedLight,
) -> Entity {
    let position = (light.cell.as_vec3() + Vec3::splat(0.5)) * VOXEL_SIZE;
    let color = Color::srgb(
        light.color[0],
        light.color[1],
        light.color[2],
    );
    match light.kind {
        VoxelLightTool::Point | VoxelLightTool::DarkPoint | VoxelLightTool::Cube => commands
            .spawn((
                PointLight {
                    color,
                    intensity: light.intensity.max(0.0),
                    range: light.range.max(VOXEL_SIZE),
                    radius: if light.kind == VoxelLightTool::Cube {
                        VOXEL_SIZE * 0.45
                    } else {
                        VOXEL_SIZE * 0.12
                    },
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_translation(position),
                light,
            ))
            .id(),
        VoxelLightTool::Spot => {
            let direction = light.direction.try_normalize().unwrap_or(Vec3::NEG_Z);
            let up = if direction.dot(Vec3::Y).abs() > 0.95 { Vec3::X } else { Vec3::Y };
            commands
                .spawn((
                    SpotLight {
                        color,
                        intensity: light.intensity.max(0.0),
                        range: light.range.max(VOXEL_SIZE),
                        radius: VOXEL_SIZE * 0.1,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(position).looking_to(direction, up),
                    light,
                ))
                .id()
        },
        VoxelLightTool::Physics => commands
            .spawn((
                PointLight {
                    color,
                    intensity: light.intensity.max(0.0),
                    range: light.range.max(VOXEL_SIZE),
                    radius: VOXEL_SIZE * 0.18,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Mesh3d(
                    meshes.add(Cuboid::from_size(Vec3::splat(
                        VOXEL_SIZE * 0.8,
                    ))),
                ),
                MeshMaterial3d(materials.handles[7].clone()),
                Transform::from_translation(position),
                VoxelPlanetGravityBody,
                RigidBody::Dynamic,
                Collider::cuboid(
                    VOXEL_SIZE * 0.8,
                    VOXEL_SIZE * 0.8,
                    VOXEL_SIZE * 0.8,
                ),
                LinearDamping(0.2),
                AngularDamping(0.35),
                light,
            ))
            .id(),
        VoxelLightTool::Edit | VoxelLightTool::Remove => Entity::PLACEHOLDER,
    }
}

fn place_creative_light(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    placed_lights: Query<(
        Entity,
        &GlobalTransform,
        &VoxelPlacedLight,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
    edit_runtime: VoxelEditRuntime,
    possession: Res<VoxelPossessionState>,
    egui_input: Res<EguiWantsInput>,
) {
    let VoxelEditRuntime {
        mut editor,
        mut dirty_chunks,
    } = edit_runtime;
    if possession.active_user_id.is_some()
        || editor.is_player_possession_tool_equipped()
        || editor.is_spaceship_possession_tool_equipped()
    {
        return;
    }
    let Some(tool) = editor.light_tool else {
        return;
    };
    if editor.creative_inventory_open
        || !mouse.just_pressed(MouseButton::Right)
        || voxel_world_pointer_blocked(
            egui_input.wants_any_pointer_input(),
            editor.right_started_over_ui,
        )
    {
        return;
    }
    let (Ok(window), Ok((camera, camera_transform)), Ok(mut grid)) = (
        windows.single(),
        cameras.single(),
        grids.single_mut(),
    ) else {
        return;
    };
    let Some(ray) = viewport_ray(
        window,
        camera,
        camera_transform,
        &editor,
    ) else {
        return;
    };

    if matches!(
        tool,
        VoxelLightTool::Edit | VoxelLightTool::Remove
    ) {
        let mut closest = None;
        for (entity, transform, light) in &placed_lights {
            let offset = transform.translation() - ray.origin;
            let distance = offset.dot(*ray.direction);
            if !(0.0..=MAX_RAY_DISTANCE).contains(&distance) {
                continue;
            }
            let nearest = ray.origin + *ray.direction * distance;
            if transform.translation().distance(nearest) > VOXEL_SIZE * 1.5 {
                continue;
            }
            if closest
                .as_ref()
                .is_none_or(|(_, closest_distance, _)| distance < *closest_distance)
            {
                closest = Some((entity, distance, light.clone()));
            }
        }
        let Some((entity, _, light)) = closest else {
            editor.physics_status = Some("没有瞄准已放置的灯光".to_owned());
            return;
        };
        if tool == VoxelLightTool::Edit {
            editor.placed_light_color = light.color;
            editor.placed_light_intensity = light.intensity;
            editor.placed_light_range = light.range;
            editor.selected_light = Some(entity);
            editor.physics_status = Some(format!(
                "已选择{}；可调整颜色、亮度和范围",
                light.kind.label()
            ));
            return;
        }
        commands.entity(entity).despawn();
        if light.kind == VoxelLightTool::Cube {
            if grid.get(light.cell).copied().unwrap_or(0) != 0 {
                grid.set(light.cell, 0);
                dirty_chunks.mark_cell_and_neighbors(light.cell);
            }
        }
        editor.physics_status = Some(format!("已移除{}", light.kind.label()));
        return;
    }

    let Some(hit) = raycast_grid(&grid, ray) else {
        return;
    };
    let Some(cell) = hit.add else {
        return;
    };
    if placed_lights.iter().any(|(_, _, light)| light.cell == cell) {
        editor.physics_status = Some("这个位置已经有灯光".to_owned());
        return;
    }
    let direction = hit
        .occupied
        .map(|occupied| (cell - occupied).as_vec3())
        .and_then(Vec3::try_normalize)
        .unwrap_or(Vec3::Y);
    let Some((color, intensity, range)) = tool.preset() else {
        return;
    };
    if tool == VoxelLightTool::Cube {
        if grid.get(cell).copied().unwrap_or(0) != 8 {
            grid.set(cell, 8);
            dirty_chunks.mark_cell_and_neighbors(cell);
        }
    }
    spawn_voxel_placed_light(
        &mut commands,
        &mut meshes,
        &materials,
        VoxelPlacedLight {
            kind: tool,
            cell,
            color,
            intensity,
            range,
            direction,
        },
    );
    editor.physics_status = Some(format!("已放置{}", tool.label()));
}

fn sync_selected_voxel_light(
    editor: Res<VoxelEditorState>,
    mut lights: Query<(
        Entity,
        &mut VoxelPlacedLight,
        Option<&mut PointLight>,
        Option<&mut SpotLight>,
    )>,
) {
    if editor.light_tool != Some(VoxelLightTool::Edit) || !editor.is_changed() {
        return;
    }
    let Some(selected) = editor.selected_light else {
        return;
    };
    let Ok((_, mut light, point_light, spot_light)) = lights.get_mut(selected) else {
        return;
    };
    light.color = editor.placed_light_color;
    light.intensity = editor.placed_light_intensity.max(0.0);
    light.range = editor.placed_light_range.max(VOXEL_SIZE);
    let color = Color::srgb(
        light.color[0],
        light.color[1],
        light.color[2],
    );
    if let Some(mut point_light) = point_light {
        point_light.color = color;
        point_light.intensity = light.intensity;
        point_light.range = light.range;
    }
    if let Some(mut spot_light) = spot_light {
        spot_light.color = color;
        spot_light.intensity = light.intensity;
        spot_light.range = light.range;
    }
}

fn drag_voxel_physics_body(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    mut bodies: Query<
        (
            &Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        With<VoxelPhysicsBody>,
    >,
    body_entities: Query<(), With<VoxelPhysicsBody>>,
    spatial_query: SpatialQuery,
    editor: Res<VoxelEditorState>,
    egui_input: Res<EguiWantsInput>,
    mut drag: ResMut<VoxelToolGunDragState>,
) {
    let active = editor.is_tool_gun_equipped()
        && editor.mode == VoxelEditMode::Drag
        && !editor.creative_inventory_open
        && !voxel_world_pointer_blocked(
            egui_input.wants_any_pointer_input(),
            editor.right_started_over_ui,
        );
    if !active || !mouse.pressed(MouseButton::Right) {
        drag.target = None;
        return;
    }
    let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single()) else {
        drag.target = None;
        return;
    };
    let Some(ray) = viewport_ray(
        window,
        camera,
        camera_transform,
        &editor,
    ) else {
        drag.target = None;
        return;
    };
    if mouse.just_pressed(MouseButton::Right) {
        let Some(hit) = spatial_query.cast_ray_predicate(
            ray.origin,
            ray.direction,
            MAX_RAY_DISTANCE,
            true,
            &SpatialQueryFilter::default(),
            &|entity| body_entities.contains(entity),
        ) else {
            return;
        };
        let Ok((transform, ..)) = bodies.get(hit.entity) else {
            return;
        };
        let hit_point = ray.origin + *ray.direction * hit.distance;
        drag.target = Some(hit.entity);
        drag.distance = hit.distance.max(VOXEL_SIZE * 2.0);
        drag.body_offset = transform.translation - hit_point;
    }
    let Some(target) = drag.target else {
        return;
    };
    let desired = ray.origin + *ray.direction * drag.distance + drag.body_offset;
    if let Ok((transform, mut velocity, mut angular_velocity)) = bodies.get_mut(target) {
        velocity.0 = tool_gun_drag_velocity(transform.translation, desired);
        angular_velocity.0 *= 0.8;
    } else {
        drag.target = None;
    }
}

fn tool_gun_drag_velocity(current: Vec3, target: Vec3) -> Vec3 {
    ((target - current) * TOOL_GUN_DRAG_RESPONSE).clamp_length_max(TOOL_GUN_DRAG_MAX_SPEED)
}

fn edit_voxel_grid(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    mut planets: Query<(
        &mut VoxelOrbitalPlanet,
        &GlobalTransform,
    )>,
    physics_bodies: Query<(
        Entity,
        &VoxelPhysicsBody,
        &Transform,
        &LinearVelocity,
        &AngularVelocity,
    )>,
    auto_doors: Query<(Entity, &VoxelAutoDoor)>,
    spatial_query: SpatialQuery,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
    edit_runtime: VoxelEditRuntime,
    possession: Res<VoxelPossessionState>,
    egui_input: Res<EguiWantsInput>,
    mut explosion_sequence: Local<u64>,
) {
    let VoxelEditRuntime {
        mut editor,
        mut dirty_chunks,
    } = edit_runtime;
    let egui_owns_pointer = egui_input.wants_any_pointer_input();
    if mouse.just_pressed(MouseButton::Left) {
        editor.left_started_over_ui = egui_owns_pointer;
    }
    if mouse.just_pressed(MouseButton::Right) {
        editor.right_started_over_ui = egui_owns_pointer;
    }
    if mouse.just_released(MouseButton::Left) {
        editor.left_started_over_ui = false;
    }
    if mouse.just_released(MouseButton::Right) {
        editor.right_started_over_ui = false;
    }
    if (mouse.just_released(MouseButton::Left) || mouse.just_released(MouseButton::Right))
        && !mouse.pressed(MouseButton::Left)
        && !mouse.pressed(MouseButton::Right)
    {
        editor.stroke_positions.clear();
        editor.edit_repeat_seconds = 0.0;
        if !editor.active_stroke.is_empty() {
            let stroke = std::mem::take(&mut editor.active_stroke);
            editor.undo.push(stroke);
            editor.redo.clear();
        }
    }
    if possession.active_user_id.is_some()
        || editor.is_player_possession_tool_equipped()
        || editor.is_spaceship_possession_tool_equipped()
        || editor.is_teleport_tool_equipped()
    {
        return;
    }
    if editor.creative_inventory_open || editor.teleport_menu_open {
        return;
    }
    let left_pressed = mouse.pressed(MouseButton::Left);
    let right_pressed = mouse.pressed(MouseButton::Right);
    let tool_gun_equipped = editor.is_tool_gun_equipped();
    if editor.equipped_item.is_none() && !left_pressed {
        return;
    }
    if editor.light_tool.is_some() && !left_pressed {
        return;
    }
    let started_over_ui = if left_pressed {
        editor.left_started_over_ui
    } else {
        editor.right_started_over_ui
    };
    if voxel_world_pointer_blocked(egui_owns_pointer, started_over_ui) {
        return;
    }
    if let Some(action) = force_tool_action(editor.mode) {
        let fired = mouse.just_pressed(voxel_tool_fire_button(
            tool_gun_equipped,
        )) && (tool_gun_equipped || !left_pressed);
        if !fired {
            return;
        }
        let (Ok(window), Ok((camera, camera_transform)), Ok(mut grid)) = (
            windows.single(),
            cameras.single(),
            grids.single_mut(),
        ) else {
            return;
        };
        let Some(ray) = viewport_ray(
            window,
            camera,
            camera_transform,
            &editor,
        ) else {
            return;
        };
        let grid_hit = raycast_grid(&grid, ray);
        let body_hit = spatial_query.cast_ray_predicate(
            ray.origin,
            ray.direction,
            MAX_RAY_DISTANCE,
            true,
            &SpatialQueryFilter::default(),
            &|entity| physics_bodies.contains(entity),
        );
        let static_distance = grid_hit
            .as_ref()
            .filter(|hit| hit.occupied.is_some())
            .map(|hit| hit.distance);
        let planet_hit = if action == VoxelPhysicsAction::Explode {
            planets
                .iter_mut()
                .next()
                .and_then(|(planet, transform)| raycast_voxel_planet(&planet, transform, ray))
        } else {
            None
        };
        let nearest_non_planet = static_distance
            .into_iter()
            .chain(body_hit.as_ref().map(|hit| hit.distance))
            .reduce(f32::min);
        if let Some(planet_hit) = planet_hit
            .filter(|hit| nearest_non_planet.is_none_or(|distance| hit.distance < distance))
        {
            let interaction_point = ray.origin + *ray.direction * planet_hit.distance;
            *explosion_sequence = explosion_sequence.wrapping_add(1);
            let fragment_seed = explosion_fragment_seed(interaction_point, *explosion_sequence);
            if let Ok((mut planet, transform)) = planets.single_mut() {
                let local_origin = transform
                    .affine()
                    .inverse()
                    .transform_point3(interaction_point);
                let removed = explode_planet_voxels(
                    &mut planet,
                    local_origin,
                    editor.physics_explosion_radius,
                );
                let removed_count = removed.len();
                let fragments = spawn_planet_explosion_fragments(
                    &mut commands,
                    &mut meshes,
                    &materials,
                    transform,
                    removed,
                    fragment_seed,
                );
                let fragment_count = fragments.len();
                if fragment_count > 0 {
                    editor.physics_action_requested = Some(VoxelPhysicsRequest {
                        action: VoxelPhysicsAction::Explode,
                        target: fragments.first().copied(),
                        origin: interaction_point,
                    });
                }
                editor.physics_status = Some(format!(
                    "行星爆炸挖出 {removed_count} 个 0.25 体素并生成 {fragment_count} 个物理碎块"
                ));
            }
            return;
        }
        let body_is_closest = body_hit
            .as_ref()
            .is_some_and(|hit| static_distance.is_none_or(|distance| hit.distance < distance));

        let (clicked_body, static_cell, interaction_distance) = if body_is_closest {
            let hit = body_hit.unwrap();
            (Some(hit.entity), None, hit.distance)
        } else {
            let Some(hit) = grid_hit.filter(|hit| hit.occupied.is_some()) else {
                return;
            };
            (None, hit.occupied, hit.distance)
        };
        let interaction_point = ray.origin + *ray.direction * interaction_distance;
        let target = if action == VoxelPhysicsAction::Explode {
            *explosion_sequence = explosion_sequence.wrapping_add(1);
            let fragment_seed = explosion_fragment_seed(interaction_point, *explosion_sequence);
            let explosion_radius = editor.physics_explosion_radius.max(VOXEL_SIZE);
            let mut target = clicked_body;
            let selected = selected_solid_voxels_in_radius(
                &grid,
                interaction_point,
                explosion_radius,
            );
            let affected_bodies = physics_bodies
                .iter()
                .filter(|(_, body, transform, ..)| {
                    body.cells.len() > 1
                        && physics_body_intersects_radius(
                            body,
                            transform,
                            interaction_point,
                            explosion_radius,
                        )
                })
                .map(
                    |(entity, body, transform, linear_velocity, angular_velocity)| {
                        (
                            entity,
                            body.cells.clone(),
                            *transform,
                            *linear_velocity,
                            *angular_velocity,
                        )
                    },
                )
                .collect::<Vec<_>>();
            let source_sizes = affected_bodies
                .iter()
                .map(|(_, cells, ..)| cells.len())
                .chain(std::iter::once(selected.len()))
                .filter(|size| *size > 0)
                .collect::<Vec<_>>();
            let part_counts = allocate_fragment_parts(
                &source_sizes,
                MAX_EXPLOSION_NEW_PHYSICS_BODIES,
            );
            let affected_body_count = affected_bodies.len();
            for (
                source_index,
                ((entity, cells, transform, linear_velocity, angular_velocity), part_count),
            ) in affected_bodies
                .into_iter()
                .zip(part_counts.iter().copied())
                .enumerate()
            {
                if part_count == 0 {
                    continue;
                }
                commands.entity(entity).despawn();
                if target == Some(entity) {
                    target = None;
                }
                for cells in split_voxel_cells_randomly(
                    cells,
                    part_count,
                    fragment_seed.wrapping_add(source_index as u64),
                ) {
                    let origin = cells
                        .iter()
                        .map(|(cell, _)| *cell)
                        .reduce(IVec3::min)
                        .unwrap_or(IVec3::ZERO);
                    let local_cells = cells
                        .into_iter()
                        .map(|(cell, material)| (cell - origin, material))
                        .collect();
                    let fragment_transform = Transform::from_matrix(
                        transform.to_matrix()
                            * Mat4::from_translation(origin.as_vec3() * VOXEL_SIZE),
                    );
                    let fragment = spawn_voxel_physics_body_at(
                        &mut commands,
                        &mut meshes,
                        &materials,
                        local_cells,
                        fragment_transform,
                        linear_velocity,
                        angular_velocity,
                    );
                    target.get_or_insert(fragment);
                }
            }
            let static_part_count = part_counts
                .get(source_sizes.len().saturating_sub(1))
                .copied()
                .filter(|_| !selected.is_empty())
                .unwrap_or(0);
            if static_part_count > 0 {
                for (cell, _) in &selected {
                    grid.set(*cell, 0);
                }
                for cells in split_voxel_cells_randomly(
                    selected,
                    static_part_count,
                    fragment_seed.wrapping_add(affected_body_count as u64),
                ) {
                    let fragment = spawn_voxel_physics_body(
                        &mut commands,
                        &mut meshes,
                        &materials,
                        cells,
                    );
                    target.get_or_insert(fragment);
                }
            }
            target
        } else if let Some(entity) = clicked_body {
            Some(entity)
        } else {
            let center = static_cell.unwrap();
            let radius = editor.brush_radius.max(0);
            let selected = selected_solid_voxels(
                &grid,
                center - IVec3::splat(radius),
                center + IVec3::splat(radius),
            );
            if selected.is_empty() {
                return;
            }
            for (cell, _) in &selected {
                grid.set(*cell, 0);
            }
            let entity = spawn_voxel_physics_body(
                &mut commands,
                &mut meshes,
                &materials,
                selected,
            );
            Some(entity)
        };
        let Some(target) = target else {
            return;
        };
        editor.physics_action_requested = Some(VoxelPhysicsRequest {
            action,
            target: Some(target),
            origin: interaction_point,
        });
        return;
    }
    if editor.mode == VoxelEditMode::Physics {
        let fired = mouse.just_pressed(voxel_tool_fire_button(
            tool_gun_equipped,
        )) && (tool_gun_equipped || !left_pressed);
        if !fired {
            return;
        }
        let (Ok(window), Ok((camera, camera_transform)), Ok(grid)) = (
            windows.single(),
            cameras.single(),
            grids.single_mut(),
        ) else {
            return;
        };
        let Some(ray) = viewport_ray(
            window,
            camera,
            camera_transform,
            &editor,
        ) else {
            return;
        };
        let grid_hit = raycast_grid(&grid, ray);
        let grid_distance = grid_hit
            .as_ref()
            .filter(|hit| hit.occupied.is_some())
            .map(|hit| hit.distance);
        if let Ok((planet, planet_transform)) = planets.single_mut() {
            if let Some(hit) = raycast_voxel_planet(&planet, planet_transform, ray)
                .filter(|hit| grid_distance.is_none_or(|distance| hit.distance < distance))
            {
                editor.select_physics_corner(hit.occupied, true);
                return;
            }
        }
        if let Some(cell) = grid_hit.and_then(|hit| hit.occupied) {
            editor.select_physics_corner(cell, false);
        }
        return;
    }
    let Some(input_mode) = voxel_edit_input_mode(
        editor.mode,
        left_pressed,
        right_pressed,
        tool_gun_equipped,
    ) else {
        return;
    };
    let just_pressed = if left_pressed {
        mouse.just_pressed(MouseButton::Left)
    } else {
        mouse.just_pressed(MouseButton::Right)
    };
    if !edit_repeat_due(
        just_pressed,
        time.delta_secs(),
        &mut editor,
    ) {
        return;
    }
    let (Ok(window), Ok((camera, camera_transform)), Ok(mut grid)) = (
        windows.single(),
        cameras.single(),
        grids.single_mut(),
    ) else {
        return;
    };
    let Some(ray) = viewport_ray(
        window,
        camera,
        camera_transform,
        &editor,
    ) else {
        return;
    };
    let grid_hit = raycast_grid(&grid, ray);
    let grid_distance = grid_hit
        .as_ref()
        .filter(|hit| hit.occupied.is_some())
        .map(|hit| hit.distance);
    if let Ok((mut planet, planet_transform)) = planets.single_mut() {
        let planet_hit = raycast_voxel_planet(&planet, planet_transform, ray)
            .filter(|hit| grid_distance.is_none_or(|distance| hit.distance < distance));
        if let Some(hit) = planet_hit {
            let center = if input_mode == VoxelEditMode::Add {
                hit.occupied + hit.normal
            } else {
                hit.occupied
            };
            let mut changed = 0;
            let brush_radius = editor.brush_radius;
            for x in -brush_radius..=brush_radius {
                for y in -brush_radius..=brush_radius {
                    for z in -brush_radius..=brush_radius {
                        let cell = center + IVec3::new(x, y, z);
                        let did_change = match input_mode {
                            VoxelEditMode::Add => {
                                set_planet_voxel(&mut planet, cell, editor.material)
                            },
                            VoxelEditMode::Remove => dig_planet_voxel(&mut planet, cell),
                            VoxelEditMode::Paint => {
                                planet.cells.contains_key(&cell)
                                    && set_planet_voxel(&mut planet, cell, editor.material)
                            },
                            _ => false,
                        };
                        changed += usize::from(did_change);
                    }
                }
            }
            if changed > 0 {
                editor.physics_status = Some(format!(
                    "已编辑 {changed} 个行星 0.25 体素"
                ));
                return;
            }
        }
    }
    if input_mode == VoxelEditMode::Remove && just_pressed {
        let door_hit = spatial_query.cast_ray_predicate(
            ray.origin,
            ray.direction,
            MAX_RAY_DISTANCE,
            true,
            &SpatialQueryFilter::default(),
            &|entity| auto_doors.contains(entity),
        );
        let grid_distance = grid_hit
            .as_ref()
            .filter(|hit| hit.occupied.is_some())
            .map(|hit| hit.distance);
        if let Some(hit) = door_hit.filter(|hit| {
            grid_distance.is_none_or(|distance| hit.distance <= distance + VOXEL_SIZE)
        }) {
            if let Ok((_, clicked_door)) = auto_doors.get(hit.entity) {
                let trigger_center = clicked_door.trigger_center;
                let mut removed = 0;
                for (entity, door) in &auto_doors {
                    if door.trigger_center == trigger_center {
                        commands.entity(entity).despawn();
                        removed += 1;
                    }
                }
                editor.physics_status = Some(format!(
                    "已拆除自动门（{removed} 扇门板）"
                ));
                return;
            }
        }
    }
    let Some(hit) = grid_hit else {
        return;
    };
    let center = match input_mode {
        VoxelEditMode::Add => hit.add,
        VoxelEditMode::Remove | VoxelEditMode::Paint => hit.occupied,
        VoxelEditMode::Physics
        | VoxelEditMode::Drag
        | VoxelEditMode::Push
        | VoxelEditMode::Pull
        | VoxelEditMode::Explode => unreachable!(),
    };
    let Some(center) = center else {
        return;
    };

    let mut stroke = Vec::new();
    let brush_radius = editor.brush_radius;
    for x in -brush_radius..=brush_radius {
        for y in -brush_radius..=brush_radius {
            for z in -brush_radius..=brush_radius {
                let position = center + IVec3::new(x, y, z);
                if !editor.stroke_positions.insert(position) {
                    continue;
                }
                let before = grid.get(position).copied().unwrap_or(0);
                let Some(after) = edited_voxel(input_mode, before, editor.material) else {
                    continue;
                };
                if before != after {
                    grid.set(position, after);
                    dirty_chunks.mark_cell_and_neighbors(position);
                    stroke.push(VoxelChange {
                        position,
                        before,
                        after,
                    });
                }
            }
        }
    }
    if !stroke.is_empty() {
        editor.active_stroke.extend(stroke);
    }
}

fn use_voxel_teleport_tool(
    mouse: Res<ButtonInput<MouseButton>>,
    egui_input: Res<EguiWantsInput>,
    possession: Res<VoxelPossessionState>,
    mut editor: ResMut<VoxelEditorState>,
) {
    if possession.active_user_id.is_some()
        || !editor.is_teleport_tool_equipped()
        || editor.creative_inventory_open
        || editor.teleport_menu_open
        || !mouse.just_pressed(MouseButton::Right)
        || voxel_world_pointer_blocked(
            egui_input.wants_any_pointer_input(),
            editor.right_started_over_ui,
        )
    {
        return;
    }
    editor.teleport_menu_open = true;
}

fn apply_voxel_teleport(
    mut editor: ResMut<VoxelEditorState>,
    possession: Res<VoxelPossessionState>,
    mut players: Query<
        (&mut Transform, &mut LinearVelocity),
        (
            With<VoxelFirstPersonPlayer>,
            Without<VoxelViewportCamera>,
            Without<VoxelPlayerCaptureCamera>,
            Without<VoxelPlayerStandee>,
            Without<VoxelSpaceship>,
        ),
    >,
    standees: Query<(&VoxelPlayerStandee, &GlobalTransform), Without<VoxelFirstPersonPlayer>>,
    spaceships: Query<
        (&VoxelSpaceship, &Transform),
        (
            With<VoxelSpaceship>,
            Without<VoxelFirstPersonPlayer>,
            Without<VoxelPlayerStandee>,
        ),
    >,
) {
    let Some(destination) = editor.teleport_requested.take() else { return };
    if possession.active_user_id.is_some() {
        editor.teleport_menu_open = false;
        return;
    }
    let target_position = match destination {
        VoxelTeleportDestination::PlayerStandee(user_id) => {
            let Some((_, standee_transform)) = standees
                .iter()
                .find(|(standee, _)| standee.user_id == user_id)
            else {
                return;
            };
            first_person_player_position(standee_transform.translation())
        },
        VoxelTeleportDestination::CombatSpaceship => {
            let Some((ship, ship_transform)) = spaceships
                .iter()
                .find(|(ship, _)| ship.id == COMBAT_SPACESHIP_ID)
            else {
                return;
            };
            ship_transform
                .compute_affine()
                .transform_point3(ship.cockpit_eye_local)
                - ship_transform.rotation * Vec3::Y * FIRST_PERSON_EYE_OFFSET
        },
        _ => {
            let Some(position) = destination.player_position() else { return };
            position
        },
    };
    let Ok((mut transform, mut velocity)) = players.single_mut() else { return };
    transform.translation = target_position;
    velocity.0 = Vec3::ZERO;
    editor.first_person_enabled = true;
    editor.first_person_flying = true;
    editor.first_person_space_tap_elapsed = f32::INFINITY;
}

fn voxel_world_pointer_blocked(egui_owns_pointer: bool, interaction_started_over_ui: bool) -> bool {
    egui_owns_pointer || interaction_started_over_ui
}

fn voxel_edit_input_mode(
    equipped_mode: VoxelEditMode,
    left_pressed: bool,
    right_pressed: bool,
    tool_gun_equipped: bool,
) -> Option<VoxelEditMode> {
    if tool_gun_equipped {
        None
    } else if left_pressed {
        Some(VoxelEditMode::Remove)
    } else if right_pressed {
        Some(equipped_mode)
    } else {
        None
    }
}

fn voxel_tool_fire_button(tool_gun_equipped: bool) -> MouseButton {
    let _ = tool_gun_equipped;
    MouseButton::Right
}

fn edited_voxel(mode: VoxelEditMode, before: u8, material: u8) -> Option<u8> {
    match mode {
        VoxelEditMode::Add => Some(material),
        VoxelEditMode::Remove => Some(0),
        VoxelEditMode::Paint => (before != 0).then_some(material),
        VoxelEditMode::Physics
        | VoxelEditMode::Drag
        | VoxelEditMode::Push
        | VoxelEditMode::Pull
        | VoxelEditMode::Explode => None,
    }
}

fn force_tool_action(mode: VoxelEditMode) -> Option<VoxelPhysicsAction> {
    match mode {
        VoxelEditMode::Push => Some(VoxelPhysicsAction::Push),
        VoxelEditMode::Pull => Some(VoxelPhysicsAction::Pull),
        VoxelEditMode::Explode => Some(VoxelPhysicsAction::Explode),
        _ => None,
    }
}

struct VoxelRayHit {
    occupied: Option<IVec3>,
    add: Option<IVec3>,
    distance: f32,
}

fn edit_repeat_due(just_pressed: bool, delta_seconds: f32, editor: &mut VoxelEditorState) -> bool {
    if just_pressed {
        editor.edit_repeat_seconds = 0.0;
        return true;
    }
    editor.edit_repeat_seconds += delta_seconds;
    if editor.edit_repeat_seconds < EDIT_REPEAT_INTERVAL {
        return false;
    }
    editor.edit_repeat_seconds %= EDIT_REPEAT_INTERVAL;
    true
}

fn raycast_grid(grid: &Grid<u8>, ray: Ray3d) -> Option<VoxelRayHit> {
    let origin = ray.origin;
    let direction = *ray.direction;
    let step = VOXEL_SIZE * 0.2;
    let mut previous = (origin / VOXEL_SIZE).floor().as_ivec3();
    let mut distance = 0.0;
    while distance <= MAX_RAY_DISTANCE {
        let point = origin + direction * distance;
        let cell = (point / VOXEL_SIZE).floor().as_ivec3();
        if grid.get(cell).copied().unwrap_or(0) != 0 {
            return Some(VoxelRayHit {
                occupied: Some(cell),
                add: Some(previous),
                distance,
            });
        }
        previous = cell;
        distance += step;
    }
    let plane_distance = -origin.y / direction.y;
    if plane_distance.is_finite() && plane_distance >= 0.0 && plane_distance <= MAX_RAY_DISTANCE {
        let point = origin + direction * plane_distance;
        return Some(VoxelRayHit {
            occupied: None,
            add: Some(IVec3::new(
                (point.x / VOXEL_SIZE).floor() as i32,
                0,
                (point.z / VOXEL_SIZE).floor() as i32,
            )),
            distance: plane_distance,
        });
    }
    None
}

fn raycast_voxel_planet(
    planet: &VoxelOrbitalPlanet,
    transform: &GlobalTransform,
    ray: Ray3d,
) -> Option<VoxelPlanetRayHit> {
    let inverse = transform.affine().inverse();
    let local_origin = inverse.transform_point3(ray.origin);
    let local_direction = inverse
        .transform_vector3(*ray.direction)
        .normalize_or_zero();
    if local_direction == Vec3::ZERO {
        return None;
    }

    let (local_min, local_max) = planet.cell_bounds?.local_aabb(planet.voxel_size);
    let (mut distance, end) = ray_aabb_distance_range(
        local_origin,
        local_direction,
        local_min,
        local_max,
        PLANET_MAX_RAY_DISTANCE,
    )?;
    let step = planet.voxel_size * 0.2;
    while distance <= end {
        let local_point = local_origin + local_direction * distance;
        let cell = (local_point / planet.voxel_size + Vec3::splat(0.5))
            .floor()
            .as_ivec3();
        if planet.cells.contains_key(&cell) {
            return Some(VoxelPlanetRayHit {
                occupied: cell,
                normal: voxel_face_normal_against_ray(local_direction),
                distance,
            });
        }
        distance += step;
    }
    None
}

fn ray_aabb_distance_range(
    origin: Vec3,
    direction: Vec3,
    min: Vec3,
    max: Vec3,
    max_distance: f32,
) -> Option<(f32, f32)> {
    let mut near: f32 = 0.0;
    let mut far = max_distance;
    for axis in 0..3 {
        if direction[axis].abs() <= f32::EPSILON {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let first = (min[axis] - origin[axis]) / direction[axis];
        let second = (max[axis] - origin[axis]) / direction[axis];
        near = near.max(first.min(second));
        far = far.min(first.max(second));
        if near > far {
            return None;
        }
    }
    Some((near, far))
}

fn voxel_face_normal_against_ray(direction: Vec3) -> IVec3 {
    let absolute = direction.abs();
    if absolute.x >= absolute.y && absolute.x >= absolute.z {
        IVec3::new(
            if direction.x > 0.0 { -1 } else { 1 },
            0,
            0,
        )
    } else if absolute.y >= absolute.z {
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

fn control_first_person_player(
    mut commands: Commands,
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    mut movement_store: Option<ResMut<Persistent<VoxelPossessionMovementStore>>>,
    mut editor: ResMut<VoxelEditorState>,
    mut possession: ResMut<VoxelPossessionState>,
    spaceship_control: Option<Res<VoxelSpaceshipControlState>>,
    mut players: Query<
        (
            Entity,
            &ShapeHits,
            &mut Transform,
            &mut LinearVelocity,
            &mut ConstantLinearAcceleration,
            Has<Sensor>,
        ),
        (
            With<VoxelFirstPersonPlayer>,
            Without<VoxelPlayerCaptureCamera>,
        ),
    >,
    capture_cameras: Query<
        (&Transform, &VoxelPlayerCaptureCamera),
        (
            With<VoxelPlayerCaptureCamera>,
            Without<VoxelFirstPersonPlayer>,
        ),
    >,
) {
    let Ok((entity, ground_hits, mut transform, mut velocity, mut acceleration, is_sensor)) =
        players.single_mut()
    else {
        return;
    };

    if let Some((cockpit_eye, cockpit_rotation)) = spaceship_control
        .as_deref()
        .and_then(|control| {
            control
                .cockpit_eye
                .map(|eye| (eye, control.cockpit_rotation))
        })
    {
        possession.applied_user_id = None;
        possession.last_player_position = None;
        possession.turn_start_position = None;
        possession.movement_happened = false;
        editor.first_person_enabled = true;
        editor.first_person_was_enabled = true;
        editor.first_person_flying = false;
        editor.creative_inventory_open = false;
        editor.teleport_menu_open = false;
        possession.player_inventory_open = false;
        transform.translation =
            cockpit_eye - cockpit_rotation * Vec3::Y * FIRST_PERSON_EYE_OFFSET;
        velocity.0 = Vec3::ZERO;
        acceleration.0 = Vec3::ZERO;
        if !is_sensor {
            commands.entity(entity).insert(Sensor);
        }
        return;
    }

    if possession.active_user_id != possession.applied_user_id {
        if let Some(previous_user_id) = possession.applied_user_id {
            if possession.movement_happened {
                if let (Some(campaign_id), Some(store)) = (
                    possession_campaign_id(manager.as_deref(), previous_user_id),
                    movement_store.as_mut(),
                ) {
                    upsert_possession_movement(
                        store,
                        &campaign_id,
                        previous_user_id,
                        possession.movement_turn,
                        possession.movement_used,
                        true,
                        possession
                            .turn_start_position
                            .unwrap_or(transform.translation),
                    );
                    if let Err(err) = store.persist() {
                        eprintln!("failed to persist completed possession movement: {err}");
                    }
                }
            }
        }
        possession.applied_user_id = possession.active_user_id;
        possession.movement_used = 0.0;
        possession.movement_completed = false;
        possession.movement_happened = false;
        possession.persist_elapsed = 0.0;
        possession.movement_persist_elapsed = 0.0;
        possession.reset_turn_overrides();
        if let Some(user_id) = possession.active_user_id {
            if let Some((camera_transform, _)) = capture_cameras
                .iter()
                .find(|(_, camera)| camera.user_id == user_id)
            {
                transform.translation = first_person_player_position(camera_transform.translation);
            }
            velocity.0 = Vec3::ZERO;
            editor.first_person_enabled = true;
            editor.first_person_was_enabled = true;
            editor.first_person_flying = false;
            editor.creative_inventory_open = false;
            editor.teleport_menu_open = false;
            possession.turn_start_position = Some(transform.translation);
            possession.last_player_position = Some(transform.translation);
            possession.movement_turn = possession_world_turn(manager.as_deref(), user_id);
            possession.movement_limit = possession_final_movement(manager.as_deref(), user_id);
            if let (Some(campaign_id), Some(store)) = (
                possession_campaign_id(manager.as_deref(), user_id),
                movement_store.as_deref(),
            ) {
                if let Some(record) = possession_movement_record(
                    store,
                    &campaign_id,
                    user_id,
                    possession.movement_turn,
                ) {
                    possession.movement_used =
                        restored_possession_movement_used(record, possession.movement_limit);
                    possession.movement_completed = record.completed;
                    possession.turn_start_position =
                        Some(Vec3::from_array(record.turn_start_position_cells) * VOXEL_SIZE);
                }
            }
        } else {
            possession.turn_start_position = None;
            possession.last_player_position = None;
            possession.movement_completed = false;
            possession.movement_happened = false;
            editor.first_person_flying = true;
        }
    }

    if let Some(user_id) = possession.active_user_id {
        let world_turn = possession_world_turn(manager.as_deref(), user_id);
        let movement_limit = possession_final_movement(manager.as_deref(), user_id);
        if possession.movement_turn != world_turn {
            possession.movement_turn = world_turn;
            possession.movement_used = 0.0;
            possession.movement_completed = false;
            possession.movement_happened = false;
            possession.turn_start_position = Some(transform.translation);
            possession.last_player_position = Some(transform.translation);
            possession.reset_turn_overrides();
        }
        possession.movement_limit = movement_limit;
        editor.first_person_enabled = true;
        editor.first_person_flying = false;
        editor.creative_inventory_open = false;
        editor.teleport_menu_open = false;
        if possession.reset_movement_requested {
            if let Some(turn_start) = possession.turn_start_position {
                transform.translation = turn_start;
                velocity.0 = Vec3::ZERO;
                possession.movement_used = 0.0;
                possession.movement_completed = false;
                possession.movement_happened = false;
                possession.last_player_position = Some(turn_start);
                if let (Some(campaign_id), Some(store)) = (
                    possession_campaign_id(manager.as_deref(), user_id),
                    movement_store.as_mut(),
                ) {
                    upsert_possession_movement(
                        store,
                        &campaign_id,
                        user_id,
                        possession.movement_turn,
                        0.0,
                        false,
                        turn_start,
                    );
                    if let Err(err) = store.persist() {
                        eprintln!("failed to persist reset possession movement: {err}");
                    }
                }
            }
            possession.reset_movement_requested = false;
        } else if let Some(previous) = possession.last_player_position {
            possession.movement_happened |=
                previous.distance_squared(transform.translation) > f32::EPSILON;
            let (clamped, movement_used, exhausted) = resolve_horizontal_movement_step(
                previous,
                transform.translation,
                possession.movement_used,
                possession.movement_limit,
                possession.movement_limit_bypassed,
            );
            transform.translation = clamped;
            possession.movement_used = movement_used;
            if exhausted {
                velocity.x = 0.0;
                velocity.z = 0.0;
            }
        }
        possession.last_player_position = Some(transform.translation);
        if let (Some(campaign_id), Some(store)) = (
            possession_campaign_id(manager.as_deref(), user_id),
            movement_store.as_mut(),
        ) {
            upsert_possession_movement(
                store,
                &campaign_id,
                user_id,
                possession.movement_turn,
                possession.movement_used,
                possession.movement_completed || possession.movement_happened,
                possession
                    .turn_start_position
                    .unwrap_or(transform.translation),
            );
            possession.movement_persist_elapsed += time.delta_secs();
            if possession.movement_persist_elapsed >= 0.5 {
                possession.movement_persist_elapsed = 0.0;
                if let Err(err) = store.persist() {
                    eprintln!("failed to persist possession movement: {err}");
                }
            }
        }
    }

    acceleration.0 = if editor.first_person_flying {
        Vec3::ZERO
    } else {
        Vec3::new(0.0, -9.81, 0.0)
    };
    if !editor.first_person_enabled {
        editor.first_person_flying = false;
        editor.first_person_space_tap_elapsed = f32::INFINITY;
        if is_sensor {
            commands.entity(entity).remove::<Sensor>();
        }
        acceleration.0 = Vec3::new(0.0, -9.81, 0.0);
        velocity.x = 0.0;
        velocity.z = 0.0;
        return;
    }
    if editor.creative_inventory_open
        || editor.teleport_menu_open
        || possession.player_inventory_open
        || editor.first_person_cursor_released
    {
        velocity.x = 0.0;
        velocity.z = 0.0;
        if editor.first_person_flying {
            velocity.y = 0.0;
            acceleration.0 = Vec3::ZERO;
        }
        return;
    }

    editor.first_person_space_tap_elapsed += time.delta_secs();
    if possession.active_user_id.is_none()
        && keyboard.just_pressed(KeyCode::Space)
        && register_first_person_space_tap(&mut editor.first_person_space_tap_elapsed)
    {
        editor.first_person_flying = !editor.first_person_flying;
    }
    // Creative flight uses the existing collider as a sensor for noclip. This
    // keeps its shape ready so normal collision can be restored immediately.
    if editor.first_person_flying != is_sensor {
        if editor.first_person_flying {
            commands.entity(entity).insert(Sensor);
        } else {
            commands.entity(entity).remove::<Sensor>();
        }
    }

    let forward_input =
        keyboard.pressed(KeyCode::KeyW) as i8 - keyboard.pressed(KeyCode::KeyS) as i8;
    let right_input = keyboard.pressed(KeyCode::KeyD) as i8 - keyboard.pressed(KeyCode::KeyA) as i8;
    let yaw_rotation = Quat::from_rotation_y(editor.camera_yaw);
    let forward = yaw_rotation * Vec3::NEG_Z;
    let right = yaw_rotation * Vec3::X;
    let movement =
        (forward * forward_input as f32 + right * right_input as f32).clamp_length_max(1.0);
    let movement_speed = editor.first_person_speed.max(VOXEL_SIZE);
    let movement_allowed = possession.active_user_id.is_none()
        || possession.movement_limit_bypassed
        || possession.movement_remaining() > f32::EPSILON;
    let movement = if movement_allowed { movement } else { Vec3::ZERO };
    velocity.x = movement.x * movement_speed;
    velocity.z = movement.z * movement_speed;

    if editor.first_person_flying {
        acceleration.0 = Vec3::ZERO;
        let vertical_input = keyboard.pressed(KeyCode::Space) as i8
            - (keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)) as i8;
        velocity.y = vertical_input as f32 * FIRST_PERSON_FLY_SPEED;
        return;
    }
    acceleration.0 = Vec3::new(0.0, -9.81, 0.0);

    let grounded = ground_hits
        .iter()
        .any(|hit| (-hit.normal2).angle_between(Vec3::Y).abs() <= 55.0_f32.to_radians());
    if grounded
        && keyboard.just_pressed(KeyCode::Space)
        && (!possession.movement_completed || possession.movement_limit_bypassed)
    {
        velocity.y = FIRST_PERSON_JUMP_SPEED;
    }
}

fn possession_world_turn(manager: Option<&Persistent<NapcatMessageManager>>, user_id: u64) -> u32 {
    let Some(manager) = manager else { return 0 };
    let target_id = user_id.to_string();
    manager
        .trpg_groups
        .values()
        .filter(|group| group.players.iter().any(|player| player == &target_id))
        .map(|group| {
            group
                .player_turns
                .get(&target_id)
                .map(|turn| turn.turns_passed)
                .unwrap_or(group.world_turn)
        })
        .max()
        .unwrap_or_default()
}

fn possession_campaign_id(
    manager: Option<&Persistent<NapcatMessageManager>>,
    user_id: u64,
) -> Option<String> {
    let manager = manager?;
    let group = manager.current_group()?;
    group
        .players
        .iter()
        .any(|target_id| target_id == &user_id.to_string())
        .then(|| group.campaign_id.clone())
}

fn possession_movement_record<'a>(
    store: &'a VoxelPossessionMovementStore,
    campaign_id: &str,
    user_id: u64,
    turn: u32,
) -> Option<&'a PersistedVoxelPossessionMovement> {
    store.records.iter().find(|record| {
        record.campaign_id == campaign_id && record.user_id == user_id && record.turn == turn
    })
}

fn restored_possession_movement_used(
    record: &PersistedVoxelPossessionMovement,
    movement_limit: f32,
) -> f32 {
    if record.completed {
        movement_limit.max(0.0)
    } else {
        record.movement_used.clamp(0.0, movement_limit.max(0.0))
    }
}

fn upsert_possession_movement(
    store: &mut VoxelPossessionMovementStore,
    campaign_id: &str,
    user_id: u64,
    turn: u32,
    movement_used: f32,
    completed: bool,
    turn_start_position: Vec3,
) {
    let record = PersistedVoxelPossessionMovement {
        campaign_id: campaign_id.to_owned(),
        user_id,
        turn,
        movement_used: movement_used.max(0.0),
        completed,
        turn_start_position_cells: (turn_start_position / VOXEL_SIZE).to_array(),
    };
    if let Some(existing) = store.records.iter_mut().find(|existing| {
        existing.campaign_id == campaign_id && existing.user_id == user_id && existing.turn == turn
    }) {
        *existing = record;
        return;
    }
    store.records.push(record);
    const MAX_POSSESSION_MOVEMENT_RECORDS: usize = 4_096;
    if store.records.len() > MAX_POSSESSION_MOVEMENT_RECORDS {
        store
            .records
            .drain(..store.records.len() - MAX_POSSESSION_MOVEMENT_RECORDS);
    }
}

pub(crate) fn clear_campaign_possession_movement(
    store: &mut VoxelPossessionMovementStore,
    campaign_id: &str,
) -> usize {
    let previous_len = store.records.len();
    store
        .records
        .retain(|record| record.campaign_id != campaign_id);
    previous_len - store.records.len()
}

pub(crate) fn clear_player_possession_movement(
    store: &mut VoxelPossessionMovementStore,
    user_id: u64,
) -> usize {
    let previous_len = store.records.len();
    store.records.retain(|record| record.user_id != user_id);
    previous_len - store.records.len()
}

pub(crate) fn clear_player_camera(
    store: &mut VoxelPlayerCameraStore,
    user_id: u64,
) -> bool {
    let previous_len = store.cameras.len();
    store.cameras.retain(|camera| camera.user_id != user_id);
    previous_len != store.cameras.len()
}

fn clamp_horizontal_movement_step(
    previous: Vec3,
    current: Vec3,
    movement_used: f32,
    movement_limit: f32,
) -> (Vec3, f32, bool) {
    let movement_limit = movement_limit.max(0.0);
    let movement_used = movement_used.clamp(0.0, movement_limit);
    let delta = Vec2::new(
        current.x - previous.x,
        current.z - previous.z,
    );
    let distance = delta.length();
    let remaining = (movement_limit - movement_used).max(0.0);
    if distance <= remaining || distance <= f32::EPSILON {
        return (
            current,
            (movement_used + distance).min(movement_limit),
            remaining <= f32::EPSILON,
        );
    }
    let fraction = remaining / distance;
    (
        Vec3::new(
            previous.x + delta.x * fraction,
            current.y,
            previous.z + delta.y * fraction,
        ),
        movement_limit,
        true,
    )
}

fn resolve_horizontal_movement_step(
    previous: Vec3,
    current: Vec3,
    movement_used: f32,
    movement_limit: f32,
    bypass_limit: bool,
) -> (Vec3, f32, bool) {
    if !bypass_limit {
        return clamp_horizontal_movement_step(
            previous,
            current,
            movement_used,
            movement_limit,
        );
    }
    let distance = Vec2::new(
        current.x - previous.x,
        current.z - previous.z,
    )
    .length();
    (
        current,
        movement_used.max(0.0) + distance,
        false,
    )
}

fn possession_final_movement(
    manager: Option<&Persistent<NapcatMessageManager>>,
    user_id: u64,
) -> f32 {
    manager
        .and_then(|manager| manager.player_characters.get(&user_id.to_string()))
        .map(possession_character_movement)
        .unwrap_or_default()
}

fn possession_character_movement(character: &PlayerCharacter) -> f32 {
    let mut speed = character.speed.max(0.0);
    if character.buff_base_stats.is_none() {
        let mut equipment = character.inventory.equipment.iter().collect::<Vec<_>>();
        equipment.sort_by_key(|(slot, _)| format!("{slot:?}"));
        for effect in equipment
            .into_iter()
            .flat_map(|(_, item)| &item.stat_effects)
            .filter(|effect| effect.field == BuffField::Speed)
        {
            let base = speed;
            match effect.value {
                BuffValue::Add(value) => speed += value,
                BuffValue::AddPercent(percent) => speed *= 1.0 + percent / 100.0,
                BuffValue::Set(value) => speed = value,
                BuffValue::SetPercentOfBase(percent) => speed = base * percent / 100.0,
            }
        }
    }
    (speed + DEFAULT_POSSESSION_MOVEMENT_BONUS).max(0.0)
}

fn sync_possessed_player_camera(
    time: Res<Time>,
    editor: Res<VoxelEditorState>,
    mut possession: ResMut<VoxelPossessionState>,
    viewport_camera: Query<
        &Transform,
        (
            With<VoxelViewportCamera>,
            Without<VoxelPlayerCaptureCamera>,
        ),
    >,
    mut capture_cameras: Query<
        (
            &VoxelPlayerCaptureCamera,
            &mut Transform,
        ),
        (
            With<VoxelPlayerCaptureCamera>,
            Without<VoxelViewportCamera>,
        ),
    >,
    mut store: ResMut<Persistent<VoxelPlayerCameraStore>>,
) {
    let Some(user_id) = possession
        .active_user_id
        .filter(|_| editor.first_person_enabled)
    else {
        return;
    };
    let Ok(viewport_transform) = viewport_camera.single() else { return };
    let Some((_, mut capture_transform)) = capture_cameras
        .iter_mut()
        .find(|(camera, _)| camera.user_id == user_id)
    else {
        return;
    };
    *capture_transform = *viewport_transform;
    upsert_voxel_player_camera(&mut store, user_id, viewport_transform);
    possession.persist_elapsed += time.delta_secs();
    if possession.persist_elapsed >= 0.5 {
        possession.persist_elapsed = 0.0;
        if let Err(err) = store.persist() {
            eprintln!("failed to persist possessed player camera: {err}");
        }
    }
}

fn register_first_person_space_tap(elapsed: &mut f32) -> bool {
    let double_tap = *elapsed <= FIRST_PERSON_DOUBLE_TAP_SECONDS;
    *elapsed = if double_tap { f32::INFINITY } else { 0.0 };
    double_tap
}

fn cycled_hotbar_slot(current: usize, wheel_steps: i32, slot_count: usize) -> usize {
    if slot_count == 0 {
        return 0;
    }
    (current as i32 - wheel_steps).rem_euclid(slot_count as i32) as usize
}

fn first_person_player_position(camera_position: Vec3) -> Vec3 {
    camera_position - Vec3::Y * FIRST_PERSON_EYE_OFFSET
}

fn orbit_focus_preserving_camera_position(
    camera_position: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
) -> Vec3 {
    let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    camera_position + rotation * Vec3::NEG_Z * distance
}

fn control_voxel_camera(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cursor_options: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut cameras: Query<
        (&mut Transform, &mut Projection),
        (
            With<VoxelViewportCamera>,
            Without<VoxelFirstPersonPlayer>,
        ),
    >,
    mut players: Query<
        (&mut Transform, &mut LinearVelocity),
        (
            With<VoxelFirstPersonPlayer>,
            Without<VoxelViewportCamera>,
        ),
    >,
    mut editor: ResMut<VoxelEditorState>,
    mut possession: ResMut<VoxelPossessionState>,
    spaceship_control: Option<Res<VoxelSpaceshipControlState>>,
    egui_input: Res<EguiWantsInput>,
) {
    editor.first_person_enabled = true;
    let window_focused = windows.single().is_ok_and(|window| window.focused);
    if !window_focused {
        editor.first_person_cursor_released = true;
    }
    if keyboard.just_pressed(KeyCode::Escape) {
        if possession.player_inventory_open {
            possession.player_inventory_open = false;
        } else if editor.creative_inventory_open {
            editor.creative_inventory_open = false;
        } else if editor.teleport_menu_open {
            editor.teleport_menu_open = false;
        } else {
            editor.first_person_cursor_released = true;
        }
    }
    let entering_first_person = editor.first_person_enabled && !editor.first_person_was_enabled;
    let exiting_first_person = !editor.first_person_enabled && editor.first_person_was_enabled;
    if entering_first_person {
        if let (Ok((camera_transform, _)), Ok((mut player_transform, mut velocity))) =
            (cameras.single(), players.single_mut())
        {
            player_transform.translation =
                first_person_player_position(camera_transform.translation);
            velocity.0 = Vec3::ZERO;
        }
    } else if exiting_first_person {
        if let Ok((camera_transform, _)) = cameras.single() {
            editor.camera_focus = orbit_focus_preserving_camera_position(
                camera_transform.translation,
                editor.camera_yaw,
                editor.camera_pitch,
                editor.camera_distance,
            );
        }
    }
    editor.first_person_was_enabled = editor.first_person_enabled;
    let inventory_open = editor.creative_inventory_open
        || editor.teleport_menu_open
        || possession.player_inventory_open;
    let cursor_in_viewport = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .is_some_and(|cursor| editor.contains_cursor(cursor));
    if editor.first_person_cursor_released
        && window_focused
        && !inventory_open
        && mouse.just_pressed(MouseButton::Left)
        && cursor_in_viewport
        && !egui_input.wants_pointer_input()
    {
        editor.first_person_cursor_released = false;
    }
    if let Ok(mut cursor) = cursor_options.single_mut() {
        if should_grab_first_person_cursor(
            window_focused,
            inventory_open,
            editor.first_person_cursor_released,
        ) {
            cursor.visible = false;
            cursor.grab_mode = CursorGrabMode::Locked;
        } else {
            cursor.visible = true;
            cursor.grab_mode = CursorGrabMode::None;
        }
    }
    if editor.view_reset_requested {
        if editor.first_person_enabled {
            if let Ok((mut player_transform, mut velocity)) = players.single_mut() {
                player_transform.translation = FIRST_PERSON_START;
                velocity.0 = Vec3::ZERO;
            }
        } else {
            editor.camera_focus = DEFAULT_SCENE_CAMERA_FOCUS;
            editor.camera_distance = DEFAULT_SCENE_CAMERA_DISTANCE;
            editor.camera_yaw = 0.7;
            editor.camera_pitch = -0.45;
        }
        editor.view_reset_requested = false;
    }
    let delta = motion.read().fold(Vec2::ZERO, |sum, event| {
        sum + event.delta
    });
    if editor.first_person_enabled {
        if !inventory_open && !editor.first_person_cursor_released {
            editor.camera_yaw -= delta.x * 0.0025;
            editor.camera_pitch = (editor.camera_pitch - delta.y * 0.0025).clamp(-1.5, 1.5);
        }
        let wheel_steps = wheel.read().fold(0, |steps, event| {
            steps + event.y.signum() as i32
        });
        if !inventory_open && !editor.first_person_cursor_released && wheel_steps != 0 {
            if possession.active_user_id.is_some() {
                possession.selected_hotbar_slot = cycled_hotbar_slot(
                    possession.selected_hotbar_slot,
                    wheel_steps,
                    9,
                );
            } else {
                let slot = cycled_hotbar_slot(
                    editor.selected_hotbar_slot,
                    wheel_steps,
                    editor.creative_hotbar.len(),
                );
                editor.select_hotbar_slot(slot);
            }
        }
        let Ok((player_transform, _)) = players.single() else {
            return;
        };
        let rotation = Quat::from_euler(
            EulerRot::YXZ,
            editor.camera_yaw,
            editor.camera_pitch,
            0.0,
        );
        if let Ok((mut camera_transform, mut projection)) = cameras.single_mut() {
            if let Some((cockpit_eye, cockpit_rotation)) = spaceship_control
                .as_deref()
                .and_then(|control| {
                    control
                        .cockpit_eye
                        .map(|eye| (eye, control.cockpit_rotation))
                })
            {
                camera_transform.translation = cockpit_eye;
                camera_transform.rotation = cockpit_rotation * rotation;
            } else {
                camera_transform.translation =
                    player_transform.translation + Vec3::Y * FIRST_PERSON_EYE_OFFSET;
                camera_transform.rotation = rotation;
            }
            if let Projection::Perspective(perspective) = &mut *projection {
                perspective.fov = FIRST_PERSON_FOV_RADIANS;
            }
        }
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let cursor_in_viewport = window
        .cursor_position()
        .is_some_and(|cursor| editor.contains_cursor(cursor));
    if mouse.just_pressed(MouseButton::Middle) {
        editor.camera_drag_started_in_viewport =
            cursor_in_viewport && !egui_input.wants_pointer_input();
    }
    if !mouse.pressed(MouseButton::Middle) {
        editor.camera_drag_started_in_viewport = false;
    }

    if editor.camera_drag_started_in_viewport && mouse.pressed(MouseButton::Middle) {
        if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
            let rotation = Quat::from_euler(
                EulerRot::YXZ,
                editor.camera_yaw,
                editor.camera_pitch,
                0.0,
            );
            editor.camera_focus += rotation * Vec3::new(delta.x, -delta.y, 0.0) * 0.006;
        } else {
            editor.camera_yaw -= delta.x * 0.006;
            editor.camera_pitch = (editor.camera_pitch - delta.y * 0.006).clamp(-1.45, 1.2);
        }
    }
    if cursor_in_viewport && !egui_input.wants_pointer_input() {
        let scroll = wheel.read().map(|event| event.y).sum::<f32>();
        editor.camera_distance =
            (editor.camera_distance * (-scroll * 0.12).exp()).clamp(8.0, 900.0);
    } else {
        wheel.clear();
    }

    if let Ok((mut transform, mut projection)) = cameras.single_mut() {
        *transform = editor_camera_transform(&editor);
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = PerspectiveProjection::default().fov;
        }
    }
}

fn should_grab_first_person_cursor(
    window_focused: bool,
    inventory_open: bool,
    cursor_released: bool,
) -> bool {
    window_focused && !inventory_open && !cursor_released
}

fn selected_voxel_skill_targeting(
    character: &PlayerCharacter,
    slot_index: usize,
) -> Option<(String, VoxelSkillTargeting)> {
    let slot = *character.inventory.hotbar.get(slot_index)?;
    match slot {
        CharacterHotbarSlot::Empty | CharacterHotbarSlot::ReleaseControl => None,
        CharacterHotbarSlot::Skill(index) => {
            let name = character.skill_names.get(index)?.trim();
            let note = character
                .skill_notes
                .get(index)
                .map(String::as_str)
                .unwrap_or_default();
            let metadata = character
                .skill_metadata
                .get(index)
                .cloned()
                .unwrap_or_default();
            voxel_skill_targeting(note, &metadata)
                .map(|targeting| (name.to_owned(), targeting))
        },
        CharacterHotbarSlot::Item(index) => {
            let item = character.inventory.items.get(index)?;
            item.skills
                .iter()
                .filter(|skill| skill.metadata.is_approved())
                .find_map(|skill| {
                    let targeting = voxel_skill_targeting(&skill.note, &skill.metadata)?;
                    let name = if skill.name.trim().is_empty() {
                        item.name.trim()
                    } else {
                        skill.name.trim()
                    };
                    Some((name.to_owned(), targeting))
                })
        },
    }
}

fn voxel_skill_targeting(
    note: &str,
    metadata: &CharacterSkillMetadata,
) -> Option<VoxelSkillTargeting> {
    if !metadata.is_approved() {
        return None;
    }
    let metadata_range = metadata
        .range
        .filter(|range| *range > 0)
        .map(|range| range as f32);
    let target_class = metadata.target_class.as_deref().map(str::trim);
    if target_class == Some("无目标") {
        return None;
    }

    let parsed_targets = parse_rule(note)
        .ok()
        .into_iter()
        .flat_map(|ast| ast.actions)
        .map(|action| match action {
            Action::Heal { target, .. }
            | Action::Damage { target, .. }
            | Action::GrantBuff { target, .. } => target,
        })
        .collect::<Vec<_>>();
    let parsed_area_radius = parsed_targets
        .iter()
        .filter_map(|target| target.area.and_then(|area| area.radius_meters))
        .max_by(f32::total_cmp);
    let parsed_as_area = parsed_targets.iter().any(|target| target.area.is_some());
    let parsed_as_person = parsed_targets
        .iter()
        .any(|target| target.area.is_none() && !matches!(target.actor, ActorRef::SelfActor));

    if target_class == Some("范围") || parsed_as_area {
        return parsed_area_radius
            .or(metadata_range)
            .map(|radius| VoxelSkillTargeting::Area { radius });
    }
    if target_class == Some("单目标") || parsed_as_person {
        return metadata_range.map(|range| VoxelSkillTargeting::Person { range });
    }
    None
}

fn targeting_ray_end_distance(range: f32, blocker_distances: impl IntoIterator<Item = f32>) -> f32 {
    blocker_distances
        .into_iter()
        .filter(|distance| *distance <= range)
        .reduce(f32::min)
        .unwrap_or(range)
}

fn draw_possessed_player_targeting(
    mut gizmos: Gizmos,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    grids: Query<&Grid<u8>, With<TrpgVoxelGrid>>,
    planets: Query<(&VoxelOrbitalPlanet, &GlobalTransform)>,
    voxel_blockers: Query<
        (),
        Or<(
            With<VoxelPhysicsBody>,
            With<VoxelAutoDoor>,
        )>,
    >,
    standees: Query<
        (
            &VoxelPlayerStandee,
            &GlobalTransform,
            &Visibility,
        ),
        Without<VoxelFirstPersonPlayer>,
    >,
    spatial_query: SpatialQuery,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    possession: Res<VoxelPossessionState>,
    editor: Res<VoxelEditorState>,
    mut preview: ResMut<VoxelTargetingPreview>,
) {
    preview.skill_name.clear();
    preview.affected_user_ids.clear();
    preview.show_affected_players = false;
    let (Some(manager), Some(active_user_id)) = (manager, possession.active_user_id) else {
        return;
    };
    let Some(character) = manager.player_characters.get(&active_user_id.to_string()) else {
        return;
    };
    let Some((skill_name, targeting)) = selected_voxel_skill_targeting(
        character,
        possession.selected_hotbar_slot,
    ) else {
        return;
    };
    preview.skill_name = skill_name;

    match targeting {
        VoxelSkillTargeting::Area { radius } => {
            preview.show_affected_players = true;
            let Some(actor_position) = standees
                .iter()
                .find(|(standee, ..)| standee.user_id == active_user_id)
                .map(|(_, transform, _)| transform.translation())
                .or_else(|| {
                    cameras
                        .single()
                        .ok()
                        .map(|(_, transform)| transform.translation())
                })
            else {
                return;
            };
            let ground_center = actor_position - Vec3::Y * FIRST_PERSON_EYE_OFFSET;
            gizmos
                .circle(
                    Isometry3d::new(
                        ground_center + Vec3::Y * (VOXEL_SIZE * 0.08),
                        Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
                    ),
                    radius,
                    Color::srgba(1.0, 0.35, 0.08, 0.9),
                )
                .resolution(64);
            for (standee, transform, visibility) in &standees {
                if standee.user_id == active_user_id || *visibility == Visibility::Hidden {
                    continue;
                }
                let target_position = transform.translation();
                if actor_position.distance(target_position) > radius {
                    continue;
                }
                preview.affected_user_ids.push(standee.user_id);
                gizmos.sphere(
                    Isometry3d::from_translation(target_position),
                    VOXEL_SIZE * 0.7,
                    Color::srgb(1.0, 0.75, 0.12),
                );
            }
            preview.affected_user_ids.sort_unstable();
        },
        VoxelSkillTargeting::Person { range } => {
            let (Ok(window), Ok((camera, camera_transform)), Ok(grid)) = (
                windows.single(),
                cameras.single(),
                grids.single(),
            ) else {
                return;
            };
            let Some(ray) = viewport_ray(
                window,
                camera,
                camera_transform,
                &editor,
            ) else {
                return;
            };
            let grid_distance = raycast_grid(grid, ray)
                .filter(|hit| hit.occupied.is_some())
                .map(|hit| hit.distance);
            let planet_distance = planets
                .iter()
                .filter_map(|(planet, transform)| {
                    raycast_voxel_planet(planet, transform, ray).map(|hit| hit.distance)
                })
                .min_by(f32::total_cmp);
            let body_distance = spatial_query
                .cast_ray_predicate(
                    ray.origin,
                    ray.direction,
                    range,
                    true,
                    &SpatialQueryFilter::default(),
                    &|entity| voxel_blockers.contains(entity),
                )
                .map(|hit| hit.distance);
            let player_hit = standees
                .iter()
                .filter(|(standee, _, visibility)| {
                    standee.user_id != active_user_id && **visibility != Visibility::Hidden
                })
                .filter_map(|(standee, transform, _)| {
                    ray_intersects_player_standee(ray, transform, standee.half_size)
                        .filter(|distance| *distance <= range)
                        .map(|distance| (standee.user_id, distance))
                })
                .min_by(|left, right| left.1.total_cmp(&right.1));
            let end_distance = targeting_ray_end_distance(
                range,
                grid_distance
                    .into_iter()
                    .chain(planet_distance)
                    .chain(body_distance)
                    .chain(player_hit.map(|(_, distance)| distance)),
            );
            let end = ray.origin + *ray.direction * end_distance;
            gizmos.line(
                ray.origin,
                end,
                Color::srgb(0.1, 0.9, 1.0),
            );
            gizmos.sphere(
                Isometry3d::from_translation(end),
                VOXEL_SIZE * 0.18,
                Color::srgb(0.1, 0.9, 1.0),
            );
        },
    }
}

fn draw_voxel_target(
    mut gizmos: Gizmos,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    grids: Query<&Grid<u8>, With<TrpgVoxelGrid>>,
    physics_bodies: Query<(), With<VoxelPhysicsBody>>,
    planets: Query<(&VoxelOrbitalPlanet, &GlobalTransform)>,
    placed_lights: Query<(&GlobalTransform, &VoxelPlacedLight)>,
    spatial_query: SpatialQuery,
    editor: Res<VoxelEditorState>,
    egui_input: Res<EguiWantsInput>,
) {
    let Ok(grid) = grids.single() else {
        return;
    };
    let (hit, explosion_origin, planet_target, planet_cell_target) = if editor
        .creative_inventory_open
        || editor.teleport_menu_open
        || editor.is_teleport_tool_equipped()
        || voxel_world_pointer_blocked(
            egui_input.wants_any_pointer_input(),
            editor.left_started_over_ui || editor.right_started_over_ui,
        ) {
        (None, None, None, None)
    } else {
        let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single())
        else {
            return;
        };
        let Some(ray) = viewport_ray(
            window,
            camera,
            camera_transform,
            &editor,
        ) else {
            return;
        };
        let hit = raycast_grid(grid, ray);
        let planet_hit_any = planets.iter().next().and_then(|(planet, transform)| {
            raycast_voxel_planet(planet, transform, ray).map(|planet_hit| (planet_hit, transform))
        });
        let planet_hit = planet_hit_any;
        let planet_cell_target = planet_hit
            .filter(|(planet_hit, _)| {
                hit.as_ref()
                    .is_none_or(|grid_hit| planet_hit.distance < grid_hit.distance)
            })
            .and_then(|(planet_hit, _)| {
                Some(match editor.mode {
                    VoxelEditMode::Add => planet_hit.occupied + planet_hit.normal,
                    VoxelEditMode::Remove | VoxelEditMode::Paint | VoxelEditMode::Physics => {
                        planet_hit.occupied
                    },
                    _ => return None,
                })
            });
        let planet_target = planet_cell_target.and_then(|cell| {
            planet_hit.map(|(_, transform)| transform.transform_point(cell.as_vec3() * VOXEL_SIZE))
        });
        let hit = planet_cell_target.is_none().then_some(hit).flatten();
        let explosion_origin = if editor.mode == VoxelEditMode::Explode {
            let body_hit = spatial_query.cast_ray_predicate(
                ray.origin,
                ray.direction,
                MAX_RAY_DISTANCE,
                true,
                &SpatialQueryFilter::default(),
                &|entity| physics_bodies.contains(entity),
            );
            let static_distance = hit
                .as_ref()
                .filter(|hit| hit.occupied.is_some())
                .map(|hit| hit.distance);
            let planet_distance = planet_hit_any.map(|(hit, _)| hit.distance);
            body_hit
                .as_ref()
                .map(|hit| hit.distance)
                .into_iter()
                .chain(static_distance)
                .chain(planet_distance)
                .reduce(f32::min)
                .map(|distance| ray.origin + *ray.direction * distance)
        } else {
            None
        };
        (
            hit,
            explosion_origin,
            planet_target,
            planet_cell_target,
        )
    };
    let target = match (editor.light_tool, editor.mode, hit) {
        (Some(VoxelLightTool::Remove), ..) => None,
        (Some(_), _, Some(hit)) => hit.add,
        (None, VoxelEditMode::Add, Some(hit)) => hit.add,
        (
            None,
            VoxelEditMode::Remove
            | VoxelEditMode::Paint
            | VoxelEditMode::Physics
            | VoxelEditMode::Drag
            | VoxelEditMode::Push
            | VoxelEditMode::Pull
            | VoxelEditMode::Explode,
            Some(hit),
        ) => hit.occupied,
        (_, _, None) => None,
    };
    if editor.light_tool.is_some() {
        for (transform, light) in &placed_lights {
            let position = transform.translation();
            let color = Color::srgb(
                light.color[0],
                light.color[1],
                light.color[2],
            );
            gizmos.sphere(
                Isometry3d::from_translation(position),
                VOXEL_SIZE * 0.65,
                color,
            );
            if light.kind == VoxelLightTool::Spot {
                gizmos.arrow(
                    position,
                    position + light.direction.normalize_or_zero() * VOXEL_SIZE * 3.0,
                    color,
                );
            }
        }
    }
    if editor.mode == VoxelEditMode::Physics {
        let selection_end = editor.selection_end.or(if editor.selection_is_planet {
            planet_cell_target
        } else {
            target
        });
        if let (Some(start), Some(end)) = (editor.selection_anchor, selection_end) {
            let (min, max) = (start.min(end), start.max(end));
            let size = (max - min + IVec3::ONE).as_vec3() * VOXEL_SIZE;
            let local_center = if editor.selection_is_planet {
                (min.as_vec3() + (max - min).as_vec3() * 0.5) * VOXEL_SIZE
            } else {
                (min.as_vec3() + (max - min + IVec3::ONE).as_vec3() * 0.5) * VOXEL_SIZE
            };
            let center = if editor.selection_is_planet {
                planets
                    .iter()
                    .next()
                    .map_or(local_center, |(_, transform)| {
                        transform.transform_point(local_center)
                    })
            } else {
                local_center
            };
            gizmos.cube(
                Transform::from_translation(center).with_scale(size),
                Color::srgb(0.15, 0.9, 1.0),
            );
            if editor.selection_end.is_some() {
                let selected_cells = if editor.selection_is_planet {
                    planets
                        .iter()
                        .next()
                        .map(|(planet, _)| {
                            planet
                                .cells
                                .iter()
                                .filter_map(|(cell, material)| {
                                    (cell.cmpge(min).all()
                                        && cell.cmple(max).all()
                                        && TrpgVoxelConnector::solid(material))
                                    .then_some((*cell, *material))
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                } else {
                    selected_solid_voxels(grid, min, max)
                };
                for (cell, _) in selected_cells {
                    let local_center = if editor.selection_is_planet {
                        cell.as_vec3() * VOXEL_SIZE
                    } else {
                        (cell.as_vec3() + Vec3::splat(0.5)) * VOXEL_SIZE
                    };
                    let center = if editor.selection_is_planet {
                        planets
                            .iter()
                            .next()
                            .map_or(local_center, |(_, transform)| {
                                transform.transform_point(local_center)
                            })
                    } else {
                        local_center
                    };
                    gizmos.cube(
                        Transform::from_translation(center)
                            .with_scale(Vec3::splat(VOXEL_SIZE * 0.88)),
                        Color::srgb(0.2, 1.0, 0.65),
                    );
                }
            }
        }
    }
    if let Some(origin) = explosion_origin {
        gizmos.sphere(
            Isometry3d::from_translation(origin),
            editor.physics_explosion_radius.max(VOXEL_SIZE),
            Color::srgb(1.0, 0.3, 0.08),
        );
    }
    if let Some(center) = planet_target {
        let size = if editor.mode == VoxelEditMode::Physics {
            VOXEL_SIZE
        } else {
            (editor.brush_radius * 2 + 1) as f32 * VOXEL_SIZE
        };
        gizmos.cube(
            Transform::from_translation(center).with_scale(Vec3::splat(size)),
            Color::srgb(1.0, 0.9, 0.2),
        );
    }
    // Keep the aimed voxel visible in both orbit and first-person views. In
    // first person the ray originates at the centered crosshair.
    if let Some(target) = target {
        let size = if matches!(
            editor.mode,
            VoxelEditMode::Physics | VoxelEditMode::Explode
        ) {
            VOXEL_SIZE
        } else {
            (editor.brush_radius * 2 + 1) as f32 * VOXEL_SIZE
        };
        let center = (target.as_vec3() + Vec3::splat(0.5)) * VOXEL_SIZE;
        gizmos.cube(
            Transform::from_translation(center).with_scale(Vec3::splat(size)),
            Color::srgb(1.0, 0.9, 0.2),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_possession_disables_the_gm_possession_tool() {
        assert!(possession_tool_can_target(true, None));
        assert!(!possession_tool_can_target(
            true,
            Some(42)
        ));
        assert!(!possession_tool_can_target(false, None));
    }

    #[test]
    fn release_control_hotbar_item_exits_player_possession() {
        let character = PlayerCharacter::default();
        let mut possession = VoxelPossessionState::default();
        possession.possess(42);

        activate_player_hotbar_slot(&mut possession, Some(&character), 8);

        assert_eq!(possession.selected_hotbar_slot, 8);
        assert_eq!(possession.active_user_id, None);
    }

    #[test]
    fn legacy_whirlwind_note_creates_a_five_meter_area_preview() {
        assert_eq!(
            voxel_skill_targeting(
                "主动使用对周围5米内的目标造成4点物理伤害",
                &CharacterSkillMetadata::default(),
            ),
            Some(VoxelSkillTargeting::Area { radius: 5.0 })
        );
    }

    #[test]
    fn selected_player_item_uses_its_targeting_skill() {
        let mut character = PlayerCharacter::default();
        character
            .inventory
            .items
            .push(crate::napcat::InventoryItem {
                name: "法杖".to_owned(),
                skills: vec![crate::napcat::InventoryItemSkill {
                    name: "射线".to_owned(),
                    metadata: CharacterSkillMetadata {
                        target_class: Some("单目标".to_owned()),
                        range: Some(12),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            });
        character.inventory.hotbar[0] = CharacterHotbarSlot::Item(0);

        assert_eq!(
            selected_voxel_skill_targeting(&character, 0),
            Some((
                "射线".to_owned(),
                VoxelSkillTargeting::Person { range: 12.0 }
            ))
        );
    }

    #[test]
    fn targeting_ray_stops_at_nearest_blocker_or_exact_skill_range() {
        assert_eq!(
            targeting_ray_end_distance(10.0, [7.0, 3.0, 12.0]),
            3.0
        );
        assert_eq!(
            targeting_ray_end_distance(10.0, [12.0, 15.0]),
            10.0
        );
        assert_eq!(
            targeting_ray_end_distance(10.0, []),
            10.0
        );
    }

    fn test_grid() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            VoxelPlugin::<u8>::default(),
            ConnectivityPlugin::<TrpgVoxelConnector>::default(),
        ))
        .add_systems(
            Startup,
            (setup_voxel_grid, populate_voxel_grid).chain(),
        );
        app.update();
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<TrpgVoxelGrid>>()
            .single(app.world())
            .unwrap();
        (app, entity)
    }

    #[test]
    fn voxel_brush_radius_allows_fifty_cells() {
        assert_eq!(MAX_VOXEL_BRUSH_RADIUS, 50);
    }

    #[test]
    fn chunk_columns_use_canonical_voxel_scale_and_floor_negative_positions() {
        let chunk_size = VOXEL_SIZE * DIMS.x as f32;
        assert_eq!(
            voxel_chunk_column(Vec3::ZERO),
            IVec2::ZERO
        );
        assert_eq!(
            voxel_chunk_column(Vec3::new(chunk_size, 900.0, -0.01)),
            IVec2::new(1, -1)
        );
    }

    #[test]
    fn physics_chunk_loading_unions_dm_and_player_neighborhoods() {
        let chunk_size = VOXEL_SIZE * DIMS.x as f32;
        let columns =
            loaded_physics_chunk_columns([Vec3::ZERO, Vec3::new(chunk_size * 30.0, 0.0, 0.0)]);
        assert!(columns.contains(&IVec2::ZERO));
        assert!(columns.contains(&IVec2::new(30, 0)));
        assert!(columns.contains(&IVec2::splat(
            VOXEL_PHYSICS_CHUNK_LOAD_RADIUS
        )));
        assert!(!columns.contains(&IVec2::new(
            VOXEL_PHYSICS_CHUNK_LOAD_RADIUS + 1,
            0
        )));
    }

    #[test]
    fn physics_body_loads_when_any_cell_overlaps_an_observer_chunk() {
        let body = VoxelPhysicsBody {
            local_center: Vec3::ZERO,
            cells: vec![(IVec3::ZERO, 1), (IVec3::new(DIMS.x, 0, 0), 1)],
        };
        let loader = VoxelPhysicsChunkLoader {
            columns: HashSet::from([IVec2::X]),
            ..default()
        };
        assert!(voxel_physics_body_is_loaded(
            &body,
            &Transform::IDENTITY,
            &loader
        ));
        assert!(!voxel_physics_body_is_loaded(
            &body,
            &Transform::from_xyz(100.0, 0.0, 0.0),
            &loader
        ));
    }

    #[test]
    fn distant_physics_body_is_cached_and_respawned_without_disabling_rigid_body() {
        let mut app = App::new();
        app.init_resource::<VoxelPhysicsChunkLoader>()
            .init_resource::<Assets<Mesh>>()
            .insert_resource(VoxelMaterials {
                handles: std::array::from_fn(|_| Handle::default()),
                planet_ocean: Handle::default(),
            })
            .add_systems(Update, stream_voxel_physics_bodies);
        let position = Vec3::new(
            VOXEL_SIZE * DIMS.x as f32 * 20.0,
            0.0,
            0.0,
        );
        app.world_mut().spawn((
            VoxelPhysicsBody {
                local_center: Vec3::splat(VOXEL_SIZE * 0.5),
                cells: vec![(IVec3::ZERO, 1)],
            },
            Transform::from_translation(position),
            LinearVelocity(Vec3::X),
            AngularVelocity(Vec3::Y),
        ));

        app.update();

        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<VoxelPhysicsBody>>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(
            app.world()
                .resource::<VoxelPhysicsChunkLoader>()
                .unloaded_bodies
                .len(),
            1
        );

        app.world_mut()
            .resource_mut::<VoxelPhysicsChunkLoader>()
            .columns
            .insert(voxel_chunk_column(position));
        app.update();

        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<VoxelPhysicsBody>>()
                .iter(app.world())
                .count(),
            1
        );
        assert!(app
            .world()
            .resource::<VoxelPhysicsChunkLoader>()
            .unloaded_bodies
            .is_empty());
    }

    #[test]
    fn player_camera_store_upserts_without_duplicating_users() {
        let mut store = VoxelPlayerCameraStore::default();
        let first = Transform::from_xyz(1.0, 2.0, 3.0);
        let second = Transform::from_xyz(4.0, 5.0, 6.0).with_rotation(Quat::from_rotation_y(0.5));

        upsert_voxel_player_camera(&mut store, 42, &first);
        upsert_voxel_player_camera(&mut store, 42, &second);

        assert_eq!(store.cameras.len(), 1);
        let restored = voxel_player_camera_transform(&store.cameras[0]);
        assert_eq!(restored.translation, second.translation);
        assert_eq!(restored.rotation, second.rotation);
    }

    #[test]
    fn completed_players_are_the_canonical_camera_roster() {
        let mut characters = HashMap::new();
        characters.insert("42".to_owned(), PlayerCharacter {
            inited: true,
            ..default()
        });
        characters.insert(
            "43".to_owned(),
            PlayerCharacter::default(),
        );
        characters.insert(
            "not-a-qq-number".to_owned(),
            PlayerCharacter {
                inited: true,
                ..default()
            },
        );

        assert_eq!(
            completed_voxel_player_user_ids(&characters).collect::<HashSet<_>>(),
            HashSet::from([42])
        );
    }

    #[test]
    fn new_player_camera_starts_at_the_current_dm_view() {
        let store = VoxelPlayerCameraStore::default();
        let dm_view = Transform::from_xyz(7.0, 8.0, 9.0).with_rotation(Quat::from_rotation_y(0.75));

        let player_view = initial_voxel_player_camera_transform(&store, 42, dm_view);

        assert_eq!(
            player_view.translation,
            dm_view.translation
        );
        assert_eq!(player_view.rotation, dm_view.rotation);
    }

    #[test]
    fn player_capture_target_supports_render_and_readback() {
        let image = voxel_player_capture_image();
        assert_eq!(
            image.texture_descriptor.size.width,
            PLAYER_CAPTURE_WIDTH
        );
        assert_eq!(
            image.texture_descriptor.size.height,
            PLAYER_CAPTURE_HEIGHT
        );
        assert!(image
            .texture_descriptor
            .usage
            .contains(TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC));
    }

    #[test]
    fn active_voxel_capture_routes_video_requests_to_mp4_frames() {
        let output_dir = Path::new("scene-captures");
        let (image_path, image_frames) = voxel_capture_output_paths(
            output_dir,
            SceneCaptureKind::Image,
            7,
            42,
        );
        let (video_path, video_frames) = voxel_capture_output_paths(
            output_dir,
            SceneCaptureKind::PanoramaVideo,
            7,
            42,
        );

        assert_eq!(
            image_path,
            output_dir.join("player_42.png")
        );
        assert!(image_frames.is_none());
        assert_eq!(
            video_path,
            output_dir.join("player_42_360_7.mp4")
        );
        assert_eq!(
            video_frames,
            Some(output_dir.join("voxel_video_7_frames"))
        );
    }

    #[test]
    fn player_standee_follows_capture_camera_exactly() {
        let camera = Transform::from_xyz(4.0, 5.0, 6.0).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            0.4,
            -0.2,
            0.1,
        ));

        let standee = voxel_player_standee_transform(&camera);

        assert_eq!(standee.translation, camera.translation);
        assert_eq!(standee.rotation, camera.rotation);
    }

    #[test]
    fn current_player_view_keeps_the_dm_in_first_person() {
        let player_view = Transform::from_xyz(4.0, 5.0, 6.0).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            0.4,
            -0.2,
            0.0,
        ));
        let mut editor = VoxelEditorState {
            first_person_enabled: true,
            first_person_flying: false,
            ..default()
        };
        let mut dm_body = Transform::from_xyz(-10.0, -10.0, -10.0);
        let mut velocity = LinearVelocity(Vec3::splat(5.0));

        apply_voxel_player_view_to_dm(
            &mut editor,
            &mut dm_body,
            &mut velocity,
            &player_view,
        );

        assert!(editor.first_person_enabled);
        assert!(editor.first_person_flying);
        assert_eq!(
            dm_body.translation,
            first_person_player_position(player_view.translation)
        );
        assert_eq!(velocity.0, Vec3::ZERO);
        let rotation = Quat::from_euler(
            EulerRot::YXZ,
            editor.camera_yaw,
            editor.camera_pitch,
            0.0,
        );
        assert!((rotation * Vec3::NEG_Z).dot(*player_view.forward()) > 0.999_9);
    }

    #[test]
    fn dm_starts_in_creative_flight() {
        let editor = VoxelEditorState::default();

        assert!(editor.first_person_enabled);
        assert!(editor.first_person_flying);
    }

    #[test]
    fn teleport_destinations_use_canonical_world_landmarks() {
        assert_eq!(
            VoxelTeleportDestination::ResearchStation.player_position(),
            Some(
                (RESEARCH_STATION_CENTER
                    + IVec3::new(NIFFY.spawn[0], 0, NIFFY.spawn[1]))
                .as_vec3()
                    * VOXEL_SIZE
                    + Vec3::Y * 0.5
            )
        );
        assert_eq!(
            VoxelTeleportDestination::CombatSpaceship.player_position(),
            Some(
                (COMBAT_SPACESHIP_CENTER
                    + IVec3::new(ARROGANCE.spawn[0], 0, ARROGANCE.spawn[1]))
                .as_vec3()
                    * VOXEL_SIZE
                    + Vec3::Y * 0.5
            )
        );
        assert_eq!(
            VoxelTeleportDestination::AbandonedStation.player_position(),
            Some(
                (ABANDONED_STATION_CENTER
                    + IVec3::new(ABANDONED.spawn[0], 0, ABANDONED.spawn[1]))
                .as_vec3()
                    * VOXEL_SIZE
                    + Vec3::Y * 0.5
            )
        );
        assert_eq!(
            VoxelTeleportDestination::PlanetScienceLab.player_position(),
            Some(
                ORBITAL_PLANET_CENTER
                    + Vec3::new(
                        (PLANET_SCIENCE_LAB_CENTER.x + XY_PLANET.spawn[0]) as f32,
                        PLANET_SCIENCE_LAB_FLOOR_Y as f32,
                        (PLANET_SCIENCE_LAB_CENTER.y + XY_PLANET.spawn[1]) as f32,
                    ) * VOXEL_SIZE
                    + Vec3::Y * 0.5
            )
        );
    }

    #[test]
    fn minimap_keeps_the_highest_voxel_in_each_top_down_tile() {
        let low = IVec3::new(-4, 2, 9);
        let high = IVec3::new(-4, 7, 9);
        let other = IVec3::new(12, 1, -3);

        let snapshot = voxel_minimap_snapshot_from_cells(&[(low, 2), (high, 8), (other, 6)], 8);
        let fraction = snapshot.world_fraction(high.as_vec3() * VOXEL_SIZE);
        let (x, z) = snapshot.tile_indices(fraction);
        let tile = snapshot.tile(x, z).unwrap();

        assert_eq!(tile.top_cell, high);
        assert_eq!(tile.material, 8);
        assert_eq!(tile.voxel_count, 2);
        assert_eq!(snapshot.nearest_cell(x, z), Some(high));
    }

    #[test]
    fn map_teleport_uses_the_canonical_voxel_scale() {
        let cell = IVec3::new(8, 12, -20);

        assert_eq!(
            VoxelTeleportDestination::MapCell(cell).player_position(),
            Some(cell.as_vec3() * VOXEL_SIZE + Vec3::Y * 0.5)
        );
    }

    #[test]
    fn teleport_request_moves_only_the_dm_body() {
        let destination = VoxelTeleportDestination::SensorStation;
        let mut editor = VoxelEditorState::default();
        editor.request_teleport(destination);
        let mut app = App::new();
        app.insert_resource(editor)
            .init_resource::<VoxelPossessionState>()
            .add_systems(Update, apply_voxel_teleport);
        let player = app
            .world_mut()
            .spawn((
                VoxelFirstPersonPlayer,
                Transform::default(),
                LinearVelocity(Vec3::ONE),
            ))
            .id();

        app.update();

        let entity = app.world().entity(player);
        assert_eq!(
            entity.get::<Transform>().unwrap().translation,
            destination.player_position().unwrap()
        );
        assert_eq!(
            entity.get::<LinearVelocity>().unwrap().0,
            Vec3::ZERO
        );
    }

    #[test]
    fn teleport_request_moves_the_dm_to_a_player_standee() {
        let user_id = 1_670_426_821;
        let standee_eye_position = Vec3::new(12.0, 3.0, -8.0);
        let mut editor = VoxelEditorState::default();
        editor.request_teleport(VoxelTeleportDestination::PlayerStandee(
            user_id,
        ));
        let mut app = App::new();
        app.insert_resource(editor)
            .init_resource::<VoxelPossessionState>()
            .add_systems(Update, apply_voxel_teleport);
        let player = app
            .world_mut()
            .spawn((
                VoxelFirstPersonPlayer,
                Transform::default(),
                LinearVelocity(Vec3::ONE),
            ))
            .id();
        app.world_mut().spawn((
            VoxelPlayerStandee {
                user_id,
                image_source: "avatar.png".to_owned(),
                half_size: Vec2::splat(VOXEL_SIZE),
            },
            GlobalTransform::from_translation(standee_eye_position),
        ));

        app.update();

        let entity = app.world().entity(player);
        assert_eq!(
            entity.get::<Transform>().unwrap().translation,
            first_person_player_position(standee_eye_position)
        );
        assert_eq!(
            entity.get::<LinearVelocity>().unwrap().0,
            Vec3::ZERO
        );
    }

    #[test]
    fn released_first_person_cursor_blocks_viewport_actions() {
        let mut editor = VoxelEditorState::default();
        editor.first_person_cursor_released = true;

        assert!(viewport_ray(
            &Window::default(),
            &Camera::default(),
            &GlobalTransform::default(),
            &editor,
        )
        .is_none());
    }

    #[test]
    fn first_person_cursor_is_only_grabbed_while_window_is_focused() {
        assert!(should_grab_first_person_cursor(
            true, false, false
        ));
        assert!(!should_grab_first_person_cursor(
            false, false, false
        ));
        assert!(!should_grab_first_person_cursor(
            true, true, false
        ));
        assert!(!should_grab_first_person_cursor(
            true, false, true
        ));
    }

    #[test]
    fn first_person_cursor_starts_released_until_viewport_click() {
        let editor = VoxelEditorState::default();
        assert!(editor.first_person_cursor_released);
        assert!(!should_grab_first_person_cursor(
            true,
            false,
            editor.first_person_cursor_released,
        ));
    }

    #[test]
    fn viewport_bounds_exclude_the_top_toolbar_from_cursor_recapture() {
        let mut editor = VoxelEditorState::default();
        editor.set_viewport_bounds(
            Vec2::new(300.0, 50.0),
            Vec2::new(1_600.0, 900.0),
            160.0,
        );

        assert!(!editor.contains_cursor(Vec2::new(800.0, 120.0)));
        assert!(editor.contains_cursor(Vec2::new(800.0, 200.0)));
    }

    #[test]
    fn player_standee_uses_canonical_two_voxel_height() {
        let size = voxel_player_standee_size(Vec2::new(300.0, 600.0));

        assert_eq!(size.y, VOXEL_SIZE * 2.0);
        assert_eq!(size.x, VOXEL_SIZE);
    }

    #[test]
    fn unit_pool_places_one_persistent_standee_at_the_gm_focus() {
        let editor = VoxelEditorState {
            first_person_enabled: false,
            camera_focus: Vec3::new(12.0, 3.0, -8.0),
            ..default()
        };
        let mut store = VoxelUnitStandeeStore::default();

        assert!(place_voxel_unit_standee(
            &mut store,
            " slime ",
            "slime.png",
            &editor,
        )
        .unwrap());
        assert!(!place_voxel_unit_standee(
            &mut store,
            "slime",
            "slime-v2.png",
            &editor,
        )
        .unwrap());
        assert!(has_voxel_unit_standee(&store, "slime"));
        assert_eq!(store.standees.len(), 1);
        assert_eq!(store.standees[0].unit_id, "slime");
        assert_eq!(
            Vec3::from_array(store.standees[0].translation),
            editor.camera_focus
        );
        assert_eq!(
            store.standees[0].visibility,
            AccessVisibility::Public
        );
        assert_eq!(
            voxel_unit_standee_target_id(" slime "),
            "unit:slime"
        );

        assert!(remove_voxel_unit_standee(
            &mut store, " slime "
        ));
        assert!(!has_voxel_unit_standee(&store, "slime"));
    }

    #[test]
    fn unit_standee_visibility_obeys_explicit_player_access() {
        let party_member = crate::napcat::PlayerAccess {
            player_id: 42,
            party_id: Some("red".to_owned()),
            party_ids: vec!["red".to_owned()],
            ..default()
        };
        let other_party = crate::napcat::PlayerAccess {
            player_id: 43,
            party_id: Some("blue".to_owned()),
            party_ids: vec!["blue".to_owned()],
            ..default()
        };
        let gm = crate::napcat::PlayerAccess {
            player_id: 99,
            is_gm: true,
            ..default()
        };

        assert!(voxel_unit_standee_visible_for_access(
            &party_member,
            &AccessVisibility::Public,
        ));
        assert!(voxel_unit_standee_visible_for_access(
            &party_member,
            &AccessVisibility::Party("red".to_owned()),
        ));
        assert!(!voxel_unit_standee_visible_for_access(
            &other_party,
            &AccessVisibility::Party("red".to_owned()),
        ));
        assert!(voxel_unit_standee_visible_for_access(
            &gm,
            &AccessVisibility::Gm,
        ));
    }

    #[test]
    fn voxel_standees_publish_positions_for_scene_commands() {
        let mut app = App::new();
        app.init_resource::<SceneCharacterPositions>().add_systems(
            Update,
            sync_voxel_scene_character_positions,
        );
        app.world_mut().spawn((
            VoxelPlayerStandee {
                user_id: 1_670_426_821,
                image_source: "avatar.png".to_owned(),
                half_size: Vec2::splat(VOXEL_SIZE),
            },
            Transform::from_xyz(12.0, 3.0, -8.0),
        ));
        app.world_mut().spawn((
            VoxelUnitStandee {
                target_id: "unit:slime".to_owned(),
                unit_id: "slime".to_owned(),
                image_source: "slime.png".to_owned(),
                access_visibility: AccessVisibility::Public,
            },
            Transform::from_xyz(7.0, 2.0, 4.0),
        ));

        app.update();

        assert_eq!(
            app.world()
                .resource::<SceneCharacterPositions>()
                .positions
                .get("1670426821"),
            Some(&Vec3::new(12.0, 3.0, -8.0))
        );
        assert_eq!(
            app.world()
                .resource::<SceneCharacterPositions>()
                .positions
                .get("unit:slime"),
            Some(&Vec3::new(7.0, 2.0, 4.0))
        );
    }

    #[test]
    fn player_standee_back_label_uses_the_opposite_face() {
        let back = voxel_player_standee_back_transform();
        let transform = voxel_player_standee_back_label_transform();

        assert!(back.translation.z < 0.0);
        assert!(transform.translation.z > 0.0);
        assert_eq!(PLAYER_STANDEE_PLANE_NORMAL, Vec3::Z);
        assert!((back.rotation * Vec3::Z).abs_diff_eq(Vec3::NEG_Z, 0.000_01));
        assert!((transform.rotation * Vec3::Z).abs_diff_eq(Vec3::Z, 0.000_01));
    }

    #[test]
    fn player_standee_back_label_texture_contains_the_chinese_glyph() {
        let image = voxel_player_standee_back_label_image();
        let pixels = image.data.as_ref().expect("back label keeps CPU texels");

        assert_eq!(image.width(), 128);
        assert_eq!(image.height(), 128);
        assert!(pixels
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 200 && pixel[1] > 100 && pixel[2] > 100));
    }

    #[test]
    fn possession_tool_ray_selects_only_inside_player_standee_bounds() {
        let standee = GlobalTransform::from_translation(Vec3::new(0.0, 0.0, -5.0));
        let center_ray = Ray3d::new(Vec3::ZERO, Dir3::NEG_Z);
        let missed_ray = Ray3d::new(Vec3::X, Dir3::NEG_Z);

        let distance = ray_intersects_player_standee(center_ray, &standee, Vec2::splat(0.5))
            .expect("center ray should hit standee");
        assert!((distance - 5.0).abs() < 0.0001);
        assert!(ray_intersects_player_standee(missed_ray, &standee, Vec2::splat(0.5),).is_none());
    }

    #[test]
    fn player_standee_material_is_opaque() {
        let material = voxel_player_standee_material(Handle::<Image>::default());

        assert!(matches!(
            material.alpha_mode,
            AlphaMode::Opaque
        ));
        assert_eq!(material.cull_mode, Some(Face::Back));
    }

    #[test]
    fn voxel_glass_is_solid_two_sided_and_five_percent_opaque() {
        let material = voxel_glass_material();

        assert_eq!(material.base_color.alpha(), VOXEL_GLASS_OPACITY);
        assert!(matches!(material.alpha_mode, AlphaMode::Blend));
        assert_eq!(material.cull_mode, None);
        assert!(TrpgVoxelConnector::solid(&VOXEL_GLASS_MATERIAL));
        assert_eq!(radiance_voxel_color(VOXEL_GLASS_MATERIAL), [0; 4]);
    }

    #[test]
    fn workbook_space_stations_have_panoramic_glass_with_metal_frames() {
        for design in [NIFFY, KYO, ARBITRATOR, ABANDONED] {
            let mut world = World::new();
            let entity = world.spawn(Grid::<u8>::new()).id();
            {
                let mut entity_mut = world.entity_mut(entity);
                let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
                build_workbook_orbital_location(&mut grid, IVec3::ZERO, design);
            }
            let grid = world.entity(entity).get::<Grid<u8>>().unwrap();
            let glass_columns = voxel_cells(grid)
                .into_iter()
                .filter(|(_, material)| *material == VOXEL_GLASS_MATERIAL)
                .map(|(cell, _)| (cell.x, cell.z))
                .collect::<HashSet<_>>();

            assert!(
                glass_columns.len() >= 8,
                "{} needs multiple panoramic glass bays",
                design.name
            );
            for (x, z) in glass_columns {
                assert_eq!(grid.get(IVec3::new(x, 1, z)).copied(), Some(6));
                assert_eq!(
                    grid.get(IVec3::new(x, WORKBOOK_ROOM_HEIGHT - 1, z))
                        .copied(),
                    Some(6)
                );
            }
        }
    }

    #[test]
    fn player_capture_hides_self_but_shows_unassigned_peers() {
        assert!(!voxel_player_standee_visible_for_access(42, 42, false, None, None,));
        assert!(voxel_player_standee_visible_for_access(
            42, 43, false, None, None,
        ));
        assert!(
            !voxel_player_standee_visible_for_access(
                42,
                43,
                false,
                Some("party-a"),
                Some("party-b"),
            )
        );
    }

    #[test]
    fn initializes_populated_trpg_grid() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        assert!(grid.count() > 225);
    }

    #[test]
    fn radiance_volume_is_a_canonical_camera_focus_clipmap() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        let focus = DEFAULT_SCENE_CAMERA_FOCUS;
        let (image, volume_min, voxel_world_size, volume_dimensions) =
            build_voxel_radiance_image(grid, focus);

        assert_eq!(
            image.texture_descriptor.dimension,
            TextureDimension::D3
        );
        assert_eq!(
            volume_dimensions,
            Vec3::splat(VOXEL_RADIANCE_VOLUME_DIMENSION as f32)
        );
        assert_eq!(voxel_world_size, VOXEL_SIZE);
        assert_eq!(
            volume_min,
            voxel_radiance_volume_origin(focus).as_vec3() * VOXEL_SIZE
        );
        assert_eq!(
            image.texture_descriptor.size.width,
            volume_dimensions.x as u32
        );
        assert_eq!(
            image.texture_descriptor.size.height,
            volume_dimensions.y as u32
        );
        assert_eq!(
            image.texture_descriptor.size.depth_or_array_layers,
            volume_dimensions.z as u32
        );
        let data = image.data.as_ref().expect("radiance volume has CPU texels");
        assert!(data.chunks_exact(4).any(|rgba| rgba[3] != 0));
    }

    #[test]
    fn radiance_clipmap_origin_moves_in_stable_canonical_steps() {
        let origin = voxel_radiance_volume_origin(Vec3::ZERO);
        assert_eq!(
            voxel_radiance_volume_origin(Vec3::splat(
                VOXEL_SIZE * (VOXEL_RADIANCE_REBUILD_STEP as f32 - 0.01),
            )),
            origin
        );
        assert_eq!(
            voxel_radiance_volume_origin(Vec3::X * VOXEL_SIZE * VOXEL_RADIANCE_REBUILD_STEP as f32),
            origin + IVec3::X * VOXEL_RADIANCE_REBUILD_STEP
        );
    }

    #[test]
    fn radiance_volume_flood_fills_emission_but_not_solid_cells() {
        let mut app = App::new();
        let entity = app.world_mut().spawn(Grid::<u8>::new()).id();
        {
            let mut entity_mut = app.world_mut().entity_mut(entity);
            let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
            grid.set(IVec3::ZERO, 5);
            grid.set(IVec3::X, 2);
            grid.set(IVec3::new(8, 0, 0), 2);
        }
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        let (image, volume_min, voxel_world_size, _) = build_voxel_radiance_image(grid, Vec3::ZERO);
        let origin = (volume_min / voxel_world_size).as_ivec3();
        let emitter_index = voxel_radiance_index(-origin) * 4;
        let empty_neighbor_index = voxel_radiance_index(IVec3::NEG_X - origin) * 4;
        let solid_neighbor_index = voxel_radiance_index(IVec3::X - origin) * 4;
        let roof_above_index = voxel_radiance_index(IVec3::new(8, 1, 0) - origin) * 4;
        let roof_below_index = voxel_radiance_index(IVec3::new(8, -1, 0) - origin) * 4;
        let data = image.data.as_ref().expect("radiance volume has CPU texels");

        assert_eq!(
            &data[emitter_index..emitter_index + 4],
            &[255, 72, 8, 255]
        );
        assert!(data[empty_neighbor_index] > VOXEL_RADIANCE_SKYLIGHT[0]);
        assert_eq!(
            &data[solid_neighbor_index..solid_neighbor_index + 4],
            &[0, 0, 0, 255]
        );
        assert_eq!(
            data[roof_above_index + 2],
            VOXEL_RADIANCE_SKYLIGHT[2]
        );
        assert!(data[roof_below_index + 2] < VOXEL_RADIANCE_SKYLIGHT[2]);
    }

    #[test]
    fn radiance_palette_separates_occupancy_from_emission() {
        assert_eq!(radiance_voxel_color(2), [0, 0, 0, 255]);
        assert_eq!(radiance_voxel_color(5), [
            255, 72, 8, 255
        ]);
        assert_eq!(radiance_voxel_color(8), [
            34, 176, 220, 255
        ]);
        assert_eq!(radiance_voxel_color(0), [0, 0, 0, 0]);
    }

    #[test]
    fn radiance_inspection_preset_disables_direct_lighting() {
        let mut editor = VoxelEditorState::default();
        editor.inspect_radiance_lighting();
        assert_eq!(editor.ambient_brightness, 0.0);
        assert_eq!(editor.key_light_illuminance, 0.0);
        assert_eq!(editor.fill_light_illuminance, 0.0);
        assert_eq!(editor.radiance_intensity, 1.2);

        editor.reset_lighting();
        assert_eq!(
            editor.ambient_brightness,
            DEFAULT_AMBIENT_BRIGHTNESS
        );
        assert_eq!(
            editor.key_light_illuminance,
            DEFAULT_KEY_LIGHT_ILLUMINANCE
        );
        assert_eq!(
            editor.fill_light_illuminance,
            DEFAULT_FILL_LIGHT_ILLUMINANCE
        );
        assert_eq!(
            editor.radiance_intensity,
            DEFAULT_RADIANCE_INTENSITY
        );
    }

    #[test]
    fn lighting_editor_values_sync_to_scene_components() {
        let mut app = App::new();
        let mut editor = VoxelEditorState::default();
        editor.ambient_brightness = 12.0;
        editor.key_light_illuminance = 3_000.0;
        editor.key_light_color = [0.9, 0.7, 0.5];
        editor.fill_light_illuminance = 900.0;
        editor.fill_light_color = [0.2, 0.4, 0.8];
        editor.radiance_intensity = 0.8;
        app.insert_resource(editor)
            .insert_resource(GlobalAmbientLight::default())
            .add_systems(Update, sync_voxel_lighting);
        let key = app
            .world_mut()
            .spawn((
                DirectionalLight::default(),
                VoxelKeyLight,
            ))
            .id();
        let fill = app
            .world_mut()
            .spawn((
                DirectionalLight::default(),
                VoxelFillLight,
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                VoxelViewportCamera,
                VoxelRadianceCascadeUniform {
                    volume_min: Vec3::ZERO,
                    voxel_world_size: VOXEL_SIZE,
                    volume_dimensions: Vec3::ONE,
                    intensity: 0.0,
                },
            ))
            .id();

        app.update();

        assert_eq!(
            app.world().resource::<GlobalAmbientLight>().brightness,
            12.0
        );
        assert_eq!(
            app.world()
                .entity(key)
                .get::<DirectionalLight>()
                .unwrap()
                .illuminance,
            3_000.0
        );
        assert_eq!(
            app.world()
                .entity(fill)
                .get::<DirectionalLight>()
                .unwrap()
                .illuminance,
            900.0
        );
        assert_eq!(
            app.world()
                .entity(key)
                .get::<DirectionalLight>()
                .unwrap()
                .color,
            Color::srgb(0.9, 0.7, 0.5)
        );
        assert_eq!(
            app.world()
                .entity(fill)
                .get::<DirectionalLight>()
                .unwrap()
                .color,
            Color::srgb(0.2, 0.4, 0.8)
        );
        assert_eq!(
            app.world()
                .entity(camera)
                .get::<VoxelRadianceCascadeUniform>()
                .unwrap()
                .intensity,
            0.8
        );
    }

    #[test]
    fn default_space_map_contains_each_workbook_floorplan() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();

        for (center, design) in static_workbook_orbital_locations() {
            let spawn = center + IVec3::new(design.spawn[0], 0, design.spawn[1]);
            assert!(
                matches!(grid.get(spawn).copied(), Some(2 | 7)),
                "{} spawn must be on its workbook floor",
                design.name
            );
            assert_eq!(
                grid.get(spawn + IVec3::Y).copied().unwrap_or(0),
                0,
                "{} spawn must have standing room",
                design.name
            );
            let decoded = design.decode();
            let wall_index = decoded
                .styles
                .iter()
                .enumerate()
                .find_map(|(index, style)| {
                    if *style != 11 {
                        return None;
                    }
                    let [wall_x, wall_z] = design.centered_offset(index);
                    (grid.get(center + IVec3::new(wall_x, 1, wall_z)).copied() == Some(6))
                        .then_some(index)
                })
                .unwrap();
            let [wall_x, wall_z] = design.centered_offset(wall_index);
            assert_eq!(
                grid.get(center + IVec3::new(wall_x, 1, wall_z)).copied(),
                Some(6),
                "{} wall must use canonical voxels",
                design.name
            );
        }
    }

    #[test]
    fn legacy_static_arrogance_is_removed_and_only_the_enlarged_carrier_is_controllable() {
        let original = original_combat_spaceship_voxel_cells();
        let mut world = World::new();
        let grid_entity = world.spawn(Grid::<u8>::new()).id();
        {
            let mut entity = world.entity_mut(grid_entity);
            let mut grid = entity.get_mut::<Grid<u8>>().unwrap();
            for (cell, material) in &original {
                grid.set(COMBAT_SPACESHIP_CENTER + *cell, *material);
            }
            assert!(original.iter().all(|(cell, material)| {
                grid.get(COMBAT_SPACESHIP_CENTER + *cell).copied() == Some(*material)
            }));
            remove_static_combat_spaceship(&mut grid);
        }
        let grid = world.entity(grid_entity).get::<Grid<u8>>().unwrap();
        assert!(original.iter().all(|(cell, _)| {
            !grid
                .get(COMBAT_SPACESHIP_CENTER + *cell)
                .is_some_and(TrpgVoxelConnector::solid)
        }));

        let specs = default_voxel_spaceship_specs();
        let carriers = specs
            .iter()
            .filter(|spec| spec.ship.id == COMBAT_SPACESHIP_ID)
            .collect::<Vec<_>>();
        assert_eq!(carriers.len(), 1);
        assert_eq!(carriers[0].ship.name, "U.S.I 狂妄号");
        assert!(carriers[0].docking.is_none());
        assert_eq!(carriers[0].cells, combat_spaceship_voxel_cells());
        assert!(carriers[0].cells.len() > original.len());
    }


    #[test]
    fn space_station_docks_are_separated_open_and_fit_medium_ships() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        let medium_ship = medium_spaceship_voxel_cells()
            .into_iter()
            .filter_map(|(cell, material)| TrpgVoxelConnector::solid(&material).then_some(cell))
            .collect::<Vec<_>>();

        for (station_center, design) in static_workbook_orbital_locations() {
            let ports = space_station_docking_ports(design);
            assert_eq!(
                ports.len(),
                3,
                "{} docking port count",
                design.name
            );
            for (index, port) in ports.iter().enumerate() {
                for other in &ports[index + 1..] {
                    assert!(
                        (port.pad_center - other.pad_center).length_squared()
                            >= STATION_DOCK_MIN_SEPARATION.pow(2),
                        "{} docking pads must not be clustered",
                        design.name
                    );
                    assert_ne!(
                        port.edge, other.edge,
                        "{} docking entries must face different directions",
                        design.name
                    );
                }

                let direction = port.edge.direction();
                let tangent = port.edge.tangent();
                let parked_origin = station_center + port.pad_center + IVec3::Y;

                for cell in &medium_ship {
                    let parked =
                        parked_origin + tangent * cell.x + IVec3::Y * cell.y + direction * cell.z;
                    assert!(
                        !grid.get(parked).is_some_and(TrpgVoxelConnector::solid),
                        "{} medium ship must fit on its docking pad at {:?}",
                        design.name,
                        port.pad_center
                    );
                    if cell.y == 0 {
                        assert!(
                            grid.get(parked - IVec3::Y)
                                .is_some_and(TrpgVoxelConnector::solid),
                            "{} docking pad must support the medium ship",
                            design.name
                        );
                    }
                    for launch_step in 1..=STATION_DOCK_PAD_HALF_LENGTH * 2 {
                        assert!(
                            !grid
                                .get(parked + direction * launch_step)
                                .is_some_and(TrpgVoxelConnector::solid),
                            "{} must have a clear outward launch lane",
                            design.name
                        );
                    }
                }

                for lateral in -STATION_DOCK_BRIDGE_HALF_WIDTH..=STATION_DOCK_BRIDGE_HALF_WIDTH {
                    let entry = station_center + port.entrance + tangent * lateral;
                    assert!(
                        grid.get(entry).is_some_and(TrpgVoxelConnector::solid),
                        "{} docking entry needs a walkable floor",
                        design.name
                    );
                    for y in 1..=WORKBOOK_ROOM_HEIGHT - 2 {
                        assert_eq!(
                            grid.get(entry + IVec3::Y * y).copied().unwrap_or(0),
                            0,
                            "{} docking entry must remain open",
                            design.name
                        );
                    }
                }
            }
        }
    }


    #[test]
    fn workbook_micro_tiles_are_sixteenth_scale_and_owned_by_canonical_cells() {
        assert_eq!(MICRO_TILE_SUBDIVISIONS, 16);
        assert_eq!(VOXEL_SIZE / MICRO_TILE_SUBDIVISIONS as f32, 0.015625);

        for design in [ARROGANCE, NIFFY, KYO, ARBITRATOR, ABANDONED] {
            let decoded = design.decode();
            let tiles = workbook_micro_tiles(design, &decoded);
            assert!(tiles.iter().any(|tile| tile.kind == VoxelMicroTileKind::Hull));
            assert!(tiles.iter().any(|tile| tile.kind == VoxelMicroTileKind::Fixture));

            let mut world = World::new();
            let entity = world.spawn(Grid::<u8>::new()).id();
            {
                let mut entity_mut = world.entity_mut(entity);
                let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
                build_workbook_orbital_location(&mut grid, IVec3::ZERO, design);
            }
            let grid = world.entity(entity).get::<Grid<u8>>().unwrap();
            for tile in tiles {
                assert!(tile.min.cmplt(tile.max).all(), "{} bounds", design.name);
                assert!(
                    tile.max.cmple(UVec3::splat(MICRO_TILE_SUBDIVISIONS)).all(),
                    "{} subdivision bounds",
                    design.name
                );
                assert_ne!(
                    grid.get(tile.owner).copied().unwrap_or(0),
                    0,
                    "{} owner {:?}",
                    design.name,
                    tile.owner
                );
                if tile.kind == VoxelMicroTileKind::Hull {
                    assert!(
                        (tile.cell - tile.owner).abs().max_element() <= 1,
                        "{} hull detail must stay on its authored contour",
                        design.name
                    );
                }
            }
        }
    }

    #[test]
    fn workbook_semantic_cells_are_micro_fixtures_instead_of_solid_columns() {
        for design in [ARROGANCE, NIFFY, KYO, ARBITRATOR, ABANDONED] {
            let decoded = design.decode();
            let decorated_owners = workbook_micro_tiles(design, &decoded)
                .into_iter()
                .filter(|tile| tile.kind == VoxelMicroTileKind::Fixture)
                .map(|tile| tile.owner)
                .collect::<HashSet<_>>();
            let mut world = World::new();
            let entity = world.spawn(Grid::<u8>::new()).id();
            {
                let mut entity_mut = world.entity_mut(entity);
                let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
                build_workbook_orbital_location(&mut grid, IVec3::ZERO, design);
            }
            let grid = world.entity(entity).get::<Grid<u8>>().unwrap();
            for (index, feature) in decoded.features.iter().copied().enumerate() {
                if feature == 0 || !decoded.enclosed[index] {
                    continue;
                }
                let [x, z] = design.centered_offset(index);
                let owner = IVec3::new(x, 0, z);
                assert_ne!(grid.get(owner).copied().unwrap_or(0), 0);
                assert!(
                    decorated_owners.contains(&owner),
                    "{} feature {} lost its workbook floor marker",
                    design.name,
                    feature
                );
                assert_eq!(
                    grid.get(owner + IVec3::Y).copied().unwrap_or(0),
                    0,
                    "{} feature {} remained a full voxel monolith",
                    design.name,
                    feature
                );
            }
        }
    }

    #[test]
    fn medical_analyzer_and_every_other_workbook_label_have_distinct_semantics() {
        let decoded = NIFFY.decode();
        assert_eq!(
            decoded.features.iter().filter(|feature| **feature == 10).count(),
            36
        );
        let labels = (1..=15)
            .map(|id| WorkbookFeatureKind::from_id(id).unwrap().label())
            .collect::<HashSet<_>>();
        assert_eq!(labels.len(), 15);
        assert!(labels.contains("验血 / GIT"));
        assert!(labels.contains("能量台"));
        assert!(labels.contains("前哨站"));
        assert!(labels.contains("补给品"));
    }

    #[test]
    fn workbook_feature_regions_cover_each_labeled_cell_once_and_use_real_anchors() {
        for design in [ARROGANCE, NIFFY, KYO, ARBITRATOR, ABANDONED] {
            let decoded = design.decode();
            let regions = workbook_feature_regions(design, &decoded);
            let expected_count = decoded
                .features
                .iter()
                .zip(&decoded.enclosed)
                .filter(|(feature, enclosed)| **feature != 0 && **enclosed)
                .count();
            let region_cells = regions
                .iter()
                .flat_map(|region| region.cells.iter().copied())
                .collect::<Vec<_>>();
            assert_eq!(region_cells.len(), expected_count, "{}", design.name);
            assert_eq!(
                region_cells.iter().copied().collect::<HashSet<_>>().len(),
                expected_count,
                "{} feature regions overlap",
                design.name
            );
            assert!(regions.iter().all(|region| {
                !region.cells.is_empty()
                    && region.cells.contains(&region.anchor)
                    && !region.kind.label().is_empty()
            }));
        }
    }

    #[test]
    fn hover_raycast_follows_canonical_feature_cells_and_moving_ship_transform() {
        let decoded = NIFFY.decode();
        let region = workbook_feature_regions(NIFFY, &decoded)
            .into_iter()
            .find(|region| region.kind == WorkbookFeatureKind::MedicalAnalyzer)
            .expect("Niffy must retain its 验血 / GIT region");
        let local_origin = (region.anchor.as_vec3() + Vec3::new(0.5, 20.0, 0.5)) * VOXEL_SIZE;
        let identity_hit = raycast_workbook_feature_region(
            Ray3d::new(local_origin, Dir3::NEG_Y),
            Affine3A::IDENTITY,
            &region,
        );
        let identity_hit = identity_hit.unwrap();

        let translation = Vec3::new(37.0, 4.0, -19.0);
        let moving_transform = Affine3A::from_translation(translation);
        let moving_hit = raycast_workbook_feature_region(
            Ray3d::new(local_origin + translation, Dir3::NEG_Y),
            moving_transform,
            &region,
        );
        assert!((identity_hit - moving_hit.unwrap()).abs() < 0.0001);
        assert!(raycast_workbook_feature_region(
            Ray3d::new(local_origin + Vec3::X * 100.0, Dir3::NEG_Y),
            Affine3A::IDENTITY,
            &region,
        )
        .is_none());
    }

    #[test]
    fn workbook_world_labels_use_an_inclusive_twenty_meter_radius() {
        let camera = Vec3::new(4.0, -2.0, 7.0);
        assert_eq!(WORKBOOK_FEATURE_LABEL_VISIBILITY_RADIUS_METERS, 20.0);
        assert!(workbook_feature_label_in_range(
            camera,
            camera + Vec3::X * 20.0,
        ));
        assert!(workbook_feature_label_in_range(
            camera,
            camera + Vec3::new(12.0, 0.0, 16.0),
        ));
        assert!(!workbook_feature_label_in_range(
            camera,
            camera + Vec3::X * 20.001,
        ));
        assert!(!workbook_feature_label_in_range(camera, Vec3::NAN));
    }

    #[test]
    fn one_micro_tile_mesh_occupies_exactly_one_sixteenth_voxel() {
        let meshes = build_micro_tile_meshes(&[VoxelMicroTile {
            owner: IVec3::ZERO,
            cell: IVec3::ZERO,
            min: UVec3::ZERO,
            max: UVec3::ONE,
            material: 1,
            kind: VoxelMicroTileKind::Fixture,
        }]);
        let VertexAttributeValues::Float32x3(positions) = meshes[0]
            .1
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
        else {
            panic!("micro tile positions must be Float32x3");
        };
        let extent = positions
            .iter()
            .flat_map(|position| position.iter().copied())
            .fold(0.0_f32, f32::max);
        assert_eq!(extent, VOXEL_SIZE / MICRO_TILE_SUBDIVISIONS as f32);
    }

    #[test]
    fn orbital_layout_is_five_times_wider_and_clear_of_the_planet() {
        assert_eq!(ORBITAL_LAYOUT_SCALE, 5);
        assert_eq!(RESEARCH_STATION_CENTER, IVec3::new(500, 0, 500));
        assert_eq!(SENSOR_STATION_CENTER, IVec3::new(500, 0, -500));
        assert_eq!(CANNON_STATION_CENTER, IVec3::new(-500, 0, -500));
        assert_eq!(COMBAT_SPACESHIP_CENTER, IVec3::new(-500, 0, 500));
        assert_eq!(ABANDONED_STATION_CENTER, IVec3::new(0, 0, 1_000));

        let planet_top = ORBITAL_PLANET_CENTER.y + ORBITAL_PLANET_RADIUS;
        assert!(planet_top <= -100.0);

        let planet_cells = voxel_orbital_planet_cells();
        assert!((80_000..=250_000).contains(&planet_cells.len()));
        assert_eq!(
            planet_cells
                .iter()
                .map(|(_, material)| *material)
                .collect::<HashSet<_>>(),
            HashSet::from([1, 2, 3, 4, 6, 7, 9, 10])
        );
        let lab_cells = xy_planet_map_cells()
            .into_iter()
            .map(|(cell, _)| cell)
            .collect::<HashSet<_>>();
        assert!(
            planet_cells.iter().all(|(cell, _)| lab_cells.contains(cell)
                || cell.abs().max_element() <= ORBITAL_PLANET_VOXEL_RADIUS)
        );
        assert!(planet_cells.iter().all(|(cell, _)| {
            cell.y >= 0 && cell.x * cell.x + cell.z * cell.z <= ORBITAL_PLANET_CAP_RADIUS.pow(2)
        }));
        assert_eq!(VOXEL_SIZE, 0.25);
        assert_eq!(ORBITAL_PLANET_RADIUS, 12.125 * 10.0);
        assert_eq!(ORBITAL_PLANET_CAP_RADIUS, 128);
        assert!(procedural_planet_material(IVec3::new(0, -1, 0)).is_none());
        assert!(procedural_planet_material(IVec3::new(129, 470, 0)).is_none());
        assert_eq!(PLANET_CLOUD_PUFF_COUNT, 24);
        assert!(PLANET_CLOUD_ALTITUDE > 0.0);
    }

    #[test]
    fn planet_gravity_is_radial_and_ends_before_orbital_installations() {
        let gravity_edge = ORBITAL_PLANET_CENTER
            + Vec3::Y * (ORBITAL_PLANET_RADIUS + ORBITAL_PLANET_GRAVITY_MAX_ALTITUDE);
        let edge_acceleration =
            voxel_planet_gravity_acceleration(gravity_edge, ORBITAL_PLANET_CENTER);
        assert!(edge_acceleration.abs_diff_eq(
            Vec3::NEG_Y * ORBITAL_PLANET_GRAVITY_ACCELERATION,
            0.0001,
        ));
        assert_eq!(
            voxel_planet_gravity_acceleration(
                gravity_edge + Vec3::Y * 0.001,
                ORBITAL_PLANET_CENTER,
            ),
            Vec3::ZERO,
        );

        let science_lab = VoxelTeleportDestination::PlanetScienceLab
            .player_position()
            .unwrap();
        let lab_acceleration =
            voxel_planet_gravity_acceleration(science_lab, ORBITAL_PLANET_CENTER);
        assert!(
            (lab_acceleration.length() - ORBITAL_PLANET_GRAVITY_ACCELERATION).abs() < 0.0001
        );
        assert!(lab_acceleration.dot(ORBITAL_PLANET_CENTER - science_lab) > 0.0);

        for orbital_center in [
            RESEARCH_STATION_CENTER,
            SENSOR_STATION_CENTER,
            CANNON_STATION_CENTER,
            COMBAT_SPACESHIP_CENTER,
            ABANDONED_STATION_CENTER,
        ] {
            assert_eq!(
                voxel_planet_gravity_acceleration(
                    orbital_center.as_vec3() * VOXEL_SIZE,
                    ORBITAL_PLANET_CENTER,
                ),
                Vec3::ZERO,
                "orbital installation at {orbital_center:?} entered the planet gravity field",
            );
        }
    }

    #[test]
    fn xy_planet_base_is_canonical_editable_workbook_geometry() {
        let lab_cells = xy_planet_map_cells()
            .into_iter()
            .collect::<HashMap<_, _>>();
        let planet_cells = voxel_orbital_planet_cells()
            .into_iter()
            .collect::<HashMap<_, _>>();
        let center_x = PLANET_SCIENCE_LAB_CENTER.x;
        let center_z = PLANET_SCIENCE_LAB_CENTER.y;
        let floor_y = PLANET_SCIENCE_LAB_FLOOR_Y;
        let decoded = XY_PLANET.decode();
        let wall_index = decoded
            .styles
            .iter()
            .position(|style| *style == 11)
            .unwrap();
        let door_index = decoded
            .styles
            .iter()
            .position(|style| *style == 15)
            .unwrap();
        let [wall_x, wall_z] = XY_PLANET.centered_offset(wall_index);
        let [door_x, door_z] = XY_PLANET.centered_offset(door_index);
        let wall = IVec3::new(center_x + wall_x, floor_y + 1, center_z + wall_z);
        let door = IVec3::new(center_x + door_x, floor_y + 1, center_z + door_z);
        let spawn = IVec3::new(
            center_x + XY_PLANET.spawn[0],
            floor_y,
            center_z + XY_PLANET.spawn[1],
        );

        assert!(lab_cells.len() > 1_000);
        assert!(lab_cells
            .iter()
            .all(|(cell, material)| planet_cells.get(cell) == Some(material)));
        assert!(matches!(lab_cells.get(&spawn), Some(2 | 7)));
        assert_eq!(lab_cells.get(&wall), Some(&6));
        assert!(!lab_cells.contains_key(&door));

        let cell_bounds = VoxelCellBounds::from_cells(lab_cells.keys().copied());
        let mut planet = VoxelOrbitalPlanet {
            cells: lab_cells,
            cell_bounds,
            removed: HashSet::new(),
            collider_entity: Entity::PLACEHOLDER,
            mesh_entities: Vec::new(),
            mesh_handles: Vec::new(),
            voxel_size: VOXEL_SIZE,
            dirty: false,
        };
        let removed = explode_planet_voxels(
            &mut planet,
            wall.as_vec3() * VOXEL_SIZE,
            VOXEL_SIZE,
        );
        assert!(removed
            .iter()
            .any(|(cell, material)| *cell == wall && *material == 6));
        assert!(planet.removed.contains(&wall));
        assert!(!planet.cells.contains_key(&wall));
        assert!(planet.dirty);
    }

    #[test]
    fn digging_planet_voxels_generates_buried_neighbors_without_refilling_holes() {
        let surface = IVec3::new(
            0,
            (ORBITAL_PLANET_RADIUS / VOXEL_SIZE) as i32,
            0,
        );
        let mut planet = VoxelOrbitalPlanet {
            cells: HashMap::from([(surface, 3)]),
            cell_bounds: Some(VoxelCellBounds::from_cell(surface)),
            removed: HashSet::new(),
            collider_entity: Entity::PLACEHOLDER,
            mesh_entities: Vec::new(),
            mesh_handles: Vec::new(),
            voxel_size: 1.0,
            dirty: false,
        };

        assert!(dig_planet_voxel(&mut planet, surface));
        assert!(planet.dirty);
        assert!(planet.removed.contains(&surface));
        assert!(!planet.cells.contains_key(&surface));
        assert!(planet.cells.contains_key(&(surface + IVec3::new(0, -1, 0))));

        let next = surface + IVec3::new(0, -1, 0);
        assert!(dig_planet_voxel(&mut planet, next));
        assert!(!planet.cells.contains_key(&surface));
        assert!(planet.cells.contains_key(&(surface + IVec3::new(0, -2, 0))));
    }

    #[test]
    fn planet_raycast_uses_centered_voxel_cells_in_planet_space() {
        let planet = VoxelOrbitalPlanet {
            cells: HashMap::from([(IVec3::ZERO, 2)]),
            cell_bounds: Some(VoxelCellBounds::from_cell(IVec3::ZERO)),
            removed: HashSet::new(),
            collider_entity: Entity::PLACEHOLDER,
            mesh_entities: Vec::new(),
            mesh_handles: Vec::new(),
            voxel_size: 2.0,
            dirty: false,
        };
        let transform = GlobalTransform::from(Transform::from_xyz(0.0, -10.0, 0.0));
        let ray = Ray3d::new(Vec3::ZERO, Dir3::NEG_Y);

        let hit = raycast_voxel_planet(&planet, &transform, ray).unwrap();
        assert_eq!(hit.occupied, IVec3::ZERO);
        assert!((8.0..=10.0).contains(&hit.distance));
    }

    #[test]
    fn planet_raycast_tracks_voxels_built_above_original_surface() {
        let surface = IVec3::new(20, ORBITAL_PLANET_VOXEL_RADIUS - 4, 0);
        let mut planet = VoxelOrbitalPlanet {
            cells: HashMap::from([(surface, 3)]),
            cell_bounds: Some(VoxelCellBounds::from_cell(surface)),
            removed: HashSet::new(),
            collider_entity: Entity::PLACEHOLDER,
            mesh_entities: Vec::new(),
            mesh_handles: Vec::new(),
            voxel_size: VOXEL_SIZE,
            dirty: false,
        };
        let built_top = IVec3::new(
            surface.x,
            ORBITAL_PLANET_VOXEL_RADIUS + 12,
            surface.z,
        );
        for y in surface.y + 1..=built_top.y {
            assert!(set_planet_voxel(
                &mut planet,
                IVec3::new(surface.x, y, surface.z),
                1,
            ));
        }

        let (_, local_max) = planet.cell_bounds.unwrap().local_aabb(VOXEL_SIZE);
        assert!(local_max.y > ORBITAL_PLANET_RADIUS);
        let ray = Ray3d::new(
            built_top.as_vec3() * VOXEL_SIZE + Vec3::Z * 2.0,
            Dir3::NEG_Z,
        );
        let hit = raycast_voxel_planet(&planet, &GlobalTransform::IDENTITY, ray)
            .expect("player-built planet voxels above the original surface should remain aimable");

        assert_eq!(hit.occupied, built_top);
    }

    #[test]
    fn planet_explosion_extracts_canonical_voxels_and_reveals_buried_cells() {
        let surface = IVec3::new(0, ORBITAL_PLANET_VOXEL_RADIUS, 0);
        let mut planet = VoxelOrbitalPlanet {
            cells: HashMap::from([(surface, 3)]),
            cell_bounds: Some(VoxelCellBounds::from_cell(surface)),
            removed: HashSet::new(),
            collider_entity: Entity::PLACEHOLDER,
            mesh_entities: Vec::new(),
            mesh_handles: Vec::new(),
            voxel_size: VOXEL_SIZE,
            dirty: false,
        };

        let removed = explode_planet_voxels(
            &mut planet,
            surface.as_vec3() * VOXEL_SIZE,
            VOXEL_SIZE,
        );

        assert_eq!(removed, vec![(surface, 3)]);
        assert_eq!(planet.voxel_size, VOXEL_SIZE);
        assert_eq!(planet.removed.len(), removed.len());
        assert!(planet.cells.contains_key(&(surface - IVec3::Y)));
        assert!(planet.dirty);
    }

    #[test]
    fn arrogance_cruiser_preserves_workbook_walls_doors_and_semantics() {
        let decoded = ARROGANCE.decode();
        assert_eq!((ARROGANCE.width, ARROGANCE.height), (216, 84));
        assert_eq!(
            decoded.styles.iter().filter(|style| **style == 11).count(),
            1_033
        );
        assert_eq!(
            decoded.styles.iter().filter(|style| **style == 15).count(),
            42
        );
        assert_eq!(
            decoded.features.iter().filter(|feature| **feature != 0).count(),
            464
        );
        assert!((1..=15).all(|feature| {
            WorkbookFeatureKind::from_id(feature).is_some()
        }));
    }

    #[test]
    fn first_person_character_is_two_voxels_tall_and_one_voxel_wide() {
        let total_height = FIRST_PERSON_BODY_LENGTH + FIRST_PERSON_RADIUS * 2.0;
        assert!((total_height - VOXEL_SIZE * 2.0).abs() < f32::EPSILON);
        assert!((FIRST_PERSON_RADIUS * 2.0 - VOXEL_SIZE).abs() < f32::EPSILON);
        assert!((FIRST_PERSON_EYE_OFFSET - VOXEL_SIZE).abs() < f32::EPSILON);
    }

    #[test]
    fn canonical_voxel_colliders_keep_quarter_scale_voxel_geometry() {
        let collider = canonical_voxel_collider(&[IVec3::ZERO, IVec3::X]);
        let voxels = collider
            .shape()
            .as_voxels()
            .expect("canonical voxel bodies must use a real voxel collider");

        assert_eq!(
            voxels.voxel_size(),
            Vec3::splat(VOXEL_SIZE)
        );
    }

    #[test]
    fn trpg_physics_uses_one_snapshot_oriented_substep() {
        assert_eq!(TRPG_PHYSICS_SUBSTEPS, 1);
        assert!(TRPG_PHYSICS_SUBSTEPS < SubstepCount::default().0);
    }

    #[test]
    fn voxel_emissive_output_is_reduced_to_thirty_percent() {
        let emissive = voxel_emissive(5.0, 2.0, 1.0);
        assert!((emissive.red - 1.5).abs() < f32::EPSILON);
        assert!((emissive.green - 0.6).abs() < f32::EPSILON);
        assert!((emissive.blue - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn replay_fade_splits_only_touched_voxels_into_a_transparent_mesh() {
        let mut app = App::new();
        app.init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Mesh>>();
        let (normal_handles, normal_planet_ocean) = {
            let mut assets = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            let handles = std::array::from_fn(|_| assets.add(StandardMaterial::default()));
            let planet_ocean = assets.add(StandardMaterial::default());
            (handles, planet_ocean)
        };
        let (fade_handles, fade_planet_ocean) = {
            let mut assets = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            let handles = std::array::from_fn(|_| {
                assets.add(StandardMaterial {
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })
            });
            let planet_ocean = assets.add(StandardMaterial {
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
            (handles, planet_ocean)
        };
        app.insert_resource(VoxelMaterials {
            handles: normal_handles.clone(),
            planet_ocean: normal_planet_ocean,
        })
        .insert_resource(VoxelReplayFadeMaterials {
            handles: fade_handles.clone(),
            planet_ocean: fade_planet_ocean.clone(),
        })
        .insert_resource(VoxelReplayOcclusionFade {
            active: false,
            camera: Vec3::new(VOXEL_SIZE * 0.5, VOXEL_SIZE * 0.5, -1.0),
            targets: vec![Vec3::new(VOXEL_SIZE * 0.5, VOXEL_SIZE * 0.5, 1.0)],
            opacity: 0.35,
            cast_width_cells: 1.0,
            cast_height_cells: 1.0,
            cast_end_width_cells: 1.0,
            cast_end_height_cells: 1.0,
            debug_gizmo: false,
        })
        .add_systems(Update, sync_voxel_occlusion_fade);
        let source_mesh = {
            let (mut material_meshes, _) =
                build_voxel_meshes_from_cells(&[(IVec3::ZERO, 1)]);
            app.world_mut()
                .resource_mut::<Assets<Mesh>>()
                .add(material_meshes.remove(0).1)
        };
        let voxel = app
            .world_mut()
            .spawn((
                Mesh3d(source_mesh.clone()),
                MeshMaterial3d(normal_handles[0].clone()),
                Aabb::from_min_max(Vec3::splat(-0.25), Vec3::splat(0.25)),
                Transform::IDENTITY,
                GlobalTransform::IDENTITY,
            ))
            .id();
        let off_axis_voxel = app
            .world_mut()
            .spawn((
                Mesh3d(source_mesh.clone()),
                MeshMaterial3d(normal_handles[1].clone()),
                Aabb::from_min_max(Vec3::splat(-0.25), Vec3::splat(0.25)),
                Transform::from_xyz(2.0, 0.0, 0.0),
                GlobalTransform::from_translation(Vec3::new(2.0, 0.0, 0.0)),
            ))
            .id();

        app.update();
        assert!(!app.world().entity(voxel).contains::<VoxelOcclusionMesh>());

        app.world_mut()
            .resource_mut::<VoxelReplayOcclusionFade>()
            .active = true;
        app.update();
        let transparent_entity = app
            .world()
            .entity(voxel)
            .get::<VoxelOcclusionMesh>()
            .unwrap()
            .transparent_entity;
        assert_eq!(
            app.world()
                .entity(transparent_entity)
                .get::<MeshMaterial3d<StandardMaterial>>()
                .unwrap()
                .0,
            fade_handles[0]
        );
        assert!(!app
            .world()
            .entity(off_axis_voxel)
            .contains::<VoxelOcclusionMesh>());
        let assets = app.world().resource::<Assets<StandardMaterial>>();
        assert_eq!(assets.get(&fade_handles[0]).unwrap().base_color.alpha(), 0.35);
        assert!(matches!(
            assets.get(&fade_handles[0]).unwrap().alpha_mode,
            AlphaMode::Blend
        ));

        app.world_mut()
            .resource_mut::<VoxelReplayOcclusionFade>()
            .opacity = 1.0;
        app.update();
        assert!(!app.world().entity(voxel).contains::<VoxelOcclusionMesh>());
        assert_eq!(app.world().entity(voxel).get::<Mesh3d>().unwrap().0, source_mesh);
        assert!(app.world().get_entity(transparent_entity).is_err());
    }

    #[test]
    fn replay_cube_cast_selects_blockers_only_between_camera_and_player() {
        let bounds = Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0));
        let blocking_transform = GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 5.0));
        let off_axis_transform = GlobalTransform::from_translation(Vec3::new(4.0, 0.0, 5.0));
        let behind_player_transform = GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 11.1));
        let touched_voxel_half_extent = VOXEL_SIZE * 1.5;

        assert!(replay_sightline_intersects_aabb(
            Vec3::ZERO,
            Vec3::Z * 10.0,
            touched_voxel_half_extent,
            &bounds,
            &blocking_transform,
        ));
        assert!(!replay_sightline_intersects_aabb(
            Vec3::ZERO,
            Vec3::Z * 10.0,
            touched_voxel_half_extent,
            &bounds,
            &off_axis_transform,
        ));
        assert!(!replay_sightline_intersects_aabb(
            Vec3::ZERO,
            Vec3::Z * 10.0,
            touched_voxel_half_extent,
            &bounds,
            &behind_player_transform,
        ));
    }

    #[test]
    fn replay_box_cast_tapers_to_independent_target_width_and_height() {
        let fade = VoxelReplayOcclusionFade {
            active: true,
            camera: Vec3::ZERO,
            targets: vec![Vec3::Z * 10.0],
            opacity: 0.5,
            cast_width_cells: 10.0,
            cast_height_cells: 10.0,
            cast_end_width_cells: 1.0,
            cast_end_height_cells: 2.0,
            debug_gizmo: true,
        };
        let transform = GlobalTransform::IDENTITY;

        assert_eq!(
            voxel_occlusion_cast_size_at(&fade, 0.0),
            Vec2::splat(VOXEL_SIZE * 10.0)
        );
        assert_eq!(
            voxel_occlusion_cast_size_at(&fade, 1.0),
            Vec2::new(VOXEL_SIZE, VOXEL_SIZE * 2.0)
        );
        assert_eq!(
            voxel_occlusion_cast_size_at(&fade, 0.5),
            Vec2::new(VOXEL_SIZE * 5.5, VOXEL_SIZE * 6.0)
        );
        assert!(voxel_is_touched_by_replay_cast(
            Vec3::new(1.0, 0.0, 1.0),
            &transform,
            &fade,
        ));
        assert!(!voxel_is_touched_by_replay_cast(
            Vec3::new(1.0, 0.0, 9.0),
            &transform,
            &fade,
        ));
        assert!(voxel_is_touched_by_replay_cast(
            Vec3::new(0.0, 0.45, 9.0),
            &transform,
            &fade,
        ));
        assert!(!voxel_is_touched_by_replay_cast(
            Vec3::new(0.0, 0.6, 9.0),
            &transform,
            &fade,
        ));
        assert!(!voxel_is_touched_by_replay_cast(
            Vec3::new(0.0, 0.0, 11.0),
            &transform,
            &fade,
        ));
    }

    #[test]
    fn planet_ocean_material_is_opaque() {
        let material = opaque_planet_ocean_material(Handle::default());
        assert!(matches!(
            material.alpha_mode,
            AlphaMode::Opaque
        ));
        assert!(material.base_color_texture.is_some());
    }

    #[test]
    fn default_static_space_map_uses_valid_materials_and_internal_auto_doors() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        let materials = voxel_cells(grid)
            .into_iter()
            .map(|(_, material)| material)
            .collect::<HashSet<_>>();
        assert!(voxel_cells(grid).len() >= 20_000);
        assert!(
            materials
                .iter()
                .all(|material| (1..=VOXEL_MATERIAL_COUNT as u8).contains(material))
        );
        assert!(
            HashSet::from([2, 4, 5, 6, 7, 9, 10]).is_subset(&materials),
            "static stations must retain their terrain, fluid, hull, armor, and door palette"
        );

        let doors = voxel_auto_doors();
        assert_eq!(doors.len(), 30);
        assert!(doors.iter().all(|door| {
            door.cells.len() >= 5
                && door.cells.len() % (WORKBOOK_ROOM_HEIGHT as usize - 2) == 0
        }));
        assert!(doors
            .iter()
            .flat_map(|door| &door.cells)
            .all(|cell| { grid.get(*cell).copied().unwrap_or(0) == 0 }));
        for door in &doors {
            assert!(voxel_auto_door_has_support(
                grid,
                &door.cells.iter().copied().collect()
            ));
            let panels = voxel_auto_door_panels(door);
            assert!(panels.iter().all(|panel| {
                (panel.open_translation.y - panel.closed_translation.y).abs() < f32::EPSILON
            }));
            for panel in &panels {
                let size = voxel_auto_door_panel_size(panel);
                let depth = if panel.width_axis == IVec3::X { size.z } else { size.x };
                assert!((depth - VOXEL_SIZE * 0.45).abs() < f32::EPSILON);
            }
            let left_delta = panels[0].open_translation - panels[0].closed_translation;
            let right_delta = panels[1].open_translation - panels[1].closed_translation;
            assert!(left_delta.dot(door.width_axis.as_vec3()) < 0.0);
            assert!(right_delta.dot(door.width_axis.as_vec3()) > 0.0);
            assert!(voxel_auto_door_should_open(
                door,
                door.trigger_center
            ));
            assert!(!voxel_auto_door_should_open(
                door,
                door.trigger_center + Vec3::Y * (door.trigger_half_height + 0.01)
            ));
        }

        let lights = workbook_interior_lights();
        assert!(lights.len() >= 20);
        assert!(lights.iter().all(|(position, _)| position.y > 0.0));
    }

    #[test]
    fn auto_door_is_destroyed_when_all_surrounding_voxels_are_empty() {
        let (mut app, grid_entity) = test_grid();
        app.add_systems(
            Update,
            despawn_unsupported_voxel_auto_doors,
        );
        let door = voxel_auto_doors().remove(0);
        let panels = voxel_auto_door_panels(&door);
        let panel_entities = panels
            .iter()
            .cloned()
            .map(|panel| app.world_mut().spawn(panel).id())
            .collect::<Vec<_>>();
        let door_cells = door.cells.iter().copied().collect::<HashSet<_>>();

        {
            let mut grid_entity = app.world_mut().entity_mut(grid_entity);
            let mut grid = grid_entity.get_mut::<Grid<u8>>().unwrap();
            for cell in &door_cells {
                for offset in VOXEL_NEIGHBORS {
                    grid.set(*cell + offset, 0);
                }
            }
            assert!(!voxel_auto_door_has_support(
                &grid,
                &door_cells
            ));
        }

        app.update();

        assert!(panel_entities
            .into_iter()
            .all(|entity| app.world().get_entity(entity).is_err()));
    }

    #[test]
    fn scene_snapshot_cells_include_solid_and_fluid_materials() {
        let (mut app, entity) = test_grid();
        let mut entity_mut = app.world_mut().entity_mut(entity);
        let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
        grid.set(IVec3::new(50, 4, 2), 2);
        grid.set(IVec3::new(51, 4, 2), 4);

        let cells = voxel_cells(&grid).into_iter().collect::<HashMap<_, _>>();

        assert_eq!(
            cells.get(&IVec3::new(50, 4, 2)),
            Some(&2)
        );
        assert_eq!(
            cells.get(&IVec3::new(51, 4, 2)),
            Some(&4)
        );
    }

    #[test]
    fn scene_history_entries_request_direct_restore() {
        let mut editor = VoxelEditorState::default();
        editor.scene_snapshots.push(VoxelSceneSnapshot {
            name: "场景快照 1".to_owned(),
            voxels: vec![(IVec3::ZERO, 1)],
            physics_bodies: Vec::new(),
            placed_lights: Vec::new(),
        });

        assert_eq!(editor.scene_snapshot_labels(), vec![
            "场景快照 1（1 方块 / 0 物理体 / 0 灯光）"
        ]);
        editor.request_scene_restore(0);
        assert_eq!(editor.restore_scene_requested, Some(0));
    }

    #[test]
    fn reset_scene_requires_an_explicit_second_confirmation() {
        let mut editor = VoxelEditorState::default();

        editor.request_reset_scene_confirmation();
        assert!(editor.reset_scene_confirmation());
        assert!(!editor.reset_requested);

        editor.cancel_reset_scene_confirmation();
        assert!(!editor.reset_scene_confirmation());
        assert!(!editor.reset_requested);

        editor.request_reset_scene_confirmation();
        editor.confirm_reset_scene();
        assert!(!editor.reset_scene_confirmation());
        assert!(editor.reset_requested);
    }

    #[test]
    fn persisted_scene_keeps_voxel_and_physics_state_in_bincode() {
        let transform = Transform {
            translation: Vec3::new(2.0, 3.0, 4.0),
            rotation: Quat::from_rotation_y(0.75),
            scale: Vec3::new(1.0, 1.5, 0.8),
        };
        let body = VoxelPhysicsBody {
            local_center: Vec3::splat(VOXEL_SIZE),
            cells: vec![(IVec3::new(1, 2, 3), 7)],
        };
        let persisted_body = persisted_voxel_physics_body(
            &body,
            &transform,
            &LinearVelocity(Vec3::new(5.0, 6.0, 7.0)),
            &AngularVelocity(Vec3::new(0.1, 0.2, 0.3)),
        );
        let light = VoxelPlacedLight {
            kind: VoxelLightTool::Physics,
            cell: IVec3::new(8, 9, 10),
            color: [0.2, 0.4, 0.8],
            intensity: 2_400.0,
            range: 9.0,
            direction: Vec3::NEG_Z,
        };
        let persisted_light = persisted_voxel_light(
            &light,
            &transform,
            Some(&LinearVelocity(Vec3::X * 4.0)),
            Some(&AngularVelocity(Vec3::Y * 2.0)),
        );
        let store = VoxelSceneStore {
            scene: Some(PersistedVoxelScene {
                voxels: vec![persisted_voxel_cell(IVec3::new(11, 12, 13), 4)],
                planet: Some(PersistedVoxelPlanet {
                    cells: vec![persisted_voxel_cell(IVec3::new(14, 15, 16), 6)],
                    removed: vec![[17, 18, 19]],
                }),
                physics_bodies: vec![persisted_body],
                placed_lights: vec![persisted_light],
            }),
            layout_revision: VOXEL_SCENE_LAYOUT_REVISION,
        };

        let bytes = StorageFormat::Bincode
            .serialize("voxel_scene_test", &store)
            .unwrap();
        let restored: VoxelSceneStore = StorageFormat::Bincode
            .deserialize("voxel_scene_test", &bytes)
            .unwrap();

        assert_eq!(restored, store);
        let scene = restored.scene.unwrap();
        let (_, restored_transform, linear_velocity, angular_velocity) =
            runtime_voxel_physics_body(&scene.physics_bodies[0]);
        assert_eq!(
            restored_transform.translation,
            transform.translation
        );
        assert_eq!(
            restored_transform.scale,
            transform.scale
        );
        assert!(restored_transform
            .rotation
            .abs_diff_eq(transform.rotation, 0.000_001));
        assert_eq!(
            linear_velocity.0,
            Vec3::new(5.0, 6.0, 7.0)
        );
        assert_eq!(
            angular_velocity.0,
            Vec3::new(0.1, 0.2, 0.3)
        );
        assert_eq!(scene.placed_lights[0].translation, [
            2.0, 3.0, 4.0
        ]);
        assert_eq!(
            scene.planet.as_ref().unwrap().removed,
            vec![[17, 18, 19]]
        );
        assert_eq!(
            scene.placed_lights[0].linear_velocity,
            [4.0, 0.0, 0.0]
        );
    }

    #[test]
    fn scene_history_restores_grid_and_physics_body_state() {
        let (mut app, grid_entity) = test_grid();
        app.init_resource::<VoxelEditorState>()
            .init_resource::<VoxelPhysicsChunkLoader>()
            .init_resource::<VoxelScenePersistenceState>()
            .init_resource::<Assets<Mesh>>()
            .insert_resource(VoxelMaterials {
                handles: std::array::from_fn(|_| Handle::default()),
                planet_ocean: Handle::default(),
            })
            .add_systems(Update, process_voxel_scene_history);
        let saved_position = IVec3::new(50, 6, 3);
        app.world_mut()
            .entity_mut(grid_entity)
            .get_mut::<Grid<u8>>()
            .unwrap()
            .set(saved_position, 3);
        app.world_mut().spawn((
            VoxelPhysicsBody {
                local_center: Vec3::splat(0.5),
                cells: vec![(IVec3::ZERO, 2)],
            },
            Transform::from_translation(Vec3::new(2.0, 3.0, 4.0)),
            LinearVelocity(Vec3::X),
            AngularVelocity(Vec3::Y),
        ));
        let saved_light_cell = IVec3::new(52, 8, 3);
        let saved_light = app
            .world_mut()
            .spawn(VoxelPlacedLight {
                kind: VoxelLightTool::Point,
                cell: saved_light_cell,
                color: [1.0, 0.5, 0.25],
                intensity: 2_400.0,
                range: 7.5,
                direction: Vec3::Y,
            })
            .id();
        app.world_mut()
            .resource_mut::<VoxelEditorState>()
            .request_scene_snapshot();
        app.update();

        app.world_mut()
            .entity_mut(grid_entity)
            .get_mut::<Grid<u8>>()
            .unwrap()
            .set(saved_position, 0);
        let body_entity = app
            .world_mut()
            .query_filtered::<Entity, With<VoxelPhysicsBody>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .entity_mut(body_entity)
            .get_mut::<Transform>()
            .unwrap()
            .translation = Vec3::splat(99.0);
        app.world_mut().despawn(saved_light);
        app.world_mut()
            .resource_mut::<VoxelEditorState>()
            .request_scene_restore(0);
        app.update();

        let grid = app.world().entity(grid_entity).get::<Grid<u8>>().unwrap();
        assert_eq!(grid.get(saved_position), Some(&3));
        let transform = app
            .world_mut()
            .query_filtered::<&Transform, With<VoxelPhysicsBody>>()
            .single(app.world())
            .unwrap();
        assert_eq!(
            transform.translation,
            Vec3::new(2.0, 3.0, 4.0)
        );
        let restored_light = app
            .world_mut()
            .query_filtered::<&VoxelPlacedLight, With<PointLight>>()
            .single(app.world())
            .unwrap();
        assert_eq!(restored_light.cell, saved_light_cell);
        assert_eq!(restored_light.color, [1.0, 0.5, 0.25]);
    }

    #[test]
    fn stroke_round_trips() {
        let (mut app, entity) = test_grid();
        let mut entity_mut = app.world_mut().entity_mut(entity);
        let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
        let position = IVec3::new(20, 3, 20);
        let stroke = [VoxelChange {
            position,
            before: 0,
            after: 2,
        }];
        apply_stroke(&mut grid, &stroke, true);
        assert_eq!(grid.get(position), Some(&2));
        apply_stroke(&mut grid, &stroke, false);
        assert_eq!(grid.get(position), Some(&0));
    }

    #[test]
    fn raycast_hits_voxel_and_adjacent_air() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        let [spawn_x, spawn_z] = NIFFY.spawn;
        let ray = Ray3d::new(
            (RESEARCH_STATION_CENTER.as_vec3()
                + Vec3::new(spawn_x as f32 + 0.5, 120.0, spawn_z as f32 + 0.5))
                * VOXEL_SIZE,
            Dir3::NEG_Y,
        );
        let hit = raycast_grid(grid, ray).unwrap();
        assert!(hit.occupied.is_some());
        assert_ne!(hit.occupied, hit.add);
    }

    #[test]
    fn textured_meshes_cover_populated_materials() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        let (meshes, colliders) = build_voxel_meshes(grid);
        let populated_materials = voxel_cells(grid)
            .into_iter()
            .map(|(_, material)| material)
            .collect::<HashSet<_>>();
        let mesh_materials = meshes
            .iter()
            .map(|(material, _)| *material)
            .collect::<HashSet<_>>();
        assert_eq!(mesh_materials, populated_materials);
        assert!(
            mesh_materials
                .iter()
                .all(|material| (1..=VOXEL_MATERIAL_COUNT as u8).contains(material))
        );
        assert!(!colliders.is_empty());
        for (_, mesh) in meshes {
            assert!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).is_some());
            assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
        }
    }

    #[test]
    fn adjacent_voxels_keep_individual_texture_faces() {
        let cells = vec![(IVec3::ZERO, 1), (IVec3::X, 1)];

        let (meshes, _) = build_voxel_meshes_from_cells(&cells);

        assert_eq!(meshes.len(), 1);
        assert_eq!(
            meshes[0]
                .1
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .len(),
            40
        );
    }

    #[test]
    fn chunk_mesh_hides_faces_against_neighbor_chunks() {
        let mut world = World::new();
        let entity = world.spawn(Grid::<u8>::new()).id();
        {
            let mut entity_mut = world.entity_mut(entity);
            let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
            grid.set(IVec3::new(DIMS.x - 1, 0, 0), 1);
            grid.set(IVec3::new(DIMS.x, 0, 0), 1);
        }
        let grid = world.entity(entity).get::<Grid<u8>>().unwrap();

        let (meshes, colliders) = build_voxel_chunk_meshes(grid, IVec3::ZERO);

        assert_eq!(colliders, vec![IVec3::new(
            DIMS.x - 1,
            0,
            0
        )]);
        assert_eq!(meshes.len(), 1);
        assert_eq!(
            meshes[0]
                .1
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .len(),
            20
        );
    }

    #[test]
    fn dirty_chunk_tracking_only_rebuilds_boundary_neighbors() {
        let mut dirty = VoxelGeometryDirtyChunks::default();
        dirty.mark_cell_and_neighbors(IVec3::new(3, 4, 5));
        assert_eq!(
            dirty.chunks,
            HashSet::from([IVec3::ZERO])
        );

        dirty.chunks.clear();
        dirty.mark_cell_and_neighbors(IVec3::new(DIMS.x - 1, 4, 5));
        assert_eq!(
            dirty.chunks,
            HashSet::from([IVec3::ZERO, IVec3::X])
        );
    }

    #[test]
    fn immediate_edit_rebuild_keeps_unrelated_chunk_entities() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<VoxelGeometryDirtyChunks>()
            .init_resource::<VoxelMicroDecorations>()
            .insert_resource(VoxelMaterials {
                handles: std::array::from_fn(|_| Handle::default()),
                planet_ocean: Handle::default(),
            })
            .add_systems(Update, rebuild_voxel_geometry);
        let grid = Grid::<u8>::new();
        let grid_entity = app.world_mut().spawn((TrpgVoxelGrid, grid)).id();
        {
            let mut entity = app.world_mut().entity_mut(grid_entity);
            let mut grid = entity.get_mut::<Grid<u8>>().unwrap();
            grid.set(IVec3::ZERO, 1);
            grid.set(IVec3::new(DIMS.x + 2, 0, 0), 1);
        }
        app.update();
        let unrelated_entities = app
            .world_mut()
            .query::<(Entity, &VoxelGeometry)>()
            .iter(app.world())
            .filter_map(|(entity, geometry)| (geometry.chunk == IVec3::X).then_some(entity))
            .collect::<Vec<_>>();
        assert!(!unrelated_entities.is_empty());

        {
            let mut entity = app.world_mut().entity_mut(grid_entity);
            entity
                .get_mut::<Grid<u8>>()
                .unwrap()
                .set(IVec3::new(1, 0, 0), 1);
        }
        app.world_mut()
            .resource_mut::<VoxelGeometryDirtyChunks>()
            .mark_cell_and_neighbors(IVec3::new(1, 0, 0));
        app.update();

        assert!(unrelated_entities
            .into_iter()
            .all(|entity| app.world().get_entity(entity).is_ok()));
    }

    #[test]
    fn connector_treats_zero_as_air() {
        assert!(!TrpgVoxelConnector::solid(&0));
        assert!(TrpgVoxelConnector::solid(&1));
        assert!(!TrpgVoxelConnector::solid(&4));
        assert!(!TrpgVoxelConnector::solid(&5));
        assert!(TrpgVoxelConnector::solid(&6));
        assert!(TrpgVoxelConnector::solid(&7));
        assert!(TrpgVoxelConnector::solid(&8));
        assert!(TrpgVoxelConnector::solid(&9));
        assert!(TrpgVoxelConnector::solid(&10));
    }

    #[test]
    fn voxel_physics_props_detail_workbook_interiors() {
        let specs = voxel_physics_prop_specs();
        assert_eq!(
            specs.len(),
            static_workbook_orbital_locations().len() * 2
        );
        assert!(specs.iter().all(|(cells, _)| !cells.is_empty()));
        assert!(specs.iter().all(
            |(cells, _)| cells.iter().all(
                |(_, material)| TrpgVoxelConnector::solid(material)
                    && (1..=VOXEL_MATERIAL_COUNT as u8).contains(material)
            )
        ));
        assert!(specs.iter().all(|(cells, _)| cells.len() == 2 * 2 * 2));
    }

    #[test]
    fn voxel_pointer_actions_stay_blocked_for_clicks_started_over_ui() {
        assert!(!voxel_world_pointer_blocked(
            false, false
        ));
        assert!(voxel_world_pointer_blocked(true, false));
        assert!(voxel_world_pointer_blocked(false, true));
        assert!(voxel_world_pointer_blocked(true, true));
    }

    #[test]
    fn physics_selection_normalizes_corners() {
        let mut editor = VoxelEditorState::default();
        editor.select_physics_corner(IVec3::new(4, 1, -2), false);
        editor.select_physics_corner(IVec3::new(-1, 3, 5), false);
        assert_eq!(
            editor.selection_bounds(),
            Some((
                IVec3::new(-1, 1, -2),
                IVec3::new(4, 3, 5)
            ))
        );
    }

    #[test]
    fn force_tools_map_to_right_click_actions() {
        assert_eq!(
            force_tool_action(VoxelEditMode::Push),
            Some(VoxelPhysicsAction::Push)
        );
        assert_eq!(
            force_tool_action(VoxelEditMode::Pull),
            Some(VoxelPhysicsAction::Pull)
        );
        assert_eq!(
            force_tool_action(VoxelEditMode::Explode),
            Some(VoxelPhysicsAction::Explode)
        );
        assert_eq!(
            force_tool_action(VoxelEditMode::Physics),
            None
        );
    }

    #[test]
    fn first_person_mouse_wheel_selects_and_wraps_hotbar_slots() {
        assert_eq!(cycled_hotbar_slot(0, -1, 10), 1);
        assert_eq!(cycled_hotbar_slot(0, 1, 10), 9);
        assert_eq!(cycled_hotbar_slot(9, -1, 10), 0);
        assert_eq!(cycled_hotbar_slot(5, 3, 10), 2);
    }

    #[test]
    fn first_person_transitions_preserve_camera_position() {
        let camera_position = Vec3::new(4.0, 7.0, -2.0);
        let player_position = first_person_player_position(camera_position);
        assert_eq!(
            player_position + Vec3::Y * FIRST_PERSON_EYE_OFFSET,
            camera_position
        );

        let yaw = 0.7;
        let pitch = -0.45;
        let distance = 42.0;
        let focus = orbit_focus_preserving_camera_position(camera_position, yaw, pitch, distance);
        let editor = VoxelEditorState {
            camera_focus: focus,
            camera_distance: distance,
            camera_yaw: yaw,
            camera_pitch: pitch,
            ..default()
        };
        assert!(editor_camera_transform(&editor)
            .translation
            .abs_diff_eq(camera_position, 0.000_01));
    }

    #[test]
    fn physics_selection_keeps_disconnected_solids_in_one_body_and_ignores_fluids() {
        let (mut app, entity) = test_grid();
        let mut entity_mut = app.world_mut().entity_mut(entity);
        let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
        for cell in occupied_cells(&grid) {
            grid.set(cell, 0);
        }
        grid.set(IVec3::ZERO, 1);
        grid.set(IVec3::X, 2);
        grid.set(IVec3::new(4, 0, 0), 3);
        grid.set(IVec3::new(2, 0, 0), 4);

        let selected = selected_solid_voxels(&grid, IVec3::ZERO, IVec3::new(4, 0, 0));
        assert_eq!(selected.len(), 3);
        assert!(selected.iter().any(|(cell, _)| *cell == IVec3::ZERO));
        assert!(selected.iter().any(|(cell, _)| *cell == IVec3::X));
        assert!(selected
            .iter()
            .any(|(cell, _)| *cell == IVec3::new(4, 0, 0)));
        assert!(!selected
            .iter()
            .any(|(cell, _)| *cell == IVec3::new(2, 0, 0)));
    }

    #[test]
    fn explosion_radius_selects_all_static_solids_inside_it() {
        let (mut app, entity) = test_grid();
        let mut entity_mut = app.world_mut().entity_mut(entity);
        let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
        for cell in occupied_cells(&grid) {
            grid.set(cell, 0);
        }
        grid.set(IVec3::ZERO, 1);
        grid.set(IVec3::X, 2);
        grid.set(IVec3::new(2, 0, 0), 3);
        grid.set(IVec3::Y, 4);

        let origin = Vec3::splat(0.5) * VOXEL_SIZE;
        let selected = selected_solid_voxels_in_radius(&grid, origin, 0.3);

        assert_eq!(selected.len(), 2);
        assert!(selected.iter().any(|(cell, _)| *cell == IVec3::ZERO));
        assert!(selected.iter().any(|(cell, _)| *cell == IVec3::X));
        assert!(!selected
            .iter()
            .any(|(cell, _)| *cell == IVec3::new(2, 0, 0)));
        assert!(!selected.iter().any(|(cell, _)| *cell == IVec3::Y));
    }

    #[test]
    fn huge_explosion_radius_scans_allocated_voxels_instead_of_empty_space() {
        let (mut app, entity) = test_grid();
        let mut entity_mut = app.world_mut().entity_mut(entity);
        let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
        for cell in occupied_cells(&grid) {
            grid.set(cell, 0);
        }
        grid.set(IVec3::new(-20_000, 0, 0), 1);
        grid.set(IVec3::ZERO, 2);
        grid.set(IVec3::new(20_000, 0, 0), 3);
        grid.set(IVec3::Y, 4);

        let selected = selected_solid_voxels_in_radius(&grid, Vec3::ZERO, 10_000.0);

        assert_eq!(selected, vec![
            (IVec3::new(-20_000, 0, 0), 1),
            (IVec3::ZERO, 2),
            (IVec3::new(20_000, 0, 0), 3),
        ]);
    }

    #[test]
    fn explosion_radius_detects_voxels_inside_a_moving_body() {
        let body = VoxelPhysicsBody {
            local_center: Vec3::splat(VOXEL_SIZE),
            cells: vec![(IVec3::ZERO, 1), (IVec3::X, 2)],
        };
        let transform = Transform::from_xyz(4.0, 2.0, -3.0).with_rotation(Quat::from_rotation_y(
            std::f32::consts::FRAC_PI_2,
        ));
        let first_center = transform
            .compute_affine()
            .transform_point3(Vec3::splat(0.5) * VOXEL_SIZE);

        assert!(physics_body_intersects_radius(
            &body,
            &transform,
            first_center,
            VOXEL_SIZE,
        ));
        assert!(!physics_body_intersects_radius(
            &body,
            &transform,
            Vec3::ZERO,
            VOXEL_SIZE,
        ));
    }

    #[test]
    fn explosion_fragment_budget_caps_large_areas_at_sixty_parts() {
        let counts = allocate_fragment_parts(
            &[8_656],
            MAX_EXPLOSION_NEW_PHYSICS_BODIES,
        );
        assert_eq!(counts, vec![60]);

        let cells = (0..8_656).map(|x| (IVec3::new(x, 0, 0), 1)).collect();
        let parts = split_voxel_cells_randomly(cells, counts[0], 7);
        assert_eq!(parts.len(), 60);
        assert_eq!(
            parts.iter().map(Vec::len).sum::<usize>(),
            8_656
        );
    }

    #[test]
    fn explosion_fragments_grow_as_connected_irregular_clusters() {
        let cells = prism(IVec3::ZERO, IVec3::splat(8))
            .map(|cell| (cell, 1))
            .collect::<Vec<_>>();
        let parts = split_voxel_cells_randomly(cells.clone(), 12, 0xbad5_eed);

        assert_eq!(parts.len(), 12);
        let assigned = parts
            .iter()
            .flatten()
            .map(|(cell, _)| *cell)
            .collect::<HashSet<_>>();
        assert_eq!(assigned.len(), cells.len());

        for part in &parts {
            let part_cells = part.iter().map(|(cell, _)| *cell).collect::<HashSet<_>>();
            let mut reached = HashSet::from([part[0].0]);
            let mut frontier = vec![part[0].0];
            while let Some(cell) = frontier.pop() {
                for neighbor in VOXEL_NEIGHBORS.map(|offset| cell + offset) {
                    if part_cells.contains(&neighbor) && reached.insert(neighbor) {
                        frontier.push(neighbor);
                    }
                }
            }
            assert_eq!(reached.len(), part.len());
        }

        assert!(parts.iter().any(|part| {
            let min = part
                .iter()
                .map(|(cell, _)| *cell)
                .reduce(IVec3::min)
                .unwrap();
            let max = part
                .iter()
                .map(|(cell, _)| *cell)
                .reduce(IVec3::max)
                .unwrap();
            let dimensions = max - min + IVec3::ONE;
            dimensions.x as usize * dimensions.y as usize * dimensions.z as usize > part.len()
        }));
    }

    #[test]
    fn explosion_fragment_seed_changes_the_break_pattern() {
        let cells = prism(IVec3::ZERO, IVec3::splat(6))
            .map(|cell| (cell, 1))
            .collect::<Vec<_>>();

        let first = split_voxel_cells_randomly(cells.clone(), 8, 11);
        let repeated = split_voxel_cells_randomly(cells.clone(), 8, 11);
        let different = split_voxel_cells_randomly(cells, 8, 12);

        assert_eq!(first, repeated);
        assert_ne!(first, different);
    }

    #[test]
    fn each_explosion_gets_sixty_new_parts_regardless_of_existing_bodies() {
        let unaffected_existing_body_count = 250;
        let counts = allocate_fragment_parts(
            &[12, 8, 8_656],
            MAX_EXPLOSION_NEW_PHYSICS_BODIES,
        );

        let new_body_count = counts.iter().sum::<usize>();
        assert_eq!(new_body_count, 60);
        assert_eq!(
            unaffected_existing_body_count + new_body_count,
            310
        );
        assert!(counts.iter().all(|count| *count > 0));
    }

    #[test]
    fn push_and_pull_impulses_are_relative_to_camera() {
        let body = Vec3::new(3.0, 0.0, 0.0);
        let camera = Vec3::ZERO;
        let push = physics_action_impulse(
            VoxelPhysicsAction::Push,
            body,
            camera,
            Vec3::ZERO,
            4.0,
            10.0,
            6.0,
        )
        .unwrap();
        let pull = physics_action_impulse(
            VoxelPhysicsAction::Pull,
            body,
            camera,
            Vec3::ZERO,
            4.0,
            10.0,
            6.0,
        )
        .unwrap();
        assert_eq!(push, Vec3::X * 4.0);
        assert_eq!(pull, Vec3::NEG_X * 4.0);
    }

    #[test]
    fn explosion_impulse_falls_off_and_stops_at_radius() {
        let inside = physics_action_impulse(
            VoxelPhysicsAction::Explode,
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::ZERO,
            4.0,
            10.0,
            6.0,
        )
        .unwrap();
        assert_eq!(inside, Vec3::X * 5.0);
        assert!(physics_action_impulse(
            VoxelPhysicsAction::Explode,
            Vec3::new(7.0, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::ZERO,
            4.0,
            10.0,
            6.0,
        )
        .is_none());
    }

    #[test]
    fn edit_repeat_has_no_initial_hold_debounce() {
        let mut editor = VoxelEditorState::default();
        assert!(edit_repeat_due(true, 0.0, &mut editor));
        assert!(!edit_repeat_due(
            false,
            EDIT_REPEAT_INTERVAL - 0.01,
            &mut editor
        ));
        assert!(edit_repeat_due(
            false,
            0.01,
            &mut editor
        ));
    }

    #[test]
    fn minecraft_mouse_buttons_remove_with_left_and_use_equipped_mode_with_right() {
        assert_eq!(
            voxel_edit_input_mode(VoxelEditMode::Add, true, false, false),
            Some(VoxelEditMode::Remove)
        );
        assert_eq!(
            voxel_edit_input_mode(VoxelEditMode::Paint, false, true, false),
            Some(VoxelEditMode::Paint)
        );
        assert_eq!(
            voxel_edit_input_mode(VoxelEditMode::Add, true, true, false),
            Some(VoxelEditMode::Remove)
        );
        assert_eq!(
            voxel_edit_input_mode(VoxelEditMode::Add, false, false, false),
            None
        );
        assert_eq!(
            voxel_edit_input_mode(VoxelEditMode::Add, true, false, true),
            None,
            "tool gun primary fire must never fall through to block deletion"
        );
    }

    #[test]
    fn paint_changes_solids_without_creating_voxels() {
        assert_eq!(
            edited_voxel(VoxelEditMode::Paint, 1, 4),
            Some(4)
        );
        assert_eq!(
            edited_voxel(VoxelEditMode::Paint, 0, 4),
            None
        );
    }

    #[test]
    fn first_person_flight_requires_two_quick_space_taps() {
        let mut elapsed = f32::INFINITY;
        assert!(!register_first_person_space_tap(
            &mut elapsed
        ));
        assert_eq!(elapsed, 0.0);

        elapsed = FIRST_PERSON_DOUBLE_TAP_SECONDS * 0.5;
        assert!(register_first_person_space_tap(
            &mut elapsed
        ));
        assert!(elapsed.is_infinite());

        elapsed = FIRST_PERSON_DOUBLE_TAP_SECONDS + 0.01;
        assert!(!register_first_person_space_tap(
            &mut elapsed
        ));
    }

    #[test]
    fn first_person_flight_enables_noclip_and_restores_collision_on_exit() {
        let mut editor = VoxelEditorState::default();
        editor.first_person_enabled = true;
        editor.first_person_flying = true;
        editor.first_person_cursor_released = false;

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(editor)
            .init_resource::<VoxelPossessionState>()
            .add_systems(Update, control_first_person_player);
        let player = app
            .world_mut()
            .spawn((
                VoxelFirstPersonPlayer,
                ShapeHits::default(),
                Transform::default(),
                LinearVelocity::ZERO,
                ConstantLinearAcceleration::new(0.0, 0.0, 0.0),
            ))
            .id();

        app.update();

        assert!(app.world().entity(player).contains::<Sensor>());
        assert_eq!(
            app.world()
                .entity(player)
                .get::<ConstantLinearAcceleration>()
                .unwrap()
                .0,
            Vec3::ZERO,
        );

        app.world_mut()
            .resource_mut::<VoxelEditorState>()
            .first_person_flying = false;
        app.update();

        assert!(!app.world().entity(player).contains::<Sensor>());
        assert_eq!(
            app.world()
                .entity(player)
                .get::<ConstantLinearAcceleration>()
                .unwrap()
                .0,
            Vec3::new(0.0, -9.81, 0.0),
        );
    }

    #[test]
    fn ctrl_shift_z_requests_redo() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EguiWantsInput>()
            .init_resource::<VoxelEditorState>()
            .init_resource::<VoxelPossessionState>()
            .add_systems(Update, voxel_editor_shortcuts);
        let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keyboard.press(KeyCode::ControlLeft);
        keyboard.press(KeyCode::ShiftLeft);
        keyboard.press(KeyCode::KeyZ);
        app.update();
        let editor = app.world().resource::<VoxelEditorState>();
        assert!(editor.redo_requested);
        assert!(!editor.undo_requested);
    }

    #[test]
    fn e_opens_creative_inventory() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EguiWantsInput>()
            .init_resource::<VoxelEditorState>()
            .init_resource::<VoxelPossessionState>()
            .add_systems(Update, voxel_editor_shortcuts);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        app.update();
        assert!(
            app.world()
                .resource::<VoxelEditorState>()
                .creative_inventory_open
        );
    }

    #[test]
    fn zero_selects_tenth_creative_material() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EguiWantsInput>()
            .init_resource::<VoxelEditorState>()
            .init_resource::<VoxelPossessionState>()
            .add_systems(Update, voxel_editor_shortcuts);
        app.world_mut()
            .resource_mut::<VoxelEditorState>()
            .light_tool = Some(VoxelLightTool::Point);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Digit0);
        app.update();
        assert_eq!(
            app.world().resource::<VoxelEditorState>().material,
            10
        );
        assert_eq!(
            app.world().resource::<VoxelEditorState>().light_tool,
            None
        );
    }

    #[test]
    fn possessed_player_shortcuts_do_not_change_creative_toolbar() {
        let mut possession = VoxelPossessionState::default();
        possession.possess(42);
        let mut editor = VoxelEditorState::default();
        editor.select_hotbar_slot(0);

        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EguiWantsInput>()
            .insert_resource(editor)
            .insert_resource(possession)
            .add_systems(Update, voxel_editor_shortcuts);
        {
            let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keyboard.press(KeyCode::Digit5);
            keyboard.press(KeyCode::KeyE);
        }

        app.update();

        assert_eq!(
            app.world()
                .resource::<VoxelEditorState>()
                .selected_hotbar_slot,
            0,
            "survival number keys must not select creative toolbar buttons"
        );
        assert!(
            !app.world()
                .resource::<VoxelEditorState>()
                .creative_inventory_open
        );
        let possession = app.world().resource::<VoxelPossessionState>();
        assert_eq!(possession.selected_hotbar_slot, 4);
        assert!(possession.player_inventory_open);
    }

    #[test]
    fn creative_catalog_items_can_replace_and_delete_hotbar_slots() {
        let mut editor = VoxelEditorState::default();
        editor.select_hotbar_slot(3);
        editor.put_in_selected_hotbar(VoxelCreativeItem::Light(
            VoxelLightTool::DarkPoint,
        ));
        assert_eq!(
            editor.creative_hotbar[3],
            Some(VoxelCreativeItem::Light(
                VoxelLightTool::DarkPoint
            ))
        );
        assert_eq!(
            editor.light_tool,
            Some(VoxelLightTool::DarkPoint)
        );
        assert_eq!(editor.placed_light_color, [
            0.18, 0.08, 0.32
        ]);
        assert_eq!(editor.placed_light_intensity, 420.0);

        editor.creative_hotbar[5] = Some(VoxelCreativeItem::Mode(
            VoxelEditMode::Explode,
        ));
        editor.swap_hotbar_slots(3, 5);
        assert_eq!(
            editor.creative_hotbar[3],
            Some(VoxelCreativeItem::Mode(
                VoxelEditMode::Explode
            ))
        );
        editor.select_hotbar_slot(3);
        assert_eq!(editor.mode, VoxelEditMode::Explode);

        editor.delete_hotbar_slot(3);
        assert_eq!(editor.creative_hotbar[3], None);
        assert_eq!(editor.light_tool, None);
        assert_eq!(editor.active_tool_label(), "空手");

        editor.put_in_selected_hotbar(VoxelCreativeItem::PlayerPossessionTool);
        assert!(editor.is_player_possession_tool_equipped());
        assert_eq!(editor.active_tool_label(), "PL接管器");

        editor.put_in_selected_hotbar(VoxelCreativeItem::TeleportTool);
        assert!(editor.is_teleport_tool_equipped());
        assert_eq!(editor.active_tool_label(), "传送器");
    }

    #[test]
    fn creative_inventory_snapshot_persists_as_toml() {
        let mut inventory = VoxelInventoryStore::default();
        inventory.hotbar[2] = Some(VoxelCreativeItem::ToolGun);
        inventory.hotbar[3] = Some(VoxelCreativeItem::PlayerPossessionTool);
        inventory.hotbar[5] = Some(VoxelCreativeItem::TeleportTool);
        inventory.hotbar[4] = Some(VoxelCreativeItem::Mode(
            VoxelEditMode::Drag,
        ));
        inventory.selected_hotbar_slot = 4;
        inventory.tool_gun_mode = VoxelEditMode::Pull;

        let path = std::env::temp_dir().join(format!(
            "willowblossom_voxel_inventory_{}.toml",
            std::process::id()
        ));
        let mut store = Persistent::<VoxelInventoryStore>::builder()
            .name("test_voxel_inventory")
            .format(StorageFormat::Toml)
            .path(&path)
            .default(VoxelInventoryStore::default())
            .build()
            .unwrap();
        *store = inventory.clone();
        store.persist().unwrap();
        let loaded = Persistent::<VoxelInventoryStore>::builder()
            .name("test_voxel_inventory_reload")
            .format(StorageFormat::Toml)
            .path(&path)
            .default(VoxelInventoryStore::default())
            .build()
            .unwrap();

        assert_eq!(*loaded, inventory);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn voxel_toolbar_settings_persist_as_toml_and_restore_editor_values() {
        let mut editor = VoxelEditorState::default();
        editor.brush_radius = 7;
        editor.first_person_speed = 9.5;
        editor.placed_light_color = [0.1, 0.2, 0.3];
        editor.placed_light_intensity = 2_400.0;
        editor.placed_light_range = 12.0;
        editor.physics_push_pull_impulse = 17.0;
        editor.physics_explosion_impulse = 31.0;
        editor.physics_explosion_radius = 18.5;
        editor.ambient_brightness = 95.0;
        editor.key_light_illuminance = 9_000.0;
        editor.key_light_color = [0.8, 0.7, 0.6];
        editor.fill_light_illuminance = 2_100.0;
        editor.fill_light_color = [0.3, 0.4, 0.5];
        editor.radiance_intensity = 0.85;
        let settings = VoxelToolbarSettingsStore::from_editor(&editor);

        let path = std::env::temp_dir().join(format!(
            "willowblossom_voxel_toolbar_settings_{}.toml",
            std::process::id()
        ));
        let mut store = Persistent::<VoxelToolbarSettingsStore>::builder()
            .name("test_voxel_toolbar_settings")
            .format(StorageFormat::Toml)
            .path(&path)
            .default(VoxelToolbarSettingsStore::default())
            .build()
            .unwrap();
        *store = settings.clone();
        store.persist().unwrap();
        let loaded = Persistent::<VoxelToolbarSettingsStore>::builder()
            .name("test_voxel_toolbar_settings_reload")
            .format(StorageFormat::Toml)
            .path(&path)
            .default(VoxelToolbarSettingsStore::default())
            .build()
            .unwrap();
        let mut restored = VoxelEditorState::default();
        loaded.apply_to(&mut restored);

        assert_eq!(*loaded, settings);
        assert_eq!(
            VoxelToolbarSettingsStore::from_editor(&restored),
            settings
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn planet_selection_becomes_a_canonical_dynamic_voxel_body() {
        let selected_cell = IVec3::new(0, 0, 0);
        let mut editor = VoxelEditorState::default();
        editor.physics_requested = true;
        editor.selection_anchor = Some(selected_cell);
        editor.selection_end = Some(selected_cell);
        editor.selection_is_planet = true;

        let mut app = App::new();
        app.insert_resource(editor)
            .insert_resource(Assets::<Mesh>::default())
            .insert_resource(VoxelMaterials {
                handles: std::array::from_fn(|_| Handle::default()),
                planet_ocean: Handle::default(),
            })
            .add_systems(Update, make_selection_physical);
        let planet_entity = app
            .world_mut()
            .spawn((
                VoxelOrbitalPlanet {
                    cells: HashMap::from([(selected_cell, 1)]),
                    cell_bounds: Some(VoxelCellBounds::from_cell(
                        selected_cell,
                    )),
                    removed: HashSet::new(),
                    collider_entity: Entity::PLACEHOLDER,
                    mesh_entities: Vec::new(),
                    mesh_handles: Vec::new(),
                    voxel_size: VOXEL_SIZE,
                    dirty: false,
                },
                GlobalTransform::from_translation(Vec3::new(10.0, 20.0, 30.0)),
            ))
            .id();

        app.update();

        let planet = app
            .world()
            .entity(planet_entity)
            .get::<VoxelOrbitalPlanet>()
            .unwrap();
        assert!(!planet.cells.contains_key(&selected_cell));
        assert!(planet.removed.contains(&selected_cell));
        assert!(!planet.cells.is_empty());
        let mut bodies = app.world_mut().query::<(
            &VoxelPhysicsBody,
            &Transform,
            &Collider,
            &Friction,
        )>();
        let (body, transform, collider, _) = bodies.single(app.world()).unwrap();
        assert_eq!(body.cells, vec![(IVec3::ZERO, 1)]);
        assert!(collider.shape().as_voxels().is_some());
        assert_eq!(
            transform.translation + Vec3::splat(VOXEL_SIZE * 0.5),
            Vec3::new(10.0, 20.0, 30.0)
        );
    }

    #[test]
    fn tool_gun_uses_secondary_fire_and_cycles_utility_modes() {
        let mut editor = VoxelEditorState::default();

        editor.equip_creative_item(VoxelCreativeItem::ToolGun);

        assert!(editor.is_tool_gun_equipped());
        assert_eq!(editor.mode, VoxelEditMode::Physics);
        assert_eq!(
            voxel_tool_fire_button(true),
            MouseButton::Right
        );
        assert_eq!(
            voxel_tool_fire_button(false),
            MouseButton::Right
        );
        assert_eq!(
            editor.active_tool_label(),
            "工具枪 · 物理选区"
        );

        editor.cycle_tool_gun_mode();

        assert_eq!(editor.mode, VoxelEditMode::Drag);
        assert_eq!(
            editor.active_tool_label(),
            "工具枪 · 拖拽"
        );
    }

    #[test]
    fn drag_velocity_points_at_target_and_is_clamped() {
        let velocity = tool_gun_drag_velocity(Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0));
        assert!((velocity.length() - TOOL_GUN_DRAG_MAX_SPEED).abs() < 0.001);
        assert!(velocity.x > 0.0);

        assert_eq!(
            tool_gun_drag_velocity(Vec3::ONE, Vec3::ONE),
            Vec3::ZERO
        );
    }

    #[test]
    fn voxel_edits_signal_geometry_rebuild_immediately() {
        let mut world = World::new();
        let entity = world.spawn(Grid::<u8>::new()).id();
        world.clear_trackers();

        let mut entity_mut = world.entity_mut(entity);
        let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
        assert!(!grid.is_changed());
        grid.set(IVec3::new(2, 3, 4), 7);
        assert_eq!(grid.get(IVec3::new(2, 3, 4)), Some(&7));
        assert!(grid.is_changed());
    }

    #[test]
    fn planet_face_normal_opposes_the_dominant_ray_axis() {
        assert_eq!(
            voxel_face_normal_against_ray(Vec3::new(0.1, -0.9, 0.2)),
            IVec3::Y
        );
        assert_eq!(
            voxel_face_normal_against_ray(Vec3::new(0.8, 0.1, 0.2)),
            IVec3::NEG_X
        );
    }

    #[test]
    fn light_editor_tool_updates_the_selected_scene_light() {
        let mut app = App::new();
        app.init_resource::<VoxelEditorState>()
            .add_systems(Update, sync_selected_voxel_light);
        let entity = app
            .world_mut()
            .spawn((
                VoxelPlacedLight {
                    kind: VoxelLightTool::Point,
                    cell: IVec3::ZERO,
                    color: [1.0, 1.0, 1.0],
                    intensity: 100.0,
                    range: 2.0,
                    direction: Vec3::Y,
                },
                PointLight::default(),
            ))
            .id();
        {
            let mut editor = app.world_mut().resource_mut::<VoxelEditorState>();
            editor.equip_creative_item(VoxelCreativeItem::Light(
                VoxelLightTool::Edit,
            ));
            editor.selected_light = Some(entity);
            editor.placed_light_color = [0.2, 0.4, 0.8];
            editor.placed_light_intensity = 3_200.0;
            editor.placed_light_range = 12.0;
        }
        app.update();

        let entity_ref = app.world().entity(entity);
        let placed = entity_ref.get::<VoxelPlacedLight>().unwrap();
        let point = entity_ref.get::<PointLight>().unwrap();
        assert_eq!(placed.color, [0.2, 0.4, 0.8]);
        assert_eq!(placed.intensity, 3_200.0);
        assert_eq!(placed.range, 12.0);
        assert_eq!(point.intensity, 3_200.0);
        assert_eq!(point.range, 12.0);
    }

    #[test]
    fn survival_movement_is_clamped_to_final_movement_budget() {
        let previous = Vec3::new(1.0, 2.0, 1.0);
        let current = Vec3::new(7.0, 3.0, 9.0);
        let (clamped, used, exhausted) =
            clamp_horizontal_movement_step(previous, current, 2.0, 7.0);

        assert!((clamped.x - 4.0).abs() < 0.0001);
        assert!((clamped.z - 5.0).abs() < 0.0001);
        assert_eq!(clamped.y, current.y);
        assert!((used - 7.0).abs() < 0.0001);
        assert!(exhausted);
    }

    #[test]
    fn completed_possession_movement_is_persisted_per_campaign_player_and_turn() {
        let mut store = VoxelPossessionMovementStore::default();
        let turn_start = Vec3::new(1.0, 2.0, 3.0);

        upsert_possession_movement(
            &mut store,
            "campaign-a",
            42,
            7,
            3.5,
            true,
            turn_start,
        );

        let record = possession_movement_record(&store, "campaign-a", 42, 7).unwrap();
        assert_eq!(record.movement_used, 3.5);
        assert!(record.completed);
        assert_eq!(restored_possession_movement_used(record, 12.0), 12.0);
        assert_eq!(
            Vec3::from_array(record.turn_start_position_cells) * VOXEL_SIZE,
            turn_start
        );
        assert!(possession_movement_record(&store, "campaign-a", 42, 8).is_none());
        assert!(possession_movement_record(&store, "campaign-b", 42, 7).is_none());
    }

    #[test]
    fn clearing_test_progress_removes_only_matching_campaign_movement() {
        let mut store = VoxelPossessionMovementStore {
            records: vec![
                PersistedVoxelPossessionMovement {
                    campaign_id: "campaign-a".to_owned(),
                    user_id: 1,
                    turn: 2,
                    movement_used: 3.0,
                    completed: true,
                    turn_start_position_cells: [1.0, 2.0, 3.0],
                },
                PersistedVoxelPossessionMovement {
                    campaign_id: "campaign-b".to_owned(),
                    user_id: 1,
                    turn: 2,
                    movement_used: 4.0,
                    completed: true,
                    turn_start_position_cells: [4.0, 5.0, 6.0],
                },
            ],
        };

        assert_eq!(
            clear_campaign_possession_movement(&mut store, "campaign-a"),
            1
        );
        assert_eq!(store.records.len(), 1);
        assert_eq!(store.records[0].campaign_id, "campaign-b");
    }

    #[test]
    fn possession_movement_adds_default_and_runtime_equipment_allowance() {
        let mut character = PlayerCharacter {
            speed: 4.5,
            ..Default::default()
        };
        assert!((possession_character_movement(&character) - 14.5).abs() < 0.0001);

        character.inventory.equipment.insert(
            crate::napcat::EquipmentSlot::Feet,
            crate::napcat::InventoryItem {
                equipment_slot: crate::napcat::EquipmentSlot::Feet,
                stat_effects: vec![crate::rule_engine::BuffEffect {
                    field: BuffField::Speed,
                    value: BuffValue::Add(10.0),
                }],
                ..Default::default()
            },
        );
        assert!((possession_character_movement(&character) - 24.5).abs() < 0.0001);

        character.speed = 14.5;
        character.buff_base_stats = Some(crate::napcat::CharacterBuffBaseStats {
            speed: 4.5,
            ..crate::napcat::CharacterBuffBaseStats::from_character(&PlayerCharacter::default())
        });
        assert!((possession_character_movement(&character) - 24.5).abs() < 0.0001);
    }

    #[test]
    fn survival_movement_accumulates_path_length_without_clamping() {
        let (position, used, exhausted) = clamp_horizontal_movement_step(
            Vec3::ZERO,
            Vec3::new(3.0, 1.0, 4.0),
            1.0,
            10.0,
        );

        assert_eq!(position, Vec3::new(3.0, 1.0, 4.0));
        assert!((used - 6.0).abs() < 0.0001);
        assert!(!exhausted);
    }

    #[test]
    fn confirmed_movement_bypass_allows_and_records_distance_past_limit() {
        let current = Vec3::new(6.0, 1.0, 8.0);
        let (position, used, exhausted) =
            resolve_horizontal_movement_step(Vec3::ZERO, current, 4.0, 5.0, true);

        assert_eq!(position, current);
        assert!((used - 14.0).abs() < 0.0001);
        assert!(!exhausted);
    }

    #[test]
    fn possession_change_clears_dangerous_movement_override() {
        let mut possession = VoxelPossessionState::default();
        possession.movement_limit_bypassed = true;
        possession.movement_bypass_confirmation_pending = true;

        possession.possess(42);

        assert!(!possession.movement_limit_bypassed);
        assert!(!possession.movement_bypass_confirmation_pending);
    }

    #[test]
    fn possession_locks_the_dm_to_the_player_first_person_view() {
        let player_view = Transform::from_xyz(3.0, 4.0, 5.0);
        let mut possession = VoxelPossessionState::default();
        possession.possess(42);

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(VoxelEditorState::default())
            .insert_resource(possession)
            .add_systems(Update, control_first_person_player);
        app.world_mut().spawn((
            VoxelPlayerCaptureCamera { user_id: 42 },
            player_view,
        ));
        let player = app
            .world_mut()
            .spawn((
                VoxelFirstPersonPlayer,
                ShapeHits::default(),
                Transform::default(),
                LinearVelocity::ZERO,
                ConstantLinearAcceleration::new(0.0, 0.0, 0.0),
            ))
            .id();

        app.update();

        let editor = app.world().resource::<VoxelEditorState>();
        assert!(editor.first_person_enabled);
        assert!(!editor.first_person_flying);
        assert_eq!(
            app.world()
                .entity(player)
                .get::<ConstantLinearAcceleration>()
                .unwrap()
                .0,
            Vec3::new(0.0, -9.81, 0.0),
        );

        app.world_mut()
            .resource_mut::<VoxelPossessionState>()
            .release();
        app.update();

        assert!(
            app.world()
                .resource::<VoxelEditorState>()
                .first_person_flying,
            "releasing a survival player should restore DM creative flight"
        );
    }

    #[test]
    fn movement_reset_returns_possessed_player_to_turn_start() {
        let turn_start = Vec3::new(2.0, 3.0, 4.0);
        let mut possession = VoxelPossessionState {
            active_user_id: Some(42),
            applied_user_id: Some(42),
            movement_used: 3.5,
            turn_start_position: Some(turn_start),
            last_player_position: Some(Vec3::new(7.0, 3.0, 4.0)),
            reset_movement_requested: true,
            ..default()
        };
        possession.movement_limit = 10.0;

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(VoxelEditorState::default())
            .insert_resource(possession)
            .add_systems(Update, control_first_person_player);
        let player = app
            .world_mut()
            .spawn((
                VoxelFirstPersonPlayer,
                ShapeHits::default(),
                Transform::from_translation(Vec3::new(7.0, 3.0, 4.0)),
                LinearVelocity(Vec3::new(4.0, 0.0, 0.0)),
                ConstantLinearAcceleration::new(0.0, -9.81, 0.0),
            ))
            .id();

        app.update();

        assert_eq!(
            app.world()
                .entity(player)
                .get::<Transform>()
                .unwrap()
                .translation,
            turn_start
        );
        assert_eq!(
            app.world()
                .entity(player)
                .get::<LinearVelocity>()
                .unwrap()
                .0,
            Vec3::ZERO
        );
        let possession = app.world().resource::<VoxelPossessionState>();
        assert_eq!(possession.movement_used, 0.0);
        assert!(!possession.reset_movement_requested);
    }

    #[test]
    fn default_fleet_contains_a_cruiser_medium_ship_and_six_unique_small_ships() {
        let specs = default_voxel_spaceship_specs();
        assert_eq!(specs.len(), SMALL_SPACESHIP_COUNT + 2);
        assert_eq!(
            specs
                .iter()
                .filter(|spec| {
                    matches!(
                        spec.ship.class,
                        VoxelSpaceshipClass::Shuttle
                            | VoxelSpaceshipClass::Interceptor
                            | VoxelSpaceshipClass::Scout
                    )
                })
                .count(),
            SMALL_SPACESHIP_COUNT
        );
        assert!(specs.iter().any(|spec| {
            spec.ship.id == MEDIUM_SPACESHIP_ID
                && spec.ship.class == VoxelSpaceshipClass::Corvette
        }));
        assert_eq!(
            specs
                .iter()
                .map(|spec| spec.ship.id.as_str())
                .collect::<HashSet<_>>()
                .len(),
            specs.len()
        );
        assert!(specs.iter().all(|spec| {
            !spec.cells.is_empty()
                && spec
                    .cells
                    .iter()
                    .all(|(_, material)| (1..=VOXEL_MATERIAL_COUNT as u8).contains(material))
                && spec
                    .cells
                    .iter()
                    .any(|(_, material)| TrpgVoxelConnector::solid(material))
        }));
        assert!(specs[0]
            .micro_tiles
            .iter()
            .any(|tile| tile.kind == VoxelMicroTileKind::Hull));
        assert!(specs[0]
            .micro_tiles
            .iter()
            .any(|tile| tile.kind == VoxelMicroTileKind::Fixture));
        let workbook_features = specs[0]
            .workbook_features
            .as_ref()
            .expect("狂妄号 must carry its labels as it moves");
        assert_eq!(workbook_features.map_name, ARROGANCE.name);
        assert!(!workbook_features.regions.is_empty());
        assert!(specs[1..].iter().all(|spec| spec.micro_tiles.is_empty()));
        assert!(specs[1..]
            .iter()
            .all(|spec| spec.workbook_features.is_none()));
        assert!(specs[0].docking.is_none());
        assert!(specs[1..].iter().all(|spec| {
            spec.docking
                .as_ref()
                .is_some_and(|docking| docking.carrier_id == COMBAT_SPACESHIP_ID)
        }));

        let scaled_original =
            scale_combat_spaceship_cells(&original_combat_spaceship_voxel_cells());
        assert!(specs[0]
            .cells
            .iter()
            .all(|(cell, material)| scaled_original.get(cell) == Some(material)));
        let carrier_cells = specs[0]
            .cells
            .iter()
            .map(|(cell, _)| *cell)
            .collect::<HashSet<_>>();
        assert!(specs[0]
            .micro_tiles
            .iter()
            .all(|tile| carrier_cells.contains(&tile.owner)));
    }

    #[test]
    fn spawned_fleet_uses_canonical_voxels_and_docked_body_state() {
        fn spawn_test_spaceship(
            mut commands: Commands,
            mut meshes: ResMut<Assets<Mesh>>,
            materials: Res<VoxelMaterials>,
        ) {
            for spec in default_voxel_spaceship_specs() {
                spawn_voxel_spaceship(
                    &mut commands,
                    &mut meshes,
                    &materials,
                    &spec,
                    spec.transform,
                    LinearVelocity::ZERO,
                    AngularVelocity::ZERO,
                    spec.docking.clone(),
                );
            }
        }

        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .insert_resource(VoxelMaterials {
                handles: std::array::from_fn(|_| Handle::default()),
                planet_ocean: Handle::default(),
            })
            .add_systems(Update, spawn_test_spaceship);
        app.update();

        let mut query = app.world_mut().query_filtered::<(
            &VoxelSpaceship,
            &RigidBody,
            &GravityScale,
            &Collider,
            Option<&VoxelSpaceshipDocked>,
            Option<&CollisionLayers>,
        ), With<VoxelSpaceship>>();
        let bodies = query.iter(app.world()).collect::<Vec<_>>();
        assert_eq!(bodies.len(), SMALL_SPACESHIP_COUNT + 2);
        for (ship, body, gravity_scale, collider, docking, collision_layers) in bodies {
            if ship.id == COMBAT_SPACESHIP_ID {
                assert_eq!(*body, RigidBody::Dynamic);
                assert!(docking.is_none());
                assert_eq!(collision_layers.copied(), Some(carrier_collision_layers()));
            } else {
                assert_eq!(*body, RigidBody::Kinematic);
                assert!(docking.is_some());
                assert_eq!(
                    collision_layers.copied(),
                    Some(docked_voxel_spaceship_collision_layers())
                );
            }
            assert_eq!(gravity_scale.0, 0.0);
            assert_eq!(
                collider
                    .shape()
                    .as_voxels()
                    .expect("spaceship collider must retain voxel geometry")
                    .voxel_size(),
                Vec3::splat(VOXEL_SIZE)
            );
        }
        let mut entities = app
            .world_mut()
            .query_filtered::<Entity, With<VoxelSpaceship>>();
        for entity in entities.iter(app.world()) {
            assert!(
                !app.world()
                    .entity(entity)
                    .contains::<ConstantLinearAcceleration>(),
                "spaceships must not receive the walking/prop gravity acceleration"
            );
            assert!(
                !app.world()
                    .entity(entity)
                    .contains::<VoxelPlanetGravityBody>(),
                "spaceships must not enter the planet-only gravity force query"
            );
        }
        let mut micro_tiles = app
            .world_mut()
            .query_filtered::<Entity, With<VoxelMicroDecoration>>();
        assert!(micro_tiles.iter(app.world()).next().is_some());
        let mut workbook_annotations = app
            .world_mut()
            .query_filtered::<&VoxelWorkbookFeatureAnnotations, With<VoxelSpaceship>>();
        let annotations = workbook_annotations.iter(app.world()).collect::<Vec<_>>();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].map_name, ARROGANCE.name);
    }

    #[test]
    fn spaceship_driver_permissions_require_the_assigned_player_identity() {
        assert!(voxel_spaceship_driver_authorized(None, None));
        assert!(!voxel_spaceship_driver_authorized(None, Some(42)));
        assert!(voxel_spaceship_driver_authorized(Some(42), Some(42)));
        assert!(!voxel_spaceship_driver_authorized(Some(42), Some(7)));
        assert!(!voxel_spaceship_driver_authorized(Some(42), None));
    }

    #[test]
    fn authorized_spaceship_controls_apply_thrust_and_publish_the_cockpit_pose() {
        let spec = default_voxel_spaceship_specs().remove(1);
        let temporary = tempfile::tempdir().unwrap();
        let store = Persistent::<VoxelSpaceshipStore>::builder()
            .name("test_voxel_spaceship_controls")
            .format(StorageFormat::Toml)
            .path(temporary.path().join("spaceships.toml"))
            .default(VoxelSpaceshipStore {
                ships: vec![persisted_voxel_spaceship_from_spec(&spec, None)],
                layout_revision: VOXEL_SPACESHIP_LAYOUT_REVISION,
            })
            .build()
            .unwrap();
        let mut editor = VoxelEditorState::default();
        editor.first_person_cursor_released = false;
        let mut control = VoxelSpaceshipControlState::default();
        control.driving_ship_id = Some(spec.ship.id.clone());

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EguiWantsInput>()
            .insert_resource(editor)
            .init_resource::<VoxelPossessionState>()
            .insert_resource(store)
            .insert_resource(control)
            .add_systems(Update, control_voxel_spaceships);
        let entity = app
            .world_mut()
            .spawn((
                spec.ship.clone(),
                spec.transform,
                LinearVelocity::ZERO,
                AngularVelocity::ZERO,
            ))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(16));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);

        app.update();

        let velocity = app
            .world()
            .entity(entity)
            .get::<LinearVelocity>()
            .unwrap()
            .0;
        assert!(velocity.dot(*spec.transform.forward()) > 0.0);
        assert!(
            app.world()
                .resource::<VoxelSpaceshipControlState>()
                .cockpit_eye
                .is_some()
        );
    }
}
