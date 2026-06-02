use bevy::{
    asset::RenderAssetUsages,
    mesh::{
        Indices,
        PrimitiveTopology,
    },
    prelude::*,
};

use super::{
    lod::PlanetChunkKey,
    sdf::SignedDistanceField,
};

const SAMPLE_GRID: usize = 18;
const CELL_COUNT: usize = 16;

#[derive(Debug)]
pub struct PlanetMeshData {
    pub key: PlanetChunkKey,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct PlanetSphereMeshSettings {
    pub latitude_segments: usize,
    pub longitude_segments: usize,
}

impl Default for PlanetSphereMeshSettings {
    fn default() -> Self {
        Self {
            latitude_segments: 96,
            longitude_segments: 192,
        }
    }
}

impl PlanetMeshData {
    pub fn is_empty(&self) -> bool { self.positions.is_empty() }

    pub fn into_mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

pub fn build_planet_sphere_mesh<S: SignedDistanceField>(
    key: PlanetChunkKey,
    sdf: &S,
    center: Vec3,
    radius: f32,
    noise_scale: f32,
    settings: PlanetSphereMeshSettings,
) -> PlanetMeshData {
    let lat_segments = settings.latitude_segments.max(8);
    let lon_segments = settings.longitude_segments.max(16);
    let mut positions = Vec::with_capacity((lat_segments + 1) * (lon_segments + 1));
    let mut normals = Vec::with_capacity((lat_segments + 1) * (lon_segments + 1));
    let mut colors = Vec::with_capacity((lat_segments + 1) * (lon_segments + 1));
    let mut indices = Vec::with_capacity(lat_segments * lon_segments * 6);

    for lat in 0..=lat_segments {
        let v = lat as f32 / lat_segments as f32;
        let theta = v * std::f32::consts::PI;
        let sin_theta = theta.sin();
        let cos_theta = theta.cos();

        for lon in 0..=lon_segments {
            let u = lon as f32 / lon_segments as f32;
            let phi = u * std::f32::consts::TAU;
            let direction = Vec3::new(
                sin_theta * phi.cos(),
                cos_theta,
                sin_theta * phi.sin(),
            )
            .normalize_or_zero();
            let position = find_surface_point(
                sdf,
                center,
                direction,
                radius,
                noise_scale,
            );
            let normal = sdf.normal(position);

            positions.push(position.to_array());
            normals.push(normal.to_array());
            colors.push(terrain_color(position - center, normal));
        }
    }

    let row = lon_segments + 1;
    for lat in 0..lat_segments {
        for lon in 0..lon_segments {
            let a = (lat * row + lon) as u32;
            let b = a + 1;
            let c = ((lat + 1) * row + lon) as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    PlanetMeshData {
        key,
        positions,
        normals,
        colors,
        indices,
    }
}

fn find_surface_point<S: SignedDistanceField>(
    sdf: &S,
    center: Vec3,
    direction: Vec3,
    radius: f32,
    noise_scale: f32,
) -> Vec3 {
    let mut low = radius - noise_scale * 1.75;
    let mut high = radius + noise_scale * 1.75;

    for _ in 0..12 {
        let mid = (low + high) * 0.5;
        let distance = sdf.distance(center + direction * mid);
        if distance < 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }

    center + direction * ((low + high) * 0.5)
}

pub fn build_surface_nets_mesh<S: SignedDistanceField>(
    key: PlanetChunkKey,
    sdf: &S,
) -> PlanetMeshData {
    let size = key.size() as f32;
    let step = size / CELL_COUNT as f32;
    let origin = key.origin.as_vec3();
    let mut samples = [[[0.0f32; SAMPLE_GRID]; SAMPLE_GRID]; SAMPLE_GRID];

    for x in 0..SAMPLE_GRID {
        for y in 0..SAMPLE_GRID {
            for z in 0..SAMPLE_GRID {
                samples[x][y][z] =
                    sdf.distance(origin + Vec3::new(x as f32, y as f32, z as f32) * step);
            }
        }
    }

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let mut cell_vertices = [[[None::<u32>; CELL_COUNT]; CELL_COUNT]; CELL_COUNT];

    for x in 0..CELL_COUNT {
        for y in 0..CELL_COUNT {
            for z in 0..CELL_COUNT {
                let mut has_inside = false;
                let mut has_outside = false;
                for dx in 0..=1 {
                    for dy in 0..=1 {
                        for dz in 0..=1 {
                            let inside = samples[x + dx][y + dy][z + dz] < 0.0;
                            has_inside |= inside;
                            has_outside |= !inside;
                        }
                    }
                }
                if !has_inside || !has_outside {
                    continue;
                }

                let mut crossing_sum = Vec3::ZERO;
                let mut crossings = 0.0;
                for &(a, b) in CELL_EDGES {
                    let va = samples[x + a[0]][y + a[1]][z + a[2]];
                    let vb = samples[x + b[0]][y + b[1]][z + b[2]];
                    if (va < 0.0) == (vb < 0.0) {
                        continue;
                    }
                    let pa = origin
                        + Vec3::new(
                            (x + a[0]) as f32,
                            (y + a[1]) as f32,
                            (z + a[2]) as f32,
                        ) * step;
                    let pb = origin
                        + Vec3::new(
                            (x + b[0]) as f32,
                            (y + b[1]) as f32,
                            (z + b[2]) as f32,
                        ) * step;
                    let t = va / (va - vb);
                    crossing_sum += pa.lerp(pb, t.clamp(0.0, 1.0));
                    crossings += 1.0;
                }

                if crossings == 0.0 {
                    continue;
                }

                let position = crossing_sum / crossings;
                let normal = sdf.normal(position);
                let vertex = positions.len() as u32;
                cell_vertices[x][y][z] = Some(vertex);
                positions.push(position.to_array());
                normals.push(normal.to_array());
                colors.push(terrain_color(position, normal));
            }
        }
    }

    connect_surface_net_cells(&samples, &cell_vertices, &mut indices);

    PlanetMeshData {
        key,
        positions,
        normals,
        colors,
        indices,
    }
}

fn connect_surface_net_cells(
    samples: &[[[f32; SAMPLE_GRID]; SAMPLE_GRID]; SAMPLE_GRID],
    cell_vertices: &[[[Option<u32>; CELL_COUNT]; CELL_COUNT]; CELL_COUNT],
    indices: &mut Vec<u32>,
) {
    for x in 0..CELL_COUNT {
        for y in 1..CELL_COUNT {
            for z in 1..CELL_COUNT {
                if (samples[x][y][z] < 0.0) != (samples[x + 1][y][z] < 0.0) {
                    push_quad(
                        [
                            cell_vertices[x][y - 1][z - 1],
                            cell_vertices[x][y][z - 1],
                            cell_vertices[x][y][z],
                            cell_vertices[x][y - 1][z],
                        ],
                        samples[x][y][z] < samples[x + 1][y][z],
                        indices,
                    );
                }
            }
        }
    }

    for x in 1..CELL_COUNT {
        for y in 0..CELL_COUNT {
            for z in 1..CELL_COUNT {
                if (samples[x][y][z] < 0.0) != (samples[x][y + 1][z] < 0.0) {
                    push_quad(
                        [
                            cell_vertices[x - 1][y][z - 1],
                            cell_vertices[x][y][z - 1],
                            cell_vertices[x][y][z],
                            cell_vertices[x - 1][y][z],
                        ],
                        samples[x][y][z] > samples[x][y + 1][z],
                        indices,
                    );
                }
            }
        }
    }

    for x in 1..CELL_COUNT {
        for y in 1..CELL_COUNT {
            for z in 0..CELL_COUNT {
                if (samples[x][y][z] < 0.0) != (samples[x][y][z + 1] < 0.0) {
                    push_quad(
                        [
                            cell_vertices[x - 1][y - 1][z],
                            cell_vertices[x][y - 1][z],
                            cell_vertices[x][y][z],
                            cell_vertices[x - 1][y][z],
                        ],
                        samples[x][y][z] < samples[x][y][z + 1],
                        indices,
                    );
                }
            }
        }
    }
}

fn push_quad(vertices: [Option<u32>; 4], flip: bool, indices: &mut Vec<u32>) {
    let [Some(a), Some(b), Some(c), Some(d)] = vertices else {
        return;
    };

    if flip {
        indices.extend_from_slice(&[a, c, b, a, d, c]);
    } else {
        indices.extend_from_slice(&[a, b, c, a, c, d]);
    }
}

fn terrain_color(position: Vec3, normal: Vec3) -> [f32; 4] {
    let slope = 1.0 - normal.y.abs();
    let height_band = ((position.length() * 0.013).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
    let color = if slope > 0.72 {
        Color::srgb(0.42, 0.40, 0.36)
    } else if height_band < 0.37 {
        Color::srgb(0.08, 0.28, 0.45)
    } else if height_band > 0.82 {
        Color::srgb(0.72, 0.76, 0.70)
    } else {
        Color::srgb(0.22, 0.47, 0.25)
    };
    color.to_linear().to_f32_array()
}

const CELL_EDGES: &[([usize; 3], [usize; 3])] = &[
    ([0, 0, 0], [1, 0, 0]),
    ([0, 1, 0], [1, 1, 0]),
    ([0, 0, 1], [1, 0, 1]),
    ([0, 1, 1], [1, 1, 1]),
    ([0, 0, 0], [0, 1, 0]),
    ([1, 0, 0], [1, 1, 0]),
    ([0, 0, 1], [0, 1, 1]),
    ([1, 0, 1], [1, 1, 1]),
    ([0, 0, 0], [0, 0, 1]),
    ([1, 0, 0], [1, 0, 1]),
    ([0, 1, 0], [0, 1, 1]),
    ([1, 1, 0], [1, 1, 1]),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet::sdf::PlanetSdf;

    #[test]
    fn sphere_chunk_crossing_creates_vertices() {
        let sdf = PlanetSdf::new(Vec3::ZERO, 32.0, 0.0, 1, 0.01);
        let mesh = build_surface_nets_mesh(
            PlanetChunkKey {
                origin: IVec3::new(16, -16, -16),
                size_exponent: 5,
                lod: 0,
            },
            &sdf,
        );

        assert!(!mesh.positions.is_empty());
        assert!(!mesh.indices.is_empty());
    }

    #[test]
    fn empty_chunk_creates_no_vertices() {
        let sdf = PlanetSdf::new(Vec3::ZERO, 32.0, 0.0, 1, 0.01);
        let mesh = build_surface_nets_mesh(
            PlanetChunkKey {
                origin: IVec3::new(256, 256, 256),
                size_exponent: 5,
                lod: 0,
            },
            &sdf,
        );

        assert!(mesh.positions.is_empty());
    }
}
