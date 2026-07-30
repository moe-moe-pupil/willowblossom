#import bevy_render::view::{View, frag_coord_to_ndc, position_ndc_to_world}

@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var voxel_volume: texture_3d<f32>;
@group(0) @binding(4) var volume_sampler: sampler;

struct CascadeSettings {
    volume_min: vec3<f32>,
    voxel_world_size: f32,
    volume_dimensions: vec3<f32>,
    intensity: f32,
};

@group(0) @binding(5) var<uniform> settings: CascadeSettings;
@group(0) @binding(6) var<uniform> view: View;

fn propagated_radiance(world_position: vec3<f32>) -> vec3<f32> {
    let volume_extent = settings.volume_dimensions * settings.voxel_world_size;
    let uvw = (world_position - settings.volume_min) / volume_extent;
    if any(uvw <= vec3(0.0)) || any(uvw >= vec3(1.0)) {
        return vec3(0.0);
    }

    // CPU flood filling has already applied visibility and distance falloff.
    // Trilinear sampling turns the canonical voxel light levels into the soft,
    // stable colored block lighting used by Minecraft-style LPV shader packs.
    let radiance = textureSampleLevel(voxel_volume, volume_sampler, uvw, 0.0).rgb;
    let edge_distance = min(
        min(uvw.x, 1.0 - uvw.x),
        min(min(uvw.y, 1.0 - uvw.y), min(uvw.z, 1.0 - uvw.z)),
    );
    let edge_width = 2.0 / min(
        settings.volume_dimensions.x,
        min(settings.volume_dimensions.y, settings.volume_dimensions.z),
    );
    return radiance * smoothstep(0.0, edge_width, edge_distance);
}

@fragment
fn fragment(
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
) -> @location(0) vec4<f32> {
    let source = textureSample(source_texture, source_sampler, uv);
    let dimensions = vec2<i32>(textureDimensions(depth_texture));
    let pixel = clamp(vec2<i32>(position.xy), vec2(0), dimensions - vec2(1));
    let depth = textureLoad(depth_texture, pixel, 0);
    if depth <= 0.000001 || settings.intensity <= 0.0 {
        return source;
    }

    let world_position = position_ndc_to_world(
        frag_coord_to_ndc(vec4(position.xy, depth, 1.0), view.viewport),
        view.world_from_clip,
    );
    let world_dx = dpdx(world_position);
    let world_dy = dpdy(world_position);
    var normal = normalize(cross(world_dx, world_dy));
    if dot(normal, view.world_position - world_position) < 0.0 {
        normal = -normal;
    }

    // Sample just outside the visible face. This keeps the solid cell itself
    // from suppressing its neighboring irradiance and prevents light leaking
    // through the back face of a one-voxel wall.
    let origin = world_position + normal * settings.voxel_world_size * 0.7;
    let indirect = propagated_radiance(origin);
    let bounced = indirect * (vec3(0.18) + source.rgb * 0.82) * settings.intensity;
    return vec4(source.rgb + bounced, source.a);
}
