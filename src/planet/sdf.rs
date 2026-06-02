use bevy::prelude::*;

pub trait SignedDistanceField: Send + Sync {
    fn distance(&self, position: Vec3) -> f32;

    fn normal(&self, position: Vec3) -> Vec3 {
        let epsilon = 1.0;
        Vec3::new(
            self.distance(position + Vec3::X * epsilon)
                - self.distance(position - Vec3::X * epsilon),
            self.distance(position + Vec3::Y * epsilon)
                - self.distance(position - Vec3::Y * epsilon),
            self.distance(position + Vec3::Z * epsilon)
                - self.distance(position - Vec3::Z * epsilon),
        )
        .try_normalize()
        .unwrap_or(Vec3::Y)
    }
}

#[derive(Debug, Clone)]
pub struct PlanetSdf {
    pub center: Vec3,
    pub radius: f32,
    pub noise_scale: f32,
    pub seed: u32,
    pub frequency: f32,
}

impl PlanetSdf {
    pub fn new(center: Vec3, radius: f32, noise_scale: f32, seed: u32, frequency: f32) -> Self {
        Self {
            center,
            radius,
            noise_scale,
            seed,
            frequency,
        }
    }

    fn displacement(&self, position: Vec3) -> f32 {
        let sample = (position - self.center) * self.frequency;
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        let mut sum = 0.0;
        let mut norm = 0.0;

        for octave in 0..4 {
            sum += smooth_value_noise(
                sample * frequency,
                self.seed.wrapping_add(octave * 9_973),
            ) * amplitude;
            norm += amplitude;
            amplitude *= 0.5;
            frequency *= 2.03;
        }

        let noise = if norm > 0.0 { sum / norm } else { 0.0 };
        let softened = if noise < 0.0 { noise * 0.5 } else { noise };
        softened * self.noise_scale
    }
}

impl SignedDistanceField for PlanetSdf {
    fn distance(&self, position: Vec3) -> f32 {
        let base_distance = position.distance(self.center) - self.radius;
        if base_distance > self.noise_scale * 1.25 {
            return base_distance;
        }
        base_distance - self.displacement(position)
    }
}

fn smooth_value_noise(position: Vec3, seed: u32) -> f32 {
    let cell = position.floor().as_ivec3();
    let local = position - cell.as_vec3();
    let fade =
        local * local * local * (local * (local * 6.0 - Vec3::splat(15.0)) + Vec3::splat(10.0));

    let mut values = [[[0.0; 2]; 2]; 2];
    for x in 0..=1 {
        for y in 0..=1 {
            for z in 0..=1 {
                values[x][y][z] = hash_noise(
                    cell + IVec3::new(x as i32, y as i32, z as i32),
                    seed,
                );
            }
        }
    }

    let x00 = lerp(values[0][0][0], values[1][0][0], fade.x);
    let x10 = lerp(values[0][1][0], values[1][1][0], fade.x);
    let x01 = lerp(values[0][0][1], values[1][0][1], fade.x);
    let x11 = lerp(values[0][1][1], values[1][1][1], fade.x);
    let y0 = lerp(x00, x10, fade.y);
    let y1 = lerp(x01, x11, fade.y);
    lerp(y0, y1, fade.z)
}

fn hash_noise(cell: IVec3, seed: u32) -> f32 {
    let mut h = seed
        ^ (cell.x as u32).wrapping_mul(0x8da6_b343)
        ^ (cell.y as u32).wrapping_mul(0xd816_3841)
        ^ (cell.z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_distance_without_noise_crosses_radius() {
        let sdf = PlanetSdf::new(Vec3::ZERO, 100.0, 0.0, 1, 0.001);

        assert!(sdf.distance(Vec3::X * 100.0).abs() < 0.001);
        assert!(sdf.distance(Vec3::X * 90.0) < 0.0);
        assert!(sdf.distance(Vec3::X * 110.0) > 0.0);
    }

    #[test]
    fn noise_displaces_surface_but_far_outside_returns_base_distance() {
        let noisy = PlanetSdf::new(Vec3::ZERO, 100.0, 12.0, 42, 0.013);
        let smooth = PlanetSdf::new(Vec3::ZERO, 100.0, 0.0, 42, 0.013);

        let surface_point = Vec3::new(57.0, 81.0, 14.0).normalize() * 100.0;
        assert_ne!(
            noisy.distance(surface_point),
            smooth.distance(surface_point)
        );

        let far_point = Vec3::X * 200.0;
        assert!((noisy.distance(far_point) - 100.0).abs() < 0.001);
    }
}
