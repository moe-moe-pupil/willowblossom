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
    window::PrimaryWindow,
};
use bevy_egui::input::EguiWantsInput;
use voxxelmaxx::prelude::*;

const VOXEL_SIZE: f32 = 0.25;
const MAX_RAY_DISTANCE: f32 = 80.0;
const EDIT_REPEAT_DELAY: f32 = 0.32;
const EDIT_REPEAT_INTERVAL: f32 = 0.09;

pub struct TrpgVoxelPlugin;

pub struct TrpgVoxelConnector;

impl Connector for TrpgVoxelConnector {
    type Item = u8;

    fn solid(voxel: &Self::Item) -> bool { matches!(*voxel, 1..=3) }
}

#[derive(Component)]
pub struct TrpgVoxelGrid;

#[derive(Component)]
struct VoxelViewportCamera;

#[derive(Component)]
struct VoxelGeometry;

#[derive(Component)]
struct VoxelPhysicsBody {
    local_center: Vec3,
}

#[derive(Resource)]
struct VoxelMaterials {
    handles: [Handle<StandardMaterial>; 5],
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
    selection_anchor: Option<IVec3>,
    selection_end: Option<IVec3>,
    physics_status: Option<String>,
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
            camera_focus: Vec3::new(0.0, 0.75, 0.0),
            camera_distance: 20.0,
            camera_yaw: 0.7,
            camera_pitch: -0.45,
            camera_drag_started_in_viewport: false,
            selection_anchor: None,
            selection_end: None,
            physics_status: None,
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
                rebuild_voxel_geometry,
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
    let handles = std::array::from_fn(|index| {
        let mut material = StandardMaterial {
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
        };
        if index == 3 {
            material.base_color = Color::srgba(0.72, 0.9, 1.0, 0.72);
            material.alpha_mode = AlphaMode::Blend;
            material.perceptual_roughness = 0.18;
            material.reflectance = 0.65;
        } else if index == 4 {
            material.emissive_texture = Some(textures[index].clone());
            material.emissive = LinearRgba::rgb(5.0, 0.55, 0.02);
            material.perceptual_roughness = 0.55;
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
    for x in -7..=7 {
        for z in -7..=7 {
            let height = 1 + ((x * x + z * z) % 3 == 0) as i32;
            for y in 0..height {
                grid.set(IVec3::new(x, y, z), 1);
            }
        }
    }

    for y in 2_i32..=7 {
        let radius = 8 - y;
        for x in -radius..=radius {
            for z in -radius..=radius {
                if x.abs() + z.abs() <= radius && (x + z + y) % 2 == 0 {
                    grid.set(IVec3::new(x, y, z), 2);
                }
            }
        }
    }
}

fn setup_voxel_view(mut commands: Commands, editor: Res<VoxelEditorState>) {
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

    for material in 1..=5 {
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
    let mut collider_voxels = Vec::with_capacity(component.len());
    let mut local_cells = Vec::with_capacity(component.len());
    for (cell, material) in component {
        let local = cell - origin;
        local_cells.push((local, material));
        collider_voxels.push(local);
    }
    let (material_meshes, _) = build_voxel_meshes_from_cells(&local_cells);
    let local_max = collider_voxels
        .iter()
        .copied()
        .reduce(IVec3::max)
        .unwrap_or(IVec3::ZERO);
    let local_center = (local_max + IVec3::ONE).as_vec3() * VOXEL_SIZE * 0.5;
    commands
        .spawn((
            VoxelPhysicsBody { local_center },
            RigidBody::Dynamic,
            Collider::voxels(
                Vec3::splat(VOXEL_SIZE),
                &collider_voxels,
            ),
            ConstantLinearAcceleration::new(0.0, -9.81, 0.0),
            LinearDamping(0.15),
            AngularDamping(0.35),
            Transform::from_translation(origin.as_vec3() * VOXEL_SIZE),
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
    physics_bodies: Query<(), With<VoxelPhysicsBody>>,
    spatial_query: SpatialQuery,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
    mut editor: ResMut<VoxelEditorState>,
    egui_input: Res<EguiWantsInput>,
) {
    if mouse.just_released(MouseButton::Left) {
        editor.stroke_positions.clear();
        editor.edit_hold_seconds = 0.0;
        editor.edit_repeat_seconds = 0.0;
        if !editor.active_stroke.is_empty() {
            let stroke = std::mem::take(&mut editor.active_stroke);
            editor.undo.push(stroke);
            editor.redo.clear();
        }
    }
    if egui_input.wants_pointer_input() {
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
        let Some(cursor) = window.cursor_position() else {
            return;
        };
        if !editor.contains_cursor(cursor) {
            return;
        }
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
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

        let (target, interaction_distance) = if body_is_closest {
            let hit = body_hit.unwrap();
            (Some(hit.entity), hit.distance)
        } else {
            let Some(hit) = grid_hit.filter(|hit| hit.occupied.is_some()) else {
                return;
            };
            let center = hit.occupied.unwrap();
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
            (Some(entity), hit.distance)
        };
        let interaction_point = ray.origin + *ray.direction * interaction_distance;
        editor.physics_action_requested = Some(VoxelPhysicsRequest {
            action,
            target,
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
        let Some(cursor) = window.cursor_position() else {
            return;
        };
        if !editor.contains_cursor(cursor) {
            return;
        }
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
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
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if !editor.contains_cursor(cursor) {
        return;
    }
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
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

fn control_voxel_camera(
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Transform, With<VoxelViewportCamera>>,
    mut editor: ResMut<VoxelEditorState>,
    egui_input: Res<EguiWantsInput>,
) {
    if editor.view_reset_requested {
        editor.camera_focus = Vec3::new(0.0, 0.75, 0.0);
        editor.camera_distance = 20.0;
        editor.camera_yaw = 0.7;
        editor.camera_pitch = -0.45;
        editor.view_reset_requested = false;
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

    let delta = motion.read().fold(Vec2::ZERO, |sum, event| {
        sum + event.delta
    });
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
        editor.camera_distance = (editor.camera_distance * (-scroll * 0.12).exp()).clamp(8.0, 60.0);
    } else {
        wheel.clear();
    }

    if let Ok(mut transform) = cameras.single_mut() {
        *transform = editor_camera_transform(&editor);
    }
}

fn draw_voxel_target(
    mut gizmos: Gizmos,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    grids: Query<&Grid<u8>, With<TrpgVoxelGrid>>,
    editor: Res<VoxelEditorState>,
    egui_input: Res<EguiWantsInput>,
) {
    let Ok(grid) = grids.single() else {
        return;
    };
    let hit = if egui_input.wants_pointer_input() {
        None
    } else {
        let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single())
        else {
            return;
        };
        window
            .cursor_position()
            .filter(|cursor| editor.contains_cursor(*cursor))
            .and_then(|cursor| camera.viewport_to_world(camera_transform, cursor).ok())
            .and_then(|ray| raycast_grid(grid, ray))
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
    if let Some(target) = target {
        let size = if editor.mode == VoxelEditMode::Physics {
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
            Vec3::new(0.125, 4.0, 0.125),
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
        assert_eq!(meshes.len(), 2);
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
