#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct PlanetAtmosphereSettings {
    planet_center: vec4<f32>,
    sun_direction: vec4<f32>,
    radii: vec4<f32>,
    day_color: vec4<f32>,
    night_color: vec4<f32>,
    params: vec4<f32>,
};

@group(3) @binding(0) var<uniform> settings: PlanetAtmosphereSettings;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let world_position = in.world_position.xyz;
    let normal = normalize(in.world_normal);
    let camera_position = view.world_from_clip[3].xyz;
    let view_ray = normalize(camera_position - world_position);

    let planet_radius = settings.radii.x;
    let atmosphere_radius = settings.radii.y;
    let height = length(world_position - settings.planet_center.xyz);
    let height_factor = clamp(
        (height - planet_radius) / max(atmosphere_radius - planet_radius, 1.0),
        0.0,
        1.0,
    );
    // 大气密度在外壳表面最高，向行星表面衰减：中心透明、边缘形成一圈辉光。
    let density = pow(height_factor, settings.params.y);

    let sun_dir = normalize(settings.sun_direction.xyz);
    let facing_sun = max(dot(normal, sun_dir), 0.0);
    // 边缘辉光：视线与球面法线接近垂直时更亮，勾勒出球体轮廓。
    let rim = pow(1.0 - abs(dot(normal, view_ray)), settings.params.x);

    let day_factor = smoothstep(0.0, 0.6, facing_sun);
    let base = mix(settings.night_color.rgb, settings.day_color.rgb, day_factor);
    let glow = base
        * (0.30 + 0.70 * settings.params.w)
        * (0.35 + 0.65 * rim)
        * (0.40 + 0.60 * facing_sun);
    let alpha = settings.params.z
        * density
        * (settings.params.w * 0.03 + 0.97 * rim);

    return vec4(glow, clamp(alpha, 0.0, 1.0));
}
