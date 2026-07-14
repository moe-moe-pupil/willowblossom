use std::collections::{
    HashMap,
    HashSet,
};

use avian3d::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
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
    window::{
        CursorGrabMode,
        CursorOptions,
        PrimaryWindow,
    },
};
use bevy_egui::input::EguiWantsInput;
use voxxelmaxx::prelude::*;

const VOXEL_SIZE: f32 = 0.25;
const MAX_RAY_DISTANCE: f32 = 200.0;
const EDIT_REPEAT_DELAY: f32 = 0.32;
const EDIT_REPEAT_INTERVAL: f32 = 0.09;
const TEST_GROUND_SIZE: Vec3 = Vec3::new(256.0, 0.5, 256.0);
const TEST_GROUND_CENTER_Y: f32 = -50.25;
const MAX_SCENE_SNAPSHOTS: usize = 20;
const MAX_EXPLOSION_NEW_PHYSICS_BODIES: usize = 40;
const VOXEL_MATERIAL_COUNT: usize = 8;
const FIRST_PERSON_RADIUS: f32 = 0.03;
const FIRST_PERSON_BODY_LENGTH: f32 = 0.065;
const FIRST_PERSON_EYE_OFFSET: f32 = 0.045;
const FIRST_PERSON_SPEED: f32 = 2.8;
const FIRST_PERSON_JUMP_SPEED: f32 = 3.4;
const FIRST_PERSON_FLY_SPEED: f32 = 3.5;
const FIRST_PERSON_FOV_RADIANS: f32 = 70.0_f32.to_radians();
const FIRST_PERSON_DOUBLE_TAP_SECONDS: f32 = 0.32;
const FIRST_PERSON_START: Vec3 = Vec3::new(-19.875, 0.32, 2.625);

pub struct TrpgVoxelPlugin;

pub struct TrpgVoxelConnector;

impl Connector for TrpgVoxelConnector {
    type Item = u8;

    fn solid(voxel: &Self::Item) -> bool { matches!(*voxel, 1..=3 | 6..=8) }
}

#[derive(Component)]
pub struct TrpgVoxelGrid;

#[derive(Component)]
struct VoxelViewportCamera;

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

#[derive(Clone)]
struct VoxelSceneSnapshot {
    name: String,
    voxels: Vec<(IVec3, u8)>,
    physics_bodies: Vec<VoxelPhysicsBodySnapshot>,
}

#[derive(Component)]
struct VoxelTestingGround;

#[derive(Component)]
struct VoxelFirstPersonPlayer;

#[derive(Component, Clone)]
struct VoxelAutoDoor {
    cells: Vec<IVec3>,
    trigger_center: Vec3,
    trigger_radius: f32,
    material: u8,
    open: bool,
}

#[derive(Resource)]
struct VoxelMaterials {
    handles: [Handle<StandardMaterial>; VOXEL_MATERIAL_COUNT],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum VoxelEditMode {
    #[default]
    Add,
    Remove,
    Paint,
    Physics,
    Push,
    Pull,
    Explode,
}

impl VoxelEditMode {
    const ALL: [Self; 7] = [
        Self::Add,
        Self::Remove,
        Self::Paint,
        Self::Physics,
        Self::Push,
        Self::Pull,
        Self::Explode,
    ];

    fn cycle(self, steps: i32) -> Self {
        let index = Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0) as i32;
        Self::ALL[(index + steps).rem_euclid(Self::ALL.len() as i32) as usize]
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Add => "添加",
            Self::Remove => "删除",
            Self::Paint => "涂色",
            Self::Physics => "物理选区",
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
    pub physics_requested: bool,
    physics_action_requested: Option<VoxelPhysicsRequest>,
    pub physics_push_pull_impulse: f32,
    pub physics_explosion_impulse: f32,
    pub physics_explosion_radius: f32,
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
    selection_anchor: Option<IVec3>,
    selection_end: Option<IVec3>,
    physics_status: Option<String>,
    scene_snapshots: Vec<VoxelSceneSnapshot>,
    next_scene_snapshot_number: u64,
    save_scene_requested: bool,
    restore_scene_requested: Option<usize>,
    first_person_space_tap_elapsed: f32,
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
            physics_requested: false,
            physics_action_requested: None,
            physics_push_pull_impulse: 4.0,
            physics_explosion_impulse: 14.0,
            physics_explosion_radius: 6.0,
            undo: Vec::new(),
            redo: Vec::new(),
            stroke_positions: HashSet::new(),
            active_stroke: Vec::new(),
            edit_hold_seconds: 0.0,
            edit_repeat_seconds: 0.0,
            camera_focus: Vec3::new(0.0, 4.0, 20.0),
            camera_distance: 80.0,
            camera_yaw: 0.7,
            camera_pitch: -0.45,
            camera_drag_started_in_viewport: false,
            left_started_over_ui: false,
            selection_anchor: None,
            selection_end: None,
            physics_status: None,
            scene_snapshots: Vec::new(),
            next_scene_snapshot_number: 1,
            save_scene_requested: false,
            restore_scene_requested: None,
            first_person_space_tap_elapsed: f32::INFINITY,
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
            "依次点击两个方块，框选物理区域"
        } else if self.selection_end.is_none() {
            "再点击一个方块，确定选区另一角"
        } else {
            "选区已确定；可重新点击起点或生成物理体"
        }
    }

    pub(crate) fn physics_status(&self) -> Option<&str> { self.physics_status.as_deref() }

    pub(crate) fn request_scene_snapshot(&mut self) { self.save_scene_requested = true; }

    pub(crate) fn scene_snapshot_labels(&self) -> Vec<String> {
        self.scene_snapshots
            .iter()
            .map(|snapshot| {
                format!(
                    "{}（{} 方块 / {} 物理体）",
                    snapshot.name,
                    snapshot.voxels.len(),
                    snapshot.physics_bodies.len()
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

    fn select_physics_corner(&mut self, cell: IVec3) {
        if self.selection_anchor.is_none() || self.selection_end.is_some() {
            self.selection_anchor = Some(cell);
            self.selection_end = None;
        } else {
            self.selection_end = Some(cell);
        }
        self.physics_status = None;
    }
}

impl Plugin for TrpgVoxelPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            PhysicsPlugins::default(),
            VoxelPlugin::<u8>::default(),
            ConnectivityPlugin::<TrpgVoxelConnector>::default(),
        ))
        .insert_resource(Gravity::ZERO)
        .init_resource::<VoxelEditorState>()
        .add_systems(
            Startup,
            (
                setup_voxel_materials,
                setup_voxel_grid,
                populate_voxel_grid,
                setup_voxel_auto_doors,
                setup_voxel_interior_lights,
                setup_voxel_sample_props,
                setup_voxel_view,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                voxel_editor_shortcuts,
                handle_editor_requests,
                edit_voxel_grid,
                make_selection_physical,
                apply_voxel_physics_action,
                process_voxel_scene_history,
                animate_voxel_auto_doors,
                rebuild_voxel_geometry,
                control_first_person_player,
                control_voxel_camera,
                draw_voxel_target,
                animate_voxel_materials,
            )
                .chain(),
        );
    }
}

fn voxel_editor_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    mut editor: ResMut<VoxelEditorState>,
) {
    if egui_input.wants_any_keyboard_input() || !keyboard.just_pressed(KeyCode::KeyZ) {
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
        } else {
            let atlas_row = [2, 0, 6][index - textures.len()];
            StandardMaterial {
                base_color_texture: Some(hifi_texture.clone()),
                base_color: Color::WHITE,
                uv_transform: hifi_voxel_tile_transform(atlas_row),
                metallic: 0.72,
                perceptual_roughness: 0.34,
                ..default()
            }
        };
        match index {
            3 => {
                material.base_color = Color::srgba(0.72, 0.9, 1.0, 0.72);
                material.alpha_mode = AlphaMode::Blend;
                material.perceptual_roughness = 0.18;
                material.reflectance = 0.65;
            },
            4 => {
                material.emissive_texture = Some(textures[index].clone());
                material.emissive = LinearRgba::rgb(5.0, 0.55, 0.02);
                material.perceptual_roughness = 0.55;
            },
            7 => {
                material.base_color = Color::srgb(0.48, 0.92, 1.0);
                material.emissive_texture = Some(hifi_texture.clone());
                material.emissive = LinearRgba::rgb(0.2, 3.2, 4.4);
                material.metallic = 0.25;
            },
            _ => {},
        }
        materials.add(material)
    });
    commands.insert_resource(VoxelMaterials { handles });
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.34, 0.42, 0.55),
        brightness: 75.0,
        ..default()
    });
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
    build_space_station(grid, IVec3::new(-90, 0, 0), false);
    build_space_station(grid, IVec3::new(90, 0, 0), true);
    build_combat_spaceship(grid);
    for door in voxel_auto_doors() {
        for cell in door.cells {
            grid.set(cell, door.material);
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
    const CENTER: IVec3 = IVec3::new(0, 0, 150);
    for x in -24..=24 {
        for y in 0..=23 {
            for z in -48..=48 {
                let local = IVec3::new(x, y, z);
                if let Some(material) = combat_corvette_voxel(local) {
                    grid.set(CENTER + local, material);
                }
            }
        }
    }
}

fn combat_corvette_voxel(position: IVec3) -> Option<u8> {
    const FLOOR: u8 = 2;
    const METAL: u8 = 3;
    const RED: u8 = 5;
    const HULL: u8 = 6;
    const DARK: u8 = 7;
    const CYAN: u8 = 8;

    let (x, y, z) = (position.x, position.y, position.z);
    let mut material = None;
    let half_width = if z >= 34 {
        18 - (z - 34)
    } else if z <= -36 {
        10 + (z + 48) / 2
    } else {
        18
    }
    .clamp(4, 18);
    let roof_y = if z >= 40 {
        15
    } else if z <= -42 {
        17
    } else {
        21
    };

    if (-48..=48).contains(&z) && x.abs() <= half_width && (0..=roof_y).contains(&y) {
        let outer_shell = x.abs() == half_width || y == 0 || y == roof_y;
        let deck = matches!(y, 7 | 14) && x.abs() < half_width;
        if outer_shell || deck {
            material = Some(if y == 0 || deck { FLOOR } else { HULL });
        }
    }

    // Sealed room divisions retain a centered route through all three decks.
    for bulkhead_z in [-34, -18, 6, 24, 36] {
        let doorway = x.abs() <= 2 && matches!(y, 1..=5 | 8..=12 | 15..=19);
        if z == bulkhead_z && x.abs() < half_width && (1..roof_y).contains(&y) && !doorway {
            material = Some(DARK);
        }
    }

    // Layered armor, landing outriggers, dorsal sensor spine, and raised red hull bands.
    for side in [-1, 1] {
        if voxel_point_in_box(
            position,
            IVec3::new(side * 24, 4, -10),
            IVec3::new(side * 19, 10, 26),
        ) {
            material = Some(if y == 7 || z % 12 == 0 { HULL } else { DARK });
        }
        if x == side * (half_width + 1) && matches!(y, 3 | 4 | 17 | 18) && (-30..=34).contains(&z) {
            material = Some(RED);
        }
        if x == side * half_width
            && (9..=11).contains(&y)
            && matches!(z, -28..=-25 | -10..=-7 | 10..=13 | 28..=31)
        {
            material = Some(CYAN);
        }
    }
    if voxel_point_in_hollow_box(
        position,
        IVec3::new(-7, 21, -4),
        IVec3::new(7, 23, 18),
    ) {
        material = Some(HULL);
    }
    if voxel_point_in_box(
        position,
        IVec3::new(-1, 22, 4),
        IVec3::new(1, 23, 13),
    ) {
        material = Some(RED);
    }

    // Three cyan engine bells with red drive cores and armored nacelles.
    for engine_x in [-15, 0, 15] {
        let dx = (x - engine_x).abs();
        if dx <= 4 && (4..=12).contains(&y) && (-48..=-38).contains(&z) {
            if dx == 4 || matches!(y, 4 | 12) || z == -38 {
                material = Some(DARK);
            }
            if z <= -44 && dx <= 2 && (6..=10).contains(&y) {
                material = Some(if z == -48 { CYAN } else { RED });
            }
        }
    }

    // Panoramic forward bridge glazing.
    if z >= 41
        && x.abs() <= half_width
        && (4..=11).contains(&y)
        && (z == 48 || x.abs() == half_width)
    {
        material = Some(CYAN);
    }

    // Functional rooms and tactical cover across engineering, cargo, crew, and bridge decks.
    for (min, max, prop) in [
        (
            IVec3::new(-2, 1, -31),
            IVec3::new(2, 5, -25),
            CYAN,
        ),
        (
            IVec3::new(-12, 1, -30),
            IVec3::new(-9, 3, -26),
            RED,
        ),
        (
            IVec3::new(9, 1, -30),
            IVec3::new(12, 3, -26),
            RED,
        ),
        (
            IVec3::new(-14, 1, -13),
            IVec3::new(-10, 4, -9),
            METAL,
        ),
        (
            IVec3::new(8, 1, -12),
            IVec3::new(13, 3, -7),
            METAL,
        ),
        (
            IVec3::new(-13, 1, -3),
            IVec3::new(-9, 3, 2),
            HULL,
        ),
        (
            IVec3::new(9, 1, -1),
            IVec3::new(13, 4, 3),
            HULL,
        ),
        (
            IVec3::new(-17, 2, 10),
            IVec3::new(-15, 5, 18),
            DARK,
        ),
        (
            IVec3::new(-15, 8, -12),
            IVec3::new(-11, 9, -7),
            HULL,
        ),
        (
            IVec3::new(-15, 11, -12),
            IVec3::new(-11, 12, -7),
            HULL,
        ),
        (
            IVec3::new(-15, 8, -3),
            IVec3::new(-11, 9, 2),
            HULL,
        ),
        (
            IVec3::new(-15, 11, -3),
            IVec3::new(-11, 12, 2),
            HULL,
        ),
        (
            IVec3::new(9, 8, -10),
            IVec3::new(15, 9, -4),
            METAL,
        ),
        (
            IVec3::new(10, 8, 10),
            IVec3::new(15, 12, 12),
            DARK,
        ),
        (
            IVec3::new(-15, 8, 10),
            IVec3::new(-11, 11, 17),
            DARK,
        ),
        (
            IVec3::new(-13, 15, -8),
            IVec3::new(-9, 17, -3),
            HULL,
        ),
        (
            IVec3::new(9, 15, -8),
            IVec3::new(13, 17, -3),
            HULL,
        ),
        (
            IVec3::new(-6, 15, 9),
            IVec3::new(6, 16, 15),
            METAL,
        ),
        (
            IVec3::new(-12, 15, 28),
            IVec3::new(-8, 17, 33),
            DARK,
        ),
        (
            IVec3::new(8, 15, 28),
            IVec3::new(12, 17, 33),
            DARK,
        ),
        (
            IVec3::new(-9, 8, 39),
            IVec3::new(-4, 10, 43),
            CYAN,
        ),
        (
            IVec3::new(4, 8, 39),
            IVec3::new(9, 10, 43),
            CYAN,
        ),
    ] {
        if voxel_point_in_box(position, min, max) {
            material = Some(prop);
        }
    }

    // Ladder/lift trunks connect decks without blocking the main corridor.
    for trunk_z in [-20, 20] {
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
            IVec3::new(-40, 0, 0),
            IVec3::Z,
            5,
            8,
            3.5,
        ),
        make_voxel_auto_door(
            IVec3::new(40, 0, 0),
            IVec3::Z,
            5,
            8,
            3.5,
        ),
        make_voxel_auto_door(
            IVec3::new(-18, 0, 160),
            IVec3::Z,
            3,
            5,
            3.5,
        ),
    ];
    for station_x in [-90, 90] {
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
    for bulkhead_z in [116, 132, 156, 174, 186] {
        for deck_y in [0, 7, 14] {
            doors.push(make_voxel_auto_door(
                IVec3::new(0, deck_y, bulkhead_z),
                IVec3::X,
                2,
                5,
                1.75,
            ));
        }
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
    VoxelAutoDoor {
        cells,
        trigger_center,
        trigger_radius,
        material: 7,
        open: false,
    }
}

fn setup_voxel_auto_doors(mut commands: Commands) {
    for door in voxel_auto_doors() {
        commands.spawn(door);
    }
}

fn voxel_interior_lights() -> Vec<(Vec3, Color)> {
    let mut lights = Vec::new();
    for (station_x, color) in [
        (-90, Color::srgb(0.55, 0.75, 1.0)),
        (90, Color::srgb(1.0, 0.72, 0.42)),
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
        for z in [120, 150, 184] {
            for x in [-10, 10] {
                lights.push((
                    (Vec3::new(x as f32, (deck_y + 5) as f32, z as f32) + Vec3::splat(0.5))
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

fn voxel_sample_prop_specs() -> Vec<(&'static str, Transform)> {
    let mut specs = Vec::new();
    let mut add = |path, position, scale, yaw| {
        specs.push((
            path,
            Transform::from_translation(position)
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::splat(scale)),
        ));
    };

    for station_x in [-22.5, 22.5] {
        add(
            "models/free_sample/SatelliteDish_1.gltf",
            Vec3::new(station_x, 14.55, 0.0),
            2.2,
            if station_x < 0.0 { 0.55 } else { -0.55 },
        );
        for (offset_x, z, yaw) in [(-4.0, -3.5, 0.2), (4.0, -3.5, -0.2), (0.0, 4.0, 0.0)] {
            add(
                "models/free_sample/SolarPanel_4.gltf",
                Vec3::new(station_x + offset_x, 14.55, z),
                2.0,
                yaw,
            );
        }
        for deck_y in [0.28, 2.53, 4.78, 7.03] {
            for z in [-5.5, 5.5] {
                add(
                    "models/free_sample/Prop_15.gltf",
                    Vec3::new(station_x, deck_y, z),
                    1.35,
                    if z < 0.0 { 0.0 } else { std::f32::consts::PI },
                );
            }
        }
    }

    for (x, z, yaw) in [(-3.6, 31.5, 0.35), (0.0, 37.0, 0.0), (3.6, 42.0, -0.35)] {
        add(
            "models/free_sample/Lander.gltf",
            Vec3::new(x, 0.28, z),
            0.46,
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
            "models/free_sample/BuildingBlock_2.gltf",
            Vec3::new(x, 0.28, z),
            0.52,
            yaw,
        );
    }
    for deck_y in [0.28, 2.03, 3.78] {
        for (x, z, yaw) in [(-3.7, 29.5, 0.0), (3.7, 29.5, std::f32::consts::PI)] {
            add(
                "models/free_sample/Prop_14.gltf",
                Vec3::new(x, deck_y, z),
                1.25,
                yaw,
            );
        }
    }
    for (x, z, yaw) in [
        (-2.0, 47.0, 0.0),
        (0.0, 48.0, 0.0),
        (2.0, 47.0, std::f32::consts::PI),
    ] {
        add(
            "models/free_sample/Prop_15.gltf",
            Vec3::new(x, 2.03, z),
            1.55,
            yaw,
        );
    }
    specs
}

fn setup_voxel_sample_props(mut commands: Commands, asset_server: Res<AssetServer>) {
    for (path, transform) in voxel_sample_prop_specs() {
        commands.spawn((
            WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(path))),
            transform,
        ));
    }
}

fn animate_voxel_auto_doors(
    editor: Res<VoxelEditorState>,
    players: Query<&Transform, With<VoxelFirstPersonPlayer>>,
    mut doors: Query<&mut VoxelAutoDoor>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
) {
    let (Ok(player), Ok(mut grid)) = (players.single(), grids.single_mut()) else {
        return;
    };
    for mut door in &mut doors {
        let should_open = editor.first_person_enabled
            && player.translation.distance(door.trigger_center) <= door.trigger_radius;
        let desired_material = if should_open { 0 } else { door.material };
        let cells_need_update = door
            .cells
            .iter()
            .any(|cell| grid.get(*cell).copied().unwrap_or(0) != desired_material);
        if should_open == door.open && !cells_need_update {
            continue;
        }
        for cell in &door.cells {
            grid.set(*cell, desired_material);
        }
        door.open = should_open;
    }
}

fn setup_voxel_view(
    mut commands: Commands,
    editor: Res<VoxelEditorState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: 8_500.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(8.0, 16.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.055, 0.065, 0.075)),
            ..default()
        },
        editor_camera_transform(&editor),
        VoxelViewportCamera,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(
            TEST_GROUND_SIZE.x,
            TEST_GROUND_SIZE.y,
            TEST_GROUND_SIZE.z,
        ))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.18, 0.2),
            perceptual_roughness: 0.95,
            ..default()
        })),
        RigidBody::Static,
        Collider::cuboid(
            TEST_GROUND_SIZE.x,
            TEST_GROUND_SIZE.y,
            TEST_GROUND_SIZE.z,
        ),
        Transform::from_xyz(0.0, TEST_GROUND_CENTER_Y, 0.0),
        VoxelTestingGround,
    ));
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
                &occupied,
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
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let seconds = time.elapsed_secs();
    if let Some(mut water) = materials.get_mut(&voxel_materials.handles[3]) {
        water.uv_transform = Affine2::from_translation(Vec2::new(
            seconds * 0.035,
            (seconds * 0.021).sin() * 0.08,
        ));
    }
    if let Some(mut lava) = materials.get_mut(&voxel_materials.handles[4]) {
        lava.uv_transform = Affine2::from_translation(Vec2::new(
            seconds * -0.018,
            seconds * 0.027,
        ));
        let pulse = 4.5 + (seconds * 2.4).sin() * 1.2;
        lava.emissive = LinearRgba::rgb(pulse, pulse * 0.11, 0.015);
    }
}

fn handle_editor_requests(
    mut commands: Commands,
    mut editor: ResMut<VoxelEditorState>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    physics_bodies: Query<Entity, With<VoxelPhysicsBody>>,
) {
    let Ok(mut grid) = grids.single_mut() else {
        return;
    };
    if editor.reset_requested {
        for entity in &physics_bodies {
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
    let min = ((origin - Vec3::splat(radius)) / VOXEL_SIZE)
        .floor()
        .as_ivec3();
    let max = ((origin + Vec3::splat(radius)) / VOXEL_SIZE)
        .floor()
        .as_ivec3();
    let radius_squared = radius * radius;
    prism(min, max + IVec3::ONE)
        .filter_map(|cell| {
            let material = grid.get(cell).copied()?;
            if !TrpgVoxelConnector::solid(&material) {
                return None;
            }
            let center = (cell.as_vec3() + Vec3::splat(0.5)) * VOXEL_SIZE;
            (center.distance_squared(origin) <= radius_squared).then_some((cell, material))
        })
        .collect()
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
        let snapshot_number = editor.next_scene_snapshot_number;
        editor.next_scene_snapshot_number += 1;
        editor.scene_snapshots.push(VoxelSceneSnapshot {
            name: format!("场景快照 {snapshot_number}"),
            voxels,
            physics_bodies,
        });
        if editor.scene_snapshots.len() > MAX_SCENE_SNAPSHOTS {
            editor.scene_snapshots.remove(0);
        }
        let snapshot = editor.scene_snapshots.last().unwrap();
        editor.physics_status = Some(format!(
            "已保存 {}：{} 个方块，{} 个物理体",
            snapshot.name,
            snapshot.voxels.len(),
            snapshot.physics_bodies.len()
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

    editor.undo.clear();
    editor.redo.clear();
    editor.active_stroke.clear();
    editor.stroke_positions.clear();
    editor.selection_anchor = None;
    editor.selection_end = None;
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

fn edit_voxel_grid(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    physics_bodies: Query<(
        Entity,
        &VoxelPhysicsBody,
        &Transform,
        &LinearVelocity,
        &AngularVelocity,
    )>,
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
    if mouse.just_released(MouseButton::Left) {
        editor.stroke_positions.clear();
        editor.edit_hold_seconds = 0.0;
        editor.edit_repeat_seconds = 0.0;
        if !editor.active_stroke.is_empty() {
            let stroke = std::mem::take(&mut editor.active_stroke);
            editor.undo.push(stroke);
            editor.redo.clear();
        }
        editor.left_started_over_ui = false;
    }
    if voxel_world_pointer_blocked(
        egui_owns_pointer,
        editor.left_started_over_ui,
    ) {
        return;
    }
    if let Some(action) = force_tool_action(editor.mode) {
        if !mouse.just_pressed(MouseButton::Left) {
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
        if !mouse.just_pressed(MouseButton::Left) {
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
        if let Some(cell) = raycast_grid(&grid, ray).and_then(|hit| hit.occupied) {
            editor.select_physics_corner(cell);
        }
        return;
    }
    if !mouse.pressed(MouseButton::Left) {
        return;
    }
    let just_pressed = mouse.just_pressed(MouseButton::Left);
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
    let Some(hit) = raycast_grid(&grid, ray) else {
        return;
    };
    let center = match editor.mode {
        VoxelEditMode::Add => hit.add,
        VoxelEditMode::Remove | VoxelEditMode::Paint => hit.occupied,
        VoxelEditMode::Physics
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
                let Some(after) = edited_voxel(editor.mode, before, editor.material) else {
                    continue;
                };
                if before != after {
                    grid.set(position, after);
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

fn voxel_world_pointer_blocked(egui_owns_pointer: bool, left_started_over_ui: bool) -> bool {
    egui_owns_pointer || left_started_over_ui
}

fn edited_voxel(mode: VoxelEditMode, before: u8, material: u8) -> Option<u8> {
    match mode {
        VoxelEditMode::Add => Some(material),
        VoxelEditMode::Remove => Some(0),
        VoxelEditMode::Paint => (before != 0).then_some(material),
        VoxelEditMode::Physics
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

fn control_first_person_player(
    mut commands: Commands,
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut editor: ResMut<VoxelEditorState>,
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &ShapeHits,
            &mut LinearVelocity,
            &mut ConstantLinearAcceleration,
            Has<Sensor>,
        ),
        With<VoxelFirstPersonPlayer>,
    >,
) {
    let Ok((entity, mut transform, ground_hits, mut velocity, mut acceleration, is_sensor)) =
        players.single_mut()
    else {
        return;
    };
    if transform.translation.y < -10.0 {
        transform.translation = FIRST_PERSON_START;
        velocity.0 = Vec3::ZERO;
    }
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
    velocity.x = movement.x * FIRST_PERSON_SPEED;
    velocity.z = movement.z * FIRST_PERSON_SPEED;

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
    if keyboard.just_pressed(KeyCode::Escape) && editor.first_person_enabled {
        editor.first_person_enabled = false;
        editor.first_person_flying = false;
    }
    if let Ok(mut cursor) = cursor_options.single_mut() {
        if editor.first_person_enabled {
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
            editor.camera_focus = Vec3::new(0.0, 4.0, 20.0);
            editor.camera_distance = 80.0;
            editor.camera_yaw = 0.7;
            editor.camera_pitch = -0.45;
        }
        editor.view_reset_requested = false;
    }
    let delta = motion.read().fold(Vec2::ZERO, |sum, event| {
        sum + event.delta
    });
    if editor.first_person_enabled {
        editor.camera_yaw -= delta.x * 0.0025;
        editor.camera_pitch = (editor.camera_pitch - delta.y * 0.0025).clamp(-1.5, 1.5);
        let tool_steps = wheel.read().fold(0, |steps, event| {
            steps - event.y.signum() as i32
        });
        if tool_steps != 0 {
            editor.mode = editor.mode.cycle(tool_steps);
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
    if mouse.just_pressed(MouseButton::Right) || mouse.just_pressed(MouseButton::Middle) {
        editor.camera_drag_started_in_viewport =
            cursor_in_viewport && !egui_input.wants_pointer_input();
    }
    if !mouse.pressed(MouseButton::Right) && !mouse.pressed(MouseButton::Middle) {
        editor.camera_drag_started_in_viewport = false;
    }

    if editor.camera_drag_started_in_viewport && mouse.pressed(MouseButton::Right) {
        editor.camera_yaw -= delta.x * 0.006;
        editor.camera_pitch = (editor.camera_pitch - delta.y * 0.006).clamp(-1.45, 1.2);
    }
    if editor.camera_drag_started_in_viewport && mouse.pressed(MouseButton::Middle) {
        let rotation = Quat::from_euler(
            EulerRot::YXZ,
            editor.camera_yaw,
            editor.camera_pitch,
            0.0,
        );
        editor.camera_focus += rotation * Vec3::new(delta.x, -delta.y, 0.0) * 0.006;
    }
    if cursor_in_viewport && !egui_input.wants_pointer_input() {
        let scroll = wheel.read().map(|event| event.y).sum::<f32>();
        editor.camera_distance =
            (editor.camera_distance * (-scroll * 0.12).exp()).clamp(8.0, 180.0);
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
    spatial_query: SpatialQuery,
    editor: Res<VoxelEditorState>,
    egui_input: Res<EguiWantsInput>,
) {
    let Ok(grid) = grids.single() else {
        return;
    };
    let (hit, explosion_origin) = if voxel_world_pointer_blocked(
        egui_input.wants_any_pointer_input(),
        editor.left_started_over_ui,
    ) {
        (None, None)
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
            body_hit
                .as_ref()
                .filter(|body_hit| {
                    static_distance.is_none_or(|distance| body_hit.distance < distance)
                })
                .map(|body_hit| ray.origin + *ray.direction * body_hit.distance)
                .or_else(|| static_distance.map(|distance| ray.origin + *ray.direction * distance))
        } else {
            None
        };
        (hit, explosion_origin)
    };
    let target = match (editor.mode, hit) {
        (VoxelEditMode::Add, Some(hit)) => hit.add,
        (
            VoxelEditMode::Remove
            | VoxelEditMode::Paint
            | VoxelEditMode::Physics
            | VoxelEditMode::Push
            | VoxelEditMode::Pull
            | VoxelEditMode::Explode,
            Some(hit),
        ) => hit.occupied,
        (_, None) => None,
    };
    if editor.mode == VoxelEditMode::Physics {
        let selection_end = editor.selection_end.or(target);
        if let (Some(start), Some(end)) = (editor.selection_anchor, selection_end) {
            let (min, max) = (start.min(end), start.max(end));
            let size = (max - min + IVec3::ONE).as_vec3() * VOXEL_SIZE;
            let center = (min.as_vec3() + (max - min + IVec3::ONE).as_vec3() * 0.5) * VOXEL_SIZE;
            gizmos.cube(
                Transform::from_translation(center).with_scale(size),
                Color::srgb(0.15, 0.9, 1.0),
            );
            if editor.selection_end.is_some() {
                for (cell, _) in selected_solid_voxels(grid, min, max) {
                    let center = (cell.as_vec3() + Vec3::splat(0.5)) * VOXEL_SIZE;
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
    fn initializes_populated_trpg_grid() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();
        assert!(grid.count() > 225);
    }

    #[test]
    fn default_space_map_has_two_station_interiors_and_a_corvette_interior() {
        let (app, entity) = test_grid();
        let grid = app.world().entity(entity).get::<Grid<u8>>().unwrap();

        for station_center in [IVec3::new(-90, 0, 0), IVec3::new(90, 0, 0)] {
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
            grid.get(IVec3::new(1, 0, 150)).copied(),
            Some(2)
        );
        assert_eq!(
            grid.get(IVec3::new(1, 4, 150)).copied().unwrap_or(0),
            0
        );
        assert_eq!(
            grid.get(IVec3::new(1, 14, 150)).copied(),
            Some(2)
        );

        let old_station_interior_volume = 19 * 7 * 15;
        let new_station_interior_volume = 99 * 35 * 79;
        let old_station_floor_area = 19 * 15;
        let new_station_floor_area = 99 * 79 * 5;
        assert!(new_station_interior_volume >= old_station_interior_volume * 100);
        assert!(new_station_floor_area >= old_station_floor_area * 100);
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
                combat_corvette_voxel(IVec3::new(0, walkway_y, 6)),
                None
            );
        }

        for (position, expected_material) in [
            (IVec3::new(0, 3, -28), 8),
            (IVec3::new(-12, 2, -11), 3),
            (IVec3::new(-13, 8, -10), 6),
            (IVec3::new(11, 8, -8), 3),
            (IVec3::new(10, 15, -5), 6),
            (IVec3::new(0, 15, 12), 3),
            (IVec3::new(6, 8, 41), 8),
            (IVec3::new(15, 8, -48), 8),
            (IVec3::new(19, 3, 0), 5),
            (IVec3::new(18, 10, 10), 8),
        ] {
            assert_eq!(
                combat_corvette_voxel(position),
                Some(expected_material),
                "unexpected corvette voxel at {position:?}"
            );
        }
        assert_eq!(
            combat_corvette_voxel(IVec3::new(0, 11, 42)),
            None
        );
    }

    #[test]
    fn first_person_character_is_exactly_quarter_scale() {
        let total_height = FIRST_PERSON_BODY_LENGTH + FIRST_PERSON_RADIUS * 2.0;
        assert!((total_height - 0.125).abs() < f32::EPSILON);
        assert!((FIRST_PERSON_RADIUS - 0.03).abs() < f32::EPSILON);
        assert!((FIRST_PERSON_EYE_OFFSET - 0.045).abs() < f32::EPSILON);
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
            HashSet::from([1, 2, 3, 4, 5, 6, 7, 8])
        );

        let doors = voxel_auto_doors();
        assert_eq!(doors.len(), 50);
        assert!(doors.iter().take(2).all(|door| door.cells.len() == 88));
        assert_eq!(doors[2].cells.len(), 35);
        assert!(doors[3..35].iter().all(|door| door.cells.len() == 63));
        assert!(doors[35..].iter().all(|door| door.cells.len() == 25));
        assert!(doors
            .iter()
            .flat_map(|door| &door.cells)
            .all(|cell| { grid.get(*cell).copied() == Some(7) }));

        let lights = voxel_interior_lights();
        assert_eq!(lights.len(), 62);
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
        });

        assert_eq!(editor.scene_snapshot_labels(), vec![
            "场景快照 1（1 方块 / 0 物理体）"
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
            Vec3::new(-19.875, 30.0, 2.625),
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
    fn connector_treats_zero_as_air() {
        assert!(!TrpgVoxelConnector::solid(&0));
        assert!(TrpgVoxelConnector::solid(&1));
        assert!(!TrpgVoxelConnector::solid(&4));
        assert!(!TrpgVoxelConnector::solid(&5));
        assert!(TrpgVoxelConnector::solid(&6));
        assert!(TrpgVoxelConnector::solid(&7));
        assert!(TrpgVoxelConnector::solid(&8));
    }

    #[test]
    fn sample_pack_props_detail_station_roofs_and_corvette_interiors() {
        let specs = voxel_sample_prop_specs();
        assert_eq!(specs.len(), 40);
        assert!(specs.iter().any(|(path, _)| path.ends_with("Lander.gltf")));
        assert!(specs
            .iter()
            .any(|(path, _)| path.ends_with("SatelliteDish_1.gltf")));
        assert!(specs
            .iter()
            .any(|(path, _)| path.ends_with("SolarPanel_4.gltf")));
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
        editor.select_physics_corner(IVec3::new(4, 1, -2));
        editor.select_physics_corner(IVec3::new(-1, 3, 5));
        assert_eq!(
            editor.selection_bounds(),
            Some((
                IVec3::new(-1, 1, -2),
                IVec3::new(4, 3, 5)
            ))
        );
    }

    #[test]
    fn force_tools_map_to_left_click_actions() {
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
    fn first_person_mouse_wheel_cycles_and_wraps_all_tools() {
        assert_eq!(
            VoxelEditMode::Add.cycle(1),
            VoxelEditMode::Remove
        );
        assert_eq!(
            VoxelEditMode::Add.cycle(-1),
            VoxelEditMode::Explode
        );
        assert_eq!(
            VoxelEditMode::Physics.cycle(3),
            VoxelEditMode::Explode
        );
        assert_eq!(
            VoxelEditMode::Explode.cycle(1),
            VoxelEditMode::Add
        );
        assert_eq!(VoxelEditMode::Pull.label(), "拉近");
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
}
