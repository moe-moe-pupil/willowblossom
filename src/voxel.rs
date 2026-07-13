use std::collections::HashSet;

use avian3d::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
    input::mouse::{
        MouseMotion,
        MouseWheel,
    },
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

    fn solid(voxel: &Self::Item) -> bool { *voxel != 0 }
}

#[derive(Component)]
pub struct TrpgVoxelGrid;

#[derive(Component)]
struct VoxelViewportCamera;

#[derive(Component)]
struct VoxelGeometry;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum VoxelEditMode {
    #[default]
    Add,
    Remove,
    Paint,
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
        }
    }
}

impl VoxelEditorState {
    fn contains_cursor(&self, cursor: Vec2) -> bool {
        cursor.cmpge(self.viewport_min).all() && cursor.cmple(self.viewport_max).all()
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
                rebuild_voxel_geometry,
                control_voxel_camera,
                draw_voxel_target,
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
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(grid) = grids.single() else {
        return;
    };

    for entity in &old_geometry {
        commands.entity(entity).despawn();
    }

    let (mesh, colliders) = build_voxel_mesh(grid);
    if colliders.is_empty() {
        return;
    }
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.76,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        VoxelGeometry,
    ));
    commands.spawn((
        RigidBody::Static,
        Collider::compound(colliders),
        VoxelGeometry,
    ));
}

fn build_voxel_mesh(
    grid: &Grid<u8>,
) -> (
    Mesh,
    Vec<(Position, Rotation, Collider)>,
) {
    let mut positions = Vec::<[f32; 3]>::new();
    let mut normals = Vec::<[f32; 3]>::new();
    let mut colors = Vec::<[f32; 4]>::new();
    let mut indices = Vec::<u32>::new();
    let mut colliders = Vec::new();

    for (chunk_position, chunk) in grid.iter() {
        for local in prism(IVec3::ZERO, DIMS) {
            let material = chunk[local];
            if material == 0 {
                continue;
            }
            let cell = *chunk_position * DIMS + local;
            append_voxel_faces(
                grid,
                cell,
                material,
                &mut positions,
                &mut normals,
                &mut colors,
                &mut indices,
            );
            let center = (cell.as_vec3() + Vec3::splat(0.5)) * VOXEL_SIZE;
            colliders.push((
                Position::new(center),
                Rotation::IDENTITY,
                Collider::cuboid(VOXEL_SIZE, VOXEL_SIZE, VOXEL_SIZE),
            ));
        }
    }

    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices));
    (mesh, colliders)
}

fn append_voxel_faces(
    grid: &Grid<u8>,
    cell: IVec3,
    material: u8,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
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
    let color = match material {
        1 => [0.22, 0.62, 0.32, 1.0],
        2 => [0.38, 0.20, 0.09, 1.0],
        3 => [0.86, 0.72, 0.38, 1.0],
        4 => [0.08, 0.38, 0.82, 0.88],
        5 => [1.0, 0.16, 0.015, 1.0],
        _ => [0.46, 0.48, 0.52, 1.0],
    };

    for (normal, corners) in FACES {
        if grid.get(cell + normal).copied().unwrap_or(0) != 0 {
            continue;
        }
        let base = positions.len() as u32;
        for corner in corners {
            positions.push(((cell.as_vec3() + Vec3::from(corner)) * VOXEL_SIZE).to_array());
            normals.push(normal.as_vec3().to_array());
            colors.push(color);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn handle_editor_requests(
    mut editor: ResMut<VoxelEditorState>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
) {
    let Ok(mut grid) = grids.single_mut() else {
        return;
    };
    if editor.reset_requested {
        let occupied = occupied_cells(&grid);
        for position in occupied {
            grid.set(position, 0);
        }
        populate_default_grid(&mut grid);
        editor.undo.clear();
        editor.redo.clear();
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
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<VoxelViewportCamera>>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
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
    if !mouse.pressed(MouseButton::Left) || egui_input.wants_pointer_input() {
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
    }
}

struct VoxelRayHit {
    occupied: Option<IVec3>,
    add: Option<IVec3>,
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
    if egui_input.wants_pointer_input() {
        return;
    }
    let (Ok(window), Ok((camera, camera_transform)), Ok(grid)) = (
        windows.single(),
        cameras.single(),
        grids.single(),
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
    let Some(hit) = raycast_grid(grid, ray) else {
        return;
    };
    let target = match editor.mode {
        VoxelEditMode::Add => hit.add,
        VoxelEditMode::Remove | VoxelEditMode::Paint => hit.occupied,
    };
    if let Some(target) = target {
        let size = (editor.brush_radius * 2 + 1) as f32 * VOXEL_SIZE;
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
    fn connector_treats_zero_as_air() {
        assert!(!TrpgVoxelConnector::solid(&0));
        assert!(TrpgVoxelConnector::solid(&1));
        assert!(TrpgVoxelConnector::solid(&u8::MAX));
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
