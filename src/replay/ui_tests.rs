//! Headless pointer/keyboard checks against the real egui workspace widgets.
//! These create no window and never read campaign data or send chat messages.

use std::{
    env::var_os,
    fs::create_dir_all,
    path::PathBuf,
};

use bevy::{
    ecs::system::SystemState,
    prelude::{
        Query,
        With,
        World,
    },
};
use bevy_egui::egui::{
    Color32,
    Context,
    Event,
    FontData,
    FontDefinitions,
    FontFamily,
    FontId,
    FullOutput,
    Id,
    IdSalt,
    Key,
    Modifiers,
    MouseWheelUnit,
    Panel,
    PointerButton,
    Pos2,
    RawInput,
    Rect,
    Shape,
    TextStyle,
    TouchPhase,
    Vec2,
    pos2,
    vec2,
};
use serde_json::{
    from_value,
    json,
    to_vec,
};
use voxxelmaxx::prelude::Grid;

use super::{
    inspector::selected_dialogue_inspector,
    speaker_rename_window,
    speech_area_edit_window,
    timeline::replay_timeline_ui,
    ui_render::EguiFrameRenderer,
    workspace::{
        workspace_header,
        workspace_preview,
        ReplayWorkspaceTab,
    },
    DialogueSide,
    ReplayMode,
    ReplayPlayerMovementHistory,
    ReplayShipTrajectoryHistory,
    ReplayStudio,
    TrpgVoxelGrid,
};

fn replay_ui_context() -> Context {
    let context = Context::default();
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".into(),
        FontData::from_static(include_bytes!(
            "../../assets/fonts/AlibabaHealthFont.ttf"
        ))
        .into(),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "cjk".into());
    }
    context.set_fonts(fonts);
    let theme = context.theme();
    let mut style = (*context.style_of(theme)).clone();
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::proportional(18.0),
        ),
        (
            TextStyle::Body,
            FontId::proportional(14.0),
        ),
        (
            TextStyle::Monospace,
            FontId::proportional(14.0),
        ),
        (
            TextStyle::Button,
            FontId::proportional(14.0),
        ),
        (
            TextStyle::Small,
            FontId::proportional(10.0),
        ),
    ]
    .into();
    context.set_style_of(theme, style);
    context
}

fn editing_studio() -> ReplayStudio {
    let line = |line_id: u64, time_ms: u64, text: &str| {
        json!({
            "line_id": line_id, "time_ms": time_ms, "duration_ms": 2000,
            "sender_id": 7, "name": "玩家", "role": "玩家", "text": text,
            "avatar": "", "visibility": {"scope": "public"},
        })
    };
    ReplayStudio {
        mode: ReplayMode::Paused,
        panel_open: true,
        video_fps: 30,
        speech_enabled: false,
        music_enabled: false,
        status: String::new(),
        replay: Some(from_value(json!({
            "format_version": 3, "title": "交互测试", "campaign_id": "ui-test",
            "created_at_unix_ms": 1, "duration_ms": 8000,
            "audience": {"scope": "public"}, "scene": {"voxels": []},
            "dialogue": [line(1, 1000, "第一句"), line(2, 4500, "第二句")],
            "camera": [
                {"time_ms": 3500, "translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]}
            ],
            "camera_cuts": [{"time_ms": 3500, "shot": null}],
            "player_movements": [{"user_id": 7, "keyframes": [
                {"time_ms": 4500, "position_cells": [1.0, 0.0, 0.0]}
            ]}],
            "ship_trajectories": [{"ship_id": "ship", "ship_name": "船", "keyframes": [
                {"time_ms": 5000, "translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]}
            ]}],
            "standee_positions": [{"time_ms": 5500, "user_id": 7, "position": [0.0, 0.0, 0.0]}],
            "terrain_changes": [{"time_ms": 6000, "position": [0, 0, 0], "material": 0}],
            "ship_hull_changes": [{"time_ms": 6500, "ship_id": "ship", "position": [0, 0, 0], "material": 0}]
        })).expect("synthetic public replay fixture")),
        ..ReplayStudio::default()
    }
}

struct TimelineHarness {
    context: Context,
    studio: ReplayStudio,
    world: World,
    grids: SystemState<Query<'static, 'static, &'static mut Grid<u8>, With<TrpgVoxelGrid>>>,
    player_history: ReplayPlayerMovementHistory,
    ship_history: ReplayShipTrajectoryHistory,
    frame: u64,
    size: Vec2,
    workspace: bool,
    workspace_docks: Option<(Rect, Rect)>,
    inspector_text_id: Option<Id>,
    renderer: EguiFrameRenderer,
    output: FullOutput,
}

impl TimelineHarness {
    fn new() -> Self {
        let mut world = World::new();
        let grids = SystemState::new(&mut world);
        let mut harness = Self {
            context: replay_ui_context(),
            studio: editing_studio(),
            world,
            grids,
            player_history: ReplayPlayerMovementHistory::default(),
            ship_history: ReplayShipTrajectoryHistory::default(),
            frame: 0,
            size: vec2(1500.0, 600.0),
            workspace: false,
            workspace_docks: None,
            inspector_text_id: None,
            renderer: EguiFrameRenderer::default(),
            output: FullOutput::default(),
        };
        // Interaction uses the prior pass's widget rectangles. Let the scroll
        // area settle before deriving pointer positions from real responses.
        harness.advance(Vec::new(), Modifiers::NONE);
        harness.advance(Vec::new(), Modifiers::NONE);
        harness
    }

    fn advance(&mut self, events: Vec<Event>, modifiers: Modifiers) {
        self.frame += 1;
        let mut grids = self
            .grids
            .get_mut(&mut self.world)
            .expect("valid grid query");
        self.output = self.context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    self.size,
                )),
                time: Some(self.frame as f64 / 60.0),
                modifiers,
                events,
                ..RawInput::default()
            },
            |ui| {
                if self.workspace {
                    workspace_header(ui, &mut self.studio);
                    let timeline_panel = Panel::bottom("replay-workspace-timeline")
                        .resizable(true)
                        .default_size(290.0)
                        .min_size(160.0)
                        .max_size((self.size.y * 0.6).max(160.0))
                        .show(ui, |ui| {
                            replay_timeline_ui(
                                ui,
                                &mut self.studio,
                                &mut grids,
                                &self.player_history,
                                &self.ship_history,
                            );
                        });
                    let properties_panel = Panel::left("replay-workspace-properties")
                        .resizable(true)
                        .default_size(340.0)
                        .min_size(280.0)
                        .max_size((self.size.x * 0.45).max(280.0))
                        .show(ui, |ui| {
                            self.inspector_text_id =
                                self.studio.selected_dialogue_id.map(|line_id| {
                                    // TextEdit::id_salt stores an IdSalt before
                                    // passing it to make_persistent_id.
                                    ui.make_persistent_id(IdSalt::new((
                                        "replay-selected-dialogue-text",
                                        line_id,
                                    )))
                                });
                            selected_dialogue_inspector(ui, &mut self.studio);
                        });
                    self.workspace_docks = Some((
                        properties_panel.response.rect,
                        timeline_panel.response.rect,
                    ));
                    workspace_preview(ui, &mut self.studio);
                    if self.studio.speech_area_edit.is_some() {
                        speech_area_edit_window(&self.context, &mut self.studio);
                    }
                    if self.studio.speaker_rename.is_some() {
                        speaker_rename_window(&self.context, &mut self.studio);
                    }
                } else {
                    replay_timeline_ui(
                        ui,
                        &mut self.studio,
                        &mut grids,
                        &self.player_history,
                        &self.ship_history,
                    );
                }
            },
        );
        self.renderer
            .apply(&self.output.textures_delta)
            .expect("apply headless egui texture updates");
    }

    fn rect(&self, id: Id) -> Rect {
        let response = self
            .context
            .read_response(id)
            .expect("rendered timeline widget");
        assert!(response.interact_rect.is_positive());
        response.interact_rect
    }

    fn ruler(&self) -> Rect { self.rect(Id::new("replay-multitrack-view").with("ruler")) }

    fn clip(&self, index: usize) -> Rect {
        self.rect(Id::new("replay-multitrack-view").with(Id::new(("dialogue", index as u64 + 1))))
    }

    fn trim_handle(&self, index: usize) -> Rect {
        self.rect(Id::new("replay-multitrack-view").with((
            "trim",
            Id::new(("dialogue", index as u64 + 1)),
        )))
    }

    fn pointer(&mut self, pos: Pos2, pressed: bool) {
        self.advance(
            vec![Event::PointerMoved(pos), Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        );
    }

    fn click(&mut self, pos: Pos2) {
        self.pointer(pos, true);
        self.pointer(pos, false);
        self.advance(Vec::new(), Modifiers::NONE);
    }

    fn button(&mut self, label: &str) { self.click(text_rect(&self.output, label).center()); }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            self.advance(
                vec![Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers,
                }],
                modifiers,
            );
        }
    }

    fn snapshot(&self) -> Vec<u8> { to_vec(self.studio.replay.as_ref().unwrap()).unwrap() }

    fn edit_numeric(&mut self, label: &str, before_label: bool, value: &str) {
        let rect = numeric_rect_next_to_label(&self.output, label, before_label);
        self.click(rect.center());
        assert!(
            self.context.egui_wants_keyboard_input(),
            "numeric control must enter text editing"
        );
        self.key(
            Key::A,
            Modifiers::COMMAND | Modifiers::CTRL,
        );
        self.advance(
            vec![Event::Text(value.into())],
            Modifiers::NONE,
        );
    }

    fn wheel(&mut self, delta: Vec2) {
        let ruler = self.ruler();
        let pointer = pos2(ruler.center().x, ruler.bottom() + 50.0);
        self.advance(
            vec![Event::PointerMoved(pointer), Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta,
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        );
        // Allow egui's scroll smoothing to consume the input; no wall-clock wait.
        for _ in 0..20 {
            self.advance(Vec::new(), Modifiers::NONE);
        }
    }

    fn save_artifact(&self, filename: &str) {
        let Some(directory) = var_os("WILLOWBLOSSOM_UI_ARTIFACT_DIR") else { return };
        let directory = PathBuf::from(directory);
        create_dir_all(&directory).expect("create headless UI artifact directory");
        self.renderer
            .render(
                &self.context,
                &self.output,
                &directory.join(filename),
                [self.size.x as u32, self.size.y as u32],
            )
            .expect("render actual egui shapes to PNG");
    }
}

fn text_rect(output: &FullOutput, expected: &str) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            Shape::Text(text) if text.galley.text() == expected => {
                Some(text.visual_bounding_rect())
            },
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing rendered text: {expected}"))
}

fn has_text(output: &FullOutput, expected: &str) -> bool {
    output.shapes.iter().any(
        |shape| matches!(&shape.shape, Shape::Text(text) if text.galley.text().contains(expected)),
    )
}

fn numeric_rect_next_to_label(output: &FullOutput, label: &str, before_label: bool) -> Rect {
    let label = text_rect(output, label);
    output
        .shapes
        .iter()
        .filter_map(|shape| {
            let Shape::Text(text) = &shape.shape else { return None };
            text.galley.text().trim().parse::<f64>().ok()?;
            let rect = text.visual_bounding_rect();
            let same_row = rect.top() < label.bottom() && rect.bottom() > label.top();
            let right_side = if before_label {
                rect.right() < label.left()
            } else {
                rect.left() > label.right()
            };
            (same_row && right_side).then_some(rect)
        })
        .min_by(|left, right| {
            (left.center().x - label.center().x)
                .abs()
                .total_cmp(&(right.center().x - label.center().x).abs())
        })
        .expect("rendered numeric value next to its label")
}

#[test]
fn pointer_selection_tail_trim_and_toolbar_history_restore_all_tracks() {
    let mut harness = TimelineHarness::new();
    harness.click(harness.clip(0).center());
    assert_eq!(harness.studio.playback_ms, 1000);
    assert_eq!(harness.studio.mode, ReplayMode::Paused);
    assert!(has_text(
        &harness.output,
        "选中：玩家：第一句"
    ));
    harness.save_artifact("timeline-default.png");
    let before = harness.snapshot();

    let start = harness.trim_handle(0).center();
    // 8000 ms duration plus 1000 ms of right-hand trim space.
    let end = start + vec2(harness.ruler().width() / 9.0, 0.0);
    harness.pointer(start, true);
    harness.advance(
        vec![Event::PointerMoved(end)],
        Modifiers::NONE,
    );
    assert_eq!(
        harness.snapshot(),
        before,
        "dragging previews without committing"
    );
    harness.pointer(end, false);
    harness.advance(Vec::new(), Modifiers::NONE);

    let replay = harness.studio.replay.as_ref().unwrap();
    assert_eq!(replay.dialogue[0].duration_ms, 3000);
    assert!(replay.dialogue[0].duration_locked);
    assert_eq!(replay.dialogue[1].time_ms, 5500);
    assert_eq!(replay.camera[0].time_ms, 4500);
    assert_eq!(replay.camera_cuts[0].time_ms, 4500);
    assert_eq!(
        replay.player_movements[0].keyframes[0].time_ms,
        5500
    );
    assert_eq!(
        replay.ship_trajectories[0].keyframes[0].time_ms,
        6000
    );
    assert_eq!(
        replay.standee_positions[0].time_ms,
        6500
    );
    assert_eq!(replay.terrain_changes[0].time_ms, 7000);
    assert_eq!(
        replay.ship_hull_changes[0].time_ms,
        7500
    );
    assert_eq!(replay.duration_ms, 9000);
    let after = harness.snapshot();

    harness.button("撤销");
    assert_eq!(harness.snapshot(), before);
    harness.button("重做");
    assert_eq!(harness.snapshot(), after);

    harness.advance(
        vec![Event::PointerMoved(harness.ruler().center())],
        Modifiers::NONE,
    );
    harness.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(harness.snapshot(), before);
    harness.key(Key::Y, Modifiers::COMMAND);
    assert_eq!(harness.snapshot(), after);
}

#[test]
fn first_clip_click_from_idle_keeps_selection_after_playback_initializes() {
    let mut harness = TimelineHarness::new();
    harness.studio.mode = ReplayMode::Idle;
    harness.advance(Vec::new(), Modifiers::NONE);
    harness.click(harness.clip(0).center());
    assert_eq!(harness.studio.mode, ReplayMode::Paused);
    assert_eq!(harness.studio.playback_ms, 1000);
    for _ in 0..3 {
        harness.advance(Vec::new(), Modifiers::NONE);
        assert!(
            has_text(&harness.output, "选中：玩家：第一句"),
            "initializing preview must preserve the clicked clip selection"
        );
    }
}

#[test]
fn timeline_keyboard_transport_steps_frames_and_respects_text_focus() {
    let mut harness = TimelineHarness::new();
    harness.advance(
        vec![Event::PointerMoved(harness.ruler().center())],
        Modifiers::NONE,
    );
    harness.key(Key::End, Modifiers::NONE);
    assert_eq!(harness.studio.playback_ms, 8000);
    harness.key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        harness.studio.playback_ms, 8000,
        "frame step clamps at composition end"
    );
    harness.key(Key::Home, Modifiers::NONE);
    harness.key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(
        harness.studio.playback_ms, 0,
        "frame step clamps at composition start"
    );
    for expected in [33, 66, 100] {
        harness.key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(harness.studio.playback_ms, expected);
    }
    harness.key(Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(
        harness.studio.playback_ms, 433,
        "Shift advances ten frames"
    );
    harness.key(Key::ArrowLeft, Modifiers::SHIFT);
    assert_eq!(harness.studio.playback_ms, 100);
    harness.key(Key::Space, Modifiers::NONE);
    assert_eq!(harness.studio.mode, ReplayMode::Playing);
    harness.key(Key::Space, Modifiers::NONE);
    assert_eq!(harness.studio.mode, ReplayMode::Paused);

    harness.advance(
        vec![Event::PointerMoved(pos2(-20.0, -20.0))],
        Modifiers::NONE,
    );
    harness.key(Key::Home, Modifiers::NONE);
    assert_eq!(
        harness.studio.playback_ms, 100,
        "timeline shortcuts require the pointer inside the panel"
    );

    harness.click(harness.clip(0).center());
    let value = numeric_rect_next_to_label(&harness.output, "时长", false);
    harness.click(value.center());
    assert!(harness.context.egui_wants_keyboard_input());
    let before = harness.snapshot();
    for key in [
        Key::Home,
        Key::End,
        Key::ArrowLeft,
        Key::ArrowRight,
        Key::Space,
    ] {
        harness.key(key, Modifiers::NONE);
        assert_eq!(
            harness.studio.playback_ms, 1000,
            "text editing must retain transport position"
        );
        assert_eq!(harness.studio.mode, ReplayMode::Paused);
        assert_eq!(harness.snapshot(), before);
    }
}

#[test]
fn numeric_trim_in_idle_keeps_undo_after_space_starts_preview() {
    let mut harness = TimelineHarness::new();
    harness.click(harness.clip(0).center());
    // An already selected clip can remain selected when preview is stopped.
    harness.studio.mode = ReplayMode::Idle;
    harness.advance(Vec::new(), Modifiers::NONE);
    let before = harness.snapshot();
    harness.edit_numeric("时长", false, "3.000");
    harness.button("应用时长");
    assert_eq!(harness.studio.mode, ReplayMode::Idle);
    assert_eq!(
        harness.studio.replay.as_ref().unwrap().dialogue[0].duration_ms,
        3000
    );
    let after = harness.snapshot();
    assert_ne!(after, before);

    harness.advance(
        vec![Event::PointerMoved(harness.ruler().center())],
        Modifiers::NONE,
    );
    harness.key(Key::Space, Modifiers::NONE);
    assert_eq!(harness.studio.mode, ReplayMode::Playing);
    harness.advance(Vec::new(), Modifiers::NONE);
    harness.key(Key::Space, Modifiers::NONE);
    assert_eq!(harness.studio.mode, ReplayMode::Paused);
    harness.button("撤销");
    assert_eq!(
        harness.snapshot(),
        before,
        "preview initialization must preserve the prior trim undo step"
    );
    harness.button("重做");
    assert_eq!(harness.snapshot(), after);
}

#[test]
fn scrolling_keeps_track_labels_and_ruler_fixed_and_clips_out_of_the_gutter() {
    let mut harness = TimelineHarness::new();
    let replay = harness.studio.replay.as_mut().unwrap();
    let sample = replay.player_movements[0].clone();
    for user_id in 100..120 {
        let mut movement = sample.clone();
        movement.user_id = user_id;
        replay.player_movements.push(movement);
    }
    harness.size = vec2(1500.0, 330.0);
    harness.advance(Vec::new(), Modifiers::NONE);
    harness.edit_numeric("缩放", true, "4");
    harness.key(Key::Enter, Modifiers::NONE);
    harness.advance(Vec::new(), Modifiers::NONE);

    let ruler_before = harness.ruler();
    let label_before = text_rect(&harness.output, "台词 / 字幕");
    let clip_before = harness.clip(0);
    assert!(
        clip_before.left() > ruler_before.left() + 250.0,
        "zoom control must enlarge the timeline"
    );
    harness.wheel(vec2(-300.0, 0.0));
    let ruler_after_horizontal = harness.ruler();
    let label_after_horizontal = text_rect(&harness.output, "台词 / 字幕");
    let clip_after_horizontal = harness.clip(0);
    assert!(
        clip_after_horizontal.left() < clip_before.left() - 100.0,
        "horizontal wheel must scroll timeline content"
    );
    assert_eq!(ruler_after_horizontal, ruler_before);
    assert_eq!(
        label_after_horizontal, label_before,
        "track names stay pinned while time scrolls"
    );
    assert!(
        clip_after_horizontal.left() >= ruler_after_horizontal.left(),
        "clip hit testing cannot enter the track-name gutter"
    );

    harness.wheel(vec2(0.0, -110.0));
    let ruler_after_vertical = harness.ruler();
    assert_eq!(
        ruler_after_vertical, ruler_before,
        "ruler stays visible while track rows scroll vertically"
    );
    let label_after_vertical = text_rect(&harness.output, "台词 / 字幕");
    assert_eq!(
        label_after_vertical.left(),
        label_before.left()
    );
    assert!(
        label_after_vertical.top() < label_before.top() - 40.0,
        "vertical wheel must scroll track rows"
    );
    let visible_names = harness
        .output
        .shapes
        .iter()
        .filter(|shape| {
            matches!(&shape.shape, Shape::Text(text)
            if text.galley.text().starts_with("玩家 ")
                && shape.clip_rect.intersects(text.visual_bounding_rect()))
        })
        .collect::<Vec<_>>();
    assert!(
        !visible_names.is_empty(),
        "scrolled player track names remain visible"
    );
    assert!(
        visible_names
            .iter()
            .all(|shape| shape.clip_rect.top() >= ruler_after_vertical.bottom()),
        "track-name paint stays below the sticky ruler"
    );
    assert!(harness.output.shapes.iter().filter(|shape| {
        matches!(&shape.shape, Shape::Rect(rect) if rect.fill == Color32::from_rgb(55, 145, 110))
    }).all(|shape| shape.clip_rect.left() >= ruler_after_vertical.left() && shape.clip_rect.top() >= ruler_after_vertical.bottom()), "moving track clips stay clipped below the ruler and outside the gutter");
    harness.save_artifact("timeline-scrolled.png");
}

#[test]
fn ruler_drag_seeks_and_pauses_preview_without_authoring_changes() {
    let mut harness = TimelineHarness::new();
    let before = harness.snapshot();
    harness.studio.mode = ReplayMode::Playing;
    harness.studio.speech_wait_cue = Some((1, 0));
    let ruler = harness.ruler();
    let start = pos2(ruler.left() + 20.0, ruler.center().y);
    let target = pos2(
        ruler.left() + ruler.width() * (6000.0 / 9000.0),
        ruler.center().y,
    );
    harness.pointer(start, true);
    harness.advance(
        vec![Event::PointerMoved(target)],
        Modifiers::NONE,
    );
    harness.pointer(target, false);
    assert!(harness.studio.playback_ms.abs_diff(6000) <= 1);
    assert_eq!(harness.studio.mode, ReplayMode::Paused);
    assert_eq!(harness.studio.speech_wait_cue, None);
    assert_eq!(harness.snapshot(), before);
}

#[test]
fn escape_cancels_a_pointer_trim_and_recording_prevents_timeline_edits() {
    let mut harness = TimelineHarness::new();
    let before = harness.snapshot();
    let start = harness.trim_handle(0).center();
    let target = start + vec2(90.0, 0.0);
    harness.pointer(start, true);
    harness.advance(
        vec![Event::PointerMoved(target)],
        Modifiers::NONE,
    );
    harness.key(Key::Escape, Modifiers::NONE);
    harness.pointer(target, false);
    assert_eq!(harness.snapshot(), before);

    harness.studio.mode = ReplayMode::Recording;
    harness.advance(Vec::new(), Modifiers::NONE);
    harness.click(harness.clip(1).center());
    assert_eq!(harness.studio.playback_ms, 0);
    let start = harness.trim_handle(0).center();
    harness.pointer(start, true);
    harness.advance(
        vec![Event::PointerMoved(start + vec2(90.0, 0.0))],
        Modifiers::NONE,
    );
    harness.pointer(start + vec2(90.0, 0.0), false);
    assert_eq!(harness.snapshot(), before);
    assert_eq!(
        harness.studio.mode,
        ReplayMode::Recording
    );
}

#[test]
fn clicking_a_partially_visible_clip_reveals_its_start_even_when_time_is_unchanged() {
    let painted_selection = |output: &FullOutput| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                Shape::Rect(rect) if rect.fill == Color32::from_rgb(180, 145, 55) => {
                    Some(rect.rect)
                },
                _ => None,
            })
            .expect("painted selected clip")
    };
    let mut harness = TimelineHarness::new();
    harness.click(harness.clip(0).center());
    assert_eq!(harness.studio.playback_ms, 1000);
    harness.edit_numeric("缩放", true, "4");
    harness.key(Key::Enter, Modifiers::NONE);
    harness.advance(Vec::new(), Modifiers::NONE);
    let before = harness.snapshot();
    let clip_id = Id::new("replay-multitrack-view").with(Id::new(("dialogue", 1_u64)));

    harness.wheel(vec2(-850.0, 0.0));
    let ruler = harness.ruler();
    let partial = harness
        .context
        .read_response(clip_id)
        .expect("partially visible clip");
    assert!(
        partial.rect.left() < ruler.left(),
        "manual scroll hides the selected clip's start"
    );
    assert!(
        partial.interact_rect.is_positive(),
        "the clip body remains clickable"
    );
    assert_eq!(harness.studio.playback_ms, 1000);
    let click_position = partial.interact_rect.center();
    harness.pointer(click_position, true);
    let pressed = harness
        .context
        .read_response(clip_id)
        .expect("pressed partial clip");
    assert!(
        pressed.is_pointer_button_down_on(),
        "partial clip owns the pointer press"
    );
    harness.pointer(click_position, false);
    let released = harness
        .context
        .read_response(clip_id)
        .expect("released partial clip");
    assert!(
        released.clicked(),
        "partial clip receives pointer release as a click"
    );
    harness.advance(Vec::new(), Modifiers::NONE);
    assert_eq!(
        harness.studio.playback_ms, 1000,
        "click seeks to the already-current time"
    );
    assert!(
        painted_selection(&harness.output).left() >= harness.ruler().left(),
        "the first follow frame paints the selected clip's start inside the ruler"
    );
    // egui swaps this_pass/prev_pass at end_pass. A read_response after
    // run_ui can still prefer the prior geometry, unlike FullOutput paint.
    harness.advance(Vec::new(), Modifiers::NONE);
    let revealed = harness
        .context
        .read_response(clip_id)
        .expect("revealed clip");
    assert!(
        revealed.rect.left() >= harness.ruler().left(),
        "the revealed clip's interaction geometry catches up to its painted position"
    );
    assert_eq!(harness.snapshot(), before);

    harness.button("跟随播放头");
    harness.wheel(vec2(-850.0, 0.0));
    let partial = harness
        .context
        .read_response(clip_id)
        .expect("partially visible clip with follow disabled");
    assert!(partial.rect.left() < harness.ruler().left());
    assert!(partial.interact_rect.is_positive());
    harness.click(partial.interact_rect.center());
    assert!(
        painted_selection(&harness.output).left() < harness.ruler().left(),
        "with follow disabled, painting retains the manually scrolled position"
    );
    harness.advance(Vec::new(), Modifiers::NONE);
    let still_partial = harness
        .context
        .read_response(clip_id)
        .expect("selected clip with follow disabled");
    assert!(
        still_partial.rect.left() < harness.ruler().left(),
        "disabling follow preserves the manually scrolled view"
    );
    assert_eq!(harness.studio.playback_ms, 1000);
    assert_eq!(harness.snapshot(), before);
}

#[test]
fn selected_dialogue_text_edit_changes_only_the_selected_cue_and_keeps_transport_paused() {
    let mut harness = TimelineHarness::new();
    harness.studio.replay.as_mut().unwrap().dialogue[1].speech_text = Some("原有读音".into());
    harness.workspace = true;
    harness.size = vec2(1500.0, 900.0);
    for _ in 0..3 {
        harness.advance(Vec::new(), Modifiers::NONE);
    }
    harness.click(harness.clip(1).center());
    assert_eq!(
        harness.studio.selected_dialogue_id,
        Some(2)
    );
    assert_eq!(harness.studio.playback_ms, 4500);
    harness.button("编辑台词");
    assert!(harness.studio.workspace_tab == ReplayWorkspaceTab::Edit);

    let editor_id = harness
        .inspector_text_id
        .expect("stable selected dialogue editor id");
    let editor = harness.rect(editor_id);
    harness.click(editor.center());
    assert_eq!(
        harness.context.memory(|memory| memory.focused()),
        Some(editor_id)
    );
    assert!(harness.context.egui_wants_keyboard_input());
    harness.key(
        Key::A,
        Modifiers::COMMAND | Modifiers::CTRL,
    );
    let mut expected = harness.studio.replay.as_ref().unwrap().clone();
    expected.dialogue[1].text = "修改后的第二句字幕".into();
    expected.dialogue[1].speech_text = None;
    harness.studio.mode = ReplayMode::Playing;
    harness.advance(
        vec![Event::Text("修改后的第二句字幕".into())],
        Modifiers::NONE,
    );
    assert_eq!(
        harness.studio.mode,
        ReplayMode::Paused,
        "editing a subtitle pauses active preview"
    );
    assert_eq!(harness.studio.playback_ms, 4500);
    assert_eq!(
        harness.snapshot(),
        to_vec(&expected).unwrap(),
        "text editing preserves every other cue, duration, speaker, camera and track"
    );

    // Leave text focused while moving over the timeline so its Space shortcut
    // must respect the editor's keyboard ownership even inside the timeline.
    harness.advance(
        vec![Event::PointerMoved(harness.ruler().center())],
        Modifiers::NONE,
    );
    assert_eq!(
        harness.context.memory(|memory| memory.focused()),
        Some(editor_id)
    );
    harness.key(Key::Space, Modifiers::NONE);
    assert_eq!(harness.studio.mode, ReplayMode::Paused);
    assert_eq!(harness.studio.playback_ms, 4500);
    assert_eq!(
        harness.snapshot(),
        to_vec(&expected).unwrap()
    );
    assert_eq!(
        harness.studio.selected_dialogue_id,
        Some(2)
    );
}

#[test]
fn workspace_tabs_click_and_preview_remains_inside_resized_docks() {
    let mut harness = TimelineHarness::new();
    harness.click(harness.clip(0).center());
    harness.workspace = true;
    harness.size = vec2(1500.0, 900.0);
    for _ in 0..3 {
        harness.advance(Vec::new(), Modifiers::NONE);
    }
    for (label, tab) in [
        ("编辑", ReplayWorkspaceTab::Edit),
        ("镜头", ReplayWorkspaceTab::Camera),
        ("导演", ReplayWorkspaceTab::Director),
        ("输出", ReplayWorkspaceTab::Export),
        ("项目", ReplayWorkspaceTab::Project),
    ] {
        harness.button(label);
        assert!(
            harness.studio.workspace_tab == tab,
            "clicked workspace tab: {label}"
        );
    }
    harness.button("编辑");
    for (size, filename) in [
        (
            vec2(1500.0, 900.0),
            "workspace-default.png",
        ),
        (
            vec2(950.0, 650.0),
            "workspace-narrow.png",
        ),
        (
            vec2(1800.0, 1000.0),
            "workspace-wide.png",
        ),
    ] {
        harness.size = size;
        for _ in 0..3 {
            harness.advance(Vec::new(), Modifiers::NONE);
        }
        let preview = harness
            .studio
            .preview_rect
            .expect("composition preview viewport");
        let (properties, timeline) = harness.workspace_docks.expect("rendered dock rectangles");
        assert!(preview.is_positive());
        assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(preview));
        assert!(
            preview.left() >= properties.right(),
            "preview overlaps properties dock: {preview:?}"
        );
        assert!(
            preview.bottom() <= timeline.top(),
            "preview overlaps timeline: {preview:?}"
        );
        assert!((preview.aspect_ratio() - size.x / size.y).abs() < 0.001);
        assert!(harness.output.shapes.iter().any(|shape| {
            matches!(&shape.shape, Shape::Rect(rect) if rect.rect == preview && rect.stroke.color == Color32::from_gray(65))
        }), "composition boundary is painted at the viewport rectangle");
        assert!(
            has_text(&harness.output, "所选台词"),
            "selected dialogue inspector is rendered in the properties dock"
        );
        harness.save_artifact(filename);
    }
    harness.button("返回主界面");
    assert!(!harness.studio.panel_open);
}

#[test]
fn inspector_speaker_combo_swaps_the_whole_identity() {
    let mut harness = TimelineHarness::new();
    {
        let replay = harness.studio.replay.as_mut().unwrap();
        replay.dialogue[1].sender_id = 8;
        replay.dialogue[1].name = "队员".into();
    }
    harness.workspace = true;
    harness.size = vec2(1500.0, 900.0);
    for _ in 0..3 {
        harness.advance(Vec::new(), Modifiers::NONE);
    }
    harness.click(harness.clip(1).center());
    harness.button("编辑台词");

    // 打开发言人下拉并选择 玩家（7）· 玩家。
    harness.button("队员（8）");
    harness.advance(Vec::new(), Modifiers::NONE);
    harness.button("玩家（7）· 玩家");

    let line = &harness.studio.replay.as_ref().unwrap().dialogue[1];
    assert_eq!(line.sender_id, 7);
    assert_eq!(line.name, "玩家");
    assert_eq!(line.role, "玩家");
    assert_eq!(line.side, DialogueSide::Right);
    assert_eq!(
        harness.studio.replay.as_ref().unwrap().dialogue[0].sender_id,
        7,
        "其他台词不受影响"
    );
    assert_eq!(harness.studio.timeline_revision, 1);
    assert!(harness.studio.status.contains("已将发言人改为 玩家"));
    assert_eq!(
        harness.studio.selected_dialogue_id,
        Some(2)
    );
}

#[test]
fn first_person_speech_area_edit_asks_to_save_or_exit_without_saving() {
    let mut harness = TimelineHarness::new();
    harness.workspace = true;
    harness.size = vec2(1500.0, 900.0);
    for _ in 0..3 {
        harness.advance(Vec::new(), Modifiers::NONE);
    }
    harness.click(harness.clip(0).center());
    harness.button("编辑台词");
    let before = harness.snapshot();

    harness.button("第一人称修改发言区域");
    assert_eq!(harness.studio.mode, ReplayMode::Paused);
    assert_eq!(
        harness.studio.playback_ms, 1000,
        "预览暂停到台词时刻"
    );
    assert!(harness.studio.live_editing_enabled);
    assert_eq!(
        harness
            .studio
            .speech_area_edit
            .as_ref()
            .map(|session| session.line_id),
        Some(1)
    );

    harness.advance(Vec::new(), Modifiers::NONE);
    assert!(
        has_text(&harness.output, "第一人称发言区域编辑"),
        "HUD 窗口在全屏会话里可见"
    );
    harness.button("结束操作");
    harness.advance(Vec::new(), Modifiers::NONE);
    assert!(has_text(
        &harness.output,
        "是否保存并应用？"
    ));

    harness.button("不保存退出");
    assert!(harness.studio.speech_area_edit.is_none());
    assert!(!harness.studio.live_editing_enabled);
    assert_eq!(
        harness.snapshot(),
        before,
        "无保存退出必须完整恢复进入前的回放项目"
    );
}

#[test]
fn speaker_rename_window_applies_the_new_name_to_every_line() {
    let mut harness = TimelineHarness::new();
    harness.workspace = true;
    harness.size = vec2(1500.0, 900.0);
    for _ in 0..3 {
        harness.advance(Vec::new(), Modifiers::NONE);
    }
    harness.click(harness.clip(0).center());
    harness.button("编辑台词");
    let before = harness.snapshot();

    harness.button("重命名");
    harness.advance(Vec::new(), Modifiers::NONE);
    assert!(
        has_text(&harness.output, "重命名发言人"),
        "重命名窗口在检查器按钮点击后打开"
    );
    assert!(
        has_text(&harness.output, "玩家（7）· 2 句台词"),
        "窗口预填当前显示名与台词数"
    );
    assert!(harness.studio.speaker_rename.is_some());

    // 通过草稿模拟输入，再点击应用。
    harness.studio.speaker_rename.as_mut().unwrap().name = "柳絮".into();
    harness.advance(Vec::new(), Modifiers::NONE);
    harness.button("应用");

    assert!(harness.studio.speaker_rename.is_none());
    let replay = harness.studio.replay.as_ref().unwrap();
    assert!(replay.dialogue.iter().all(|line| line.name == "柳絮"));
    assert_eq!(harness.studio.timeline_revision, 1);
    assert!(harness.studio.status.contains("已将 玩家（7）重命名为 柳絮"));

    // 撤销路径：重命名前后的项目快照可通过时间轴历史以外的方式对比。
    assert_ne!(harness.snapshot(), before);
}

#[test]
fn speaker_rename_window_cancel_keeps_every_name() {
    let mut harness = TimelineHarness::new();
    harness.workspace = true;
    harness.size = vec2(1500.0, 900.0);
    for _ in 0..3 {
        harness.advance(Vec::new(), Modifiers::NONE);
    }
    harness.click(harness.clip(0).center());
    harness.button("编辑台词");
    let before = harness.snapshot();

    harness.button("重命名");
    harness.advance(Vec::new(), Modifiers::NONE);
    harness.studio.speaker_rename.as_mut().unwrap().name = "改名".into();
    harness.advance(Vec::new(), Modifiers::NONE);
    harness.button("取消");

    assert!(harness.studio.speaker_rename.is_none());
    assert_eq!(harness.snapshot(), before);
    assert_eq!(harness.studio.timeline_revision, 0);
}
