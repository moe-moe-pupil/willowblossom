use bevy::{
    core_pipeline::{
        prepass::ViewPrepassTextures,
        schedule::{
            Core3d,
            Core3dSystems,
        },
        tonemapping::tonemapping,
        FullscreenShader,
    },
    ecs::error::BevyError,
    prelude::*,
    render::{
        extract_component::{
            ComponentUniforms,
            DynamicUniformIndex,
            ExtractComponent,
            ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{
                sampler,
                texture_2d,
                texture_3d,
                texture_depth_2d,
                uniform_buffer,
            },
            *,
        },
        renderer::{
            RenderContext,
            RenderDevice,
            ViewQuery,
        },
        texture::GpuImage,
        view::{
            ExtractedView,
            ViewTarget,
            ViewUniform,
            ViewUniformOffset,
            ViewUniforms,
        },
        Render,
        RenderApp,
        RenderStartup,
        RenderSystems,
    },
};

const SHADER_PATH: &str = "shaders/voxel_radiance_cascade.wgsl";

pub(crate) struct VoxelRadianceCascadePlugin;

impl Plugin for VoxelRadianceCascadePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<VoxelRadianceCascade>::default(),
            ExtractComponentPlugin::<VoxelRadianceCascadeUniform>::default(),
            UniformComponentPlugin::<VoxelRadianceCascadeUniform>::default(),
        ));

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_radiance_pipeline)
            .add_systems(
                Render,
                (
                    prepare_radiance_pipelines.in_set(RenderSystems::Prepare),
                    prepare_radiance_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                ),
            )
            .add_systems(
                Core3d,
                voxel_radiance_cascade
                    .in_set(Core3dSystems::PostProcess)
                    .before(tonemapping),
            );
    }
}

#[derive(Component, Clone, ExtractComponent)]
pub(crate) struct VoxelRadianceCascade {
    pub volume: Handle<Image>,
}

#[derive(Component, Clone, Copy, ExtractComponent, ShaderType)]
pub(crate) struct VoxelRadianceCascadeUniform {
    pub volume_min: Vec3,
    pub voxel_world_size: f32,
    pub volume_dimensions: Vec3,
    pub intensity: f32,
}

#[derive(Resource)]
struct RadiancePipeline {
    layout: BindGroupLayoutDescriptor,
    source_sampler: Sampler,
    volume_sampler: Sampler,
    variants: Variants<RenderPipeline, RadiancePipelineSpecializer>,
}

struct RadiancePipelineSpecializer;

#[derive(PartialEq, Eq, Hash, Clone, Copy, SpecializerKey)]
struct RadiancePipelineKey {
    target_format: TextureFormat,
}

impl Specializer<RenderPipeline> for RadiancePipelineSpecializer {
    type Key = RadiancePipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.fragment_mut()?.set_target(0, ColorTargetState {
            format: key.target_format,
            blend: None,
            write_mask: ColorWrites::ALL,
        });
        Ok(key)
    }
}

fn init_radiance_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    asset_server: Res<AssetServer>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "voxel_radiance_cascade_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
                texture_3d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<VoxelRadianceCascadeUniform>(true),
                uniform_buffer::<ViewUniform>(true),
            ),
        ),
    );
    let source_sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("voxel_radiance_source_sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    let volume_sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("voxel_radiance_volume_sampler"),
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        ..default()
    });
    let descriptor = RenderPipelineDescriptor {
        label: Some("voxel_radiance_cascade_pipeline".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen_shader.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: asset_server.load(SHADER_PATH),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba16Float,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    };
    commands.insert_resource(RadiancePipeline {
        layout,
        source_sampler,
        volume_sampler,
        variants: Variants::new(RadiancePipelineSpecializer, descriptor),
    });
}

#[derive(Component)]
struct RadiancePipelineId(CachedRenderPipelineId);

fn prepare_radiance_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<RadiancePipeline>,
    views: Query<(Entity, &ExtractedView), With<VoxelRadianceCascade>>,
) -> Result<(), BevyError> {
    for (entity, view) in &views {
        let pipeline_id = pipeline
            .variants
            .specialize(&pipeline_cache, RadiancePipelineKey {
                target_format: view.target_format,
            })?;
        commands
            .entity(entity)
            .insert(RadiancePipelineId(pipeline_id));
    }
    Ok(())
}

#[derive(Component)]
struct RadianceBindGroups {
    a: (TextureViewId, BindGroup),
    b: (TextureViewId, BindGroup),
}

fn prepare_radiance_bind_groups(
    mut commands: Commands,
    views: Query<(
        Entity,
        &ViewTarget,
        &ViewPrepassTextures,
        &VoxelRadianceCascade,
    )>,
    pipeline: Res<RadiancePipeline>,
    pipeline_cache: Res<PipelineCache>,
    settings_uniforms: Res<ComponentUniforms<VoxelRadianceCascadeUniform>>,
    view_uniforms: Res<ViewUniforms>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
) {
    let (Some(settings_binding), Some(view_binding)) = (
        settings_uniforms.uniforms().binding(),
        view_uniforms.uniforms.binding(),
    ) else {
        return;
    };
    for (entity, target, prepass, cascade) in &views {
        let Some(depth) = prepass.depth_view() else {
            continue;
        };
        let Some(volume) = gpu_images.get(&cascade.volume) else {
            continue;
        };
        let create = |source: &TextureView| {
            (
                source.id(),
                render_device.create_bind_group(
                    "voxel_radiance_cascade_bind_group",
                    &pipeline_cache.get_bind_group_layout(&pipeline.layout),
                    &BindGroupEntries::sequential((
                        source,
                        &pipeline.source_sampler,
                        depth,
                        &volume.texture_view,
                        &pipeline.volume_sampler,
                        settings_binding.clone(),
                        view_binding.clone(),
                    )),
                ),
            )
        };
        commands.entity(entity).insert(RadianceBindGroups {
            a: create(target.main_texture_view()),
            b: create(target.main_texture_other_view()),
        });
    }
}

fn voxel_radiance_cascade(
    view: ViewQuery<(
        &ViewTarget,
        &DynamicUniformIndex<VoxelRadianceCascadeUniform>,
        &ViewUniformOffset,
        &RadianceBindGroups,
        &RadiancePipelineId,
    )>,
    pipeline_cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let (target, settings_index, view_offset, bind_groups, pipeline_id) = view.into_inner();
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else {
        return;
    };
    let post_process = target.post_process_write();
    let bind_group = if bind_groups.a.0 == post_process.source.id() {
        &bind_groups.a.1
    } else {
        &bind_groups.b.1
    };
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("voxel_radiance_cascade_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post_process.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[
        settings_index.index(),
        view_offset.offset,
    ]);
    pass.draw(0..3, 0..1);
}
