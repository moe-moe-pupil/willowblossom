use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlanetChunkKey {
    pub origin: IVec3,
    pub size_exponent: u8,
    pub lod: u8,
}

impl PlanetChunkKey {
    pub fn size(&self) -> i32 { 1i32 << self.size_exponent }

    pub fn center(&self) -> Vec3 {
        let size = self.size() as f32;
        self.origin.as_vec3() + Vec3::splat(size * 0.5)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LodShellConfig {
    pub min_chunk_size: u8,
    pub lod_levels: u8,
    pub lod_shell_size: u8,
}

pub fn lod_for_distance(distance: f32, chunk_size: f32, lod_levels: u8, shell_size: u8) -> u8 {
    if lod_levels <= 1 {
        return 0;
    }
    let shell_width = chunk_size * shell_size.max(1) as f32;
    let lod = (distance / shell_width).floor() as u8;
    lod.min(lod_levels - 1)
}

pub fn snap_to_chunk_grid(position: Vec3, size_exponent: u8) -> IVec3 {
    let size = 1i32 << size_exponent;
    let p = position.floor().as_ivec3();
    IVec3::new(
        p.x.div_euclid(size) * size,
        p.y.div_euclid(size) * size,
        p.z.div_euclid(size) * size,
    )
}

pub fn same_lod_chunk_keys(
    camera_position: Vec3,
    size_exponent: u8,
    shell_size: u8,
) -> Vec<PlanetChunkKey> {
    let size = 1i32 << size_exponent;
    let radius = shell_size.max(1) as i32;
    let snapped = snap_to_chunk_grid(camera_position, size_exponent);
    let mut keys = Vec::new();

    for x in -radius..=radius {
        for y in -radius..=radius {
            for z in -radius..=radius {
                keys.push(PlanetChunkKey {
                    origin: snapped + IVec3::new(x * size, y * size, z * size),
                    size_exponent,
                    lod: 0,
                });
            }
        }
    }

    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod_gets_coarser_with_distance() {
        assert_eq!(lod_for_distance(0.0, 16.0, 4, 2), 0);
        assert_eq!(lod_for_distance(33.0, 16.0, 4, 2), 1);
        assert_eq!(lod_for_distance(130.0, 16.0, 4, 2), 3);
    }
}
