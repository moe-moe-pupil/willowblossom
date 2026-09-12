//! Explicit offscreen GPU regression for the production replay camera output.
//! Winit is disabled: all cameras target an image, and no native window exists.

use std::{
    env::var_os,
    fs::create_dir_all,
    path::PathBuf,
    time::Duration,
};

use bevy::{
    audio::AudioPlugin,
    camera::{
        visibility::RenderLayers,
        CameraUpdateSystems,
        RenderTarget,
    },
    log::LogPlugin,
    prelude::{
        App,
        Assets,
        Camera,
        Camera2d,
        ClearColorConfig,
        Color,
        DefaultPlugins,
        Entity,
        Handle,
        Image,
        IntoScheduleConfigs,
        Msaa,
        On,
        PluginGroup,
        PostUpdate,
        ResMut,
        Resource,
        Window,
        WindowPlugin,
    },
    render::{
        render_resource::{
            PollType,
            TextureFormat,
            TextureUsages,
        },
        renderer::RenderDevice,
        view::screenshot::{
            Screenshot,
            ScreenshotCaptured,
        },
        RenderPlugin,
    },
    window::{
        ExitCondition,
        PrimaryWindow,
        WindowResolution,
    },
    winit::WinitPlugin,
};
use bevy_egui::{
    egui::{
        pos2,
        Color32,
        Id,
        LayerId,
        Order,
        Rect,
    },
    EguiContexts,
    EguiGlobalSettings,
    EguiPlugin,
    EguiPostUpdateSet,
    EguiPrimaryContextPass,
    PrimaryEguiContext,
};
use image::RgbaImage;

use super::{
    viewport::{
        replay_ui_camera,
        update_replay_preview_viewport,
    },
    ReplayStudio,
    ReplayVideoCaptureActive,
};
use crate::voxel::VoxelViewportCamera;

#[derive(Resource, Default)]
struct CapturedFrame(Option<Image>);

fn draw_translucent_probe(mut contexts: EguiContexts) {
    let Ok(context) = contexts.ctx_mut() else { return };
    context
        .layer_painter(LayerId::new(
            Order::Foreground,
            Id::new("alpha-probe"),
        ))
        .rect_filled(
            Rect::from_min_max(pos2(40.0, 40.0), pos2(88.0, 88.0)),
            0.0,
            Color32::from_white_alpha(128),
        );
}

fn advance(app: &mut App) {
    app.update();
    app.world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .poll(PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })
        .expect("offscreen GPU frame completes");
}

fn capture(app: &mut App, target: &Handle<Image>, filename: &str) -> RgbaImage {
    app.world_mut().resource_mut::<CapturedFrame>().0 = None;
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |event: On<ScreenshotCaptured>, mut captured: ResMut<CapturedFrame>| {
                captured.0 = Some(event.image.clone());
            },
        );
    for _ in 0..40 {
        advance(app);
        if let Some(image) = app.world_mut().resource_mut::<CapturedFrame>().0.take() {
            let image = image.try_into_dynamic().expect("RGBA GPU image").to_rgba8();
            if let Some(directory) = var_os("WILLOWBLOSSOM_UI_ARTIFACT_DIR") {
                let directory = PathBuf::from(directory);
                create_dir_all(&directory).expect("create GPU artifact directory");
                image
                    .save(directory.join(filename))
                    .expect("save GPU artifact");
            }
            return image;
        }
    }
    panic!("offscreen screenshot readback did not complete");
}

fn assert_pixel(image: &RgbaImage, x: u32, y: u32, expected: [u8; 4]) {
    let actual = image.get_pixel(x, y).0;
    for (actual_channel, expected_channel) in actual.into_iter().zip(expected) {
        assert!(
            actual_channel.abs_diff(expected_channel) <= 3,
            "pixel ({x}, {y}) is {actual:?}, expected {expected:?}"
        );
    }
}

#[test]
#[ignore = "requires an offscreen GPU adapter; creates no native window"]
fn offscreen_replay_preview_capture_and_alpha_compositing() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..WindowPlugin::default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..RenderPlugin::default()
            })
            .disable::<WinitPlugin>()
            .disable::<AudioPlugin>()
            .disable::<LogPlugin>(),
    )
    .insert_resource(EguiGlobalSettings {
        auto_create_primary_context: false,
        enable_cursor_icon_updates: false,
        enable_ime: false,
        ..EguiGlobalSettings::default()
    })
    .add_plugins(EguiPlugin::default())
    .insert_resource(ReplayStudio {
        panel_open: true,
        preview_rect: Some(Rect::from_min_max(
            pos2(16.0, 16.0),
            pos2(112.0, 112.0),
        )),
        ..ReplayStudio::default()
    })
    .init_resource::<ReplayVideoCaptureActive>()
    .init_resource::<CapturedFrame>()
    .add_systems(
        EguiPrimaryContextPass,
        draw_translucent_probe,
    )
    .add_systems(
        PostUpdate,
        update_replay_preview_viewport
            .after(EguiPostUpdateSet::ProcessOutput)
            .before(CameraUpdateSystems),
    );
    app.finish();
    app.cleanup();

    // Only size metadata for the production viewport system. Without Winit this
    // entity does not create an OS window, and every RenderTarget is explicit.
    app.world_mut().spawn((PrimaryWindow, Window {
        resolution: WindowResolution::new(128, 128),
        ..Window::default()
    }));
    let mut image = Image::new_target_texture(
        128,
        128,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let render_target = RenderTarget::Image(target.clone().into());
    app.world_mut().spawn((
        Camera2d,
        Msaa::Off,
        RenderLayers::none(),
        render_target.clone(),
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::linear_rgb(0.0, 0.0, 1.0)),
            ..Camera::default()
        },
    ));
    let scene: Entity = app
        .world_mut()
        .spawn((
            Camera2d,
            Msaa::Off,
            RenderLayers::none(),
            render_target.clone(),
            VoxelViewportCamera,
            Camera {
                order: 0,
                clear_color: ClearColorConfig::Custom(Color::linear_rgb(0.0, 1.0, 0.0)),
                ..Camera::default()
            },
        ))
        .id();
    let overlay = app
        .world_mut()
        .spawn((
            Camera2d,
            Msaa::Off,
            RenderLayers::none(),
            render_target,
            PrimaryEguiContext,
            replay_ui_camera(),
        ))
        .id();
    for _ in 0..12 {
        advance(&mut app);
    }

    for (capturing, filename) in [
        (false, "gpu-replay-preview.png"),
        (true, "gpu-replay-capture.png"),
        (false, "gpu-replay-restored.png"),
    ] {
        app.world_mut().resource_mut::<ReplayVideoCaptureActive>().0 = capturing;
        for _ in 0..3 {
            advance(&mut app);
        }
        let frame = capture(&mut app, &target, filename);
        assert_pixel(
            &frame,
            4,
            4,
            if capturing { [0, 255, 0, 255] } else { [0, 0, 255, 255] },
        );
        assert_pixel(&frame, 24, 24, [0, 255, 0, 255]);
        // Bevy egui tints in gamma space and blends in linear space. The white
        // probe contributes ~0.216 linear RGB over half the green background.
        // Straight-alpha output would multiply it twice (~[93, 204, 93]).
        assert_pixel(&frame, 64, 64, [128, 220, 128, 255]);
        assert_eq!(
            app.world().get::<Camera>(scene).unwrap().viewport.is_none(),
            capturing
        );
        assert!(app
            .world()
            .get::<Camera>(overlay)
            .unwrap()
            .viewport
            .is_none());
    }
}
