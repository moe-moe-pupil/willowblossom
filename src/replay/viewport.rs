//! Keeps the composition camera inside the studio while egui covers the window.

use bevy::{
    camera::{
        CameraOutputMode,
        Viewport,
    },
    prelude::*,
    render::render_resource::BlendState,
    window::PrimaryWindow,
};
use bevy_egui::{
    egui::Rect,
    EguiOutput,
    PrimaryEguiContext,
};

use super::{
    ReplayStudio,
    ReplayVideoCaptureActive,
};
use crate::voxel::VoxelViewportCamera;

/// Egui renders premultiplied colors into this transparent camera target. The
/// output pass must preserve that convention when compositing over the scene.
pub(crate) fn replay_ui_camera() -> Camera {
    Camera {
        order: 1,
        output_mode: CameraOutputMode::Write {
            blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            clear_color: ClearColorConfig::None,
        },
        clear_color: ClearColorConfig::Custom(Color::NONE),
        ..default()
    }
}

/// Native editing panels stay available when the studio explicitly enables
/// scene editing. Video capture always excludes them.
pub(crate) fn replay_auxiliary_ui_visible(
    studio: Option<Res<ReplayStudio>>,
    capture_active: Option<Res<ReplayVideoCaptureActive>>,
) -> bool {
    !capture_active.is_some_and(|active| active.0)
        && studio.is_none_or(|studio| !studio.panel_open || studio.live_editing_enabled)
}

/// Run after egui's output has been processed and before `CameraUpdateSystems`.
/// Egui's scale includes both native DPI and the user's UI zoom.
pub(super) fn update_replay_preview_viewport(
    studio: Res<ReplayStudio>,
    capture_active: Res<ReplayVideoCaptureActive>,
    windows: Query<&Window, With<PrimaryWindow>>,
    contexts: Query<&EguiOutput, With<PrimaryEguiContext>>,
    mut cameras: Query<&mut Camera, With<VoxelViewportCamera>>,
) {
    // 第一人称修改发言区域：预览相机让位，全屏显示真实带团第一人称画面。
    let session_active = studio.speech_area_edit.is_some();
    let viewport = if studio.panel_open && !capture_active.0 && !session_active {
        studio.preview_rect.and_then(|rect| {
            let window = windows.single().ok()?;
            let pixels_per_point = contexts.single().ok()?.pixels_per_point;
            preview_viewport(
                rect,
                pixels_per_point,
                window.physical_size(),
            )
        })
    } else {
        None
    };
    for mut camera in &mut cameras {
        if !same_viewport(
            camera.viewport.as_ref(),
            viewport.as_ref(),
        ) {
            camera.viewport = viewport.clone();
        }
        let should_render = !(studio.panel_open && session_active);
        if camera.is_active != should_render {
            camera.is_active = should_render;
        }
    }
}

fn same_viewport(current: Option<&Viewport>, next: Option<&Viewport>) -> bool {
    match (current, next) {
        (Some(current), Some(next)) => {
            current.physical_position == next.physical_position
                && current.physical_size == next.physical_size
                && current.depth == next.depth
        },
        (None, None) => true,
        _ => false,
    }
}

/// Clamp on both edges: resizing a window may invalidate the previous UI rect.
/// Empty or invalid geometry falls back to the ordinary full-window camera.
fn preview_viewport(rect: Rect, pixels_per_point: f32, window_size: UVec2) -> Option<Viewport> {
    if !rect.is_finite()
        || !rect.is_positive()
        || !pixels_per_point.is_finite()
        || pixels_per_point <= 0.0
        || window_size.min_element() == 0
    {
        return None;
    }
    let min = Vec2::new(rect.min.x, rect.min.y) * pixels_per_point;
    let max = Vec2::new(rect.max.x, rect.max.y) * pixels_per_point;
    let min = min.max(Vec2::ZERO).min(window_size.as_vec2());
    let max = max.max(Vec2::ZERO).min(window_size.as_vec2());
    if max.x <= min.x || max.y <= min.y {
        return None;
    }
    let position = min.round().as_uvec2().min(window_size - UVec2::ONE);
    let end = max
        .round()
        .as_uvec2()
        .max(position + UVec2::ONE)
        .min(window_size);
    Some(Viewport {
        physical_position: position,
        physical_size: end - position,
        ..default()
    })
}

#[cfg(test)]
mod tests {
    use bevy::{
        prelude::*,
        window::{
            PrimaryWindow,
            WindowResolution,
        },
    };
    use bevy_egui::{
        egui::{
            pos2,
            Rect,
        },
        EguiOutput,
        PrimaryEguiContext,
    };

    use super::{
        preview_viewport,
        replay_auxiliary_ui_visible,
        update_replay_preview_viewport,
    };
    use crate::{
        replay::{
            ReplayStudio,
            ReplayVideoCaptureActive,
        },
        voxel::VoxelViewportCamera,
    };

    #[test]
    fn auxiliary_ui_condition_preserves_background_work_for_all_studio_states() {
        #[derive(Resource, Default)]
        struct Ticks {
            background: u32,
            auxiliary: u32,
        }
        fn background_tick(mut ticks: ResMut<Ticks>) { ticks.background += 1; }
        fn auxiliary_tick(mut ticks: ResMut<Ticks>) { ticks.auxiliary += 1; }

        let mut app = App::new();
        app.init_resource::<Ticks>().add_systems(
            Update,
            (
                background_tick,
                auxiliary_tick.run_if(replay_auxiliary_ui_visible),
            ),
        );
        // The standalone scene/rule plugins do not require ReplayPlugin.
        app.update();
        assert_eq!(
            app.world().resource::<Ticks>().auxiliary,
            1
        );
        app.insert_resource(ReplayVideoCaptureActive(true));
        app.update();
        assert_eq!(
            app.world().resource::<Ticks>().auxiliary,
            1
        );

        app.init_resource::<ReplayStudio>();
        let mut expected_auxiliary = 1;
        let mut expected_background = 2;
        for panel_open in [false, true] {
            for live_editing in [false, true] {
                for capturing in [false, true] {
                    {
                        let mut studio = app.world_mut().resource_mut::<ReplayStudio>();
                        studio.panel_open = panel_open;
                        studio.live_editing_enabled = live_editing;
                    }
                    app.world_mut().resource_mut::<ReplayVideoCaptureActive>().0 = capturing;
                    app.update();
                    expected_background += 1;
                    if !capturing && (!panel_open || live_editing) {
                        expected_auxiliary += 1;
                    }
                    let ticks = app.world().resource::<Ticks>();
                    assert_eq!(ticks.background, expected_background);
                    assert_eq!(
                        ticks.auxiliary, expected_auxiliary,
                        "panel={panel_open}, live editing={live_editing}, capture={capturing}",
                    );
                }
            }
        }
    }

    #[test]
    fn viewport_uses_egui_zoom_and_clamps_after_window_resize() {
        let viewport = preview_viewport(
            Rect::from_min_max(pos2(120.0, 70.0), pos2(950.0, 650.0)),
            1.5,
            UVec2::new(1280, 800),
        )
        .expect("visible composition");
        assert_eq!(
            viewport.physical_position,
            UVec2::new(180, 105)
        );
        assert_eq!(
            viewport.physical_size,
            UVec2::new(1100, 695)
        );

        let viewport = preview_viewport(
            Rect::from_min_max(pos2(-20.0, -10.0), pos2(100.0, 80.0)),
            2.0,
            UVec2::new(1280, 800),
        )
        .expect("partially visible composition");
        assert_eq!(viewport.physical_position, UVec2::ZERO);
        assert_eq!(
            viewport.physical_size,
            UVec2::new(200, 160)
        );
    }

    #[test]
    fn viewport_rejects_hidden_or_invalid_geometry() {
        let window = UVec2::new(1280, 800);
        let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(120.0, 80.0));
        assert!(preview_viewport(rect, 0.0, window).is_none());
        assert!(preview_viewport(rect, f32::NAN, window).is_none());
        assert!(preview_viewport(rect, 1.0, UVec2::ZERO).is_none());
        assert!(preview_viewport(Rect::NOTHING, 1.0, window).is_none());
        assert!(preview_viewport(
            Rect::from_min_max(pos2(1300.0, 0.0), pos2(1500.0, 400.0)),
            1.0,
            window,
        )
        .is_none());
    }

    #[test]
    fn viewport_remains_nonzero_for_subpixel_composition() {
        let viewport = preview_viewport(
            Rect::from_min_max(pos2(9.8, 9.8), pos2(10.0, 10.0)),
            1.0,
            UVec2::new(10, 10),
        )
        .expect("subpixel sliver is safe to render");
        assert_eq!(
            viewport.physical_position,
            UVec2::splat(9)
        );
        assert_eq!(viewport.physical_size, UVec2::ONE);
    }

    #[test]
    fn capture_and_closing_restore_full_scene_without_cropping_egui() {
        let mut app = App::new();
        app.init_resource::<ReplayStudio>()
            .init_resource::<ReplayVideoCaptureActive>()
            .add_systems(Update, update_replay_preview_viewport);
        app.world_mut().spawn((PrimaryWindow, Window {
            resolution: WindowResolution::new(1280, 800),
            ..default()
        }));
        let overlay = app
            .world_mut()
            .spawn((
                PrimaryEguiContext,
                EguiOutput {
                    pixels_per_point: 2.0,
                    ..default()
                },
                Camera::default(),
            ))
            .id();
        let scene = app
            .world_mut()
            .spawn((VoxelViewportCamera, Camera::default()))
            .id();
        {
            let mut studio = app.world_mut().resource_mut::<ReplayStudio>();
            studio.panel_open = true;
            studio.preview_rect = Some(Rect::from_min_max(
                pos2(50.0, 40.0),
                pos2(500.0, 300.0),
            ));
        }
        app.update();
        let camera = app.world().get::<Camera>(scene).unwrap();
        assert_eq!(
            camera.viewport.as_ref().unwrap().physical_size,
            UVec2::new(900, 520)
        );
        assert!(app
            .world()
            .get::<Camera>(overlay)
            .unwrap()
            .viewport
            .is_none());

        app.world_mut().resource_mut::<ReplayVideoCaptureActive>().0 = true;
        app.update();
        assert!(app.world().get::<Camera>(scene).unwrap().viewport.is_none());

        app.world_mut().resource_mut::<ReplayVideoCaptureActive>().0 = false;
        app.update();
        assert!(app.world().get::<Camera>(scene).unwrap().viewport.is_some());

        app.world_mut().resource_mut::<ReplayStudio>().panel_open = false;
        app.update();
        assert!(app.world().get::<Camera>(scene).unwrap().viewport.is_none());
    }
}
