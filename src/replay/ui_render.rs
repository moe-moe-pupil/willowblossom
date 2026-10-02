//! CPU rendering of real egui output for window-free layout artifacts.
//!
//! Apply every frame's texture delta, including warmup frames, before rendering
//! an output. This implements the egui mesh path, not Bevy scene rendering or
//! custom GPU paint callbacks. Texture sampling uses the base mip level.

use std::{
    array::from_fn,
    collections::HashMap,
    fs::create_dir_all,
    path::Path,
};

use bevy_egui::egui::{
    epaint::{
        Mesh,
        Primitive,
        Vertex,
    },
    Color32,
    Context,
    FullOutput,
    ImageData,
    Pos2,
    Rect,
    TextureFilter,
    TextureId,
    TextureOptions,
    TextureWrapMode,
    TexturesDelta,
    Vec2,
};
use image::{
    Rgba,
    RgbaImage,
};

struct CpuTexture {
    size: [usize; 2],
    // Bevy uploads egui atlases as sRGB textures: filtering happens in linear
    // space before the shader converts the sampled RGB back to gamma space.
    pixels: Vec<[f32; 4]>,
    options: TextureOptions,
}

pub(crate) struct EguiFrameRenderer {
    textures: HashMap<TextureId, CpuTexture>,
    pending_free: Vec<TextureId>,
    background: Color32,
}

impl Default for EguiFrameRenderer {
    fn default() -> Self {
        Self {
            textures: HashMap::new(),
            pending_free: Vec::new(),
            background: Color32::from_rgb(18, 20, 24),
        }
    }
}

impl EguiFrameRenderer {
    /// Consume deltas once per frame. Frees apply at the next frame so a texture
    /// released during this frame remains available to its final draw commands.
    pub(crate) fn apply(&mut self, delta: &TexturesDelta) -> Result<(), String> {
        for id in self.pending_free.drain(..) {
            self.textures.remove(&id);
        }
        for (id, update) in &delta.set {
            let ImageData::Color(image) = &update.image;
            if image.size.contains(&0)
                || image.size[0].checked_mul(image.size[1]) != Some(image.pixels.len())
            {
                return Err(format!("invalid egui texture {id:?}"));
            }
            let pixels = image
                .pixels
                .iter()
                .copied()
                .map(linear_color)
                .collect::<Vec<_>>();
            if let Some([x, y]) = update.pos {
                let texture = self
                    .textures
                    .get_mut(id)
                    .ok_or_else(|| format!("patch for missing egui texture {id:?}"))?;
                if x.checked_add(image.size[0])
                    .is_none_or(|end| end > texture.size[0])
                    || y.checked_add(image.size[1])
                        .is_none_or(|end| end > texture.size[1])
                {
                    return Err(format!(
                        "patch exceeds egui texture {id:?}"
                    ));
                }
                for row in 0..image.size[1] {
                    let target = (y + row) * texture.size[0] + x;
                    let source = row * image.size[0];
                    texture.pixels[target..target + image.size[0]]
                        .copy_from_slice(&pixels[source..source + image.size[0]]);
                }
                texture.options = update.options;
            } else {
                self.textures.insert(*id, CpuTexture {
                    size: image.size,
                    pixels,
                    options: update.options,
                });
            }
        }
        self.pending_free.clone_from(&delta.free);
        Ok(())
    }

    /// Render a frame whose deltas have already been applied. `size` is physical
    /// pixels; point coordinates are scaled by `output.pixels_per_point`.
    pub(crate) fn render(
        &self,
        context: &Context,
        output: &FullOutput,
        path: impl AsRef<Path>,
        size: [u32; 2],
    ) -> Result<(), String> {
        let image = self.render_image(context, output, size)?;
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        image.save(path).map_err(|error| error.to_string())
    }

    pub(super) fn render_image(
        &self,
        context: &Context,
        output: &FullOutput,
        size: [u32; 2],
    ) -> Result<RgbaImage, String> {
        let scale = output.pixels_per_point;
        if size.contains(&0) || !scale.is_finite() || scale <= 0.0 {
            return Err("invalid egui artifact dimensions or scale".into());
        }
        let count = (size[0] as usize)
            .checked_mul(size[1] as usize)
            .ok_or("egui artifact dimensions overflow")?;
        let mut pixels = vec![linear_color(self.background); count];
        for clipped in context.tessellate(output.shapes.clone(), scale) {
            let Primitive::Mesh(mesh) = clipped.primitive else {
                return Err(
                    "CPU egui artifact renderer does not support GPU paint callbacks".into(),
                );
            };
            self.paint_mesh(
                &mesh,
                clipped.clip_rect,
                scale,
                size,
                &mut pixels,
            )?;
        }
        let mut image = RgbaImage::new(size[0], size[1]);
        for (target, source) in image.pixels_mut().zip(pixels) {
            let alpha = source[3].clamp(0.0, 1.0);
            let channel = |value: f32| {
                let straight = if alpha > 0.0 { value / alpha } else { 0.0 };
                (gamma_from_linear(straight.clamp(0.0, 1.0)) * 255.0).round() as u8
            };
            *target = Rgba([
                channel(source[0]),
                channel(source[1]),
                channel(source[2]),
                (alpha * 255.0).round() as u8,
            ]);
        }
        Ok(image)
    }

    fn paint_mesh(
        &self,
        mesh: &Mesh,
        clip: Rect,
        scale: f32,
        size: [u32; 2],
        pixels: &mut [[f32; 4]],
    ) -> Result<(), String> {
        if mesh.indices.is_empty() || !clip.is_positive() {
            return Ok(());
        }
        if !mesh.is_valid() || mesh.indices.len() % 3 != 0 {
            return Err("invalid egui triangle mesh".into());
        }
        let texture = self.textures.get(&mesh.texture_id).ok_or_else(|| {
            format!(
                "missing egui texture {:?}",
                mesh.texture_id
            )
        })?;
        let clip = [
            (clip.min.x * scale).round().clamp(0.0, size[0] as f32) as u32,
            (clip.min.y * scale).round().clamp(0.0, size[1] as f32) as u32,
            (clip.max.x * scale).round().clamp(0.0, size[0] as f32) as u32,
            (clip.max.y * scale).round().clamp(0.0, size[1] as f32) as u32,
        ];
        for triangle in mesh.indices.chunks_exact(3) {
            let mut vertices = [0, 1, 2].map(|index| {
                let mut vertex = mesh.vertices[triangle[index] as usize];
                vertex.pos.x *= scale;
                vertex.pos.y *= scale;
                vertex
            });
            if vertices
                .iter()
                .any(|vertex| !vertex.pos.is_finite() || !vertex.uv.is_finite())
            {
                return Err("nonfinite egui mesh coordinate".into());
            }
            let mut area = edge(
                vertices[0].pos,
                vertices[1].pos,
                vertices[2].pos,
            );
            if area == 0.0 {
                continue;
            }
            if area < 0.0 {
                vertices.swap(1, 2);
                area = -area;
            }
            let min_x = vertices
                .iter()
                .map(|vertex| vertex.pos.x)
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(clip[0] as f32)
                .min(clip[2] as f32) as u32;
            let min_y = vertices
                .iter()
                .map(|vertex| vertex.pos.y)
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(clip[1] as f32)
                .min(clip[3] as f32) as u32;
            let max_x = vertices
                .iter()
                .map(|vertex| vertex.pos.x)
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .min(clip[2] as f32)
                .max(clip[0] as f32) as u32;
            let max_y = vertices
                .iter()
                .map(|vertex| vertex.pos.y)
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .min(clip[3] as f32)
                .max(clip[1] as f32) as u32;
            let sides = [
                (vertices[1].pos, vertices[2].pos),
                (vertices[2].pos, vertices[0].pos),
                (vertices[0].pos, vertices[1].pos),
            ];
            let top_left =
                sides.map(|(from, to)| to.y < from.y || (to.y == from.y && to.x > from.x));
            let filter = texture_filter(texture, &vertices, area);
            let colors = vertices.map(|vertex| {
                vertex
                    .color
                    .to_array()
                    .map(|channel| channel as f32 / 255.0)
            });
            for y in min_y..max_y {
                for x in min_x..max_x {
                    let point = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let edges = sides.map(|(from, to)| edge(from, to, point));
                    if edges
                        .iter()
                        .zip(top_left)
                        .any(|(value, inclusive)| *value < 0.0 || (*value == 0.0 && !inclusive))
                    {
                        continue;
                    }
                    let weights = edges.map(|value| value / area);
                    let uv = Pos2::new(
                        (0..3)
                            .map(|index| weights[index] * vertices[index].uv.x)
                            .sum(),
                        (0..3)
                            .map(|index| weights[index] * vertices[index].uv.y)
                            .sum(),
                    );
                    let sample = texture.sample(uv, filter);
                    let vertex_color: [f32; 4] = from_fn(|channel| {
                        (0..3)
                            .map(|index| weights[index] * colors[index][channel])
                            .sum()
                    });
                    // Match bevy_egui's shader: tint in gamma space, then blend
                    // premultiplied source-over into the linear render target.
                    let alpha = (sample[3] * vertex_color[3]).clamp(0.0, 1.0);
                    let target = &mut pixels[y as usize * size[0] as usize + x as usize];
                    for channel in 0..3 {
                        let source = linear_from_gamma(
                            gamma_from_linear(sample[channel]) * vertex_color[channel],
                        );
                        target[channel] = source + target[channel] * (1.0 - alpha);
                    }
                    target[3] = alpha + target[3] * (1.0 - alpha);
                }
            }
        }
        Ok(())
    }
}

impl CpuTexture {
    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        let coordinate = |value: i64, size: usize| -> usize {
            let size = size as i64;
            match self.options.wrap_mode {
                TextureWrapMode::ClampToEdge => value.clamp(0, size - 1) as usize,
                TextureWrapMode::Repeat => value.rem_euclid(size) as usize,
                TextureWrapMode::MirroredRepeat => {
                    let repeated = value.rem_euclid(size * 2);
                    if repeated < size {
                        repeated as usize
                    } else {
                        (2 * size - repeated - 1) as usize
                    }
                },
            }
        };
        self.pixels[coordinate(y, self.size[1]) * self.size[0] + coordinate(x, self.size[0])]
    }

    fn sample(&self, uv: Pos2, filter: TextureFilter) -> [f32; 4] {
        let x = uv.x as f64 * self.size[0] as f64 - 0.5;
        let y = uv.y as f64 * self.size[1] as f64 - 0.5;
        if filter == TextureFilter::Nearest {
            return self.texel(
                (x + 0.5).floor() as i64,
                (y + 0.5).floor() as i64,
            );
        }
        let left = x.floor() as i64;
        let top = y.floor() as i64;
        let dx = (x - x.floor()) as f32;
        let dy = (y - y.floor()) as f32;
        let samples = [
            self.texel(left, top),
            self.texel(left + 1, top),
            self.texel(left, top + 1),
            self.texel(left + 1, top + 1),
        ];
        let weights = [
            (1.0 - dx) * (1.0 - dy),
            dx * (1.0 - dy),
            (1.0 - dx) * dy,
            dx * dy,
        ];
        from_fn(|channel| {
            (0..4)
                .map(|index| samples[index][channel] * weights[index])
                .sum()
        })
    }
}

fn texture_filter(texture: &CpuTexture, vertices: &[Vertex; 3], area: f32) -> TextureFilter {
    let delta_1 = vertices[1].pos - vertices[0].pos;
    let delta_2 = vertices[2].pos - vertices[0].pos;
    let uv_1 = vertices[1].uv - vertices[0].uv;
    let uv_2 = vertices[2].uv - vertices[0].uv;
    let dx = (uv_1 * delta_2.y - uv_2 * delta_1.y) / area;
    let dy = (uv_2 * delta_1.x - uv_1 * delta_2.x) / area;
    let footprint =
        |value: Vec2| (value.x * texture.size[0] as f32).hypot(value.y * texture.size[1] as f32);
    if footprint(dx).max(footprint(dy)) > 1.0 {
        texture.options.minification
    } else {
        texture.options.magnification
    }
}

fn edge(from: Pos2, to: Pos2, point: Pos2) -> f32 {
    (to.x - from.x) * (point.y - from.y) - (to.y - from.y) * (point.x - from.x)
}

fn linear_from_gamma(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn gamma_from_linear(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn linear_color(color: Color32) -> [f32; 4] {
    let [red, green, blue, alpha] = color.to_array().map(|channel| channel as f32 / 255.0);
    [
        linear_from_gamma(red),
        linear_from_gamma(green),
        linear_from_gamma(blue),
        alpha,
    ]
}

#[cfg(test)]
mod tests {
    use bevy_egui::egui::{
        epaint::ImageDelta,
        ColorImage,
        TextureOptions,
    };

    use super::{
        linear_from_gamma,
        Color32,
        CpuTexture,
        EguiFrameRenderer,
        Mesh,
        Pos2,
        Rect,
        TextureId,
        TexturesDelta,
    };

    #[test]
    fn triangles_respect_clipping_and_blend_shared_edges_only_once() {
        let mut renderer = EguiFrameRenderer::default();
        let id = TextureId::Managed(4);
        renderer
            .apply(&TexturesDelta {
                set: vec![(
                    id,
                    ImageDelta::full(
                        ColorImage::filled([1, 1], Color32::WHITE),
                        TextureOptions::LINEAR,
                    ),
                )],
                free: vec![],
            })
            .unwrap();
        let mut mesh = Mesh::with_texture(id);
        mesh.add_rect_with_uv(
            Rect::from_min_max(Pos2::ZERO, Pos2::new(4.0, 4.0)),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::from_rgba_premultiplied(128, 0, 0, 128),
        );
        let clip = Rect::from_min_max(Pos2::new(1.0, 1.0), Pos2::new(3.0, 3.0));
        let blue = [0.0, 0.0, 1.0, 1.0];
        let mut pixels = vec![blue; 16];
        renderer
            .paint_mesh(&mesh, clip, 1.0, [4, 4], &mut pixels)
            .unwrap();
        let alpha = 128.0 / 255.0;
        let blended = [linear_from_gamma(alpha), 0.0, 1.0 - alpha, 1.0];
        for y in 0..4 {
            for x in 0..4 {
                let expected =
                    if (1..3).contains(&x) && (1..3).contains(&y) { blended } else { blue };
                for (actual, expected) in pixels[y * 4 + x].into_iter().zip(expected) {
                    assert!(
                        (actual - expected).abs() < 0.000_001,
                        "pixel ({x}, {y})"
                    );
                }
            }
        }
        // Egui allows either triangle winding order.
        for triangle in mesh.indices.chunks_exact_mut(3) {
            triangle.swap(0, 2);
        }
        let mut reversed = vec![blue; 16];
        renderer
            .paint_mesh(&mesh, clip, 1.0, [4, 4], &mut reversed)
            .unwrap();
        for (actual, expected) in reversed
            .into_iter()
            .flatten()
            .zip(pixels.into_iter().flatten())
        {
            assert!((actual - expected).abs() < 0.000_001);
        }
    }

    #[test]
    fn texture_updates_preserve_unchanged_texels_and_defer_frees() {
        let mut renderer = EguiFrameRenderer::default();
        let id = TextureId::Managed(4);
        renderer
            .apply(&TexturesDelta {
                set: vec![(
                    id,
                    ImageDelta::full(
                        ColorImage::filled([2, 2], Color32::WHITE),
                        TextureOptions::NEAREST,
                    ),
                )],
                free: vec![],
            })
            .unwrap();
        renderer
            .apply(&TexturesDelta {
                set: vec![(
                    id,
                    ImageDelta::partial(
                        [1, 0],
                        ColorImage::filled([1, 1], Color32::BLACK),
                        TextureOptions::NEAREST,
                    ),
                )],
                free: vec![id],
            })
            .unwrap();
        let CpuTexture { pixels, .. } = &renderer.textures[&id];
        assert_eq!(pixels[0], [1.0; 4]);
        assert_eq!(pixels[1], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(pixels[2], [1.0; 4]);
        renderer.apply(&TexturesDelta::default()).unwrap();
        assert!(!renderer.textures.contains_key(&id));
    }
}
