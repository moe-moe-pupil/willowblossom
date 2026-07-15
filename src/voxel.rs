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
};

use avian3d::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
    camera::RenderTarget,
    core_pipeline::prepass::DepthPrepass,
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
    math::Affine2,
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
        NapcatIOSender,
        NapcatMessageManager,
        NapcatOutboundMessage,
    },
    scene::SceneCaptureRequests,
    voxel_radiance::{
        VoxelRadianceCascade,
        VoxelRadianceCascadePlugin,
        VoxelRadianceCascadeUniform,
    },
};

const VOXEL_SIZE: f32 = 0.25;
const MAX_RAY_DISTANCE: f32 = 200.0;
const EDIT_REPEAT_DELAY: f32 = 0.32;
const EDIT_REPEAT_INTERVAL: f32 = 0.09;
const TRPG_PHYSICS_SUBSTEPS: u32 = 2;
// 48.5 LOD cells at 19 canonical voxels each keeps the distant shell and
// refined 0.25-cell surface on exactly the same boundary.
const ORBITAL_PLANET_RADIUS: f32 = 230.375;
const ORBITAL_PLANET_CENTER: Vec3 = Vec3::new(0.0, -360.0, 0.0);
const ORBITAL_PLANET_VOXEL_RADIUS: i32 = 48;
const ORBITAL_PLANET_SHELL_THICKNESS: f32 = 2.25;
const ORBITAL_PLANET_LOD_SUBDIVISIONS: i32 = 19;
const ORBITAL_PLANET_LOD_VOXEL_SIZE: f32 = VOXEL_SIZE * ORBITAL_PLANET_LOD_SUBDIVISIONS as f32;
const MAX_SCENE_SNAPSHOTS: usize = 20;
const MAX_EXPLOSION_NEW_PHYSICS_BODIES: usize = 40;
const VOXEL_MATERIAL_COUNT: usize = 10;
const VOXEL_EMISSIVE_SCALE: f32 = 0.3;
const VOXEL_RADIANCE_MAX_DIMENSION: i32 = 192;
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
const ORBITAL_LAYOUT_SCALE: i32 = 5;
const RESEARCH_STATION_CENTER: IVec3 = IVec3::new(-100 * ORBITAL_LAYOUT_SCALE, 0, 0);
const SENSOR_STATION_CENTER: IVec3 = IVec3::new(100 * ORBITAL_LAYOUT_SCALE, 0, 0);
const CANNON_STATION_CENTER: IVec3 = IVec3::new(0, 0, -150 * ORBITAL_LAYOUT_SCALE);
const COMBAT_SPACESHIP_CENTER: IVec3 = IVec3::new(0, 0, 150 * ORBITAL_LAYOUT_SCALE);
const FIRST_PERSON_START: Vec3 = Vec3::new(
    -6.5,
    0.5,
    (COMBAT_SPACESHIP_CENTER.z as f32 + 15.0) * VOXEL_SIZE,
);
const DEFAULT_SCENE_CAMERA_FOCUS: Vec3 = Vec3::new(
    0.0,
    2.5,
    COMBAT_SPACESHIP_CENTER.z as f32 * VOXEL_SIZE,
);
const DEFAULT_SCENE_CAMERA_DISTANCE: f32 = 50.0;
const PLAYER_CAPTURE_WIDTH: u32 = 1024;
const PLAYER_CAPTURE_HEIGHT: u32 = 768;
const PLAYER_CAPTURE_PREPARE_FRAMES: u8 = 3;
const PLAYER_STANDEE_HEIGHT: f32 = FIRST_PERSON_BODY_LENGTH + FIRST_PERSON_RADIUS * 2.0;
const TOOL_GUN_DRAG_RESPONSE: f32 = 12.0;
const TOOL_GUN_DRAG_MAX_SPEED: f32 = 24.0;
const PLANET_AUTO_REFINEMENTS_PER_FRAME: usize = 8;

pub struct TrpgVoxelPlugin;

pub struct TrpgVoxelConnector;

fn voxel_emissive(red: f32, green: f32, blue: f32) -> LinearRgba {
    LinearRgba::rgb(
        red * VOXEL_EMISSIVE_SCALE,
        green * VOXEL_EMISSIVE_SCALE,
        blue * VOXEL_EMISSIVE_SCALE,
    )
}

impl Connector for TrpgVoxelConnector {
    type Item = u8;

    fn solid(voxel: &Self::Item) -> bool { matches!(*voxel, 1..=3 | 6..=10) }
}

#[derive(Component)]
pub struct TrpgVoxelGrid;

#[derive(Component)]
struct VoxelViewportCamera;

#[derive(Component)]
struct VoxelPlayerCaptureCamera {
    user_id: u64,
}

#[derive(Component)]
struct VoxelPlayerStandee {
    user_id: u64,
    image_source: String,
}

#[derive(Resource, Default)]
struct VoxelPlayerStandeeAssets {
    entities: HashMap<u64, Entity>,
    textures: HashMap<String, Handle<Image>>,
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
struct VoxelPlayerCameraStore {
    cameras: Vec<PersistedVoxelPlayerCamera>,
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
    camera_entity: Entity,
    target: Handle<Image>,
    output_path: std::path::PathBuf,
    prepare_frames_remaining: u8,
    activated: bool,
    hidden_standees: Vec<(Entity, Visibility)>,
}

#[derive(Component)]
struct VoxelGeometry;

#[derive(Component, Clone)]
struct VoxelPhysicsBody {
    local_center: Vec3,
    cells: Vec<(IVec3, u8)>,
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

#[derive(Component)]
struct VoxelOrbitalPlanet {
    lod_cells: HashMap<IVec3, u8>,
    refined_lod_cells: HashSet<IVec3>,
    cells: HashMap<IVec3, u8>,
    removed: HashSet<IVec3>,
    mesh_entities: Vec<Entity>,
    mesh_handles: Vec<Handle<Mesh>>,
    voxel_size: f32,
    dirty: bool,
    auto_refine_pending: bool,
}

#[derive(Clone, Copy)]
struct VoxelPlanetRayHit {
    occupied: IVec3,
    normal: IVec3,
    distance: f32,
    lod: bool,
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
    edit_hold_seconds: f32,
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
    first_person_space_tap_elapsed: f32,
    first_person_was_enabled: bool,
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
            first_person_enabled: false,
            first_person_flying: false,
            first_person_speed: FIRST_PERSON_SPEED,
            creative_inventory_open: false,
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
            edit_hold_seconds: 0.0,
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
            first_person_space_tap_elapsed: f32::INFINITY,
            first_person_was_enabled: false,
        }
    }
}

impl VoxelEditorState {
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
        app.add_plugins((
            PhysicsPlugins::default(),
            VoxelPlugin::<u8>::default(),
            ConnectivityPlugin::<TrpgVoxelConnector>::default(),
            VoxelRadianceCascadePlugin,
        ))
        // Avian defaults to six substeps. Two is sufficient for this creative TRPG scene and
        // avoids repeating the solver six times when an explosion creates many fragments.
        .insert_resource(SubstepCount(TRPG_PHYSICS_SUBSTEPS))
        .insert_resource(Gravity::ZERO)
        .init_resource::<VoxelEditorState>()
        .init_resource::<VoxelRadianceVolume>()
        .init_resource::<SceneCaptureRequests>()
        .init_resource::<VoxelPlayerCameraRuntimes>()
        .init_resource::<VoxelPlayerCameraEditor>()
        .init_resource::<VoxelPlayerCaptureState>()
        .init_resource::<VoxelPlayerStandeeAssets>()
        .init_resource::<VoxelToolGunDragState>()
        .insert_resource(player_camera_store)
        .insert_resource(inventory_store)
        .add_systems(
            Startup,
            (
                load_voxel_inventory,
                setup_voxel_materials,
                setup_voxel_grid,
                populate_voxel_grid,
                setup_voxel_radiance_volume,
                setup_voxel_auto_doors,
                setup_voxel_interior_lights,
                setup_voxel_sample_props,
                setup_voxel_view,
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
                    place_creative_light,
                    sync_selected_voxel_light,
                    refine_visible_planet_voxels,
                    edit_voxel_grid,
                    drag_voxel_physics_body,
                    make_selection_physical,
                    rebuild_voxel_orbital_planet,
                    apply_voxel_physics_action,
                    process_voxel_scene_history,
                )
                    .chain(),
                (
                    animate_voxel_auto_doors,
                    rebuild_voxel_geometry,
                    sync_voxel_radiance_volume,
                    sync_voxel_lighting,
                    control_first_person_player,
                    control_voxel_camera,
                    sync_voxel_player_cameras,
                    sync_voxel_player_standees,
                    capture_voxel_player_view,
                    draw_voxel_target,
                    animate_voxel_materials,
                    persist_voxel_inventory,
                )
                    .chain(),
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, voxel_player_camera_panel);
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

fn voxel_editor_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    mut editor: ResMut<VoxelEditorState>,
) {
    if egui_input.wants_any_keyboard_input() {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyE) {
        editor.creative_inventory_open = !editor.creative_inventory_open;
    }
    if keyboard.just_pressed(KeyCode::KeyR)
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
            editor.select_hotbar_slot(slot);
        }
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
        } else {
            StandardMaterial {
                base_color: Color::srgb(0.46, 0.12, 0.018),
                emissive: voxel_emissive(0.42, 0.075, 0.008),
                metallic: 0.62,
                perceptual_roughness: 0.32,
                ..default()
            }
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
        materials.add(material)
    });
    let planet_ocean = materials.add(opaque_planet_ocean_material(
        textures[3].clone(),
    ));
    commands.insert_resource(VoxelMaterials {
        handles,
        planet_ocean,
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
        // Alpha stores occupancy for visibility. RGB stores emitted radiance,
        // not albedo, so ordinary walls do not incorrectly cast brown light.
        5 => [255, 72, 8, 255],
        8 => [34, 176, 220, 255],
        10 => [196, 78, 18, 255],
        _ if TrpgVoxelConnector::solid(&material) => [0, 0, 0, 255],
        _ => [0, 0, 0, 0],
    }
}

fn build_voxel_radiance_image(grid: &Grid<u8>) -> (Image, Vec3, f32, Vec3) {
    let solid_cells = grid
        .iter()
        .flat_map(|(chunk_position, chunk)| {
            prism(IVec3::ZERO, DIMS).filter_map(move |local| {
                let material = chunk[local];
                TrpgVoxelConnector::solid(&material)
                    .then_some((*chunk_position * DIMS + local, material))
            })
        })
        .collect::<Vec<_>>();
    let min = solid_cells
        .iter()
        .map(|(cell, _)| *cell)
        .reduce(IVec3::min)
        .unwrap_or(IVec3::ZERO);
    let max = solid_cells
        .iter()
        .map(|(cell, _)| *cell)
        .reduce(IVec3::max)
        .unwrap_or(IVec3::ZERO);
    let extent = max - min + IVec3::ONE;
    let stride = ((extent.max_element() + VOXEL_RADIANCE_MAX_DIMENSION - 1)
        / VOXEL_RADIANCE_MAX_DIMENSION)
        .max(1);
    let dimensions = (extent + IVec3::splat(stride - 1)) / stride;
    let texel_count = dimensions.x as usize * dimensions.y as usize * dimensions.z as usize;
    let mut data = vec![0; texel_count * 4];
    for (cell, material) in solid_cells {
        let local = (cell - min) / stride;
        let index = (local.x + dimensions.x * (local.y + dimensions.y * local.z)) as usize * 4;
        let color = radiance_voxel_color(material);
        let old_energy = data[index] as u16 + data[index + 1] as u16 + data[index + 2] as u16;
        let new_energy = color[0] as u16 + color[1] as u16 + color[2] as u16;
        if data[index + 3] == 0 || new_energy >= old_energy {
            data[index..index + 4].copy_from_slice(&color);
        }
    }
    let size = Extent3d {
        width: dimensions.x.max(1) as u32,
        height: dimensions.y.max(1) as u32,
        depth_or_array_layers: dimensions.z.max(1) as u32,
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
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    });
    (
        image,
        min.as_vec3() * VOXEL_SIZE,
        VOXEL_SIZE * stride as f32,
        dimensions.as_vec3(),
    )
}

fn setup_voxel_radiance_volume(
    grids: Query<&Grid<u8>, With<TrpgVoxelGrid>>,
    mut images: ResMut<Assets<Image>>,
    mut volume: ResMut<VoxelRadianceVolume>,
) {
    let Ok(grid) = grids.single() else {
        return;
    };
    let (image, volume_min, voxel_world_size, volume_dimensions) = build_voxel_radiance_image(grid);
    volume.image = images.add(image);
    volume.volume_min = volume_min;
    volume.voxel_world_size = voxel_world_size;
    volume.volume_dimensions = volume_dimensions;
}

fn sync_voxel_radiance_volume(
    grids: Query<&Grid<u8>, (With<TrpgVoxelGrid>, Changed<Grid<u8>>)>,
    editor: Res<VoxelEditorState>,
    mut images: ResMut<Assets<Image>>,
    mut volume: ResMut<VoxelRadianceVolume>,
    mut cameras: Query<&mut VoxelRadianceCascadeUniform, With<VoxelViewportCamera>>,
) {
    let Ok(grid) = grids.single() else {
        return;
    };
    let (image, volume_min, voxel_world_size, volume_dimensions) = build_voxel_radiance_image(grid);
    if images.contains(&volume.image) {
        *images.get_mut(&volume.image).unwrap() = image;
    } else {
        volume.image = images.add(image);
    }
    volume.volume_min = volume_min;
    volume.voxel_world_size = voxel_world_size;
    volume.volume_dimensions = volume_dimensions;
    let uniform = volume.uniform(editor.radiance_intensity);
    for mut camera_uniform in &mut cameras {
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

fn populate_default_grid(grid: &mut Mut<Grid<u8>>) {
    build_space_station(grid, RESEARCH_STATION_CENTER, false);
    build_space_station(grid, SENSOR_STATION_CENTER, true);
    build_space_cannon_station(grid, CANNON_STATION_CENTER);
    build_combat_spaceship(grid);
    for door in voxel_auto_doors() {
        for cell in door.cells {
            grid.set(cell, 0);
        }
    }
}

fn build_space_cannon_station(grid: &mut Mut<Grid<u8>>, center: IVec3) {
    // A compact fortified station with three playable decks and a central fire-control chamber.
    build_hollow_voxel_room(
        grid,
        center + IVec3::new(-45, 0, -28),
        center + IVec3::new(45, 30, 28),
        7,
        2,
    );
    for deck_y in [0, 10, 20, 30] {
        fill_voxel_box(
            grid,
            center + IVec3::new(-44, deck_y, -27),
            center + IVec3::new(44, deck_y, 27),
            2,
        );
    }
    for deck_y in [0, 10, 20] {
        for local_x in [-20, 20] {
            fill_voxel_box(
                grid,
                center + IVec3::new(local_x, deck_y + 1, -27),
                center + IVec3::new(local_x, deck_y + 9, 27),
                7,
            );
            clear_voxel_box(
                grid,
                center + IVec3::new(local_x, deck_y + 1, -4),
                center + IVec3::new(local_x, deck_y + 7, 4),
            );
        }
        fill_voxel_box(
            grid,
            center + IVec3::new(-44, deck_y + 1, 0),
            center + IVec3::new(44, deck_y + 9, 0),
            7,
        );
        clear_voxel_box(
            grid,
            center + IVec3::new(-4, deck_y + 1, 0),
            center + IVec3::new(4, deck_y + 7, 0),
        );
    }

    // Reactor vaults, ammunition stores, and an illuminated firing-control dais.
    for side in [-1, 1] {
        let reactor_center = center + IVec3::new(side * 32, 1, 14);
        build_hollow_voxel_room(
            grid,
            reactor_center + IVec3::new(-8, 0, -7),
            reactor_center + IVec3::new(8, 8, 7),
            6,
            2,
        );
        for y in 2..=7 {
            for z in -4..=4 {
                grid.set(reactor_center + IVec3::new(0, y, z), 8);
            }
        }
        for z in (-22..=-8).step_by(4) {
            fill_voxel_box(
                grid,
                center + IVec3::new(side * 33 - 2, 11, z),
                center + IVec3::new(side * 33 + 2, 14, z + 2),
                9,
            );
        }
    }
    fill_voxel_box(
        grid,
        center + IVec3::new(-10, 21, -20),
        center + IVec3::new(10, 22, -8),
        6,
    );
    for x in (-8..=8).step_by(4) {
        grid.set(center + IVec3::new(x, 23, -12), 8);
        grid.set(center + IVec3::new(x, 23, -16), 10);
    }

    // The spinal spatial-artillery barrel leaves the aft hull and tapers toward a bright muzzle.
    let axis_y = center.y + 22;
    let hull_back_z = center.z - 28;
    for z in center.z - 112..=hull_back_z {
        let distance = hull_back_z - z;
        let radius = (12 - distance / 16).clamp(6, 12);
        for x in -radius..=radius {
            for y in -radius..=radius {
                let edge = x.abs().max(y.abs()) == radius;
                let brace = (z - center.z).rem_euclid(10) == 0
                    && (x.abs() == radius - 1 || y.abs() == radius - 1);
                let energy_rail = (x == 0 && y.abs() == radius) || (y == 0 && x.abs() == radius);
                if edge || brace || energy_rail {
                    let material = if energy_rail || (z - center.z).rem_euclid(20) == 0 {
                        8
                    } else if brace {
                        9
                    } else {
                        6
                    };
                    grid.set(
                        IVec3::new(center.x + x, axis_y + y, z),
                        material,
                    );
                }
            }
        }
    }
    let muzzle_z = center.z - 112;
    for z in muzzle_z - 4..=muzzle_z + 4 {
        for x in -15_i32..=15 {
            for y in -15_i32..=15 {
                if x.abs().max(y.abs()) >= 12 {
                    grid.set(
                        IVec3::new(center.x + x, axis_y + y, z),
                        if z == muzzle_z || x.abs().max(y.abs()) == 15 { 9 } else { 6 },
                    );
                }
            }
        }
    }
    for z in muzzle_z - 3..=muzzle_z + 3 {
        grid.set(IVec3::new(center.x, axis_y, z), 8);
    }

    // A sensor crown and targeting vanes distinguish the cannon station at long range.
    build_hollow_voxel_room(
        grid,
        center + IVec3::new(-14, 31, -10),
        center + IVec3::new(14, 43, 10),
        6,
        2,
    );
    for y in 44..=58 {
        grid.set(center + IVec3::new(0, y, 0), 7);
        if y % 4 == 0 {
            for x in -12..=12 {
                grid.set(center + IVec3::new(x, y, 0), 8);
            }
        }
    }
}

fn build_space_station(grid: &mut Mut<Grid<u8>>, center: IVec3, command_station: bool) {
    let min = center + IVec3::new(-50, 0, -40);
    let max = center + IVec3::new(50, 36, 40);
    build_hollow_voxel_room(grid, min, max, 6, 2);

    // Five full decks make each station over 100 times larger by usable floor area.
    for deck_y in [0, 9, 18, 27, 36] {
        fill_voxel_box(
            grid,
            center + IVec3::new(-49, deck_y, -39),
            center + IVec3::new(49, deck_y, 39),
            2,
        );
    }

    // Room partitions, broad passages, lift shafts, and deck-specific color bands.
    for deck_y in [0, 9, 18, 27] {
        for local_x in [-25, 0, 25] {
            for y in deck_y + 1..deck_y + 9 {
                for z in -39..=39 {
                    if !(-4..=4).contains(&z) && !(-30..=-24).contains(&z) {
                        grid.set(center + IVec3::new(local_x, y, z), 7);
                    }
                }
            }
        }
        for local_z in [-20, 20] {
            for y in deck_y + 1..deck_y + 9 {
                for x in -49..=49 {
                    if !(-4..=4).contains(&x) && !(-38..=-32).contains(&x) {
                        grid.set(center + IVec3::new(x, y, local_z), 7);
                    }
                }
            }
        }
        clear_voxel_box(
            grid,
            center + IVec3::new(-3, deck_y, -3),
            center + IVec3::new(3, deck_y + 9, 3),
        );
        let band_material = if (deck_y / 9 + command_station as i32) % 2 == 0 { 8 } else { 7 };
        for x in (-45..=45).step_by(5) {
            grid.set(
                center + IVec3::new(x, deck_y + 2, -38),
                band_material,
            );
            grid.set(
                center + IVec3::new(x, deck_y + 2, 38),
                band_material,
            );
        }
    }

    // Large observation crown and a hollow docking corridor facing the center.
    build_hollow_voxel_room(
        grid,
        center + IVec3::new(-26, 37, -26),
        center + IVec3::new(26, 58, 26),
        6,
        2,
    );
    for y in [45, 52] {
        fill_voxel_box(
            grid,
            center + IVec3::new(-25, y, -25),
            center + IVec3::new(25, y, 25),
            2,
        );
    }
    clear_voxel_box(
        grid,
        center + IVec3::new(-3, 37, -3),
        center + IVec3::new(3, 58, 3),
    );
    let dock_min_x = if center.x < 0 { center.x + 50 } else { center.x - 80 };
    let dock_max_x = if center.x < 0 { center.x + 80 } else { center.x - 50 };
    build_hollow_voxel_room(
        grid,
        IVec3::new(dock_min_x, 0, center.z - 10),
        IVec3::new(dock_max_x, 12, center.z + 10),
        6,
        2,
    );

    // Gardens, command consoles, cargo racks, reactors, antennae, and hull ribs.
    for x in -45..=-28 {
        for z in -34..=-12 {
            grid.set(center + IVec3::new(x, 1, z), 1);
        }
    }
    let accent = if command_station { 8 } else { 7 };
    for deck_y in [1, 10, 19, 28, 38, 46, 53] {
        for z in (-32..=32).step_by(4) {
            for x in [-43, -42, 42, 43] {
                grid.set(
                    center + IVec3::new(x, deck_y, z),
                    accent,
                );
            }
        }
    }
    for deck_y in [1, 10, 19, 28] {
        for x in [-36, -12, 12, 36] {
            for z in [-30, -10, 10, 30] {
                fill_voxel_box(
                    grid,
                    center + IVec3::new(x - 1, deck_y, z - 1),
                    center + IVec3::new(x + 1, deck_y + 1, z + 1),
                    if (x + z) % 3 == 0 { 4 } else { accent },
                );
            }
        }
    }
    for x in [30, 36, 42] {
        for z in [-28, -14, 14, 28] {
            for y in 1..=7 {
                grid.set(center + IVec3::new(x, y, z), 5);
            }
        }
    }
    for x in (-48..=48).step_by(8) {
        for y in 0..=38 {
            grid.set(center + IVec3::new(x, y, -41), 7);
            grid.set(center + IVec3::new(x, y, 41), 7);
        }
    }
    for y in 59..=78 {
        grid.set(center + IVec3::new(0, y, 0), 7);
        if y % 4 == 0 {
            for x in -8..=8 {
                grid.set(center + IVec3::new(x, y, 0), 8);
            }
        }
    }
}

fn build_combat_spaceship(grid: &mut Mut<Grid<u8>>) {
    for x in -28..=28 {
        for y in -5..=25 {
            for z in -80..=80 {
                let local = IVec3::new(x, y, z);
                if let Some(material) = combat_corvette_voxel(local) {
                    grid.set(
                        COMBAT_SPACESHIP_CENTER + local,
                        material,
                    );
                }
            }
        }
    }
}

fn combat_corvette_voxel(position: IVec3) -> Option<u8> {
    const FLOOR: u8 = 2;
    const METAL: u8 = 3;
    const RED: u8 = 9;
    const HULL: u8 = 6;
    const DARK: u8 = 7;
    const CYAN: u8 = 8;

    let (x, y, z) = (position.x, position.y, position.z);
    let mut material = None;
    let half_width = corvette_half_width(z);
    let roof_y = corvette_roof_y(z);
    let vertical_inset = if y <= 1 || y >= roof_y - 1 {
        2
    } else if y <= 3 || y >= roof_y - 3 {
        1
    } else {
        0
    };
    let section_width = (half_width - vertical_inset).max(2);

    if (-80..=80).contains(&z) && (0..=roof_y).contains(&y) && x.abs() <= section_width {
        let outer_shell = x.abs() == section_width || y == 0 || y == roof_y;
        let deck = matches!(y, 7 | 14) && y < roof_y && x.abs() < section_width;
        if outer_shell || deck {
            material = Some(if y == 0 || deck { FLOOR } else { HULL });
        }
    }

    // Six room divisions with broad doors through every deck that reaches the section.
    for bulkhead_z in [-55, -35, -10, 15, 38, 55] {
        let doorway = x.abs() <= 2 && matches!(y, 1..=5 | 8..=12 | 15..=19);
        if z == bulkhead_z && x.abs() < half_width - 1 && (1..roof_y).contains(&y) && !doorway {
            material = Some(DARK);
        }
    }

    // Thin, tapered armor rails replace the previous solid rectangular side slabs.
    if (-24..=34).contains(&z) {
        let wing_reach = 22 + (12 - (z - 5).abs()).max(0) / 4;
        for side in [-1, 1] {
            let side_x = x * side;
            let on_rail = (21..=wing_reach).contains(&side_x)
                && (4..=9).contains(&y)
                && (matches!(y, 4 | 9)
                    || matches!(side_x, 21)
                    || side_x == wing_reach
                    || (z - 5).rem_euclid(10) == 0);
            if on_rail {
                material = Some(if y == 9 && z.rem_euclid(12) < 5 { RED } else { DARK });
            }
        }
    }

    // Port-side airlock vestibule joins the hull and frames the automatic outer door.
    if voxel_point_in_hollow_box(
        position,
        IVec3::new(-24, 0, 10),
        IVec3::new(-18, 7, 20),
    ) {
        material = Some(DARK);
    }
    if x == -18 && (1..=5).contains(&y) && (12..=18).contains(&z) {
        material = None;
    }

    // Raised red armor bands and cyan side ports follow the tapered hull instead of flattening it.
    for side in [-1, 1] {
        if x == side * (section_width + 1)
            && matches!(y, 3 | 4 | 17)
            && matches!(z, -48..=-34 | -22..=-8 | 5..=18 | 29..=42)
        {
            material = Some(RED);
        }
        if x == side * section_width
            && (9..=11).contains(&y)
            && matches!(z, -42..=-38 | -20..=-16 | 3..=7 | 25..=29 | 45..=49)
        {
            material = Some(CYAN);
        }
    }

    // Broad top plates provide large red fields instead of repetitive glowing stripes.
    if y == roof_y + 1 && x.abs() <= (half_width - 9).max(1) {
        if matches!(z, -44..=-34 | -16..=-7 | 11..=19 | 34..=41) {
            material = Some(RED);
        } else if matches!(z, -55..=-50 | -27..=-23 | 24..=28 | 46..=50) {
            material = Some(HULL);
        }
    }

    // Three long engine nacelles with layered casings, cyan bells, and red drive cores.
    for engine_x in [-16, 0, 16] {
        let dx = (x - engine_x).abs();
        if dx <= 4 && (4..=12).contains(&y) && (-80..=-58).contains(&z) {
            let casing = dx == 4
                || matches!(y, 4 | 12)
                || z == -58
                || (z + 80).rem_euclid(6) == 0 && dx >= 3;
            if casing {
                material = Some(DARK);
            }
            if z <= -72 && dx <= 2 && (6..=10).contains(&y) {
                material = Some(if z == -80 { CYAN } else { RED });
            }
            if z == -61 && dx <= 3 && (6..=10).contains(&y) {
                material = Some(HULL);
            }
        }
    }

    // Layered dorsal command spine, sensor mast, twin turret, and forward gun rails.
    if voxel_point_in_hollow_box(
        position,
        IVec3::new(-7, 22, -18),
        IVec3::new(7, 23, 20),
    ) {
        material = Some(HULL);
    }
    if voxel_point_in_box(
        position,
        IVec3::new(-4, 24, -7),
        IVec3::new(4, 25, 7),
    ) {
        material = Some(DARK);
    }
    if voxel_point_in_box(
        position,
        IVec3::new(-1, 23, -28),
        IVec3::new(1, 25, -9),
    ) || voxel_point_in_box(
        position,
        IVec3::new(-1, 23, 7),
        IVec3::new(1, 25, 30),
    ) {
        material = Some(RED);
    }
    for gun_x in [-5, 5] {
        if voxel_point_in_box(
            position,
            IVec3::new(gun_x - 1, 22, 18),
            IVec3::new(gun_x + 1, 24, 30),
        ) {
            material = Some(DARK);
        }
    }
    if x == 0 && y == 25 && (-2..=2).contains(&z) {
        material = Some(CYAN);
    }

    // Four landing struts and broad feet make the ship read as a vehicle rather than a building.
    for leg_x in [-13, 13] {
        for leg_z in [-28, 22] {
            if (x - leg_x).abs() <= 1 && z == leg_z && (-4..=-1).contains(&y) {
                material = Some(DARK);
            }
            if (x - leg_x).abs() <= 3 && (z - leg_z).abs() <= 2 && y == -5 {
                material = Some(HULL);
            }
        }
    }

    // Tapered panoramic bridge glazing and inset pilot consoles in the forward middle deck.
    if z >= 68
        && x.abs() <= section_width
        && (6..=9).contains(&y)
        && ((z == 80 && x.abs() <= 1) || x.abs() == section_width)
    {
        material = Some(CYAN);
    }

    // Playable room dressing and tactical cover, kept off the central circulation route.
    for (min, max, prop) in [
        (
            IVec3::new(-2, 1, -52),
            IVec3::new(2, 5, -44),
            CYAN,
        ),
        (
            IVec3::new(-13, 1, -51),
            IVec3::new(-9, 4, -45),
            RED,
        ),
        (
            IVec3::new(9, 1, -51),
            IVec3::new(13, 4, -45),
            RED,
        ),
        (
            IVec3::new(-15, 1, -31),
            IVec3::new(-10, 4, -25),
            METAL,
        ),
        (
            IVec3::new(9, 1, -30),
            IVec3::new(15, 3, -23),
            METAL,
        ),
        (
            IVec3::new(-15, 1, -18),
            IVec3::new(-10, 3, -12),
            HULL,
        ),
        (
            IVec3::new(10, 1, -17),
            IVec3::new(15, 4, -12),
            HULL,
        ),
        (
            IVec3::new(-17, 2, -3),
            IVec3::new(-15, 5, 7),
            DARK,
        ),
        (
            IVec3::new(-16, 8, -31),
            IVec3::new(-11, 9, -25),
            HULL,
        ),
        (
            IVec3::new(-16, 11, -31),
            IVec3::new(-11, 12, -25),
            HULL,
        ),
        (
            IVec3::new(-16, 8, -22),
            IVec3::new(-11, 9, -16),
            HULL,
        ),
        (
            IVec3::new(-16, 11, -22),
            IVec3::new(-11, 12, -16),
            HULL,
        ),
        (
            IVec3::new(9, 8, -29),
            IVec3::new(15, 9, -22),
            METAL,
        ),
        (
            IVec3::new(11, 8, -5),
            IVec3::new(16, 12, -3),
            DARK,
        ),
        (
            IVec3::new(-16, 8, -5),
            IVec3::new(-12, 11, 5),
            DARK,
        ),
        (
            IVec3::new(-14, 15, -8),
            IVec3::new(-10, 17, -2),
            HULL,
        ),
        (
            IVec3::new(10, 15, -8),
            IVec3::new(14, 17, -2),
            HULL,
        ),
        (
            IVec3::new(-6, 15, 20),
            IVec3::new(6, 16, 27),
            METAL,
        ),
        (
            IVec3::new(-14, 15, 31),
            IVec3::new(-9, 17, 36),
            DARK,
        ),
        (
            IVec3::new(9, 15, 31),
            IVec3::new(14, 17, 36),
            DARK,
        ),
        (
            IVec3::new(-9, 8, 52),
            IVec3::new(-4, 10, 60),
            CYAN,
        ),
        (
            IVec3::new(4, 8, 52),
            IVec3::new(9, 10, 60),
            CYAN,
        ),
        (
            IVec3::new(-2, 8, 64),
            IVec3::new(2, 9, 69),
            DARK,
        ),
    ] {
        if voxel_point_in_box(position, min, max) {
            material = Some(prop);
        }
    }

    // Two ladder/lift trunks provide vertical routes without blocking the main corridor.
    for trunk_z in [-36, 32] {
        if voxel_point_in_box(
            position,
            IVec3::new(4, 1, trunk_z),
            IVec3::new(5, 20, trunk_z + 1),
        ) && y % 2 == 1
        {
            material = Some(DARK);
        }
    }

    material
}

fn corvette_half_width(z: i32) -> i32 {
    if z >= 20 {
        20 - (z - 20) * 18 / 60
    } else if z <= -52 {
        14 + (z + 80) * 6 / 28
    } else {
        20
    }
    .clamp(2, 20)
}

fn corvette_roof_y(z: i32) -> i32 {
    if z >= 60 {
        12
    } else if z >= 42 {
        16
    } else if z <= -62 {
        18
    } else {
        22
    }
}

fn voxel_point_in_box(position: IVec3, min: IVec3, max: IVec3) -> bool {
    let normalized_min = min.min(max);
    let normalized_max = min.max(max);
    position.cmpge(normalized_min).all() && position.cmple(normalized_max).all()
}

fn voxel_point_in_hollow_box(position: IVec3, min: IVec3, max: IVec3) -> bool {
    let normalized_min = min.min(max);
    let normalized_max = min.max(max);
    voxel_point_in_box(position, normalized_min, normalized_max)
        && (position.x == normalized_min.x
            || position.x == normalized_max.x
            || position.y == normalized_min.y
            || position.y == normalized_max.y
            || position.z == normalized_min.z
            || position.z == normalized_max.z)
}

fn fill_voxel_box(grid: &mut Mut<Grid<u8>>, min: IVec3, max: IVec3, material: u8) {
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                grid.set(IVec3::new(x, y, z), material);
            }
        }
    }
}

fn clear_voxel_box(grid: &mut Mut<Grid<u8>>, min: IVec3, max: IVec3) {
    fill_voxel_box(grid, min, max, 0);
}

fn build_hollow_voxel_room(
    grid: &mut Mut<Grid<u8>>,
    min: IVec3,
    max: IVec3,
    wall_material: u8,
    floor_material: u8,
) {
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let on_boundary = x == min.x
                    || x == max.x
                    || y == min.y
                    || y == max.y
                    || z == min.z
                    || z == max.z;
                if on_boundary {
                    grid.set(
                        IVec3::new(x, y, z),
                        if y == min.y { floor_material } else { wall_material },
                    );
                }
            }
        }
    }
}

fn voxel_auto_doors() -> Vec<VoxelAutoDoor> {
    let mut doors = vec![
        make_voxel_auto_door(
            RESEARCH_STATION_CENTER + IVec3::new(50, 0, 0),
            IVec3::Z,
            5,
            8,
            3.5,
        ),
        make_voxel_auto_door(
            SENSOR_STATION_CENTER + IVec3::new(-50, 0, 0),
            IVec3::Z,
            5,
            8,
            3.5,
        ),
        make_voxel_auto_door(
            COMBAT_SPACESHIP_CENTER + IVec3::new(-24, 0, 15),
            IVec3::Z,
            3,
            5,
            3.5,
        ),
    ];
    for station_x in [RESEARCH_STATION_CENTER.x, SENSOR_STATION_CENTER.x] {
        for deck_y in [0, 9, 18, 27] {
            for local_x in [-25, 25] {
                doors.push(make_voxel_auto_door(
                    IVec3::new(station_x + local_x, deck_y, 0),
                    IVec3::Z,
                    4,
                    7,
                    1.75,
                ));
            }
            for local_z in [-20, 20] {
                doors.push(make_voxel_auto_door(
                    IVec3::new(station_x, deck_y, local_z),
                    IVec3::X,
                    4,
                    7,
                    1.75,
                ));
            }
        }
    }
    for local_z in [-55, -35, -10, 15, 38, 55] {
        for deck_y in [0, 7, 14] {
            doors.push(make_voxel_auto_door(
                COMBAT_SPACESHIP_CENTER + IVec3::new(0, deck_y, local_z),
                IVec3::X,
                2,
                5,
                1.75,
            ));
        }
    }
    for deck_y in [0, 10, 20] {
        for local_x in [-20, 20] {
            doors.push(make_voxel_auto_door(
                CANNON_STATION_CENTER + IVec3::new(local_x, deck_y, 0),
                IVec3::Z,
                4,
                7,
                1.75,
            ));
        }
        doors.push(make_voxel_auto_door(
            CANNON_STATION_CENTER + IVec3::new(0, deck_y, 0),
            IVec3::X,
            4,
            7,
            1.75,
        ));
    }
    for local_z in [-28, 28] {
        doors.push(make_voxel_auto_door(
            CANNON_STATION_CENTER + IVec3::new(0, 0, local_z),
            IVec3::X,
            4,
            7,
            2.5,
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
    for door in voxel_auto_doors() {
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

fn voxel_interior_lights() -> Vec<(Vec3, Color)> {
    let mut lights = Vec::new();
    for (station_x, color) in [
        (
            RESEARCH_STATION_CENTER.x,
            Color::srgb(0.55, 0.75, 1.0),
        ),
        (
            SENSOR_STATION_CENTER.x,
            Color::srgb(1.0, 0.72, 0.42),
        ),
    ] {
        for deck_y in [0, 9, 18, 27] {
            for x in [-28, 28] {
                for z in [-22, 22] {
                    lights.push((
                        (Vec3::new(
                            (station_x + x) as f32,
                            (deck_y + 7) as f32,
                            z as f32,
                        ) + Vec3::splat(0.5))
                            * VOXEL_SIZE,
                        color,
                    ));
                }
            }
        }
        for y in [43, 51, 56] {
            for x in [-12, 12] {
                lights.push((
                    (Vec3::new((station_x + x) as f32, y as f32, 0.0) + Vec3::splat(0.5))
                        * VOXEL_SIZE,
                    color,
                ));
            }
        }
    }
    for deck_y in [0, 7, 14] {
        for local_z in [-45, 0, 55] {
            for x in [-10, 10] {
                lights.push((
                    (COMBAT_SPACESHIP_CENTER.as_vec3()
                        + Vec3::new(
                            x as f32,
                            (deck_y + 5) as f32,
                            local_z as f32,
                        )
                        + Vec3::splat(0.5))
                        * VOXEL_SIZE,
                    if deck_y == 0 {
                        Color::srgb(0.3, 0.85, 1.0)
                    } else {
                        Color::srgb(1.0, 0.66, 0.42)
                    },
                ));
            }
        }
    }
    for deck_y in [0, 10, 20] {
        for x in [-32, 0, 32] {
            for z in [-18, 18] {
                lights.push((
                    (CANNON_STATION_CENTER.as_vec3()
                        + Vec3::new(x as f32, (deck_y + 7) as f32, z as f32)
                        + Vec3::splat(0.5))
                        * VOXEL_SIZE,
                    Color::srgb(0.45, 0.8, 1.0),
                ));
            }
        }
    }
    for local_z in [-48, -64, -80, -96] {
        for x in [-6, 6] {
            lights.push((
                (CANNON_STATION_CENTER.as_vec3()
                    + Vec3::new(x as f32, 22.0, local_z as f32)
                    + Vec3::splat(0.5))
                    * VOXEL_SIZE,
                Color::srgb(0.25, 0.75, 1.0),
            ));
        }
    }
    lights
}

fn setup_voxel_interior_lights(mut commands: Commands) {
    for (position, color) in voxel_interior_lights() {
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
    let mut add = |position, size, base_material, accent_material, yaw| {
        specs.push((
            voxel_prop_cells(size, base_material, accent_material),
            Transform::from_translation(position).with_rotation(Quat::from_rotation_y(yaw)),
        ));
    };

    for station_x in [
        RESEARCH_STATION_CENTER.x as f32 * VOXEL_SIZE,
        SENSOR_STATION_CENTER.x as f32 * VOXEL_SIZE,
    ] {
        add(
            Vec3::new(station_x, 14.55, 0.0),
            IVec3::new(5, 3, 3),
            6,
            8,
            if station_x < 0.0 { 0.55 } else { -0.55 },
        );
        for (offset_x, z, yaw) in [(-4.0, -3.5, 0.2), (4.0, -3.5, -0.2), (0.0, 4.0, 0.0)] {
            add(
                Vec3::new(station_x + offset_x, 14.55, z),
                IVec3::new(5, 1, 2),
                7,
                8,
                yaw,
            );
        }
        for deck_y in [0.28, 2.53, 4.78, 7.03] {
            for z in [-5.5, 5.5] {
                add(
                    Vec3::new(station_x, deck_y, z),
                    IVec3::new(3, 2, 2),
                    6,
                    10,
                    if z < 0.0 { 0.0 } else { std::f32::consts::PI },
                );
            }
        }
    }

    for (x, z, yaw) in [(-3.6, 31.5, 0.35), (0.0, 37.0, 0.0), (3.6, 42.0, -0.35)] {
        add(
            Vec3::new(x, 0.28, z),
            IVec3::new(5, 3, 5),
            6,
            8,
            yaw,
        );
    }
    for (x, z, yaw) in [
        (-3.2, 34.0, 0.0),
        (3.2, 34.0, 0.0),
        (-3.2, 36.5, 0.4),
        (3.2, 36.5, -0.4),
    ] {
        add(
            Vec3::new(x, 0.28, z),
            IVec3::new(3, 2, 3),
            2,
            8,
            yaw,
        );
    }
    for deck_y in [0.28, 2.03, 3.78] {
        for (x, z, yaw) in [(-3.7, 29.5, 0.0), (3.7, 29.5, std::f32::consts::PI)] {
            add(
                Vec3::new(x, deck_y, z),
                IVec3::new(2, 3, 2),
                7,
                10,
                yaw,
            );
        }
    }
    for (x, z, yaw) in [
        (-1.8, 52.5, 0.0),
        (0.0, 54.0, 0.0),
        (1.8, 52.5, std::f32::consts::PI),
    ] {
        add(
            Vec3::new(x, 2.03, z),
            IVec3::new(3, 2, 2),
            6,
            8,
            yaw,
        );
    }
    specs
}

fn setup_voxel_sample_props(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    for (cells, transform) in voxel_physics_prop_specs() {
        spawn_voxel_physics_body_at(
            &mut commands,
            &mut meshes,
            &materials,
            cells,
            transform,
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
        );
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
    editor: Res<VoxelEditorState>,
    radiance_volume: Res<VoxelRadianceVolume>,
    mut meshes: ResMut<Assets<Mesh>>,
    voxel_materials: Res<VoxelMaterials>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: DEFAULT_KEY_LIGHT_ILLUMINANCE,
            shadow_maps_enabled: true,
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
        // The radiance-cascade pass samples the viewport depth buffer directly.
        // Keep this camera single-sampled so that depth is available as a normal
        // texture instead of an unresolved multisampled attachment.
        Msaa::Off,
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
        let transform = persisted_voxel_player_camera(&store, user_id)
            .map(voxel_player_camera_transform)
            .unwrap_or(default_transform);
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

fn voxel_player_camera_panel(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut images: ResMut<Assets<Image>>,
    manager: Option<Res<Persistent<NapcatMessageManager>>>,
    mut editor: ResMut<VoxelPlayerCameraEditor>,
    mut voxel_editor: ResMut<VoxelEditorState>,
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
                apply_voxel_player_view_to_editor(&mut voxel_editor, &transform);
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

fn apply_voxel_player_view_to_editor(editor: &mut VoxelEditorState, transform: &Transform) {
    let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
    editor.first_person_enabled = false;
    editor.first_person_flying = false;
    editor.camera_yaw = yaw;
    editor.camera_pitch = pitch;
    editor.camera_focus = orbit_focus_preserving_camera_position(
        transform.translation,
        yaw,
        pitch,
        editor.camera_distance,
    );
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
                let entity = commands
                    .spawn((
                        Mesh3d(meshes.add(Plane3d::new(Vec3::Z, size * 0.5).mesh())),
                        MeshMaterial3d(materials.add(voxel_player_standee_material(texture))),
                        voxel_player_standee_transform(&camera_transform),
                        Visibility::Visible,
                        VoxelPlayerStandee {
                            user_id,
                            image_source,
                        },
                    ))
                    .id();
                assets.entities.insert(user_id, entity);
            },
            Err(err) => {
                assets.failed_sources.insert(image_source);
                eprintln!("failed to load voxel player standee for {user_id}: {err}");
            },
        }
    }
}

fn voxel_player_standee_transform(camera_transform: &Transform) -> Transform { *camera_transform }

fn voxel_player_standee_material(texture: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(texture),
        alpha_mode: AlphaMode::Opaque,
        cull_mode: None,
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

fn cached_or_local_voxel_standee_path(source: &str) -> Result<PathBuf, String> {
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
    runtimes: Res<VoxelPlayerCameraRuntimes>,
    mut state: ResMut<VoxelPlayerCaptureState>,
    mut cameras: Query<
        &mut Camera,
        (
            With<VoxelPlayerCaptureCamera>,
            Without<VoxelPlayerStandee>,
        ),
    >,
    mut standees: Query<
        (
            Entity,
            &VoxelPlayerStandee,
            &mut Visibility,
        ),
        (
            With<VoxelPlayerStandee>,
            Without<VoxelPlayerCaptureCamera>,
        ),
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
        state.pending.push(PendingVoxelPlayerCapture {
            request_id,
            user_id: request.user_id,
            camera_entity: camera.entity,
            target: camera.target.clone(),
            output_path: output_dir.join(format!(
                "player_{}.png",
                request.user_id
            )),
            prepare_frames_remaining: PLAYER_CAPTURE_PREPARE_FRAMES,
            activated: false,
            hidden_standees: Vec::new(),
        });
    }

    let Some(current) = state.pending.first_mut() else { return };
    if !current.activated {
        if let Ok(mut camera) = cameras.get_mut(current.camera_entity) {
            camera.is_active = true;
        }
        for (entity, standee, mut visibility) in &mut standees {
            if voxel_player_standee_visible_to(
                manager.as_deref(),
                current.user_id,
                standee.user_id,
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
                    Without<VoxelPlayerStandee>,
                ),
            >,
                  mut standees: Query<
                &mut Visibility,
                (
                    With<VoxelPlayerStandee>,
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

fn voxel_player_standee_visible_to(
    manager: Option<&Persistent<NapcatMessageManager>>,
    requester_id: u64,
    standee_user_id: u64,
) -> bool {
    let Some(manager) = manager else { return false };
    let requester = manager.player_access_for_user(requester_id);
    if requester.is_gm || requester_id == standee_user_id {
        return true;
    }
    let standee = manager.player_access_for_user(standee_user_id);
    requester.party_id.is_some() && requester.party_id == standee.party_id
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

fn voxel_orbital_planet_cells() -> Vec<(IVec3, u8)> {
    let radius = ORBITAL_PLANET_VOXEL_RADIUS as f32;
    let inner_radius = radius - ORBITAL_PLANET_SHELL_THICKNESS;
    let outer_squared = (radius + 0.5).powi(2);
    let inner_squared = inner_radius.powi(2);
    let mut cells = Vec::new();
    for x in -ORBITAL_PLANET_VOXEL_RADIUS..=ORBITAL_PLANET_VOXEL_RADIUS {
        for y in -ORBITAL_PLANET_VOXEL_RADIUS..=ORBITAL_PLANET_VOXEL_RADIUS {
            for z in -ORBITAL_PLANET_VOXEL_RADIUS..=ORBITAL_PLANET_VOXEL_RADIUS {
                let distance_squared = IVec3::new(x, y, z).as_vec3().length_squared();
                if !(inner_squared..=outer_squared).contains(&distance_squared) {
                    continue;
                }
                let cell = IVec3::new(x, y, z);
                if let Some(material) = procedural_planet_lod_material(cell) {
                    cells.push((cell, material));
                }
            }
        }
    }
    cells
}

fn procedural_planet_lod_material(cell: IVec3) -> Option<u8> {
    let radius = ORBITAL_PLANET_VOXEL_RADIUS as f32 + 0.5;
    let distance = cell.as_vec3().length();
    if distance > radius {
        return None;
    }
    let depth = radius - distance;
    if depth > 5.0 {
        return Some(6);
    }
    if depth > 1.5 {
        return Some(2);
    }
    let continental = (cell.x as f32 * 0.095).sin()
        + (cell.z as f32 * 0.08).cos()
        + ((cell.x + cell.z) as f32 * 0.055).sin()
        + (cell.y as f32 * 0.115).cos() * 0.35;
    Some(
        if cell.y.abs() >= ORBITAL_PLANET_VOXEL_RADIUS - 4 {
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

fn procedural_planet_material(cell: IVec3) -> Option<u8> {
    let local_position = cell.as_vec3() * VOXEL_SIZE;
    let distance = local_position.length();
    if distance > ORBITAL_PLANET_RADIUS - VOXEL_SIZE * 0.5 {
        return None;
    }
    let depth = ORBITAL_PLANET_RADIUS - distance;
    if depth > 5.0 * ORBITAL_PLANET_LOD_VOXEL_SIZE {
        return Some(6);
    }
    if depth > 1.5 * ORBITAL_PLANET_LOD_VOXEL_SIZE {
        return Some(2);
    }
    let continental = (local_position.x * 0.019).sin()
        + (local_position.z * 0.016).cos()
        + ((local_position.x + local_position.z) * 0.011).sin()
        + (local_position.y * 0.023).cos() * 0.35;
    Some(
        if local_position.y.abs() >= ORBITAL_PLANET_RADIUS - 20.0 {
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

fn refine_planet_lod_cell(planet: &mut VoxelOrbitalPlanet, lod_cell: IVec3) -> bool {
    if planet.lod_cells.remove(&lod_cell).is_none() {
        return false;
    }
    planet.refined_lod_cells.insert(lod_cell);
    let min = lod_cell * ORBITAL_PLANET_LOD_SUBDIVISIONS
        - IVec3::splat(ORBITAL_PLANET_LOD_SUBDIVISIONS / 2);
    let max = min + IVec3::splat(ORBITAL_PLANET_LOD_SUBDIVISIONS - 1);
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let cell = IVec3::new(x, y, z);
                if planet.removed.contains(&cell) || planet.cells.contains_key(&cell) {
                    continue;
                }
                if let Some(material) = procedural_planet_material(cell) {
                    planet.cells.insert(cell, material);
                }
            }
        }
    }
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let neighbor = lod_cell + IVec3::new(x, y, z);
                if neighbor == lod_cell
                    || planet.refined_lod_cells.contains(&neighbor)
                    || planet.lod_cells.contains_key(&neighbor)
                {
                    continue;
                }
                if let Some(material) = procedural_planet_lod_material(neighbor) {
                    planet.lod_cells.insert(neighbor, material);
                }
            }
        }
    }
    planet.dirty = true;
    true
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
    planet.dirty = true;
    true
}

fn explode_planet_voxels(
    planet: &mut VoxelOrbitalPlanet,
    local_origin: Vec3,
    radius: f32,
) -> usize {
    let radius = radius.max(VOXEL_SIZE);
    let lod_reach = radius + ORBITAL_PLANET_LOD_VOXEL_SIZE * 3.0_f32.sqrt() * 0.5;
    let lod_to_refine = planet
        .lod_cells
        .keys()
        .copied()
        .filter(|cell| {
            let center = cell.as_vec3() * ORBITAL_PLANET_LOD_VOXEL_SIZE;
            center.distance_squared(local_origin) <= lod_reach * lod_reach
        })
        .collect::<Vec<_>>();
    for cell in lod_to_refine {
        refine_planet_lod_cell(planet, cell);
    }

    let radius_squared = radius * radius;
    let removed = planet
        .cells
        .keys()
        .copied()
        .filter(|cell| {
            let center = cell.as_vec3() * VOXEL_SIZE;
            center.distance_squared(local_origin) <= radius_squared
        })
        .collect::<Vec<_>>();
    for cell in &removed {
        planet.cells.remove(cell);
        planet.removed.insert(*cell);
    }
    if !removed.is_empty() {
        planet.dirty = true;
    }
    removed.len()
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

fn sorted_planet_lod_cells(planet: &VoxelOrbitalPlanet) -> Vec<(IVec3, u8)> {
    let mut cells = planet
        .lod_cells
        .iter()
        .map(|(cell, material)| (*cell, *material))
        .collect::<Vec<_>>();
    cells.sort_unstable_by_key(|(cell, _)| (cell.y, cell.z, cell.x));
    cells
}

fn exposed_solid_voxel_cells(cells: &[(IVec3, u8)]) -> Vec<IVec3> {
    let solids = cells
        .iter()
        .filter_map(|(cell, material)| TrpgVoxelConnector::solid(material).then_some(*cell))
        .collect::<HashSet<_>>();
    solids
        .iter()
        .copied()
        .filter(|cell| {
            VOXEL_FACES
                .iter()
                .any(|(normal, _)| !solids.contains(&(*cell + *normal)))
        })
        .collect()
}

fn planet_material_handle(materials: &VoxelMaterials, material_id: u8) -> Handle<StandardMaterial> {
    if material_id == 4 {
        materials.planet_ocean.clone()
    } else {
        materials.handles[material_id as usize - 1].clone()
    }
}

fn spawn_voxel_orbital_planet(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &VoxelMaterials,
) -> Entity {
    let lod_cells = voxel_orbital_planet_cells();
    let (material_meshes, _) = build_voxel_meshes_from_cells(&lod_cells);
    let voxel_scale = ORBITAL_PLANET_LOD_VOXEL_SIZE / VOXEL_SIZE;
    let mesh_center_offset = Vec3::splat(-0.5 * ORBITAL_PLANET_LOD_VOXEL_SIZE);
    let collider_cells = exposed_solid_voxel_cells(&lod_cells);
    let entity = commands
        .spawn((
            RigidBody::Static,
            Collider::compound(vec![(
                Vec3::splat(-0.5 * ORBITAL_PLANET_LOD_VOXEL_SIZE),
                Quat::IDENTITY,
                Collider::voxels(
                    Vec3::splat(ORBITAL_PLANET_LOD_VOXEL_SIZE),
                    &collider_cells,
                ),
            )]),
            Transform::from_translation(ORBITAL_PLANET_CENTER),
        ))
        .id();
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
                        Transform::from_translation(mesh_center_offset)
                            .with_scale(Vec3::splat(voxel_scale)),
                    ))
                    .id(),
            );
        }
    });
    commands.entity(entity).insert(VoxelOrbitalPlanet {
        lod_cells: lod_cells.into_iter().collect(),
        refined_lod_cells: HashSet::new(),
        cells: HashMap::new(),
        removed: HashSet::new(),
        mesh_entities,
        mesh_handles,
        voxel_size: VOXEL_SIZE,
        dirty: false,
        auto_refine_pending: false,
    });
    entity
}

fn rebuild_voxel_orbital_planet(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    editor: Res<VoxelEditorState>,
    mut planets: Query<(
        Entity,
        &mut VoxelOrbitalPlanet,
        &mut Collider,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    let batching_edits = !editor.is_tool_gun_equipped()
        && matches!(
            editor.mode,
            VoxelEditMode::Add | VoxelEditMode::Remove | VoxelEditMode::Paint
        )
        && (mouse.pressed(MouseButton::Left) || mouse.pressed(MouseButton::Right));
    for (entity, mut planet, mut collider) in &mut planets {
        if !planet.dirty || batching_edits || planet.auto_refine_pending {
            continue;
        }
        for mesh_entity in planet.mesh_entities.drain(..) {
            commands.entity(mesh_entity).despawn();
        }
        for mesh_handle in planet.mesh_handles.drain(..) {
            meshes.remove(mesh_handle.id());
        }
        let cells = sorted_planet_cells(&planet);
        let lod_cells = sorted_planet_lod_cells(&planet);
        let fine_collider_cells = exposed_solid_voxel_cells(&cells);
        let lod_collider_cells = exposed_solid_voxel_cells(&lod_cells);
        let mut collider_parts = Vec::new();
        if !lod_collider_cells.is_empty() {
            collider_parts.push((
                Vec3::splat(-0.5 * ORBITAL_PLANET_LOD_VOXEL_SIZE),
                Quat::IDENTITY,
                Collider::voxels(
                    Vec3::splat(ORBITAL_PLANET_LOD_VOXEL_SIZE),
                    &lod_collider_cells,
                ),
            ));
        }
        if !fine_collider_cells.is_empty() {
            collider_parts.push((
                Vec3::splat(-0.5 * VOXEL_SIZE),
                Quat::IDENTITY,
                Collider::voxels(
                    Vec3::splat(VOXEL_SIZE),
                    &fine_collider_cells,
                ),
            ));
        }
        *collider = Collider::compound(collider_parts);
        let (lod_material_meshes, _) = build_voxel_meshes_from_cells(&lod_cells);
        let (fine_material_meshes, _) = build_voxel_meshes_from_cells(&cells);
        let mut mesh_entities = Vec::new();
        let mut mesh_handles = Vec::new();
        commands.entity(entity).with_children(|parent| {
            for (material_id, mesh) in lod_material_meshes {
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
                            Transform::from_translation(Vec3::splat(
                                -0.5 * ORBITAL_PLANET_LOD_VOXEL_SIZE,
                            ))
                            .with_scale(Vec3::splat(
                                ORBITAL_PLANET_LOD_VOXEL_SIZE / VOXEL_SIZE,
                            )),
                        ))
                        .id(),
                );
            }
            for (material_id, mesh) in fine_material_meshes {
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

fn rebuild_voxel_geometry(
    mut commands: Commands,
    grids: Query<&Grid<u8>, (With<TrpgVoxelGrid>, Changed<Grid<u8>>)>,
    old_geometry: Query<Entity, With<VoxelGeometry>>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    let Ok(grid) = grids.single() else {
        return;
    };

    for entity in &old_geometry {
        commands.entity(entity).despawn();
    }

    let (material_meshes, collider_voxels) = build_voxel_meshes(grid);
    for (material_id, mesh) in material_meshes {
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.handles[material_id as usize - 1].clone()),
            VoxelGeometry,
        ));
    }
    if !collider_voxels.is_empty() {
        commands.spawn((
            RigidBody::Static,
            Collider::voxels(
                Vec3::splat(VOXEL_SIZE),
                &collider_voxels,
            ),
            VoxelGeometry,
        ));
    }
}

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

fn build_voxel_meshes_from_cells(cells: &[(IVec3, u8)]) -> (Vec<(u8, Mesh)>, Vec<IVec3>) {
    let mut material_meshes = Vec::new();
    let occupied = cells.iter().copied().collect::<HashMap<_, _>>();
    let mut cells_by_material = HashMap::<u8, Vec<IVec3>>::new();
    for (cell, material) in cells {
        if *material != 0 {
            cells_by_material.entry(*material).or_default().push(*cell);
        }
    }
    let collider_voxels = cells
        .iter()
        .filter_map(|(cell, material)| TrpgVoxelConnector::solid(material).then_some(*cell))
        .collect::<Vec<_>>();

    let mut material_ids = cells_by_material.keys().copied().collect::<Vec<_>>();
    material_ids.sort_unstable();
    for material in material_ids {
        let mut positions = Vec::<[f32; 3]>::new();
        let mut normals = Vec::<[f32; 3]>::new();
        let mut uvs = Vec::<[f32; 2]>::new();
        let mut indices = Vec::<u32>::new();
        for (normal, corners) in VOXEL_FACES {
            append_greedy_voxel_faces(
                &occupied,
                &cells_by_material[&material],
                normal,
                corners,
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

const VOXEL_FACES: [(IVec3, [[f32; 3]; 4]); 6] = [
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

fn append_greedy_voxel_faces(
    occupied: &HashMap<IVec3, u8>,
    material_cells: &[IVec3],
    normal: IVec3,
    corners: [[f32; 3]; 4],
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
) {
    let normal_axis = if normal.x != 0 {
        0
    } else if normal.y != 0 {
        1
    } else {
        2
    };
    let (u_axis, v_axis) = match normal_axis {
        0 => (2, 1),
        1 => (0, 2),
        _ => (0, 1),
    };
    let mut faces = material_cells
        .iter()
        .filter_map(|cell| {
            (occupied.get(&(*cell + normal)).copied().unwrap_or(0) == 0).then_some(*cell)
        })
        .collect::<Vec<_>>();
    faces.sort_unstable_by_key(|cell| {
        (
            cell[normal_axis],
            cell[v_axis],
            cell[u_axis],
        )
    });
    let face_set = faces.iter().copied().collect::<HashSet<_>>();
    let mut consumed = HashSet::new();
    for cell in faces {
        if consumed.contains(&cell) {
            continue;
        }
        let mut width = 1;
        while face_set.contains(&offset_axis(cell, u_axis, width))
            && !consumed.contains(&offset_axis(cell, u_axis, width))
        {
            width += 1;
        }
        let mut height = 1;
        'height: loop {
            for u in 0..width {
                let candidate = offset_axis(
                    offset_axis(cell, v_axis, height),
                    u_axis,
                    u,
                );
                if !face_set.contains(&candidate) || consumed.contains(&candidate) {
                    break 'height;
                }
            }
            height += 1;
        }
        for v in 0..height {
            for u in 0..width {
                consumed.insert(offset_axis(
                    offset_axis(cell, v_axis, v),
                    u_axis,
                    u,
                ));
            }
        }
        let base = positions.len() as u32;
        for (corner, uv) in corners
            .into_iter()
            .zip([[0., 1.], [0., 0.], [1., 0.], [1., 1.]])
        {
            let mut corner = Vec3::from(corner);
            if corner[u_axis] > 0.5 {
                corner[u_axis] = width as f32;
            }
            if corner[v_axis] > 0.5 {
                corner[v_axis] = height as f32;
            }
            positions.push(((cell.as_vec3() + corner) * VOXEL_SIZE).to_array());
            normals.push(normal.as_vec3().to_array());
            uvs.push([uv[0] * width as f32, uv[1] * height as f32]);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn offset_axis(mut cell: IVec3, axis: usize, amount: i32) -> IVec3 {
    cell[axis] += amount;
    cell
}

fn animate_voxel_materials(
    time: Res<Time>,
    voxel_materials: Res<VoxelMaterials>,
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
    if let Some(mut lava) = materials.get_mut(&voxel_materials.handles[4]) {
        lava.uv_transform = Affine2::from_translation(Vec2::new(
            seconds * -0.018,
            seconds * 0.027,
        ));
        let pulse = 4.5 + (seconds * 2.4).sin() * 1.2;
        lava.emissive = voxel_emissive(pulse, pulse * 0.11, 0.015);
    }
}

fn handle_editor_requests(
    mut commands: Commands,
    mut editor: ResMut<VoxelEditorState>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    physics_bodies: Query<Entity, With<VoxelPhysicsBody>>,
    placed_lights: Query<Entity, With<VoxelPlacedLight>>,
) {
    let Ok(mut grid) = grids.single_mut() else {
        return;
    };
    if editor.reset_requested {
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
        editor.undo.clear();
        editor.redo.clear();
        editor.selection_anchor = None;
        editor.selection_end = None;
        editor.selection_is_planet = false;
        editor.selected_light = None;
        editor.physics_status = None;
        editor.reset_requested = false;
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

fn split_voxel_cells(cells: Vec<(IVec3, u8)>, requested_parts: usize) -> Vec<Vec<(IVec3, u8)>> {
    let part_count = requested_parts.min(cells.len());
    if part_count == 0 {
        return Vec::new();
    }
    let cell_count = cells.len();
    let mut parts = vec![Vec::new(); part_count];
    for (index, cell) in cells.into_iter().enumerate() {
        parts[index * part_count / cell_count].push(cell);
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
            RigidBody::Dynamic,
            Collider::voxels(
                Vec3::splat(VOXEL_SIZE),
                &collider_voxels,
            ),
            ConstantLinearAcceleration::new(0.0, -9.81, 0.0),
            linear_velocity,
            angular_velocity,
            LinearDamping(0.15),
            AngularDamping(0.35),
            transform,
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

fn process_voxel_scene_history(
    mut commands: Commands,
    mut editor: ResMut<VoxelEditorState>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    physics_bodies: Query<(
        Entity,
        &VoxelPhysicsBody,
        &Transform,
        &LinearVelocity,
        &AngularVelocity,
    )>,
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
        let physics_bodies = physics_bodies
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
                RigidBody::Dynamic,
                Collider::cuboid(
                    VOXEL_SIZE * 0.8,
                    VOXEL_SIZE * 0.8,
                    VOXEL_SIZE * 0.8,
                ),
                ConstantLinearAcceleration::new(0.0, -9.81, 0.0),
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
    mut editor: ResMut<VoxelEditorState>,
    egui_input: Res<EguiWantsInput>,
) {
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

fn refine_visible_planet_voxels(
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    mut planets: Query<(
        &mut VoxelOrbitalPlanet,
        &GlobalTransform,
    )>,
    editor: Res<VoxelEditorState>,
) {
    let (Ok((camera, camera_transform)), Ok((mut planet, planet_transform))) =
        (cameras.single(), planets.single_mut())
    else {
        return;
    };

    // Refine the planet around the camera before the player aims or clicks.
    // This keeps nearby terrain at the canonical 0.25 gameplay scale while
    // retaining coarse sparse storage for the distant planet.
    let local_camera = planet_transform
        .affine()
        .inverse()
        .transform_point3(camera_transform.translation());
    let camera_lod_cell = (local_camera / ORBITAL_PLANET_LOD_VOXEL_SIZE)
        .round()
        .as_ivec3();
    let mut requested = prism(
        camera_lod_cell - IVec3::splat(2),
        camera_lod_cell + IVec3::splat(3),
    )
    .filter(|cell| planet.lod_cells.contains_key(cell))
    .collect::<HashSet<_>>();

    // Also refine the visible surface in orbit view, where the camera may be
    // farther than the proximity radius. Sampling the viewport avoids making
    // the entire planet resident at canonical resolution.
    for sample_y in 0..3 {
        for sample_x in 0..5 {
            let fraction = Vec2::new(
                sample_x as f32 / 4.0,
                sample_y as f32 / 2.0,
            );
            let screen_position =
                editor.viewport_min + (editor.viewport_max - editor.viewport_min) * fraction;
            let Ok(ray) = camera.viewport_to_world(camera_transform, screen_position) else {
                continue;
            };
            if let Some(hit) =
                raycast_voxel_planet(&planet, planet_transform, ray).filter(|hit| hit.lod)
            {
                requested.insert(hit.occupied);
            }
        }
    }
    let mut requested = requested.into_iter().collect::<Vec<_>>();
    requested.sort_unstable_by(|left, right| {
        let left_distance = left.as_vec3() * ORBITAL_PLANET_LOD_VOXEL_SIZE - local_camera;
        let right_distance = right.as_vec3() * ORBITAL_PLANET_LOD_VOXEL_SIZE - local_camera;
        left_distance
            .length_squared()
            .total_cmp(&right_distance.length_squared())
    });
    planet.auto_refine_pending = requested.len() > PLANET_AUTO_REFINEMENTS_PER_FRAME;
    for cell in requested
        .into_iter()
        .take(PLANET_AUTO_REFINEMENTS_PER_FRAME)
    {
        refine_planet_lod_cell(&mut planet, cell);
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
    mut editor: ResMut<VoxelEditorState>,
    egui_input: Res<EguiWantsInput>,
) {
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
        editor.edit_hold_seconds = 0.0;
        editor.edit_repeat_seconds = 0.0;
        if !editor.active_stroke.is_empty() {
            if let Ok(mut grid) = grids.single_mut() {
                grid.set_changed();
            }
            let stroke = std::mem::take(&mut editor.active_stroke);
            editor.undo.push(stroke);
            editor.redo.clear();
        }
    }
    if editor.creative_inventory_open {
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
                editor.physics_status = Some(format!(
                    "行星爆炸已移除 {removed} 个 0.25 体素"
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
            for ((entity, cells, transform, linear_velocity, angular_velocity), part_count) in
                affected_bodies.into_iter().zip(part_counts.iter().copied())
            {
                if part_count == 0 {
                    continue;
                }
                commands.entity(entity).despawn();
                if target == Some(entity) {
                    target = None;
                }
                for cells in split_voxel_cells(cells, part_count) {
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
                for cells in split_voxel_cells(selected, static_part_count) {
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
        if let Ok((mut planet, planet_transform)) = planets.single_mut() {
            let mut planet_hit = raycast_voxel_planet(&planet, planet_transform, ray);
            if let Some(hit) = planet_hit.filter(|hit| hit.lod) {
                refine_planet_lod_cell(&mut planet, hit.occupied);
                planet_hit =
                    raycast_voxel_planet(&planet, planet_transform, ray).filter(|hit| !hit.lod);
            }
            if let Some(hit) = planet_hit.filter(|hit| {
                !hit.lod && grid_distance.is_none_or(|distance| hit.distance < distance)
            }) {
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
        let mut planet_hit = raycast_voxel_planet(&planet, planet_transform, ray)
            .filter(|hit| grid_distance.is_none_or(|distance| hit.distance < distance));
        if let Some(hit) = planet_hit.filter(|hit| hit.lod) {
            refine_planet_lod_cell(&mut planet, hit.occupied);
            planet_hit = raycast_voxel_planet(&planet, planet_transform, ray).filter(|hit| {
                !hit.lod && grid_distance.is_none_or(|distance| hit.distance < distance)
            });
        }
        if let Some(hit) = planet_hit.filter(|hit| !hit.lod) {
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
                    grid.set_batched(position, after);
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
        editor.edit_hold_seconds = 0.0;
        editor.edit_repeat_seconds = 0.0;
        return true;
    }
    editor.edit_hold_seconds += delta_seconds;
    if editor.edit_hold_seconds < EDIT_REPEAT_DELAY {
        return false;
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

    let step = planet.voxel_size * 0.2;
    let mut distance = 0.0;
    while distance <= MAX_RAY_DISTANCE {
        let local_point = local_origin + local_direction * distance;
        let cell = (local_point / planet.voxel_size + Vec3::splat(0.5))
            .floor()
            .as_ivec3();
        if planet.cells.contains_key(&cell) {
            return Some(VoxelPlanetRayHit {
                occupied: cell,
                normal: voxel_face_normal_against_ray(local_direction),
                distance,
                lod: false,
            });
        }
        let lod_cell = (local_point / ORBITAL_PLANET_LOD_VOXEL_SIZE + Vec3::splat(0.5))
            .floor()
            .as_ivec3();
        if planet.lod_cells.contains_key(&lod_cell) {
            return Some(VoxelPlanetRayHit {
                occupied: lod_cell,
                normal: voxel_face_normal_against_ray(local_direction),
                distance,
                lod: true,
            });
        }
        distance += step;
    }
    None
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
    mut editor: ResMut<VoxelEditorState>,
    mut players: Query<
        (
            Entity,
            &ShapeHits,
            &mut LinearVelocity,
            &mut ConstantLinearAcceleration,
            Has<Sensor>,
        ),
        With<VoxelFirstPersonPlayer>,
    >,
) {
    let Ok((entity, ground_hits, mut velocity, mut acceleration, is_sensor)) = players.single_mut()
    else {
        return;
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
    if editor.creative_inventory_open {
        velocity.x = 0.0;
        velocity.z = 0.0;
        if editor.first_person_flying {
            velocity.y = 0.0;
            acceleration.0 = Vec3::ZERO;
        }
        return;
    }

    editor.first_person_space_tap_elapsed += time.delta_secs();
    if keyboard.just_pressed(KeyCode::Space)
        && register_first_person_space_tap(&mut editor.first_person_space_tap_elapsed)
    {
        editor.first_person_flying = !editor.first_person_flying;
    }
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
    if grounded && keyboard.just_pressed(KeyCode::Space) {
        velocity.y = FIRST_PERSON_JUMP_SPEED;
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
    egui_input: Res<EguiWantsInput>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        if editor.creative_inventory_open {
            editor.creative_inventory_open = false;
        } else if editor.first_person_enabled {
            editor.first_person_enabled = false;
            editor.first_person_flying = false;
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
    if let Ok(mut cursor) = cursor_options.single_mut() {
        if editor.first_person_enabled && !editor.creative_inventory_open {
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
        if !editor.creative_inventory_open {
            editor.camera_yaw -= delta.x * 0.0025;
            editor.camera_pitch = (editor.camera_pitch - delta.y * 0.0025).clamp(-1.5, 1.5);
        }
        let wheel_steps = wheel.read().fold(0, |steps, event| {
            steps + event.y.signum() as i32
        });
        if !editor.creative_inventory_open && wheel_steps != 0 {
            let slot = cycled_hotbar_slot(
                editor.selected_hotbar_slot,
                wheel_steps,
                editor.creative_hotbar.len(),
            );
            editor.select_hotbar_slot(slot);
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
            camera_transform.translation =
                player_transform.translation + Vec3::Y * FIRST_PERSON_EYE_OFFSET;
            camera_transform.rotation = rotation;
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
        let planet_hit = planet_hit_any.filter(|(planet_hit, _)| !planet_hit.lod);
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
    fn current_player_view_moves_the_gm_orbit_camera() {
        let player_view = Transform::from_xyz(4.0, 5.0, 6.0).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            0.4,
            -0.2,
            0.0,
        ));
        let mut editor = VoxelEditorState {
            first_person_enabled: true,
            first_person_flying: true,
            ..default()
        };

        apply_voxel_player_view_to_editor(&mut editor, &player_view);
        let gm_view = editor_camera_transform(&editor);

        assert!(!editor.first_person_enabled);
        assert!(!editor.first_person_flying);
        assert!(gm_view.translation.distance(player_view.translation) < 0.000_1);
        assert!(gm_view.forward().dot(*player_view.forward()) > 0.999_9);
    }

    #[test]
    fn player_standee_uses_canonical_two_voxel_height() {
        let size = voxel_player_standee_size(Vec2::new(300.0, 600.0));

        assert_eq!(size.y, VOXEL_SIZE * 2.0);
        assert_eq!(size.x, VOXEL_SIZE);
    }

    #[test]
    fn player_standee_material_is_opaque() {
        let material = voxel_player_standee_material(Handle::<Image>::default());

        assert!(matches!(
            material.alpha_mode,
            AlphaMode::Opaque
        ));
    }

    #[test]
    fn initializes_populated_trpg_grid() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        assert!(grid.count() > 225);
    }

    #[test]
    fn radiance_volume_contains_scene_voxels_and_stays_gpu_bounded() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        let (image, _volume_min, voxel_world_size, volume_dimensions) =
            build_voxel_radiance_image(grid);

        assert_eq!(
            image.texture_descriptor.dimension,
            TextureDimension::D3
        );
        assert!(volume_dimensions.max_element() <= VOXEL_RADIANCE_MAX_DIMENSION as f32);
        assert!(voxel_world_size >= VOXEL_SIZE);
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
    fn default_space_map_has_three_station_interiors_and_a_corvette_interior() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();

        for station_center in [RESEARCH_STATION_CENTER, SENSOR_STATION_CENTER] {
            assert_eq!(
                grid.get(station_center + IVec3::new(10, 0, 10)).copied(),
                Some(2)
            );
            assert_eq!(
                grid.get(station_center + IVec3::new(10, 4, 10))
                    .copied()
                    .unwrap_or(0),
                0
            );
        }
        assert_eq!(
            grid.get(COMBAT_SPACESHIP_CENTER + IVec3::new(1, 0, 0))
                .copied(),
            Some(2)
        );
        assert_eq!(
            grid.get(COMBAT_SPACESHIP_CENTER + IVec3::new(1, 4, 0))
                .copied()
                .unwrap_or(0),
            0
        );
        assert_eq!(
            grid.get(COMBAT_SPACESHIP_CENTER + IVec3::new(1, 14, 0))
                .copied(),
            Some(2)
        );
        assert_eq!(
            grid.get(CANNON_STATION_CENTER + IVec3::new(10, 0, 10))
                .copied(),
            Some(2)
        );
        assert_eq!(
            grid.get(CANNON_STATION_CENTER + IVec3::new(10, 4, 10))
                .copied()
                .unwrap_or(0),
            0
        );
        assert_eq!(
            grid.get(CANNON_STATION_CENTER + IVec3::new(0, 22, -112))
                .copied(),
            Some(8)
        );
        assert_eq!(
            grid.get(CANNON_STATION_CENTER + IVec3::new(15, 22, -112))
                .copied(),
            Some(9)
        );

        let old_station_interior_volume = 19 * 7 * 15;
        let new_station_interior_volume = 99 * 35 * 79;
        let old_station_floor_area = 19 * 15;
        let new_station_floor_area = 99 * 79 * 5;
        assert!(new_station_interior_volume >= old_station_interior_volume * 100);
        assert!(new_station_floor_area >= old_station_floor_area * 100);
    }

    #[test]
    fn orbital_layout_is_five_times_wider_and_clear_of_the_planet() {
        assert_eq!(ORBITAL_LAYOUT_SCALE, 5);
        assert_eq!(
            SENSOR_STATION_CENTER.x - RESEARCH_STATION_CENTER.x,
            1_000
        );
        assert_eq!(COMBAT_SPACESHIP_CENTER.z, 750);
        assert_eq!(CANNON_STATION_CENTER.z, -750);

        let planet_top = ORBITAL_PLANET_CENTER.y + ORBITAL_PLANET_RADIUS;
        assert!(planet_top <= -100.0);
        for center in [
            RESEARCH_STATION_CENTER,
            SENSOR_STATION_CENTER,
            CANNON_STATION_CENTER,
            COMBAT_SPACESHIP_CENTER,
        ] {
            let horizontal = center.as_vec3() * VOXEL_SIZE - ORBITAL_PLANET_CENTER;
            assert!(Vec2::new(horizontal.x, horizontal.z).length() < ORBITAL_PLANET_RADIUS);
        }

        let planet_cells = voxel_orbital_planet_cells();
        assert!((40_000..=100_000).contains(&planet_cells.len()));
        assert_eq!(
            planet_cells
                .iter()
                .map(|(_, material)| *material)
                .collect::<HashSet<_>>(),
            HashSet::from([1, 2, 3, 4])
        );
        assert!(planet_cells
            .iter()
            .all(|(cell, _)| { cell.abs().max_element() <= ORBITAL_PLANET_VOXEL_RADIUS }));
        assert_eq!(VOXEL_SIZE, 0.25);
        assert_eq!(
            ORBITAL_PLANET_LOD_VOXEL_SIZE / ORBITAL_PLANET_LOD_SUBDIVISIONS as f32,
            VOXEL_SIZE
        );
    }

    #[test]
    fn digging_planet_voxels_generates_buried_neighbors_without_refilling_holes() {
        let surface = IVec3::new(
            0,
            (ORBITAL_PLANET_RADIUS / VOXEL_SIZE) as i32,
            0,
        );
        let mut planet = VoxelOrbitalPlanet {
            lod_cells: HashMap::new(),
            refined_lod_cells: HashSet::new(),
            cells: HashMap::from([(surface, 3)]),
            removed: HashSet::new(),
            mesh_entities: Vec::new(),
            mesh_handles: Vec::new(),
            voxel_size: 1.0,
            dirty: false,
            auto_refine_pending: false,
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
            lod_cells: HashMap::new(),
            refined_lod_cells: HashSet::new(),
            cells: HashMap::from([(IVec3::ZERO, 2)]),
            removed: HashSet::new(),
            mesh_entities: Vec::new(),
            mesh_handles: Vec::new(),
            voxel_size: 2.0,
            dirty: false,
            auto_refine_pending: false,
        };
        let transform = GlobalTransform::from(Transform::from_xyz(0.0, -10.0, 0.0));
        let ray = Ray3d::new(Vec3::ZERO, Dir3::NEG_Y);

        let hit = raycast_voxel_planet(&planet, &transform, ray).unwrap();
        assert_eq!(hit.occupied, IVec3::ZERO);
        assert!((8.0..=10.0).contains(&hit.distance));
    }

    #[test]
    fn planet_explosion_refines_lod_into_quarter_unit_voxels_and_carves_them() {
        let lod_surface = IVec3::new(0, ORBITAL_PLANET_VOXEL_RADIUS, 0);
        let mut planet = VoxelOrbitalPlanet {
            lod_cells: HashMap::from([(lod_surface, 3)]),
            refined_lod_cells: HashSet::new(),
            cells: HashMap::new(),
            removed: HashSet::new(),
            mesh_entities: Vec::new(),
            mesh_handles: Vec::new(),
            voxel_size: VOXEL_SIZE,
            dirty: false,
            auto_refine_pending: false,
        };

        let removed = explode_planet_voxels(
            &mut planet,
            lod_surface.as_vec3() * ORBITAL_PLANET_LOD_VOXEL_SIZE,
            1.0,
        );

        assert!(removed > 0);
        assert_eq!(planet.voxel_size, VOXEL_SIZE);
        assert!(!planet.lod_cells.contains_key(&lod_surface));
        assert!(planet.refined_lod_cells.contains(&lod_surface));
        assert_eq!(planet.removed.len(), removed);
        assert!(planet.dirty);
    }

    #[test]
    fn combat_corvette_matches_the_three_deck_trpg_layout() {
        for floor_y in [0, 7, 14] {
            assert_eq!(
                combat_corvette_voxel(IVec3::new(0, floor_y, 0)),
                Some(2)
            );
        }
        for walkway_y in [4, 11, 18] {
            assert_eq!(
                combat_corvette_voxel(IVec3::new(0, walkway_y, 0)),
                None
            );
            assert_eq!(
                combat_corvette_voxel(IVec3::new(0, walkway_y, 15)),
                None
            );
        }

        for (position, expected_material) in [
            (IVec3::new(0, 3, -48), 8),
            (IVec3::new(-12, 2, -28), 3),
            (IVec3::new(-13, 8, -28), 6),
            (IVec3::new(11, 8, -25), 3),
            (IVec3::new(10, 15, -5), 6),
            (IVec3::new(0, 15, 23), 3),
            (IVec3::new(6, 8, 55), 8),
            (IVec3::new(16, 8, -80), 8),
            (IVec3::new(20, 3, 5), 9),
            (IVec3::new(20, 10, 5), 8),
            (IVec3::new(13, -5, -28), 6),
            (IVec3::new(0, 24, 0), 7),
            (IVec3::new(5, 23, 25), 7),
            (IVec3::new(23, 4, 5), 7),
        ] {
            assert_eq!(
                combat_corvette_voxel(position),
                Some(expected_material),
                "unexpected corvette voxel at {position:?}"
            );
        }
        assert_eq!(
            combat_corvette_voxel(IVec3::new(0, 11, 65)),
            None
        );
        assert_eq!(
            combat_corvette_voxel(IVec3::new(23, 6, 6)),
            None
        );
        assert_eq!(corvette_half_width(-80), 14);
        assert_eq!(corvette_half_width(0), 20);
        assert_eq!(corvette_half_width(80), 2);
    }

    #[test]
    fn first_person_character_is_two_voxels_tall_and_one_voxel_wide() {
        let total_height = FIRST_PERSON_BODY_LENGTH + FIRST_PERSON_RADIUS * 2.0;
        assert!((total_height - VOXEL_SIZE * 2.0).abs() < f32::EPSILON);
        assert!((FIRST_PERSON_RADIUS * 2.0 - VOXEL_SIZE).abs() < f32::EPSILON);
        assert!((FIRST_PERSON_EYE_OFFSET - VOXEL_SIZE).abs() < f32::EPSILON);
    }

    #[test]
    fn trpg_physics_uses_fewer_substeps_than_avian_default() {
        assert_eq!(TRPG_PHYSICS_SUBSTEPS, 2);
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
    fn planet_ocean_material_is_opaque() {
        let material = opaque_planet_ocean_material(Handle::default());
        assert!(matches!(
            material.alpha_mode,
            AlphaMode::Opaque
        ));
        assert!(material.base_color_texture.is_some());
    }

    #[test]
    fn default_space_map_uses_all_materials_and_internal_auto_doors() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        let materials = voxel_cells(grid)
            .into_iter()
            .map(|(_, material)| material)
            .collect::<HashSet<_>>();
        assert!(voxel_cells(grid).len() >= 100_000);
        assert_eq!(
            materials,
            HashSet::from([1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
        );

        let doors = voxel_auto_doors();
        assert_eq!(doors.len(), 64);
        assert!(doors.iter().take(2).all(|door| door.cells.len() == 88));
        assert_eq!(doors[2].cells.len(), 35);
        assert!(doors[3..35].iter().all(|door| door.cells.len() == 63));
        assert!(doors[35..53].iter().all(|door| door.cells.len() == 25));
        assert!(doors[53..].iter().all(|door| door.cells.len() == 63));
        assert!(doors
            .iter()
            .flat_map(|door| &door.cells)
            .all(|cell| { grid.get(*cell).copied().unwrap_or(0) == 0 }));
        for door in &doors {
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

        let lights = voxel_interior_lights();
        assert_eq!(lights.len(), 88);
        assert!(lights.iter().all(|(position, _)| position.y > 0.0));
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
    fn scene_history_restores_grid_and_physics_body_state() {
        let (mut app, grid_entity) = test_grid();
        app.init_resource::<VoxelEditorState>()
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
        let ray = Ray3d::new(
            (RESEARCH_STATION_CENTER.as_vec3() + Vec3::new(20.5, 120.0, 10.5)) * VOXEL_SIZE,
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
        assert_eq!(meshes.len(), VOXEL_MATERIAL_COUNT);
        assert!(!colliders.is_empty());
        for (_, mesh) in meshes {
            assert!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).is_some());
            assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
        }
    }

    #[test]
    fn greedy_meshing_collapses_a_solid_cuboid_to_six_quads() {
        let cells = prism(IVec3::ZERO, IVec3::new(4, 3, 2))
            .map(|cell| (cell, 1))
            .collect::<Vec<_>>();

        let (meshes, _) = build_voxel_meshes_from_cells(&cells);
        assert_eq!(meshes.len(), 1);
        let mesh = &meshes[0].1;
        assert_eq!(
            mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().len(),
            24
        );
        assert_eq!(mesh.indices().unwrap().len(), 36);
    }

    #[test]
    fn planet_collider_keeps_only_exposed_solid_voxels() {
        let cells = prism(IVec3::ZERO, IVec3::splat(3))
            .map(|cell| (cell, 1))
            .collect::<Vec<_>>();

        let exposed = exposed_solid_voxel_cells(&cells);

        assert_eq!(exposed.len(), 26);
        assert!(!exposed.contains(&IVec3::ONE));
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
    fn voxel_physics_props_detail_station_roofs_and_corvette_interiors() {
        let specs = voxel_physics_prop_specs();
        assert_eq!(specs.len(), 40);
        assert!(specs.iter().all(|(cells, _)| !cells.is_empty()));
        assert!(specs.iter().all(
            |(cells, _)| cells.iter().all(
                |(_, material)| TrpgVoxelConnector::solid(material)
                    && (1..=VOXEL_MATERIAL_COUNT as u8).contains(material)
            )
        ));
        assert!(specs.iter().any(|(cells, _)| cells.len() == 5 * 3 * 5));
        assert!(specs.iter().any(|(cells, _)| cells.len() == 5 * 1 * 2));
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
    fn explosion_fragment_budget_caps_large_areas_at_forty_parts() {
        let counts = allocate_fragment_parts(
            &[8_656],
            MAX_EXPLOSION_NEW_PHYSICS_BODIES,
        );
        assert_eq!(counts, vec![40]);

        let cells = (0..8_656).map(|x| (IVec3::new(x, 0, 0), 1)).collect();
        let parts = split_voxel_cells(cells, counts[0]);
        assert_eq!(parts.len(), 40);
        assert_eq!(
            parts.iter().map(Vec::len).sum::<usize>(),
            8_656
        );
    }

    #[test]
    fn each_explosion_gets_forty_new_parts_regardless_of_existing_bodies() {
        let unaffected_existing_body_count = 250;
        let counts = allocate_fragment_parts(
            &[12, 8, 8_656],
            MAX_EXPLOSION_NEW_PHYSICS_BODIES,
        );

        let new_body_count = counts.iter().sum::<usize>();
        assert_eq!(new_body_count, 40);
        assert_eq!(
            unaffected_existing_body_count + new_body_count,
            290
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
    fn edit_repeat_waits_before_repeating() {
        let mut editor = VoxelEditorState::default();
        assert!(edit_repeat_due(true, 0.0, &mut editor));
        for _ in 0..18 {
            assert!(!edit_repeat_due(
                false,
                0.016,
                &mut editor
            ));
        }
        assert!(!edit_repeat_due(
            false,
            0.04,
            &mut editor
        ));
        assert!(edit_repeat_due(
            false,
            0.05,
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
    fn ctrl_shift_z_requests_redo() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EguiWantsInput>()
            .init_resource::<VoxelEditorState>()
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
    }

    #[test]
    fn creative_inventory_snapshot_persists_as_toml() {
        let mut inventory = VoxelInventoryStore::default();
        inventory.hotbar[2] = Some(VoxelCreativeItem::ToolGun);
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
                    lod_cells: HashMap::new(),
                    refined_lod_cells: HashSet::new(),
                    cells: HashMap::from([(selected_cell, 1)]),
                    removed: HashSet::new(),
                    mesh_entities: Vec::new(),
                    mesh_handles: Vec::new(),
                    voxel_size: VOXEL_SIZE,
                    dirty: false,
                    auto_refine_pending: false,
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
        let mut bodies = app.world_mut().query::<(&VoxelPhysicsBody, &Transform)>();
        let (body, transform) = bodies.single(app.world()).unwrap();
        assert_eq!(body.cells, vec![(IVec3::ZERO, 1)]);
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
    fn held_voxel_edits_defer_the_grid_change_signal_until_release() {
        let mut world = World::new();
        let entity = world.spawn(Grid::<u8>::new()).id();
        world.clear_trackers();

        let mut entity_mut = world.entity_mut(entity);
        let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
        assert!(!grid.is_changed());
        grid.set_batched(IVec3::new(2, 3, 4), 7);
        assert_eq!(grid.get(IVec3::new(2, 3, 4)), Some(&7));
        assert!(!grid.is_changed());
        grid.set_changed();
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
}
