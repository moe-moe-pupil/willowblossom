use std::{
    collections::BTreeMap,
    hash::{
        DefaultHasher,
        Hash,
        Hasher,
    },
    sync::Arc,
};

use bevy::prelude::{
    Query,
    With,
};
use bevy_egui::egui::{
    pos2,
    vec2,
    Align2,
    Button,
    Color32,
    CursorIcon,
    DragValue,
    FontId,
    Id,
    Key,
    Label,
    Modifiers,
    Rect,
    ScrollArea,
    Sense,
    Slider,
    Stroke,
    Ui,
};
use serde_json::{
    from_slice,
    to_vec,
};
use voxxelmaxx::prelude::Grid;

use super::{
    format_time,
    frame_time_ms,
    invalidate_replay_timeline,
    prepare_replay_movement_for_playback,
    replay_dialogue_is_playable,
    replay_edit_fingerprint_state,
    replay_has_live_take,
    set_exact_dialogue_duration_at_playhead,
    start_playback,
    ReplayCameraCut,
    ReplayCameraKeyframe,
    ReplayDialogue,
    ReplayFile,
    ReplayMode,
    ReplayPlayerMovement,
    ReplayPlayerMovementHistory,
    ReplayShipHullChange,
    ReplayShipTrajectory,
    ReplayShipTrajectoryHistory,
    ReplayStandeePosition,
    ReplayStudio,
    ReplayTerrainChange,
    TrpgVoxelGrid,
    MAX_LIVE_DIALOGUE_MS,
    MIN_LIVE_DIALOGUE_MS,
};

#[derive(Clone)]
struct TimelineView {
    zoom: f32,
    selected: Option<(usize, usize)>,
    duration_seconds: Option<f64>,
    snap: bool,
    trim: Option<TrimGesture>,
    history: TimelineHistory,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            selected: None,
            duration_seconds: None,
            snap: true,
            trim: None,
            history: TimelineHistory::default(),
        }
    }
}

struct Clip {
    start: u64,
    end: u64,
    label: String,
    dialogue_index: Option<usize>,
}

#[derive(Clone, Copy)]
struct TrimGesture {
    dialogue_index: usize,
    start: u64,
    end: u64,
    width: f32,
    extent: u64,
    duration_ms: u64,
}

#[derive(Clone)]
struct TimelineSnapshot {
    // Serialized timing tracks avoid retaining an entire voxel world for every
    // edit. Arc keeps egui's temporary view-state clone cheap each frame.
    tracks: Arc<[u8]>,
    fingerprint: u64,
    playback_ms: u64,
}

type TimelineTracks = (
    u64,
    Vec<ReplayDialogue>,
    Vec<ReplayCameraKeyframe>,
    Vec<ReplayCameraCut>,
    Vec<ReplayPlayerMovement>,
    Vec<ReplayShipTrajectory>,
    Vec<ReplayStandeePosition>,
    Vec<ReplayTerrainChange>,
    Vec<ReplayShipHullChange>,
);

fn timeline_fingerprint(replay: &ReplayFile) -> Option<u64> {
    let mut hasher = DefaultHasher::new();
    replay_edit_fingerprint_state(replay)
        .ok()?
        .hash(&mut hasher);
    Some(hasher.finish())
}

impl TimelineSnapshot {
    fn capture(studio: &ReplayStudio) -> Option<Self> {
        let replay = studio.replay.as_ref()?;
        let tracks = to_vec(&(
            replay.duration_ms,
            &replay.dialogue,
            &replay.camera,
            &replay.camera_cuts,
            &replay.player_movements,
            &replay.ship_trajectories,
            &replay.standee_positions,
            &replay.terrain_changes,
            &replay.ship_hull_changes,
        ))
        .ok()?
        .into();
        Some(Self {
            tracks,
            fingerprint: timeline_fingerprint(replay)?,
            playback_ms: studio.playback_ms,
        })
    }

    fn restore(&self, studio: &mut ReplayStudio) -> bool {
        let Ok(tracks) = from_slice::<TimelineTracks>(&self.tracks) else {
            return false;
        };
        let Some(replay) = studio.replay.as_mut() else { return false };
        (
            replay.duration_ms,
            replay.dialogue,
            replay.camera,
            replay.camera_cuts,
            replay.player_movements,
            replay.ship_trajectories,
            replay.standee_positions,
            replay.terrain_changes,
            replay.ship_hull_changes,
        ) = tracks;
        studio.playback_ms = self.playback_ms.min(replay.duration_ms);
        invalidate_replay_timeline(studio);
        clear_speech_wait(studio);
        true
    }
}

#[derive(Clone)]
struct TimelineEdit {
    before: TimelineSnapshot,
    after: TimelineSnapshot,
}

#[derive(Clone, Default)]
struct TimelineHistory {
    project: Option<(String, u64)>,
    revision: u64,
    undo: Vec<TimelineEdit>,
    redo: Vec<TimelineEdit>,
}

impl TimelineHistory {
    fn synchronize(&mut self, studio: &ReplayStudio) -> bool {
        let project = studio.replay.as_ref().map(|replay| {
            (
                replay.campaign_id.clone(),
                replay.created_at_unix_ms,
            )
        });
        let changed = self.project != project || self.revision != studio.timeline_revision;
        if changed {
            self.project = project;
            self.revision = studio.timeline_revision;
            self.undo.clear();
            self.redo.clear();
        }
        changed
    }

    fn trim(&mut self, studio: &mut ReplayStudio, index: usize, duration_ms: u64) -> bool {
        self.synchronize(studio);
        if !timeline_editing_enabled(studio) {
            return false;
        }
        let duration_ms = duration_ms.clamp(
            MIN_LIVE_DIALOGUE_MS,
            MAX_LIVE_DIALOGUE_MS,
        );
        let Some(line) = studio
            .replay
            .as_ref()
            .and_then(|replay| replay.dialogue.get(index))
        else {
            return false;
        };
        if !replay_dialogue_is_playable(line)
            || (line.duration_ms == duration_ms && line.duration_locked)
        {
            return false;
        }
        let Some(before) = TimelineSnapshot::capture(studio) else { return false };
        if self
            .undo
            .last()
            .is_some_and(|edit| edit.after.fingerprint != before.fingerprint)
        {
            self.undo.clear();
        }
        set_exact_dialogue_duration_at_playhead(studio, index, duration_ms);
        clear_speech_wait(studio);
        let Some(after) = TimelineSnapshot::capture(studio) else {
            before.restore(studio);
            self.revision = studio.timeline_revision;
            return false;
        };
        self.undo.push(TimelineEdit { before, after });
        self.redo.clear();
        // Bound both the action count and memory retained by large event tracks.
        while self.undo.len() > 32
            || (self.undo.len() > 1
                && self
                    .undo
                    .iter()
                    .map(|edit| edit.before.tracks.len() + edit.after.tracks.len())
                    .sum::<usize>()
                    > 32 * 1024 * 1024)
        {
            self.undo.remove(0);
        }
        self.revision = studio.timeline_revision;
        studio.status = format!(
            "已修剪台词至 {:.3} 秒，后续轨道同步移动",
            duration_ms as f64 / 1000.0
        );
        true
    }

    fn restore_edit(&mut self, studio: &mut ReplayStudio, redo: bool) -> bool {
        self.synchronize(studio);
        if !timeline_editing_enabled(studio) {
            return false;
        }
        let stack = if redo { &mut self.redo } else { &mut self.undo };
        let Some(edit) = stack.last() else { return false };
        let expected = if redo { &edit.before } else { &edit.after };
        // Some legacy inspector operations do not bump timeline_revision. Check
        // the authored content before restoring so undo cannot erase those edits.
        if studio.replay.as_ref().and_then(timeline_fingerprint) != Some(expected.fingerprint) {
            self.undo.clear();
            self.redo.clear();
            studio.status = "回放已在其他面板修改，时间轴撤销记录已清空".into();
            return false;
        }
        let target = if redo { &edit.after } else { &edit.before };
        if !target.restore(studio) {
            return false;
        }
        let edit = stack.pop().expect("checked above");
        if redo {
            self.undo.push(edit);
        } else {
            self.redo.push(edit);
        }
        self.revision = studio.timeline_revision;
        studio.status = if redo { "已重做台词修剪" } else { "已撤销台词修剪" }.into();
        true
    }
}

fn timeline_editing_enabled(studio: &ReplayStudio) -> bool {
    studio.mode != ReplayMode::Recording
        && !replay_has_live_take(studio)
        && studio.video_render.is_none()
        && studio.video_encoding.is_none()
}

fn clear_speech_wait(studio: &mut ReplayStudio) {
    if studio.mode == ReplayMode::Playing {
        studio.mode = ReplayMode::Paused;
    }
    studio.speech_wait_cue = None;
    studio.speech_wait_elapsed_seconds = 0.0;
    studio.speech_bypass_cues.clear();
}

fn trimmed_duration(
    start: u64,
    end: u64,
    delta_x: f32,
    width: f32,
    extent: u64,
    fps: u32,
    snap: bool,
) -> u64 {
    let endpoint = (end as f64 + delta_x as f64 / width.max(1.0) as f64 * extent as f64).max(0.0);
    let endpoint = if snap {
        frame_time_ms(
            (endpoint * fps.max(1) as f64 / 1000.0).round() as u64,
            fps.max(1),
        )
    } else {
        endpoint.round() as u64
    };
    endpoint.saturating_sub(start).clamp(
        MIN_LIVE_DIALOGUE_MS,
        MAX_LIVE_DIALOGUE_MS,
    )
}

struct Track {
    name: String,
    color: Color32,
    clips: Vec<Clip>,
}

fn time_at_x(x: f32, left: f32, width: f32, duration: u64) -> u64 {
    (((x - left) / width.max(1.0)).clamp(0.0, 1.0) as f64 * duration as f64).round() as u64
}

fn adjacent_frame(time: u64, fps: u32, forward: bool) -> u64 {
    let fps = fps.max(1) as u64;
    // Export timestamps round down to milliseconds. Recover the frame index
    // with ceiling division so repeated stepping never accumulates rounding drift.
    let frame = time.saturating_mul(fps).div_ceil(1000);
    if forward {
        frame_time_ms(
            time.saturating_add(1).saturating_mul(fps).div_ceil(1000),
            fps as u32,
        )
    } else {
        frame_time_ms(frame.saturating_sub(1), fps as u32)
    }
}

pub(super) fn replay_timeline_ui(
    ui: &mut Ui,
    studio: &mut ReplayStudio,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    player_history: &ReplayPlayerMovementHistory,
    ship_history: &ReplayShipTrajectoryHistory,
) {
    let ctx = ui.ctx().clone();
    let id = Id::new("replay-multitrack-view");
    let mut view = ctx.data_mut(|data| data.get_temp::<TimelineView>(id).unwrap_or_default());
    if view.history.synchronize(studio) {
        view.selected = None;
        view.duration_seconds = None;
        view.trim = None;
    }
    let Some(replay) = studio.replay.as_ref() else {
        ui.strong("时间轴");
        ui.weak("从项目面板生成或导入回放，台词、镜头、角色移动和场景事件会显示在这里。");
        ctx.data_mut(|data| data.insert_temp(id, view));
        return;
    };
    let duration = replay.duration_ms.max(1);
    let extent = duration.saturating_add((duration / 10).max(1000));
    let title = replay.title.clone();
    let mut tracks = vec![
        Track {
            name: "台词 / 字幕".into(),
            color: Color32::from_rgb(65, 125, 175),
            clips: replay
                .dialogue
                .iter()
                .enumerate()
                .filter(|(_, line)| replay_dialogue_is_playable(line))
                .map(|(index, line)| Clip {
                    start: line.time_ms,
                    end: line.time_ms.saturating_add(line.duration_ms),
                    label: format!("{}：{}", line.name, line.text),
                    dialogue_index: Some(index),
                })
                .collect(),
        },
        Track {
            name: "镜头关键帧".into(),
            color: Color32::from_rgb(135, 95, 180),
            clips: replay
                .camera
                .iter()
                .map(|frame| Clip {
                    start: frame.time_ms,
                    end: frame.time_ms,
                    label: "镜头关键帧".into(),
                    dialogue_index: None,
                })
                .chain(
                    replay.camera_cuts.iter().map(|cut| Clip {
                        start: cut.time_ms,
                        end: cut.time_ms,
                        label: "手动切镜".into(),
                        dialogue_index: None,
                    }),
                )
                .collect(),
        },
    ];
    for movement in &replay.player_movements {
        tracks.push(Track {
            name: format!("玩家 {}", movement.user_id),
            color: Color32::from_rgb(55, 145, 110),
            clips: movement
                .keyframes
                .first()
                .zip(movement.keyframes.last())
                .map(|(first, last)| Clip {
                    start: first.time_ms,
                    end: last.time_ms,
                    label: format!(
                        "移动 · {} 个关键帧",
                        movement.keyframes.len()
                    ),
                    dialogue_index: None,
                })
                .into_iter()
                .collect(),
        });
    }
    for ship in &replay.ship_trajectories {
        tracks.push(Track {
            name: format!("飞船 {}", ship.ship_name),
            color: Color32::from_rgb(160, 115, 55),
            clips: ship
                .keyframes
                .first()
                .zip(ship.keyframes.last())
                .map(|(first, last)| Clip {
                    start: first.time_ms,
                    end: last.time_ms,
                    label: format!(
                        "航迹 · {} 个关键帧",
                        ship.keyframes.len()
                    ),
                    dialogue_index: None,
                })
                .into_iter()
                .collect(),
        });
    }
    let mut events = BTreeMap::<u64, usize>::new();
    for time in replay
        .terrain_changes
        .iter()
        .filter(|change| change.enabled)
        .map(|change| change.time_ms)
        .chain(
            replay
                .ship_hull_changes
                .iter()
                .filter(|change| change.enabled)
                .map(|change| change.time_ms),
        )
    {
        *events.entry(time).or_default() += 1;
    }
    tracks.push(Track {
        name: "场景事件".into(),
        color: Color32::from_rgb(170, 75, 85),
        clips: events
            .into_iter()
            .map(|(time, count)| Clip {
                start: time,
                end: time,
                label: format!("场景修改 · {count} 个体素"),
                dialogue_index: None,
            })
            .collect(),
    });
    let mut seek = None;
    let mut trim_request = None;
    let mut undo = false;
    let mut redo = false;
    let enabled = timeline_editing_enabled(studio);
    if !enabled {
        view.trim = None;
    }
    if view.trim.is_some() && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
    {
        view.trim = None;
    }
    if enabled && !ctx.egui_wants_keyboard_input() && ui.rect_contains_pointer(ui.max_rect()) {
        redo = ui.input_mut(|input| {
            input.consume_key(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::Z,
            ) || input.consume_key(Modifiers::COMMAND, Key::Y)
        });
        undo = !redo && ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Z));
    }
    ui.horizontal_wrapped(|ui| {
        ui.strong(&title);
        ui.monospace(format!(
            "{} / {}",
            format_time(studio.playback_ms),
            format_time(duration)
        ));
        ui.add_enabled_ui(enabled, |ui| {
            if ui.button("⏮ 片头").clicked() {
                seek = Some(0);
            }
            if ui.button("◀ 前一帧").clicked() {
                seek = Some(adjacent_frame(
                    studio.playback_ms,
                    studio.video_fps,
                    false,
                ));
            }
            if ui
                .button(if studio.mode == ReplayMode::Playing { "暂停" } else { "▶ 播放" })
                .clicked()
            {
                match studio.mode {
                    ReplayMode::Playing => studio.mode = ReplayMode::Paused,
                    ReplayMode::Paused => {
                        if studio.playback_ms >= duration {
                            studio.playback_ms = 0;
                        }
                        studio.mode = ReplayMode::Playing;
                    },
                    ReplayMode::Idle => {
                        let time = studio.playback_ms;
                        prepare_replay_movement_for_playback(studio, player_history, ship_history);
                        start_playback(studio, grids);
                        studio.playback_ms = time.min(duration);
                    },
                    ReplayMode::Recording => {},
                }
            }
            if ui.button("后一帧 ▶").clicked() {
                seek = Some(
                    adjacent_frame(
                        studio.playback_ms,
                        studio.video_fps,
                        true,
                    )
                    .min(duration),
                );
            }
        });
        ui.add(
            Slider::new(&mut view.zoom, 1.0..=20.0)
                .text("缩放")
                .logarithmic(true),
        );
        if ui.button("适合全部").clicked() {
            view.zoom = 1.0;
        }
        ui.checkbox(&mut view.snap, "帧吸附");
        undo |= ui
            .add_enabled(
                enabled && !view.history.undo.is_empty(),
                Button::new("撤销"),
            )
            .on_hover_text("撤销台词修剪 · Ctrl+Z")
            .clicked();
        redo |= ui
            .add_enabled(
                enabled && !view.history.redo.is_empty(),
                Button::new("重做"),
            )
            .on_hover_text("重做台词修剪 · Ctrl+Shift+Z / Ctrl+Y")
            .clicked();
    });
    let selected_clip = view
        .selected
        .and_then(|(row, index)| tracks.get(row)?.clips.get(index));
    // Keep the inspector present before selection so a first edge drag does not
    // move the track vertically underneath the pointer.
    ui.horizontal_wrapped(|ui| {
        ui.add_sized(
            vec2(ui.available_width().min(280.0), 20.0),
            Label::new(selected_clip.map_or_else(
                || "选择一个片段查看属性".into(),
                |clip| format!("选中：{}", clip.label),
            ))
            .truncate(),
        );
        ui.monospace(format!(
            "{} — {}",
            format_time(selected_clip.map_or(0, |clip| clip.start)),
            format_time(selected_clip.map_or(0, |clip| clip.end))
        ));
        let dialogue_index = selected_clip.and_then(|clip| clip.dialogue_index);
        let seconds = view
            .duration_seconds
            .get_or_insert(selected_clip.map_or(0.1, |clip| {
                (clip.end - clip.start) as f64 / 1000.0
            }));
        ui.add_enabled_ui(
            enabled && dialogue_index.is_some(),
            |ui| {
                ui.label("时长");
                let response = ui.add(
                    DragValue::new(seconds)
                        .speed(0.01)
                        .range(0.1..=120.0)
                        .fixed_decimals(3)
                        .suffix(" 秒"),
                );
                let enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                if ui.button("应用时长").clicked() || enter {
                    trim_request = dialogue_index.map(|index| {
                        (
                            index,
                            (*seconds * 1000.0).round() as u64,
                        )
                    });
                }
            },
        );
    });
    ui.small(
        "拖动标尺定位；拖动台词右边缘修剪，后续轨道同步移动。Shift 临时关闭帧吸附；Esc 取消拖动。",
    );
    let viewport_width = ui.available_width();
    ScrollArea::both()
        .id_salt("replay-tracks-scroll")
        .auto_shrink([false, false])
        .max_height(ui.available_height().max(60.0))
        .show(ui, |ui| {
            let label_width = 145.0;
            let width = (viewport_width - label_width).max(220.0) * view.zoom;
            let (rect, _) = ui.allocate_exact_size(
                vec2(
                    label_width + width,
                    30.0 + tracks.len() as f32 * 32.0,
                ),
                Sense::hover(),
            );
            let left = rect.left() + label_width;
            let x_at = |time: u64| left + (time as f64 / extent as f64) as f32 * width;
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, Color32::from_rgb(24, 27, 33));
            let ruler = Rect::from_min_max(
                pos2(left, rect.top()),
                pos2(rect.right(), rect.top() + 30.0),
            );
            let response = ui.interact(
                ruler,
                id.with("ruler"),
                Sense::click_and_drag(),
            );
            if enabled && (response.clicked() || response.dragged()) {
                if let Some(pointer) = response.interact_pointer_pos() {
                    seek = Some(time_at_x(pointer.x, left, width, extent).min(duration));
                }
            }
            let ticks = (width / 95.0).ceil().max(1.0) as u64;
            for tick in 0..=ticks {
                let time = (extent as f64 * tick as f64 / ticks as f64) as u64;
                let x = x_at(time);
                painter.line_segment(
                    [pos2(x, rect.top() + 22.0), pos2(x, rect.bottom())],
                    Stroke::new(1.0, Color32::from_gray(48)),
                );
                painter.text(
                    pos2(x + 3.0, rect.top() + 3.0),
                    Align2::LEFT_TOP,
                    format_time(time),
                    FontId::monospace(11.0),
                    Color32::LIGHT_GRAY,
                );
            }
            for (row, track) in tracks.iter().enumerate() {
                let top = rect.top() + 30.0 + row as f32 * 32.0;
                painter
                    .with_clip_rect(Rect::from_min_max(
                        pos2(rect.left(), top),
                        pos2(left - 4.0, top + 32.0),
                    ))
                    .text(
                        pos2(rect.left() + 6.0, top + 15.0),
                        Align2::LEFT_CENTER,
                        &track.name,
                        FontId::proportional(12.0),
                        Color32::LIGHT_GRAY,
                    );
                for (index, clip) in track.clips.iter().enumerate() {
                    let preview = view
                        .trim
                        .filter(|trim| Some(trim.dialogue_index) == clip.dialogue_index);
                    let clip_end = preview.map_or(clip.end, |trim| {
                        trim.start.saturating_add(trim.duration_ms)
                    });
                    let clip_left = x_at(clip.start.min(extent)).min(rect.right() - 5.0);
                    let clip_rect = Rect::from_min_max(
                        pos2(clip_left, top + 3.0),
                        pos2(
                            (x_at(clip_end.min(extent)).max(clip_left + 5.0)).min(rect.right()),
                            top + 28.0,
                        ),
                    );
                    if !ui.is_rect_visible(clip_rect) {
                        continue;
                    }
                    let selected = view.selected == Some((row, index));
                    painter.rect_filled(
                        clip_rect,
                        2.0,
                        if selected { Color32::from_rgb(180, 145, 55) } else { track.color },
                    );
                    if clip_rect.width() > 35.0 {
                        let preview_label = preview.map(|trim| {
                            format!(
                                "时长 {:.3} 秒",
                                trim.duration_ms as f64 / 1000.0
                            )
                        });
                        painter.with_clip_rect(clip_rect.shrink(2.0)).text(
                            clip_rect.left_center() + vec2(4.0, 0.0),
                            Align2::LEFT_CENTER,
                            preview_label.as_deref().unwrap_or(&clip.label),
                            FontId::proportional(12.0),
                            Color32::WHITE,
                        );
                    }
                    let mut body_rect = clip_rect;
                    if clip.dialogue_index.is_some() {
                        body_rect.max.x = (body_rect.right() - 8.0).max(body_rect.left());
                    }
                    let response = ui.interact(
                        body_rect,
                        id.with((row, index)),
                        Sense::click(),
                    );
                    if response.clicked() && enabled {
                        view.selected = Some((row, index));
                        view.duration_seconds = None;
                        seek = Some(clip.start);
                    }
                    response.on_hover_text(format!(
                        "{}\n{} — {}",
                        clip.label,
                        format_time(clip.start),
                        format_time(clip.end)
                    ));
                    if let Some(dialogue_index) = clip.dialogue_index {
                        let handle = Rect::from_min_max(
                            pos2(
                                (clip_rect.right() - 8.0).max(clip_rect.left()),
                                clip_rect.top(),
                            ),
                            clip_rect.max,
                        );
                        painter.rect_filled(
                            handle.shrink2(vec2(2.0, 5.0)),
                            1.0,
                            Color32::from_white_alpha(135),
                        );
                        let response = ui
                            .interact(
                                handle,
                                id.with(("trim", dialogue_index)),
                                Sense::click_and_drag(),
                            )
                            .on_hover_cursor(CursorIcon::ResizeHorizontal)
                            .on_hover_text("拖动句尾修剪时长；后续台词、镜头、移动和事件同步移动");
                        if enabled && response.clicked() {
                            view.selected = Some((row, index));
                            view.duration_seconds = None;
                            seek = Some(clip.start);
                        }
                        if enabled && response.drag_started() {
                            view.selected = Some((row, index));
                            view.duration_seconds = None;
                            if studio.mode == ReplayMode::Playing {
                                studio.mode = ReplayMode::Paused;
                            }
                            view.trim = Some(TrimGesture {
                                dialogue_index,
                                start: clip.start,
                                end: clip.end,
                                width,
                                extent,
                                duration_ms: clip.end - clip.start,
                            });
                        }
                        if enabled && (response.dragged() || response.drag_stopped()) {
                            if let Some(trim) = view
                                .trim
                                .as_mut()
                                .filter(|trim| trim.dialogue_index == dialogue_index)
                            {
                                if let Some(delta) = response.total_drag_delta() {
                                    trim.duration_ms = trimmed_duration(
                                        trim.start,
                                        trim.end,
                                        delta.x,
                                        trim.width,
                                        trim.extent,
                                        studio.video_fps,
                                        view.snap && !ui.input(|input| input.modifiers.shift),
                                    );
                                }
                                response.clone().on_hover_text(format!(
                                    "时长 {:.3} 秒",
                                    trim.duration_ms as f64 / 1000.0
                                ));
                                if response.drag_stopped() {
                                    trim_request = Some((dialogue_index, trim.duration_ms));
                                    view.trim = None;
                                }
                            }
                        }
                    }
                }
            }
            let x = x_at(studio.playback_ms.min(duration));
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(2.0, Color32::from_rgb(245, 100, 90)),
            );
        });
    if undo || redo {
        view.history.restore_edit(studio, redo);
        view.duration_seconds = None;
        view.trim = None;
    } else if let Some((index, duration_ms)) = trim_request {
        view.history.trim(studio, index, duration_ms);
        view.duration_seconds = None;
    }
    if let Some(time) = seek {
        if studio.mode == ReplayMode::Idle {
            prepare_replay_movement_for_playback(studio, player_history, ship_history);
            start_playback(studio, grids);
        }
        studio.playback_ms = time.min(duration);
        studio.mode = ReplayMode::Paused;
        studio.speech_wait_cue = None;
        studio.speech_wait_elapsed_seconds = 0.0;
    }
    ctx.data_mut(|data| data.insert_temp(id, view));
}

#[cfg(test)]
mod tests {
    use serde_json::{
        from_value,
        json,
        to_vec,
    };

    use super::{
        adjacent_frame,
        frame_time_ms,
        time_at_x,
        trimmed_duration,
        ReplayMode,
        ReplayStudio,
        TimelineHistory,
    };

    fn editing_studio() -> ReplayStudio {
        let line = |line_id: u64, time_ms: u64| {
            json!({
                "line_id": line_id, "time_ms": time_ms, "duration_ms": 2000,
                "sender_id": 7, "name": "玩家", "role": "玩家", "text": "台词",
                "avatar": "", "visibility": {"scope": "public"},
            })
        };
        ReplayStudio {
            replay: Some(from_value(json!({
                "format_version": 3, "title": "修剪测试", "campaign_id": "campaign",
                "created_at_unix_ms": 1, "duration_ms": 6000,
                "audience": {"scope": "public"},
                "scene": {"voxels": [{"position": [0, 0, 0], "material": 1}]},
                "dialogue": [line(1, 1000), line(2, 3500), line(3, u64::MAX)],
                "camera": [
                    {"time_ms": 2500, "translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                    {"time_ms": 3000, "translation": [1.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]}
                ],
                "camera_cuts": [{"time_ms": 3000, "shot": null}],
                "player_movements": [{"user_id": 7, "keyframes": [
                    {"time_ms": 4000, "position_cells": [1.0, 0.0, 0.0]}
                ]}],
                "ship_trajectories": [{"ship_id": "ship", "ship_name": "船", "keyframes": [
                    {"time_ms": 4500, "translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]}
                ]}],
                "standee_positions": [{"time_ms": 5000, "user_id": 7, "position": [0.0, 0.0, 0.0]}],
                "terrain_changes": [{"time_ms": 5200, "position": [0, 0, 0], "material": 0}],
                "ship_hull_changes": [{"time_ms": 5400, "ship_id": "ship", "position": [0, 0, 0], "material": 0}]
            })).unwrap()),
            playback_ms: 2800,
            ..ReplayStudio::default()
        }
    }

    #[test]
    fn undo_redo_restores_every_track_and_keyframe_order_after_ripple_trim() {
        let mut studio = editing_studio();
        let mut history = TimelineHistory::default();
        let before = to_vec(studio.replay.as_ref().unwrap()).unwrap();
        studio.mode = ReplayMode::Playing;
        studio.speech_wait_cue = Some((1, 0));
        studio.speech_bypass_cues.insert((1, 0));
        assert!(history.trim(&mut studio, 0, 1000));
        assert_eq!(studio.mode, ReplayMode::Paused);
        assert_eq!(studio.playback_ms, 1999);
        assert!(studio.speech_wait_cue.is_none());
        assert!(studio.speech_bypass_cues.is_empty());
        let replay = studio.replay.as_ref().unwrap();
        assert_eq!(replay.dialogue[0].duration_ms, 1000);
        assert!(replay.dialogue[0].duration_locked);
        assert_eq!(replay.dialogue[1].time_ms, 2500);
        assert_eq!(replay.dialogue[2].time_ms, u64::MAX);
        assert_eq!(
            replay.camera[0].translation[0], 1.0,
            "shrinking reordered camera samples"
        );
        assert_eq!(replay.camera_cuts[0].time_ms, 2000);
        assert_eq!(
            replay.player_movements[0].keyframes[0].time_ms,
            3000
        );
        assert_eq!(
            replay.ship_trajectories[0].keyframes[0].time_ms,
            3500
        );
        assert_eq!(
            replay.standee_positions[0].time_ms,
            4000
        );
        assert_eq!(replay.terrain_changes[0].time_ms, 4200);
        assert_eq!(
            replay.ship_hull_changes[0].time_ms,
            4400
        );
        let after = to_vec(replay).unwrap();
        let revision = studio.scene_revision;
        assert!(history.restore_edit(&mut studio, false));
        assert_eq!(studio.playback_ms, 2800);
        assert!(studio.scene_revision > revision);
        assert_eq!(
            to_vec(studio.replay.as_ref().unwrap()).unwrap(),
            before
        );
        assert!(history.restore_edit(&mut studio, true));
        assert_eq!(studio.playback_ms, 1999);
        assert_eq!(
            to_vec(studio.replay.as_ref().unwrap()).unwrap(),
            after
        );
    }

    #[test]
    fn undo_preserves_scene_and_title_but_never_overwrites_external_dialogue_edits() {
        let mut studio = editing_studio();
        let mut history = TimelineHistory::default();
        assert!(history.trim(&mut studio, 0, 1000));
        let replay = studio.replay.as_mut().unwrap();
        replay.title = "新标题".into();
        replay.scene.voxels[0].material = 2;
        assert!(history.restore_edit(&mut studio, false));
        let replay = studio.replay.as_ref().unwrap();
        assert_eq!(replay.title, "新标题");
        assert_eq!(replay.scene.voxels[0].material, 2);
        assert!(history.restore_edit(&mut studio, true));
        studio.replay.as_mut().unwrap().dialogue[0].text = "另一面板的新台词".into();
        assert!(!history.restore_edit(&mut studio, false));
        assert_eq!(
            studio.replay.as_ref().unwrap().dialogue[0].text,
            "另一面板的新台词"
        );
        assert!(history.undo.is_empty());
        assert!(history.redo.is_empty());
    }

    #[test]
    fn history_discards_redo_branches_and_replay_switches() {
        let mut studio = editing_studio();
        let mut history = TimelineHistory::default();
        assert!(history.trim(&mut studio, 0, 1000));
        assert!(history.restore_edit(&mut studio, false));
        assert!(history.trim(&mut studio, 0, 1500));
        assert!(!history.restore_edit(&mut studio, true));
        assert!(
            !history.trim(&mut studio, 0, 1500),
            "unchanged value is not an undo step"
        );
        assert!(
            !history.trim(&mut studio, 2, 500),
            "unscheduled cues cannot be trimmed"
        );
        assert_eq!(history.undo.len(), 1);
        studio.replay.as_mut().unwrap().created_at_unix_ms = 2;
        assert!(!history.restore_edit(&mut studio, false));
        assert!(history.undo.is_empty());
        assert_eq!(
            studio.replay.as_ref().unwrap().dialogue[0].duration_ms,
            1500
        );
    }

    #[test]
    fn recording_locks_edits_and_history_is_bounded() {
        let mut studio = editing_studio();
        let mut history = TimelineHistory::default();
        studio.mode = ReplayMode::Recording;
        assert!(!history.trim(&mut studio, 0, 1000));
        studio.mode = ReplayMode::Idle;
        for duration_ms in 1000..1040 {
            assert!(history.trim(&mut studio, 0, duration_ms));
        }
        assert_eq!(history.undo.len(), 32);
        studio.mode = ReplayMode::Recording;
        assert!(!history.restore_edit(&mut studio, false));
        assert_eq!(
            studio.replay.as_ref().unwrap().dialogue[0].duration_ms,
            1039
        );
    }

    #[test]
    fn trim_drag_uses_total_distance_frame_snap_and_duration_bounds() {
        assert_eq!(
            trimmed_duration(1000, 3000, 5.6, 1000.0, 10_000, 30, false),
            2056
        );
        assert_eq!(
            trimmed_duration(1000, 3000, 5.6, 1000.0, 10_000, 30, true),
            2066
        );
        assert_eq!(
            trimmed_duration(1000, 3000, -900.0, 1000.0, 10_000, 30, false),
            100
        );
        assert_eq!(
            trimmed_duration(1000, 3000, 90_000.0, 1000.0, 10_000, 30, false),
            120_000
        );
    }

    #[test]
    fn stepping_matches_export_frames_without_drift() {
        assert_eq!(adjacent_frame(34, 30, true), 66);
        assert_eq!(adjacent_frame(34, 30, false), 33);
        for fps in [24, 30, 60] {
            let mut time = 0;
            for frame in 1..=300 {
                time = adjacent_frame(time, fps, true);
                assert_eq!(time, frame_time_ms(frame, fps));
            }
            for frame in (0..300).rev() {
                time = adjacent_frame(time, fps, false);
                assert_eq!(time, frame_time_ms(frame, fps));
            }
            assert_eq!(adjacent_frame(0, fps, false), 0);
        }
    }

    #[test]
    fn ruler_coordinates_clamp_and_scale() {
        assert_eq!(
            time_at_x(150.0, 100.0, 200.0, 10_000),
            2_500
        );
        assert_eq!(
            time_at_x(-10.0, 100.0, 200.0, 10_000),
            0
        );
        assert_eq!(
            time_at_x(900.0, 100.0, 200.0, 10_000),
            10_000
        );
        assert_eq!(
            time_at_x(150.0, 100.0, 400.0, 10_000),
            1_250
        );
    }
}
