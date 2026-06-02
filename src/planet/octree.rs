use bevy::prelude::*;

#[derive(Debug, Clone)]
pub struct PlanetOctreeNode {
    pub center: Vec3,
    pub size_exponent: u8,
    pub sdf_value: f32,
    pub dirty: bool,
    pub enqueued: bool,
    pub chunk_entity: Option<Entity>,
    pub children: Option<Box<[PlanetOctreeNode; 8]>>,
}

impl PlanetOctreeNode {
    pub fn new(center: Vec3, size_exponent: u8, sdf_value: f32) -> Self {
        Self {
            center,
            size_exponent,
            sdf_value,
            dirty: true,
            enqueued: false,
            chunk_entity: None,
            children: None,
        }
    }

    pub fn size(&self) -> f32 { (1u32 << self.size_exponent) as f32 }
}
