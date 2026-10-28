use std::{
    collections::BTreeMap,
    hash::{
        DefaultHasher,
        Hash,
        Hasher,
    },
    sync::Arc,
};

use bevy::log::{info, info_span};
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
    UiBuilder,
    Vec2,
    WidgetInfo,
    WidgetType,
};
use serde_json::{
    from_slice,
    to_vec,
};
use voxxelmaxx::prelude::Grid;

use super::{
    frame_time_ms,
    invalidate_replay_timeline,
    prepare_replay_movement_for_playback,
    replay_dialogue_is_playable,
    replay_has_live_take,
    set_exact_dialogue_duration_at_playhead,
    start_playback,
    DialogueSide,
    ReplayCameraCut,
    ReplayCameraFocus,
    ReplayCameraKeyframe,
    ReplayCameraSubject,
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
    ReplayWorkspaceTab,
    TrpgVoxelGrid,
    MAX_LIVE_DIALOGUE_MS,
    MIN_LIVE_DIALOGUE_MS,
};

/// 片段宽度低于该值（像素）时只绘制不注册交互：全景缩放下每句台词只有
/// 几个像素，逐片段注册 egui 交互、无障碍信息和悬停文本的开销是平方级的。
const MIN_CLIP_INTERACT_WIDTH: f32 = 6.0;
/// 修剪手柄的最小可交互宽度：更窄的片段放大后再修剪。
const MIN_TRIM_HANDLE_WIDTH: f32 = 16.0;

#[derive(Clone)]
struct TimelineView {
    zoom: f32,
    selected: Option<Id>,
    duration_seconds: Option<f64>,
    snap: bool,
    trim: Option<TrimGesture>,
    /// 正在拖动移动的台词卡片：（对话索引，抓取偏移，当前起始时间）。
    /// current_start 每帧随指针更新，绘制与最终提交都以此为准。
    move_drag: Option<(usize, u64, u64)>,
    /// 键盘 Delete 标记的待删台词（行 id），函数末尾统一删除。
    pending_delete_line_id: Option<u64>,
    history: TimelineHistory,
    follow_playhead: bool,
    scroll_x: f32,
    last_playback_ms: Option<u64>,
    /// 已构建的轨道数据缓存；指纹不匹配时才整体重建，避免每帧为全部
    /// 台词和场景事件重新分配字符串、Clip 和 BTreeMap。
    tracks_cache: Option<TimelineTracksCache>,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            selected: None,
            duration_seconds: None,
            snap: true,
            trim: None,
            move_drag: None,
            pending_delete_line_id: None,
            history: TimelineHistory::default(),
            follow_playhead: true,
            scroll_x: 0.0,
            last_playback_ms: None,
            tracks_cache: None,
        }
    }
}

struct Clip {
    id: Id,
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
    // 覆盖与 replay_edit_fingerprint_state 相同的轨道字段，但直接哈希，
    // 不再把全部轨道深拷贝成 JSON 字符串——那是打开面板瞬间与每次修剪
    // 快照的毫秒级尖峰来源。该指纹只在本次会话的时间轴撤销栈里比较，
    // 不会持久化，因此实现与导演请求的持久指纹相互独立。
    let mut hasher = DefaultHasher::new();
    replay.duration_ms.hash(&mut hasher);
    for line in &replay.dialogue {
        line.time_ms.hash(&mut hasher);
        line.duration_ms.hash(&mut hasher);
        line.duration_locked.hash(&mut hasher);
        line.sender_id.hash(&mut hasher);
        line.camera_focus_id.hash(&mut hasher);
        line.name.hash(&mut hasher);
        line.role.hash(&mut hasher);
        line.text.hash(&mut hasher);
        line.speech_text.hash(&mut hasher);
        line.speech_enabled.hash(&mut hasher);
        line.speech_rate.to_bits().hash(&mut hasher);
        line.speech_volume.to_bits().hash(&mut hasher);
        line.avatar.hash(&mut hasher);
        line.avatar_data_url.hash(&mut hasher);
        // Visibility 未实现 Hash；序列化成 JSON 代替，保证内容变化必然
        // 改变指纹。
        if let Ok(visibility) = serde_json::to_string(&line.visibility) {
            visibility.hash(&mut hasher);
        }
        (line.side == DialogueSide::Left).hash(&mut hasher);
        line.line_id.hash(&mut hasher);
        line.source_time.hash(&mut hasher);
        line.turn_index.hash(&mut hasher);
        line.position_cells.hash(&mut hasher);
        line.area.hash(&mut hasher);
        line.included.hash(&mut hasher);
        line.snapshot_recorded.hash(&mut hasher);
        line.metadata_estimated.hash(&mut hasher);
        line.forwarded.hash(&mut hasher);
    }
    for block in &replay.area_blocks {
        block.hash(&mut hasher);
    }
    replay.manual_dialogue_order.hash(&mut hasher);
    for frame in &replay.camera {
        frame.time_ms.hash(&mut hasher);
        hash_f32s(&mut hasher, &frame.translation);
        hash_f32s(&mut hasher, &frame.rotation);
        match &frame.focus {
            Some(focus) => {
                1_u8.hash(&mut hasher);
                hash_replay_camera_focus(&mut hasher, focus);
            },
            None => 0_u8.hash(&mut hasher),
        }
    }
    for cut in &replay.camera_cuts {
        cut.time_ms.hash(&mut hasher);
        match &cut.shot {
            Some(shot) => {
                1_u8.hash(&mut hasher);
                hash_replay_camera_focus(&mut hasher, &shot.focus);
                hash_f32s(&mut hasher, &shot.offset);
            },
            None => 0_u8.hash(&mut hasher),
        }
    }
    replay.camera_distance_scale.to_bits().hash(&mut hasher);
    replay.camera_yaw_degrees.to_bits().hash(&mut hasher);
    replay.camera_transition_curve.to_bits().hash(&mut hasher);
    for movement in &replay.player_movements {
        movement.user_id.hash(&mut hasher);
        for keyframe in &movement.keyframes {
            keyframe.time_ms.hash(&mut hasher);
            hash_f32s(&mut hasher, &keyframe.position_cells);
        }
    }
    replay.authored_player_movements.hash(&mut hasher);
    replay.authored_ship_trajectories.hash(&mut hasher);
    for trajectory in &replay.ship_trajectories {
        trajectory.ship_id.hash(&mut hasher);
        trajectory.ship_name.hash(&mut hasher);
        for keyframe in &trajectory.keyframes {
            keyframe.time_ms.hash(&mut hasher);
            hash_f32s(&mut hasher, &keyframe.translation);
            hash_f32s(&mut hasher, &keyframe.rotation);
        }
    }
    for sample in &replay.standee_positions {
        sample.time_ms.hash(&mut hasher);
        sample.user_id.hash(&mut hasher);
        hash_f32s(&mut hasher, &sample.position);
    }
    for change in &replay.terrain_changes {
        change.time_ms.hash(&mut hasher);
        change.position.hash(&mut hasher);
        change.material.hash(&mut hasher);
        change.enabled.hash(&mut hasher);
    }
    for change in &replay.ship_hull_changes {
        change.time_ms.hash(&mut hasher);
        change.ship_id.hash(&mut hasher);
        change.position.hash(&mut hasher);
        change.material.hash(&mut hasher);
        change.enabled.hash(&mut hasher);
    }
    replay.master_dialogue_duration.to_bits().hash(&mut hasher);
    replay.master_speech_speed.to_bits().hash(&mut hasher);
    replay.player_movement_curve.to_bits().hash(&mut hasher);
    replay.ship_motion_speed.to_bits().hash(&mut hasher);
    replay.dialogue_waits_for_ship_motion.hash(&mut hasher);
    Some(hasher.finish())
}

fn hash_f32s(hasher: &mut DefaultHasher, values: &[f32]) {
    for value in values {
        value.to_bits().hash(hasher);
    }
}

fn hash_replay_camera_focus(hasher: &mut DefaultHasher, focus: &ReplayCameraFocus) {
    match &focus.subject {
        ReplayCameraSubject::Player(user_id) => {
            0_u8.hash(hasher);
            user_id.hash(hasher);
        },
        ReplayCameraSubject::Ship {
            ship_id,
            local_center,
        } => {
            1_u8.hash(hasher);
            ship_id.hash(hasher);
            hash_f32s(hasher, local_center);
        },
    }
    hash_f32s(hasher, &focus.position);
    hash_f32s(hasher, &focus.axis_origin);
    hash_f32s(hasher, &focus.camera_side);
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
        let project_changed = self.project != project;
        let changed = project_changed || self.revision != studio.timeline_revision;
        if changed {
            // Entering preview invalidates playback caches even when the authored
            // tracks are unchanged. That transition must not discard edit history.
            let expected = self
                .undo
                .last()
                .map(|edit| edit.after.fingerprint)
                .or_else(|| self.redo.last().map(|edit| edit.before.fingerprint));
            let authored_changed = expected.is_some_and(|fingerprint| {
                studio.replay.as_ref().and_then(timeline_fingerprint) != Some(fingerprint)
            });
            if project_changed || authored_changed {
                self.undo.clear();
                self.redo.clear();
            }
            self.project = project;
            self.revision = studio.timeline_revision;
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

#[derive(Clone)]
struct TimelineTracksCache {
    fingerprint: u64,
    tracks: Arc<Vec<Track>>,
}

/// 每帧用廉价哈希检测时间轴轨道显示是否变化；只哈希轨道绘制用到的字段。
/// 台词文本与名字的编辑路径都会递增 timeline_revision（台词编辑窗口、
/// 修剪、撤销重做等），因此这里无需逐句哈希文本——逐句哈希上万句台词
/// 的全文曾让打开大项目的时间轴每帧多花约 40ms。
fn timeline_tracks_fingerprint(studio: &ReplayStudio) -> u64 {
    let mut hasher = DefaultHasher::new();
    studio.timeline_revision.hash(&mut hasher);
    // 分段视图窗口参与指纹：切换分段时轨道按段内数据重建。
    let window = studio
        .replay
        .as_ref()
        .and_then(|replay| super::active_speech_window(studio, replay));
    window.hash(&mut hasher);
    if let Some(replay) = studio.replay.as_ref() {
        replay.campaign_id.hash(&mut hasher);
        replay.created_at_unix_ms.hash(&mut hasher);
        for line in &replay.dialogue {
            replay_dialogue_is_playable(line).hash(&mut hasher);
            line.line_id.hash(&mut hasher);
            line.time_ms.hash(&mut hasher);
            line.duration_ms.hash(&mut hasher);
        }
        for frame in &replay.camera {
            frame.time_ms.hash(&mut hasher);
        }
        for cut in &replay.camera_cuts {
            cut.time_ms.hash(&mut hasher);
        }
        for movement in &replay.player_movements {
            movement.user_id.hash(&mut hasher);
            movement.keyframes.len().hash(&mut hasher);
            if let Some(first) = movement.keyframes.first() {
                first.time_ms.hash(&mut hasher);
            }
            if let Some(last) = movement.keyframes.last() {
                last.time_ms.hash(&mut hasher);
            }
        }
        for ship in &replay.ship_trajectories {
            ship.ship_id.hash(&mut hasher);
            ship.ship_name.hash(&mut hasher);
            ship.keyframes.len().hash(&mut hasher);
            if let Some(first) = ship.keyframes.first() {
                first.time_ms.hash(&mut hasher);
            }
            if let Some(last) = ship.keyframes.last() {
                last.time_ms.hash(&mut hasher);
            }
        }
        for change in &replay.terrain_changes {
            change.time_ms.hash(&mut hasher);
            change.enabled.hash(&mut hasher);
        }
        for change in &replay.ship_hull_changes {
            change.time_ms.hash(&mut hasher);
            change.enabled.hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn build_timeline_tracks(replay: &ReplayFile, window: Option<(u64, u64)>) -> Vec<Track> {
    // 分段视图：段外数据不进入轨道（数据级隔离，构建与绘制的开销都
    // 只与段内内容成正比）。
    let in_window = |time_ms: u64| {
        window.map_or(true, |(start_ms, end_ms)| {
            time_ms >= start_ms && time_ms < end_ms
        })
    };
    let spans_window = |first_ms: u64, last_ms: u64| {
        window.map_or(true, |(start_ms, end_ms)| {
            first_ms < end_ms && last_ms >= start_ms
        })
    };
    let mut tracks = vec![
        Track {
            name: "台词 / 字幕".into(),
            color: Color32::from_rgb(65, 125, 175),
            clips: replay
                .dialogue
                .iter()
                .enumerate()
                .filter(|(_, line)| {
                    replay_dialogue_is_playable(line)
                        && spans_window(
                            line.time_ms,
                            line.time_ms.saturating_add(line.duration_ms),
                        )
                })
                .map(|(index, line)| Clip {
                    id: Id::new(("dialogue", line.line_id)),
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
                .filter(|frame| in_window(frame.time_ms))
                .map(|frame| Clip {
                    id: Id::new(("camera", frame.time_ms)),
                    start: frame.time_ms,
                    end: frame.time_ms,
                    label: "镜头关键帧".into(),
                    dialogue_index: None,
                })
                .chain(
                    replay
                        .camera_cuts
                        .iter()
                        .filter(|cut| in_window(cut.time_ms))
                        .map(|cut| Clip {
                            id: Id::new(("cut", cut.time_ms)),
                            start: cut.time_ms,
                            end: cut.time_ms,
                            label: "手动切镜".into(),
                            dialogue_index: None,
                        }),
                )
                .collect(),
        },
    ];
    for movement in replay.player_movements.iter().filter(|movement| {
        movement
            .keyframes
            .first()
            .zip(movement.keyframes.last())
            .is_some_and(|(first, last)| spans_window(first.time_ms, last.time_ms))
    }) {
        tracks.push(Track {
            name: format!("玩家 {}", movement.user_id),
            color: Color32::from_rgb(55, 145, 110),
            clips: movement
                .keyframes
                .first()
                .zip(movement.keyframes.last())
                .map(|(first, last)| Clip {
                    id: Id::new(("player", movement.user_id)),
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
    for ship in replay.ship_trajectories.iter().filter(|ship| {
        ship.keyframes
            .first()
            .zip(ship.keyframes.last())
            .is_some_and(|(first, last)| spans_window(first.time_ms, last.time_ms))
    }) {
        tracks.push(Track {
            name: format!("飞船 {}", ship.ship_name),
            color: Color32::from_rgb(160, 115, 55),
            clips: ship
                .keyframes
                .first()
                .zip(ship.keyframes.last())
                .map(|(first, last)| Clip {
                    id: Id::new(("ship", &ship.ship_id)),
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
        .filter(|change| change.enabled && in_window(change.time_ms))
        .map(|change| change.time_ms)
        .chain(
            replay
                .ship_hull_changes
                .iter()
                .filter(|change| change.enabled && in_window(change.time_ms))
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
                id: Id::new(("event", time)),
                start: time,
                end: time,
                label: format!("场景修改 · {count} 个体素"),
                dialogue_index: None,
            })
            .collect(),
    });
    tracks
}

fn time_at_x(x: f32, left: f32, width: f32, duration: u64, origin: u64) -> u64 {
    origin
        + (((x - left) / width.max(1.0)).clamp(0.0, 1.0) as f64 * duration as f64)
            .round() as u64
}

fn reveal_playhead_offset(target: f32, offset: f32, visible: f32, content: f32) -> f32 {
    let margin = visible * 0.1;
    let next = if target < offset + margin {
        target - margin
    } else if target > offset + visible - margin {
        target - visible + margin
    } else {
        offset
    };
    next.clamp(0.0, (content - visible).max(0.0))
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

fn timeline_timecode(time_ms: u64, fps: u32) -> String {
    let fps = u64::from(fps.max(1));
    // Match export's millisecond truncation without displaying the previous frame.
    let frame = time_ms
        .saturating_add(1)
        .saturating_mul(fps)
        .div_ceil(1000)
        .saturating_sub(1);
    let seconds = frame / fps;
    format!(
        "{:02}:{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60,
        frame % fps,
    )
}

pub(super) fn timeline_time_label(time_ms: u64) -> String {
    format!(
        "{:02}:{:02}.{:03}",
        time_ms / 60_000,
        (time_ms / 1000) % 60,
        time_ms % 1000,
    )
}

fn ruler_tick_step(extent: u64, width: f32) -> u64 {
    let minimum = (extent as f64 * 105.0 / width.max(1.0) as f64).ceil() as u64;
    let mut magnitude = 1_u64;
    loop {
        for multiple in [1, 2, 5] {
            let step = magnitude.saturating_mul(multiple);
            if step >= minimum {
                return step;
            }
        }
        if magnitude > u64::MAX / 10 {
            return u64::MAX;
        }
        magnitude *= 10;
    }
}

pub(super) fn replay_timeline_ui(
    ui: &mut Ui,
    studio: &mut ReplayStudio,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    player_history: &ReplayPlayerMovementHistory,
    ship_history: &ReplayShipTrajectoryHistory,
) {
    let _profile = info_span!("replay_timeline_ui").entered();
    let ctx = ui.ctx().clone();
    let id = Id::new("replay-multitrack-view");
    let mut view = ctx.data_mut(|data| data.get_temp::<TimelineView>(id).unwrap_or_default());
    let mut playhead_changed = view.last_playback_ms != Some(studio.playback_ms);
    // Remember the incoming time so a seek made while drawing this frame is
    // revealed next frame, including a partially visible clip's offscreen start.
    view.last_playback_ms = Some(studio.playback_ms);
    let project = studio.replay.as_ref().map(|replay| {
        (
            replay.campaign_id.clone(),
            replay.created_at_unix_ms,
        )
    });
    if view.history.project != project {
        playhead_changed = true;
        view.selected = None;
        view.move_drag = None;
        studio.selected_dialogue_id = None;
    }
    if view.history.synchronize(studio) {
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
    // 分段视图：时间轴域收缩为分段窗口（标尺从段起点开始，段外片段
    // 自然被裁剪），seek 与播放头都被钳制在窗口内。
    let view_segment = studio
        .active_custom_tab
        .and_then(|tab_id| replay.custom_tabs.iter().find(|tab| tab.id == tab_id));
    let view_origin = view_segment.map_or(0, |segment| segment.start_ms);
    let view_end = match view_segment {
        Some(segment) => segment.end_ms.min(duration),
        None => duration,
    };
    let extent = match view_segment {
        Some(segment) => {
            let span = segment.end_ms.saturating_sub(segment.start_ms).max(1);
            span.saturating_add((span / 10).max(500))
        },
        None => duration.saturating_add((duration / 10).max(1000)),
    };
    let title = replay.title.clone();
    let tracks_fingerprint = timeline_tracks_fingerprint(studio);
    if view
        .tracks_cache
        .as_ref()
        .is_none_or(|cache| cache.fingerprint != tracks_fingerprint)
    {
        view.tracks_cache = Some(TimelineTracksCache {
            fingerprint: tracks_fingerprint,
            tracks: Arc::new(build_timeline_tracks(
                replay,
                super::active_speech_window(studio, replay),
            )),
        });
    }
    let tracks = view
        .tracks_cache
        .as_ref()
        .expect("tracks cache was just refreshed")
        .tracks
        .clone();
    // Delete/Backspace 快速删除选中的台词片段：全局按键（不要求指针
    // 悬停在时间轴上），但键盘焦点在文本框时让位给 egui。
    if timeline_editing_enabled(studio)
        && view.selected.is_some()
        && view.trim.is_none()
        && !ctx.egui_wants_keyboard_input()
    {
        let delete_requested = ui.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Delete)
                || input.consume_key(Modifiers::NONE, Key::Backspace)
        });
        if delete_requested {
            let selected_line_id = tracks
                .iter()
                .flat_map(|track| &track.clips)
                .find(|clip| Some(clip.id) == view.selected)
                .and_then(|clip| clip.dialogue_index)
                .and_then(|index| replay.dialogue.get(index))
                .map(|line| line.line_id);
            if let Some(line_id) = selected_line_id {
                info!(?line_id, "timeline delete requested");
                view.pending_delete_line_id = Some(line_id);
                view.selected = None;
                view.duration_seconds = None;
            }
        }
    }
    let mut seek = None;
    let mut custom_tab_to_add: Option<super::ReplayCustomTab> = None;
    // 拖动结束后统一提交的台词移动（对话索引，新起始时间）。
    let mut deferred_moves: Vec<(usize, u64)> = Vec::new();
    let mut toggle_playback = false;
    let mut zoom_changed = false;
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
        if view.trim.is_none() {
            ui.input_mut(|input| {
                toggle_playback = input.consume_key(Modifiers::NONE, Key::Space);
                if input.consume_key(Modifiers::NONE, Key::Home) {
                    seek = Some(view_origin);
                }
                if input.consume_key(Modifiers::NONE, Key::End) {
                    seek = Some(view_end);
                }
                let previous = input.consume_key(Modifiers::NONE, Key::ArrowLeft);
                let next = input.consume_key(Modifiers::NONE, Key::ArrowRight);
                if previous || next {
                    let mut time = studio.playback_ms;
                    for _ in 0..if input.modifiers.shift { 10 } else { 1 } {
                        time = adjacent_frame(time, studio.video_fps, next);
                    }
                    seek = Some(time.min(view_end));
                }
            });
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.strong(&title);
        ui.monospace(format!(
            "{} / {}",
            timeline_timecode(studio.playback_ms, studio.video_fps),
            timeline_timecode(duration, studio.video_fps)
        ));
        ui.add_enabled_ui(enabled, |ui| {
            if ui.button("⏮ 片头").clicked() {
                seek = Some(view_origin);
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
                toggle_playback = true;
            }
            if ui.button("后一帧 ▶").clicked() {
                seek = Some(
                    adjacent_frame(
                        studio.playback_ms,
                        studio.video_fps,
                        true,
                    )
                    .min(view_end),
                );
            }
        });
        zoom_changed = ui
            .add(
                Slider::new(&mut view.zoom, 1.0..=20.0)
                    .text("缩放")
                    .logarithmic(true),
            )
            .changed();
        if ui.button("适合全部").clicked() {
            view.zoom = 1.0;
            zoom_changed = true;
        }
        ui.checkbox(&mut studio.speech_enabled, "生成语音")
            .on_hover_text(
                "开启后回放会为台词生成并等待语音；编辑时间轴或不想等待时关闭，回放立即推进、不生成语音。",
            );
        ui.separator();
        // 快速分割：输入起止时间，在顶边栏创建一个只含该时段台词的
        // 分段视图 tab（随回放项目持久化），用于专注编辑某些时间段。
        let split_start_id = Id::new("replay-quick-split-start");
        let split_end_id = Id::new("replay-quick-split-end");
        let (mut start_draft, mut end_draft) = ui.ctx().data_mut(|data| {
            (
                data.get_temp::<String>(split_start_id)
                    .unwrap_or_default(),
                data.get_temp::<String>(split_end_id)
                    .unwrap_or_default(),
            )
        });
        let start_edit = ui.add(
            Label::new(
                egui::RichText::new("起").small(),
            ),
        );
        let _ = start_edit;
        ui.add(
            egui::TextEdit::singleline(&mut start_draft)
                .desired_width(72.0)
                .hint_text("分:秒"),
        );
        ui.label("~");
        ui.add(
            egui::TextEdit::singleline(&mut end_draft)
                .desired_width(72.0)
                .hint_text("分:秒"),
        );
        if start_edit.changed()
            || start_draft
                != ui
                    .ctx()
                    .data_mut(|data| {
                        data.get_temp::<String>(split_start_id)
                            .unwrap_or_default()
                    })
        {
            let draft = start_draft.clone();
            ui.ctx().data_mut(|data| data.insert_temp(split_start_id, draft));
        }
        if end_draft
            != ui
                .ctx()
                .data_mut(|data| {
                    data.get_temp::<String>(split_end_id)
                        .unwrap_or_default()
                })
        {
            let draft = end_draft.clone();
            ui.ctx().data_mut(|data| data.insert_temp(split_end_id, draft));
        }
        if ui.button("✂ 快速分割").clicked() {
            match (
                super::parse_user_time_ms(&start_draft),
                super::parse_user_time_ms(&end_draft),
            ) {
                (Some(start_ms), Some(end_ms)) if start_ms < end_ms => {
                    let start_ms = start_ms.min(duration);
                    let end_ms = end_ms.min(duration);
                    let speaking_lines = replay
                        .dialogue
                        .iter()
                        .filter(|line| {
                            replay_dialogue_is_playable(line)
                                && line.time_ms >= start_ms
                                && line.time_ms < end_ms
                        })
                        .count();
                    // 借用中的 replay 不能在此处可变访问：先收集，
                    // 函数末尾统一提交到回放项目。
                    custom_tab_to_add = Some(super::ReplayCustomTab {
                        id: replay
                            .custom_tabs
                            .iter()
                            .map(|tab| tab.id)
                            .max()
                            .unwrap_or_default()
                            .saturating_add(1),
                        label: format!(
                            "{}~{}",
                            timeline_time_label(start_ms),
                            timeline_time_label(end_ms)
                        ),
                        start_ms,
                        end_ms,
                    });
                    studio.status = format!(
                        "已创建分段视图（{speaking_lines} 句台词），顶边栏可随时切换"
                    );
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(split_start_id, String::new());
                        data.insert_temp(split_end_id, String::new());
                    });
                },
                (Some(start_ms), Some(end_ms)) => {
                    studio.status = format!(
                        "分割起点必须早于终点（{start_ms}ms ≥ {end_ms}ms）"
                    );
                },
                _ => {
                    studio.status = "时间格式：分:秒 或 时:分:秒，可带 .毫秒（例如 3:20 或 1:03:20.5）".to_owned();
                },
            }
        }
        ui.checkbox(&mut view.snap, "帧吸附");
        ui.checkbox(&mut view.follow_playhead, "跟随播放头");
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
    if toggle_playback {
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
    let selected_clip = view.selected.and_then(|selected| {
        tracks
            .iter()
            .flat_map(|track| &track.clips)
            .find(|clip| clip.id == selected)
    });
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
            timeline_time_label(selected_clip.map_or(0, |clip| clip.start)),
            timeline_time_label(selected_clip.map_or(0, |clip| clip.end))
        ));
        let dialogue_index = selected_clip.and_then(|clip| clip.dialogue_index);
        if ui
            .add_enabled(
                enabled && dialogue_index.is_some(),
                Button::new("编辑台词"),
            )
            .clicked()
        {
            studio.workspace_tab = ReplayWorkspaceTab::Edit;
        }
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
        "拖动标尺或空白轨道定位；拖动台词右边缘修剪，后续轨道同步移动；Ctrl+滚轮以指针为中心缩放。Shift 临时关闭帧吸附；Esc 取消拖动。",
    );
    let viewport_width = ui.available_width();
    let label_width = 145.0;
    let visible_width = (viewport_width - label_width).max(1.0);
    let width = visible_width * view.zoom;
    // Ctrl+滚轮在指针位置缩放时间轴，缩放前后指针下的时间点保持不动
    // （AE / Premiere 的标准行为）；普通滚轮仍用于滚动。
    let mut ctrl_zoom_offset = None;
    let timeline_rect = ui.max_rect();
    if ui.rect_contains_pointer(timeline_rect)
        && ui.input(|input| input.modifiers.ctrl && input.smooth_scroll_delta.y != 0.0)
    {
        let (delta_y, pointer) = ui.input(|input| {
            (input.smooth_scroll_delta.y, input.pointer.hover_pos())
        });
        if let Some(pointer) = pointer {
            let pointer_local = pointer.x - timeline_rect.left();
            let time = (((view.scroll_x + pointer_local - label_width) / width).max(0.0)
                * extent as f32) as f64;
            let new_zoom = (view.zoom * (delta_y * 0.004_f32).exp()).clamp(1.0, 20.0);
            if (new_zoom - view.zoom).abs() > f32::EPSILON {
                let new_width = visible_width * new_zoom;
                view.zoom = new_zoom;
                zoom_changed = true;
                ctrl_zoom_offset = Some(
                    (time / extent as f64 * new_width as f64
                        - (pointer_local - label_width) as f64)
                        .max(0.0) as f32,
                );
            }
            ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
        }
    }
    let mut scroll_area = ScrollArea::both()
        .id_salt("replay-tracks-scroll")
        .auto_shrink([false, false])
        .max_height(ui.available_height().max(60.0));
    if view.follow_playhead
        && (studio.mode == ReplayMode::Playing
            || playhead_changed
            || seek.is_some()
            || zoom_changed)
    {
        let target = seek.unwrap_or(studio.playback_ms) as f64 / extent as f64 * width as f64;
        scroll_area = scroll_area.horizontal_scroll_offset(reveal_playhead_offset(
            target as f32,
            view.scroll_x,
            visible_width,
            width,
        ));
    }
    if let Some(offset) = ctrl_zoom_offset {
        // Ctrl 缩放以指针为锚点，优先于跟随播放头的定位。
        scroll_area = scroll_area.horizontal_scroll_offset(offset);
    }
    let scroll = scroll_area.show_viewport(ui, |ui, viewport| {
        let (rect, _) = ui.allocate_exact_size(
            vec2(
                label_width + width,
                30.0 + tracks.len() as f32 * 32.0,
            ),
            Sense::hover(),
        );
        let visible = Rect::from_min_size(
            rect.min + viewport.min.to_vec2(),
            viewport.size(),
        )
        .intersect(ui.clip_rect());
        let plot_clip = Rect::from_min_max(
            pos2(
                visible.left() + label_width,
                visible.top() + 30.0,
            ),
            visible.max,
        );
        let mut plot_ui = ui.new_child(UiBuilder::new().max_rect(rect).id_salt("track-plot"));
        plot_ui.set_clip_rect(plot_clip);
        let left = rect.left() + label_width;
        let x_at = |time: u64| {
            left + (time.saturating_sub(view_origin) as f64 / extent as f64) as f32
                * width
        };
        let full_painter = ui.painter_at(visible);
        let painter = ui.painter_at(plot_clip);
        full_painter.rect_filled(
            visible,
            0.0,
            Color32::from_rgb(24, 27, 33),
        );
        let ruler = Rect::from_min_max(
            pos2(plot_clip.left(), visible.top()),
            pos2(visible.right(), visible.top() + 30.0),
        );
        let response = ui.interact(
            ruler,
            id.with("ruler"),
            Sense::click_and_drag(),
        );
        if enabled && (response.clicked() || response.dragged()) {
            if let Some(pointer) = response.interact_pointer_pos() {
                seek = Some(
                    time_at_x(pointer.x, left, width, extent, view_origin)
                        .min(view_end),
                );
            }
        }
        if response.hovered() {
            // 悬停标尺时显示指针处的时间码和半透明指示线，方便对位。
            if let Some(pointer) = response.hover_pos() {
                let hover_time =
                    time_at_x(pointer.x, left, width, extent, view_origin);
                painter.line_segment(
                    [pos2(pointer.x, visible.top() + 30.0), pos2(pointer.x, visible.bottom())],
                    Stroke::new(1.0, Color32::from_white_alpha(28)),
                );
                full_painter.with_clip_rect(ruler).text(
                    pos2(pointer.x + 4.0, visible.top() + 3.0),
                    Align2::LEFT_TOP,
                    timeline_time_label(hover_time),
                    FontId::monospace(11.0),
                    Color32::WHITE,
                );
            }
        }
        let step = ruler_tick_step(extent, width);
        let ticks = extent / step;
        for tick in 0..=ticks {
            let time = tick * step;
            let x = x_at(time);
            painter.line_segment(
                [pos2(x, rect.top() + 22.0), pos2(x, rect.bottom())],
                Stroke::new(1.0, Color32::from_gray(48)),
            );
            full_painter.with_clip_rect(ruler).text(
                pos2(x + 3.0, visible.top() + 3.0),
                Align2::LEFT_TOP,
                timeline_time_label(time),
                FontId::monospace(11.0),
                Color32::LIGHT_GRAY,
            );
        }
        // 空白轨道区域直接点击/拖动定位播放头（AE 习惯）；片段和修剪手柄
        // 注册得更晚，在自己的矩形内优先响应，不会冲突。
        let background = Rect::from_min_max(
            pos2(plot_clip.left(), visible.top() + 30.0),
            pos2(visible.right(), visible.bottom()),
        );
        let scrub = ui.interact(
            background,
            id.with("timeline-scrub"),
            Sense::click_and_drag(),
        );
        if scrub.clicked() {
            view.selected = None;
            studio.selected_dialogue_id = None;
            view.duration_seconds = None;
        }
        if enabled && (scrub.clicked() || scrub.dragged()) {
            if let Some(pointer) = scrub.interact_pointer_pos() {
                seek = Some(
                    time_at_x(pointer.x, left, width, extent, view_origin)
                        .min(view_end),
                );
            }
        }
        for (row, track) in tracks.iter().enumerate() {
            let top = rect.top() + 30.0 + row as f32 * 32.0;
            // 当前行内相接的窄片段色带，遇到可交互片段或行结束时落笔。
            let mut band: Option<(Rect, Color32)> = None;
            full_painter
                .with_clip_rect(Rect::from_min_max(
                    pos2(visible.left(), visible.top() + 30.0),
                    pos2(plot_clip.left() - 4.0, visible.bottom()),
                ))
                .text(
                    pos2(visible.left() + 6.0, top + 15.0),
                    Align2::LEFT_CENTER,
                    &track.name,
                    FontId::proportional(12.0),
                    Color32::LIGHT_GRAY,
                );
            for clip in &track.clips {
                let preview = view
                    .trim
                    .filter(|trim| Some(trim.dialogue_index) == clip.dialogue_index);
                // 被拖动的卡片以 move_drag 的当前位置实时绘制（跟随指针）；
                // 其余卡片用轨道缓存的位置（编辑后随修订号重建而更新）。
                let dragged_times = view
                    .move_drag
                    .filter(|(index, _, _)| Some(*index) == clip.dialogue_index)
                    .map(|(_, _, current_start)| {
                        let duration = clip.end.saturating_sub(clip.start);
                        (current_start, current_start.saturating_add(duration))
                    });
                let (draw_start, draw_base_end) =
                    dragged_times.unwrap_or((clip.start, clip.end));
                let clip_end = preview.map_or(draw_base_end, |trim| {
                    trim.start.saturating_add(trim.duration_ms)
                });
                let clip_left = x_at(draw_start.min(view_end)).min(rect.right() - 5.0);
                let clip_rect = Rect::from_min_max(
                    pos2(clip_left, top + 3.0),
                    pos2(
                        (x_at(clip_end.min(extent)).max(clip_left + 5.0)).min(rect.right()),
                        top + 28.0,
                    ),
                );
                if !plot_clip.intersects(clip_rect) && preview.is_none() {
                    continue;
                }
                // 缩放到全景时几千句台词每句只有几个像素宽；每帧为每个
                // 片段注册交互、无障碍信息和悬停文本曾让时间轴每帧多花
                // 几十毫秒。过窄的片段只绘制色块，点击落到背景上仍然
                // 定位播放头；放大后即可正常选中与修剪。
                let selected = view.selected == Some(clip.id);
                let interactive =
                    clip_rect.width() >= MIN_CLIP_INTERACT_WIDTH || selected;
                if !interactive {
                    // 相邻的窄片段合并成一条色带：逐片段填充会让
                    // tessellation 每帧生成上万个矩形，视觉完全相同。
                    match &mut band {
                        Some((rect, _)) if rect.right() + 1.5 >= clip_rect.left() => {
                            rect.max.x = rect.max.x.max(clip_rect.max.x);
                        },
                        _ => {
                            if let Some((rect, color)) = band.take() {
                                painter.rect_filled(rect, 2.0, color);
                            }
                            band = Some((clip_rect, track.color));
                        },
                    }
                    continue;
                }
                if let Some((rect, color)) = band.take() {
                    painter.rect_filled(rect, 2.0, color);
                }
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
                let response = if interactive {
                    let response = plot_ui.interact(
                        body_rect,
                        id.with(clip.id),
                        Sense::click_and_drag(),
                    );
                    response
                        .widget_info(|| {
                            WidgetInfo::labeled(WidgetType::Button, enabled, &clip.label)
                        });
                    Some(response.on_hover_text(format!(
                        "{}\n{} — {}",
                        clip.label,
                        timeline_time_label(clip.start),
                        timeline_time_label(clip.end)
                    )))
                } else {
                    None
                };
                if let Some(response) = &response {
                    if response.clicked() && enabled {
                        view.selected = Some(clip.id);
                        studio.selected_dialogue_id = clip.dialogue_index.map(|index| {
                            studio.replay.as_ref().unwrap().dialogue[index].line_id
                        });
                        view.duration_seconds = None;
                        seek = Some(clip.start);
                    }
                    // 拖动卡片主体 = 把该台词移动到指针处的时间位置
                    // （重新排序其播放位置）；修剪仍由右侧手柄处理。
                    // 拖动期间只更新 move_drag 的当前位置（回放项目在本
                    // 函数里是不可变借用），松手后统一提交到 replay。
                    if enabled && clip.dialogue_index.is_some() && view.trim.is_none() {
                        let dragging_this = view
                            .move_drag
                            .is_some_and(|(index, _, _)| Some(index) == clip.dialogue_index);
                        if response.drag_started() {
                            if let Some(pointer) = response.interact_pointer_pos() {
                                let grab_offset = time_at_x(
                                    pointer.x,
                                    left,
                                    width,
                                    extent,
                                    view_origin,
                                )
                                .saturating_sub(draw_start);
                                view.move_drag = Some((
                                    clip.dialogue_index.unwrap_or(usize::MAX),
                                    grab_offset,
                                    clip.start,
                                ));
                            }
                        } else if dragging_this && response.dragged() {
                            if let Some(pointer) = response.interact_pointer_pos() {
                                let pointer_time = time_at_x(
                                    pointer.x,
                                    left,
                                    width,
                                    extent,
                                    view_origin,
                                );
                                let grab_offset = view
                                    .move_drag
                                    .map(|(_, offset, _)| offset)
                                    .unwrap_or_default();
                                let new_start =
                                    pointer_time.saturating_sub(grab_offset);
                                if let Some((_, _, current_start)) = view.move_drag.as_mut() {
                                    if *current_start != new_start {
                                        *current_start = new_start;
                                    }
                                }
                            }
                        }
                        if dragging_this && response.drag_stopped() {
                            if let Some((index, _, current_start)) = view.move_drag {
                                if Some(index) == clip.dialogue_index
                                    && current_start != clip.start
                                {
                                    deferred_moves.push((index, current_start));
                                }
                            }
                            view.move_drag = None;
                        }
                    }
                }
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
                    if !interactive || clip_rect.width() < MIN_TRIM_HANDLE_WIDTH {
                        continue;
                    }
                    let response = plot_ui
                        .interact(
                            handle,
                            id.with(("trim", clip.id)),
                            Sense::click_and_drag(),
                        )
                        .on_hover_cursor(CursorIcon::ResizeHorizontal)
                        .on_hover_text("拖动句尾修剪时长；后续台词、镜头、移动和事件同步移动");
                    if enabled && response.clicked() {
                        view.selected = Some(clip.id);
                        studio.selected_dialogue_id = clip
                            .dialogue_index
                            .map(|index| studio.replay.as_ref().unwrap().dialogue[index].line_id);
                        view.duration_seconds = None;
                        seek = Some(clip.start);
                    }
                    if enabled && response.drag_started() {
                        view.selected = Some(clip.id);
                        studio.selected_dialogue_id = clip
                            .dialogue_index
                            .map(|index| studio.replay.as_ref().unwrap().dialogue[index].line_id);
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
            if let Some((rect, color)) = band.take() {
                painter.rect_filled(rect, 2.0, color);
            }
        }
        let x = x_at(studio.playback_ms.min(duration));
        full_painter
            .with_clip_rect(Rect::from_min_max(
                ruler.min,
                visible.max,
            ))
            .line_segment(
                [pos2(x, visible.top()), pos2(x, visible.bottom())],
                Stroke::new(2.0, Color32::from_rgb(245, 100, 90)),
            );
    });
    view.scroll_x = scroll.state.offset.x;
    if undo || redo {
        view.history.restore_edit(studio, redo);
        view.duration_seconds = None;
        view.trim = None;
    } else if let Some((index, duration_ms)) = trim_request {
        view.history.trim(studio, index, duration_ms);
        view.duration_seconds = None;
    }
    if let Some(time) = seek {
        // A late clip click must reveal its start even if it seeks to the
        // already-paused time after the user manually scrolled elsewhere.
        view.last_playback_ms = None;
        if studio.mode == ReplayMode::Idle {
            prepare_replay_movement_for_playback(studio, player_history, ship_history);
            start_playback(studio, grids);
        }
        studio.playback_ms = time.min(duration);
        studio.mode = ReplayMode::Paused;
        studio.speech_wait_cue = None;
        studio.speech_wait_elapsed_seconds = 0.0;
    }
    if let Some(tab) = custom_tab_to_add {
        let tab_id = tab.id;
        if let Some(replay) = studio.replay.as_mut() {
            replay.custom_tabs.push(tab);
        }
        studio.workspace_tab = ReplayWorkspaceTab::Edit;
        studio.active_custom_tab = Some(tab_id);
        studio.pending_replay_autosave = true;
    }
    // Delete 快速删除的统一提交：删除台词、递增修订号并触发自动保存。
    if let Some(line_id) = view.pending_delete_line_id.take() {
        if let Some(replay) = studio.replay.as_mut() {
            if super::delete_replay_dialogue(replay, line_id) {
                studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
                studio.pending_replay_autosave = true;
            }
        }
    }
    // 拖动移动的台词统一提交到回放项目（一次修订 + 触发自动保存）。
    if !deferred_moves.is_empty() {
        if let Some(replay) = studio.replay.as_mut() {
            for (dialogue_index, new_start) in &deferred_moves {
                if let Some(line) = replay.dialogue.get_mut(*dialogue_index) {
                    let duration = line.duration_ms.max(1);
                    line.time_ms = *new_start;
                    // 推挤：与其他台词重叠时，把它们按时间顺序往后顺延
                    // （保持各自的时长），不允许两个片段重叠。
                    let mut cursor_end = new_start.saturating_add(duration);
                    let mut others: Vec<(usize, u64, u64)> = replay
                        .dialogue
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != *dialogue_index)
                        .map(|(i, line)| {
                            (i, line.time_ms, line.duration_ms.max(1))
                        })
                        .collect();
                    others.sort_by_key(|(_, time, _)| *time);
                    for (other_index, time, other_duration) in others {
                        // 完全在移动后片段之前（或之后足够远）的台词不动；
                        // 只推挤真正与新片段重叠的台词。
                        let other_end = time.saturating_add(other_duration);
                        if other_end <= *new_start {
                            continue;
                        }
                        if time >= cursor_end {
                            cursor_end = other_end;
                            continue;
                        }
                        if let Some(other) = replay.dialogue.get_mut(other_index) {
                            other.time_ms = cursor_end;
                        }
                        cursor_end = cursor_end.saturating_add(other_duration);
                    }
                }
            }
        }
        studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
        studio.selected_dialogue_id = None;
        studio.pending_replay_autosave = true;
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
        ruler_tick_step,
        time_at_x,
        timeline_time_label,
        timeline_timecode,
        trimmed_duration,
        ReplayMode,
        ReplayStudio,
        TimelineHistory,
    };

    #[test]
    fn timeline_labels_distinguish_export_frames_and_ruler_subseconds() {
        for fps in [24, 25, 30, 60] {
            for frame in 0..fps * 3 {
                let time = frame_time_ms(u64::from(frame), fps);
                assert_eq!(
                    timeline_timecode(time, fps),
                    format!(
                        "00:00:{:02}:{:02}",
                        frame / fps,
                        frame % fps
                    ),
                );
            }
        }
        assert_eq!(
            timeline_timecode(999, 30),
            "00:00:00:29"
        );
        assert_eq!(timeline_time_label(500), "00:00.500");
        assert_eq!(timeline_time_label(1000), "00:01.000");
        assert_eq!(ruler_tick_step(9000, 1400.0), 1000);
        assert_eq!(ruler_tick_step(9000, 14_000.0), 100);
        assert_eq!(ruler_tick_step(u64::MAX, 1.0), u64::MAX);
    }

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
            time_at_x(150.0, 100.0, 200.0, 10_000, 0),
            2_500
        );
        assert_eq!(
            time_at_x(-10.0, 100.0, 200.0, 10_000, 0),
            0
        );
        assert_eq!(
            time_at_x(900.0, 100.0, 200.0, 10_000, 0),
            10_000
        );
        assert_eq!(
            time_at_x(150.0, 100.0, 400.0, 10_000, 0),
            1_250
        );
    }
}
