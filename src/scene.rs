use std::sync::Arc;

use bevy::prelude::*;
use bevy_voxel_world::prelude::*;

use crate::camera::GameCamera;

pub struct ScenePreviewPlugin;

#[derive(Resource, Clone, Default)]
pub struct TrpgVoxelWorld;

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
        .add_systems(Startup, setup_scene_preview);
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

fn setup_scene_preview(mut commands: Commands) {
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
        GameCamera,
    ));
}
