use std::collections::{
    HashMap,
    HashSet,
    VecDeque,
};

use bevy::{
    prelude::*,
    tasks::{
        block_on,
        futures_lite::future,
        AsyncComputeTaskPool,
        Task,
    },
};

use super::{
    lod::{
        same_lod_chunk_keys,
        PlanetChunkKey,
    },
    meshing::{
        build_planet_sphere_mesh,
        build_surface_nets_mesh,
        PlanetMeshData,
        PlanetSphereMeshSettings,
    },
    sdf::PlanetSdf,
};
use crate::camera::GameCamera;

const PLANET_CHUNK_TASK_BUDGET_PER_FRAME: usize = 2;
const PLANET_CHUNK_APPLY_BUDGET_PER_FRAME: usize = 4;

pub struct PlanetTerrainPlugin;

#[derive(Resource, Debug, Clone)]
pub struct PlanetTerrainSettings {
    pub center: Vec3,
    pub radius: f32,
    pub noise_scale: f32,
    pub frequency: f32,
    pub seed: u32,
    pub root_size: u8,
    pub min_chunk_size: u8,
    pub lod_levels: u8,
    pub lod_shell_size: u8,
    pub lod_update_distance: f32,
}

impl Default for PlanetTerrainSettings {
    fn default() -> Self {
        Self {
            center: Vec3::new(0.0, -4_096.0, 0.0),
            radius: 4_096.0,
            noise_scale: 200.0,
            frequency: 0.0025,
            seed: 7_331,
            root_size: 20,
            min_chunk_size: 4,
            lod_levels: 5,
            lod_shell_size: 2,
            lod_update_distance: 32.0,
        }
    }
}

#[derive(Component)]
pub struct PlanetTerrainRoot;

#[derive(Component)]
pub struct PlanetPreviewMesh;

#[derive(Component, Clone, Copy)]
pub struct PlanetChunk {
    pub center: Vec3,
    pub size_exponent: u8,
    pub lod: u8,
    pub generation: u64,
}

#[derive(Component)]
struct PlanetMeshTask {
    key: PlanetChunkKey,
    generation: u64,
    task: Task<PlanetMeshData>,
}

#[derive(Resource, Default)]
pub struct PlanetTerrainRuntime {
    material: Handle<StandardMaterial>,
    chunks: HashMap<PlanetChunkKey, Entity>,
    queued: HashSet<PlanetChunkKey>,
    queue: VecDeque<PlanetChunkKey>,
    generation: u64,
    last_update_position: Option<Vec3>,
    last_anchor: Option<Vec3>,
    last_desired_chunks: usize,
    completed_meshes: usize,
    empty_meshes: usize,
}

impl PlanetTerrainRuntime {
    pub fn visible_chunks(&self) -> usize { self.chunks.len() }

    pub fn queued_chunks(&self) -> usize { self.queued.len() + self.queue.len() }

    pub fn last_anchor(&self) -> Option<Vec3> { self.last_anchor }

    pub fn last_desired_chunks(&self) -> usize { self.last_desired_chunks }

    pub fn completed_meshes(&self) -> usize { self.completed_meshes }

    pub fn empty_meshes(&self) -> usize { self.empty_meshes }
}

impl Plugin for PlanetTerrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlanetTerrainSettings>()
            .init_resource::<PlanetTerrainRuntime>()
            .add_systems(Startup, setup_planet_terrain)
            .add_systems(
                Update,
                (
                    refresh_planet_lod,
                    spawn_planet_mesh_tasks,
                    apply_planet_mesh_tasks,
                )
                    .chain(),
            );
    }
}

fn setup_planet_terrain(
    mut commands: Commands,
    settings: Res<PlanetTerrainSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut runtime: ResMut<PlanetTerrainRuntime>,
) {
    runtime.material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.42, 0.66, 0.58),
        emissive: Color::srgb(0.015, 0.04, 0.035).into(),
        perceptual_roughness: 0.86,
        metallic: 0.0,
        cull_mode: None,
        ..default()
    });

    let root = commands
        .spawn((PlanetTerrainRoot, Transform::default()))
        .id();
    let sdf = PlanetSdf::new(
        settings.center,
        settings.radius,
        settings.noise_scale,
        settings.seed,
        settings.frequency,
    );
    let preview_mesh = build_planet_sphere_mesh(
        PlanetChunkKey {
            origin: settings.center.floor().as_ivec3(),
            size_exponent: settings.root_size,
            lod: 0,
        },
        &sdf,
        settings.center,
        settings.radius,
        settings.noise_scale,
        PlanetSphereMeshSettings::default(),
    );
    commands.entity(root).with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(preview_mesh.into_mesh())),
            MeshMaterial3d(runtime.material.clone()),
            Transform::default(),
            PlanetPreviewMesh,
        ));
    });
}

fn refresh_planet_lod(
    mut commands: Commands,
    settings: Res<PlanetTerrainSettings>,
    mut runtime: ResMut<PlanetTerrainRuntime>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, With<GameCamera>)>,
) {
    let Some(camera_position) = cameras
        .iter()
        .next()
        .map(|transform| transform.translation())
    else {
        return;
    };

    let surface_anchor = planet_surface_anchor(camera_position, &settings);

    if let Some(last) = runtime.last_update_position {
        if surface_anchor.distance(last) < settings.lod_update_distance
            && !runtime.chunks.is_empty()
        {
            return;
        }
    }
    runtime.last_update_position = Some(surface_anchor);
    runtime.last_anchor = Some(surface_anchor);
    runtime.generation = runtime.generation.wrapping_add(1);

    let size_exponent = visible_chunk_size_exponent(&settings, camera_position);
    let desired = same_lod_chunk_keys(
        surface_anchor,
        size_exponent,
        settings.lod_shell_size,
    )
    .into_iter()
    .filter(|key| chunk_may_cross_planet(*key, &settings))
    .collect::<HashSet<_>>();
    runtime.last_desired_chunks = desired.len();

    let existing = runtime.chunks.keys().copied().collect::<Vec<_>>();
    for key in existing {
        if desired.contains(&key) {
            continue;
        }
        if let Some(entity) = runtime.chunks.remove(&key) {
            commands.entity(entity).despawn();
        }
    }

    runtime.queue.retain(|key| desired.contains(key));
    runtime.queued.retain(|key| desired.contains(key));

    for key in desired {
        if runtime.chunks.contains_key(&key) || runtime.queued.contains(&key) {
            continue;
        }
        runtime.queued.insert(key);
        runtime.queue.push_back(key);
    }
}

fn spawn_planet_mesh_tasks(
    mut commands: Commands,
    settings: Res<PlanetTerrainSettings>,
    mut runtime: ResMut<PlanetTerrainRuntime>,
) {
    let task_pool = AsyncComputeTaskPool::get();
    for _ in 0..PLANET_CHUNK_TASK_BUDGET_PER_FRAME {
        let Some(key) = runtime.queue.pop_front() else {
            break;
        };
        let sdf = PlanetSdf::new(
            settings.center,
            settings.radius,
            settings.noise_scale,
            settings.seed,
            settings.frequency,
        );
        let generation = runtime.generation;
        let task = task_pool.spawn(async move { build_surface_nets_mesh(key, &sdf) });
        commands.spawn(PlanetMeshTask {
            key,
            generation,
            task,
        });
    }
}

fn apply_planet_mesh_tasks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut runtime: ResMut<PlanetTerrainRuntime>,
    mut tasks: Query<(Entity, &mut PlanetMeshTask)>,
) {
    let mut applied = 0;
    for (task_entity, mut mesh_task) in &mut tasks {
        if applied >= PLANET_CHUNK_APPLY_BUDGET_PER_FRAME {
            break;
        }

        let Some(mesh_data) = block_on(future::poll_once(&mut mesh_task.task)) else {
            continue;
        };

        applied += 1;
        runtime.queued.remove(&mesh_task.key);
        commands.entity(task_entity).despawn();

        if mesh_task.generation != runtime.generation || mesh_data.is_empty() {
            if mesh_data.is_empty() {
                runtime.empty_meshes += 1;
            }
            continue;
        }
        runtime.completed_meshes += 1;

        if let Some(entity) = runtime.chunks.remove(&mesh_task.key) {
            commands.entity(entity).despawn();
        }

        let key = mesh_data.key;
        let entity = commands
            .spawn((
                Mesh3d(meshes.add(mesh_data.into_mesh())),
                MeshMaterial3d(runtime.material.clone()),
                Transform::default(),
                PlanetChunk {
                    center: key.center(),
                    size_exponent: key.size_exponent,
                    lod: key.lod,
                    generation: mesh_task.generation,
                },
            ))
            .id();
        runtime.chunks.insert(key, entity);
    }
}

fn visible_chunk_size_exponent(settings: &PlanetTerrainSettings, camera_position: Vec3) -> u8 {
    let altitude = (camera_position.distance(settings.center) - settings.radius).abs();
    let near_surface = settings.min_chunk_size.saturating_add(2);
    let far_surface = settings.min_chunk_size.saturating_add(4);

    if altitude < settings.noise_scale * 6.0 { near_surface } else { far_surface }
        .min(settings.root_size)
}

fn planet_surface_anchor(camera_position: Vec3, settings: &PlanetTerrainSettings) -> Vec3 {
    let outward = (camera_position - settings.center)
        .try_normalize()
        .unwrap_or(Vec3::Y);
    settings.center + outward * settings.radius
}

fn chunk_may_cross_planet(key: PlanetChunkKey, settings: &PlanetTerrainSettings) -> bool {
    let center = key.center();
    let half_diagonal = Vec3::splat(key.size() as f32 * 0.5).length();
    let distance_to_surface = (center.distance(settings.center) - settings.radius).abs();
    distance_to_surface <= half_diagonal + settings.noise_scale * 1.25
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet::{
        lod::same_lod_chunk_keys,
        meshing::build_surface_nets_mesh,
    };

    #[test]
    fn real_planet_surface_anchor_generates_meshes() {
        let settings = PlanetTerrainSettings {
            center: Vec3::new(8029.0, 3122.0, 7806.0),
            radius: 1200.0,
            noise_scale: 80.0,
            frequency: 0.006,
            seed: 7_331,
            root_size: 20,
            min_chunk_size: 4,
            lod_levels: 5,
            lod_shell_size: 2,
            lod_update_distance: 32.0,
        };
        let camera_position = Vec3::new(7020.0, 2820.0, 6825.0);
        let anchor = planet_surface_anchor(camera_position, &settings);
        let size_exponent = visible_chunk_size_exponent(&settings, camera_position);
        let keys = same_lod_chunk_keys(
            anchor,
            size_exponent,
            settings.lod_shell_size,
        )
        .into_iter()
        .filter(|key| chunk_may_cross_planet(*key, &settings))
        .collect::<Vec<_>>();

        assert!(!keys.is_empty());

        let sdf = PlanetSdf::new(
            settings.center,
            settings.radius,
            settings.noise_scale,
            settings.seed,
            settings.frequency,
        );
        let non_empty = keys
            .iter()
            .take(16)
            .filter(|key| !build_surface_nets_mesh(**key, &sdf).is_empty())
            .count();

        assert!(non_empty > 0);
    }
}
