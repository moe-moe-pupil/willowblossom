use avian3d::prelude::*;
use bevy::{
    camera::visibility::RenderLayers,
    prelude::*,
};
use voxxelmaxx::prelude::*;

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
struct VoxelDisplay;

impl Plugin for TrpgVoxelPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            PhysicsPlugins::default(),
            VoxelPlugin::<u8>::default(),
            ConnectivityPlugin::<TrpgVoxelConnector>::default(),
        ))
        .insert_resource(Gravity::ZERO)
        .add_systems(
            Startup,
            (
                setup_voxel_grid,
                populate_voxel_grid,
                display_voxel_grid,
            )
                .chain(),
        )
        .add_systems(Update, orbit_voxel_camera);
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

fn display_voxel_grid(
    mut commands: Commands,
    grids: Query<&Grid<u8>, With<TrpgVoxelGrid>>,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let (Some(meshes), Some(materials)) = (meshes.as_mut(), materials.as_mut()) else {
        return;
    };
    let Ok(grid) = grids.single() else {
        return;
    };

    let cube = meshes.add(Cuboid::new(0.94, 0.94, 0.94));
    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.22, 0.62, 0.48),
        perceptual_roughness: 0.82,
        ..default()
    });
    let crystal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.96, 0.46, 0.18),
        metallic: 0.18,
        perceptual_roughness: 0.42,
        ..default()
    });

    let mut colliders = Vec::new();
    for (chunk_position, chunk) in grid.iter() {
        for local in prism(IVec3::ZERO, DIMS) {
            let material = chunk[local];
            if material == 0 {
                continue;
            }
            let position = *chunk_position * DIMS + local;
            let translation = position.as_vec3() + Vec3::Y * 0.5;
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(if material == 1 { stone.clone() } else { crystal.clone() }),
                Transform::from_translation(translation),
                RenderLayers::layer(0),
                VoxelDisplay,
            ));
            colliders.push((
                Position::new(translation),
                Rotation::IDENTITY,
                Collider::cuboid(1.0, 1.0, 1.0),
            ));
        }
    }

    commands.spawn((
        RigidBody::Static,
        Collider::compound(colliders),
        VoxelDisplay,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8_500.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(8.0, 16.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        VoxelDisplay,
    ));
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.055, 0.065, 0.075)),
            ..default()
        },
        Transform::from_xyz(20.0, 15.0, 24.0).looking_at(Vec3::new(0.0, 2.5, 0.0), Vec3::Y),
        VoxelViewportCamera,
    ));
}

fn orbit_voxel_camera(
    time: Res<Time>,
    mut cameras: Query<&mut Transform, With<VoxelViewportCamera>>,
) {
    for mut transform in &mut cameras {
        transform.rotate_around(
            Vec3::new(0.0, 2.5, 0.0),
            Quat::from_rotation_y(time.delta_secs() * 0.12),
        );
        transform.look_at(Vec3::new(0.0, 2.5, 0.0), Vec3::Y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_populated_trpg_grid() {
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

        let world = app.world_mut();
        let mut grids = world.query_filtered::<&Grid<u8>, With<TrpgVoxelGrid>>();
        let grid = grids.single(world).unwrap();
        assert!(grid.count() > 225);
    }

    #[test]
    fn connector_treats_zero_as_air() {
        assert!(!TrpgVoxelConnector::solid(&0));
        assert!(TrpgVoxelConnector::solid(&1));
        assert!(TrpgVoxelConnector::solid(&u8::MAX));
    }
}
