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

const CUBEMAP_DIRECTIONS = array<vec3<f32>, 6>(
    vec3(1.0, 0.0, 0.0),
    vec3(-1.0, 0.0, 0.0),
    vec3(0.0, 1.0, 0.0),
    vec3(0.0, -1.0, 0.0),
    vec3(0.0, 0.0, 1.0),
    vec3(0.0, 0.0, -1.0),
);

fn volume_sample(world_position: vec3<f32>) -> vec4<f32> {
    // Convert through the exact effective voxel size instead of normalized UVs.
    // textureLoad avoids filtering neighboring occupied cells at voxel boundaries.
    let cell = vec3<i32>(floor(
        (world_position - settings.volume_min) / settings.voxel_world_size,
    ));
    let dimensions = vec3<i32>(settings.volume_dimensions);
    if any(cell < vec3(0)) || any(cell >= dimensions) {
        return vec4(0.0);
    }
    return textureLoad(voxel_volume, cell, 0);
}

fn trace_interval(
    origin: vec3<f32>,
    direction: vec3<f32>,
    interval_start: f32,
    interval_end: f32,
) -> vec4<f32> {
    let step_length = (interval_end - interval_start) / 4.0;
    for (var step = 0; step < 4; step += 1) {
        let distance = interval_start + (f32(step) + 0.5) * step_length;
        let probe = volume_sample(origin + direction * distance);
        if probe.a > 0.5 {
            // The first occupied sample is also the one-fetch visibility test used
            // when merging this interval with the next cascade.
            return vec4(probe.rgb, 1.0);
        }
    }
    return vec4(0.0);
}

fn cascade_radiance(origin: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    var merged = vec3(0.0);
    var cascade_scale = 27.0;

    // Merge far intervals into near intervals. Each interval uses cubemap-grouped
    // directions, while the normal bias distributes them over the local hemisphere.
    for (var cascade = 3; cascade >= 0; cascade -= 1) {
        let interval_start = settings.voxel_world_size * cascade_scale;
        let interval_end = interval_start * 3.0;
        var radiance = vec3(0.0);
        var weight_sum = 0.0;
        for (var ray = 0; ray < 6; ray += 1) {
            let direction = normalize(CUBEMAP_DIRECTIONS[ray] + normal * 1.35);
            let weight = max(dot(normal, direction), 0.0);
            if weight > 0.05 {
                let traced = trace_interval(origin, direction, interval_start, interval_end);
                radiance += traced.rgb * weight;
                // Missed and occluded rays are black samples and must remain in
                // the hemisphere average. Normalizing by hits alone amplified a
                // single emissive hit into the yellow center-screen speckles.
                weight_sum += weight;
            }
        }
        if weight_sum > 0.0 {
            radiance /= weight_sum;
            let merge_weight = 1.0 / (f32(cascade) + 1.0);
            merged = mix(merged, radiance, merge_weight);
        }
        cascade_scale /= 3.0;
    }
    return merged;
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

    let origin = world_position + normal * settings.voxel_world_size * 1.25;
    let indirect = cascade_radiance(origin, normal);
    let bounced = indirect * (vec3(0.2) + source.rgb * 0.8) * settings.intensity;
    return vec4(source.rgb + bounced, source.a);
}
