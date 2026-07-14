#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct VoxelCartoonFilter {
    radius: f32,
    brightness: f32,
    saturation: f32,
    edge_strength: f32,
}

struct RegionStats {
    mean: vec3<f32>,
    variance: f32,
}

@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: VoxelCartoonFilter;

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.299, 0.587, 0.114));
}

fn region_stats(
    uv: vec2<f32>,
    texel: vec2<f32>,
    min_offset: vec2<i32>,
    max_offset: vec2<i32>,
) -> RegionStats {
    var sum = vec3<f32>(0.0);
    var squared_sum = vec3<f32>(0.0);
    var sample_count = 0.0;

    for (var y = min_offset.y; y <= max_offset.y; y += 1) {
        for (var x = min_offset.x; x <= max_offset.x; x += 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * texel;
            let sample_color = textureSample(source_texture, source_sampler, uv + offset).rgb;
            sum += sample_color;
            squared_sum += sample_color * sample_color;
            sample_count += 1.0;
        }
    }

    let mean = sum / sample_count;
    let channel_variance = max(squared_sum / sample_count - mean * mean, vec3<f32>(0.0));
    return RegionStats(mean, luminance(channel_variance));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(source_texture));
    let texel = 1.0 / dimensions;
    let radius = i32(clamp(round(settings.radius), 1.0, 3.0));

    let upper_left = region_stats(in.uv, texel, vec2<i32>(-radius), vec2<i32>(0));
    let upper_right = region_stats(
        in.uv,
        texel,
        vec2<i32>(0, -radius),
        vec2<i32>(radius, 0),
    );
    let lower_left = region_stats(
        in.uv,
        texel,
        vec2<i32>(-radius, 0),
        vec2<i32>(0, radius),
    );
    let lower_right = region_stats(in.uv, texel, vec2<i32>(0), vec2<i32>(radius));

    var selected = upper_left;
    if (upper_right.variance < selected.variance) {
        selected = upper_right;
    }
    if (lower_left.variance < selected.variance) {
        selected = lower_left;
    }
    if (lower_right.variance < selected.variance) {
        selected = lower_right;
    }

    let left = textureSample(source_texture, source_sampler, in.uv - vec2<f32>(texel.x, 0.0)).rgb;
    let right = textureSample(source_texture, source_sampler, in.uv + vec2<f32>(texel.x, 0.0)).rgb;
    let up = textureSample(source_texture, source_sampler, in.uv - vec2<f32>(0.0, texel.y)).rgb;
    let down = textureSample(source_texture, source_sampler, in.uv + vec2<f32>(0.0, texel.y)).rgb;
    let edge = smoothstep(0.06, 0.32, abs(luminance(right) - luminance(left)) + abs(luminance(down) - luminance(up)));

    let selected_luma = luminance(selected.mean);
    var color = vec3<f32>(selected_luma) + (selected.mean - vec3<f32>(selected_luma)) * settings.saturation;
    color = max(color * settings.brightness + vec3<f32>(0.025), vec3<f32>(0.0));
    color = floor(color * 10.0 + vec3<f32>(0.5)) / 10.0;
    color *= 1.0 - edge * settings.edge_strength;

    return vec4<f32>(color, 1.0);
}
