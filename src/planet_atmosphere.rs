//! 行星半透明大气外壳的共享材质与生成辅助，供体素场景和预览场景复用。

use bevy::{
    asset::Asset,
    material::AlphaMode,
    pbr::{
        Material,
        MaterialPlugin,
        MaterialPipeline,
        MaterialPipelineKey,
    },
    prelude::*,
    reflect::TypePath,
    render::{
        mesh::MeshVertexBufferLayoutRef,
        render_resource::{
            AsBindGroup,
            RenderPipelineDescriptor,
            ShaderType,
            SpecializedMeshPipelineError,
        },
    },
    shader::ShaderRef,
};

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct PlanetAtmosphereMaterial {
    #[uniform(0)]
    pub settings: PlanetAtmosphereUniform,
}

#[derive(ShaderType, Clone, Copy, Debug)]
pub struct PlanetAtmosphereUniform {
    pub planet_center: Vec4,
    pub sun_direction: Vec4,
    pub radii: Vec4,
    pub day_color: Vec4,
    pub night_color: Vec4,
    pub params: Vec4,
}

impl PlanetAtmosphereMaterial {
    pub fn new(planet_center: Vec3, planet_radius: f32, atmosphere_radius: f32) -> Self {
        Self {
            settings: PlanetAtmosphereUniform {
                planet_center: planet_center.extend(1.0),
                sun_direction: Vec4::new(0.0, 1.0, 0.0, 0.0),
                radii: Vec4::new(planet_radius, atmosphere_radius, 0.0, 0.0),
                day_color: Vec4::new(0.30, 0.44, 0.80, 1.0),
                night_color: Vec4::new(0.03, 0.06, 0.14, 1.0),
                params: Vec4::new(3.0, 1.8, 1.0, 1.0),
            },
        }
    }
}

impl Material for PlanetAtmosphereMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path("shaders/planet_atmosphere.wgsl".into())
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // 双面渲染：从星球表面仰望时也能看到大气辉光。
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// 注册大气材质，供任意包含行星的场景插件使用。
pub struct PlanetAtmospherePlugin;

impl Plugin for PlanetAtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<PlanetAtmosphereMaterial>::default());
    }
}

/// 在行星中心生成半透明大气外壳，返回材质句柄供昼夜系统更新。
pub fn spawn_atmosphere_shell(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<PlanetAtmosphereMaterial>,
    planet_center: Vec3,
    planet_radius: f32,
    atmosphere_radius: f32,
) -> Handle<PlanetAtmosphereMaterial> {
    let material = materials.add(PlanetAtmosphereMaterial::new(
        planet_center,
        planet_radius,
        atmosphere_radius,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(atmosphere_radius).mesh().uv(64, 32))),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(planet_center),
        Visibility::Visible,
    ));
    material
}

#[derive(Resource)]
pub struct PlanetAtmosphereHandles {
    pub material: Handle<PlanetAtmosphereMaterial>,
}
