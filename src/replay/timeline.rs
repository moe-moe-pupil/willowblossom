use std::collections::BTreeMap;

use bevy::prelude::{
    Query,
    With,
};
use bevy_egui::egui::{
    pos2,
    vec2,
    Align2,
    Color32,
    Context,
    FontId,
    Id,
    Label,
    Rect,
    ScrollArea,
    Sense,
    Slider,
    Stroke,
    Window,
};
use voxxelmaxx::prelude::Grid;

use super::{
    format_time,
    frame_time_ms,
    prepare_replay_movement_for_playback,
    replay_dialogue_is_playable,
    replay_has_live_take,
    start_playback,
    ReplayMode,
    ReplayPlayerMovementHistory,
    ReplayShipTrajectoryHistory,
    ReplayStudio,
    TrpgVoxelGrid,
};

#[derive(Clone)]
struct TimelineView {
    zoom: f32,
    selected: Option<(usize, usize)>,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            selected: None,
        }
    }
}

struct Clip {
    start: u64,
    end: u64,
    label: String,
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

pub(super) fn replay_timeline_window(
    ctx: &Context,
    studio: &mut ReplayStudio,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    player_history: &ReplayPlayerMovementHistory,
    ship_history: &ReplayShipTrajectoryHistory,
) {
    let Some(replay) = studio.replay.as_ref() else { return };
    let duration = replay.duration_ms.max(1);
    let title = replay.title.clone();
    let mut tracks = vec![
        Track {
            name: "台词 / 字幕".into(),
            color: Color32::from_rgb(65, 125, 175),
            clips: replay
                .dialogue
                .iter()
                .filter(|line| replay_dialogue_is_playable(line))
                .map(|line| Clip {
                    start: line.time_ms,
                    end: line.time_ms.saturating_add(line.duration_ms),
                    label: format!("{}：{}", line.name, line.text),
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
                })
                .chain(
                    replay.camera_cuts.iter().map(|cut| Clip {
                        start: cut.time_ms,
                        end: cut.time_ms,
                        label: "手动切镜".into(),
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
            })
            .collect(),
    });
    let id = Id::new("replay-multitrack-view");
    let mut view = ctx.data_mut(|data| data.get_temp::<TimelineView>(id).unwrap_or_default());
    let mut seek = None;
    let enabled = studio.mode != ReplayMode::Recording && !replay_has_live_take(studio);
    Window::new("时间轴 · 合成预览")
        .id(Id::new("replay-multitrack-window"))
        .default_pos(pos2(24.0, (ctx.content_rect().height() - 340.0).max(24.0)))
        .default_size(vec2(960.0, 290.0))
        .min_width(360.0)
        .max_width((ctx.content_rect().width() - 32.0).max(360.0))
        .show(ctx, |ui| {
            ui.set_max_width((ctx.content_rect().width() - 32.0).max(360.0));
            ui.horizontal_wrapped(|ui| {
                ui.strong(&title);
                ui.monospace(format!("{} / {}", format_time(studio.playback_ms), format_time(duration)));
                ui.add_enabled_ui(enabled, |ui| {
                    if ui.button("⏮ 片头").clicked() { seek = Some(0); }
                    if ui.button("◀ 前一帧").clicked() { seek = Some(adjacent_frame(studio.playback_ms, studio.video_fps, false)); }
                    if ui.button(if studio.mode == ReplayMode::Playing { "暂停" } else { "▶ 播放" }).clicked() {
                        match studio.mode {
                            ReplayMode::Playing => studio.mode = ReplayMode::Paused,
                            ReplayMode::Paused => {
                                if studio.playback_ms >= duration { studio.playback_ms = 0; }
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
                    if ui.button("后一帧 ▶").clicked() { seek = Some(adjacent_frame(studio.playback_ms, studio.video_fps, true).min(duration)); }
                });
                ui.add(Slider::new(&mut view.zoom, 1.0..=20.0).text("缩放").logarithmic(true));
                if ui.button("适合全部").clicked() { view.zoom = 1.0; }
            });
            ui.small("拖动标尺定位并暂停；点击片段选择并定位。录制期间锁定导航。详细编辑位于回放工作室。");
            let viewport_width = ui.available_width();
            ScrollArea::both().id_salt("replay-tracks-scroll").auto_shrink([false, false])
                .max_height(220.0).show(ui, |ui| {
                    let label_width = 145.0;
                    let width = (viewport_width - label_width).max(220.0) * view.zoom;
                    let (rect, _) = ui.allocate_exact_size(vec2(label_width + width, 30.0 + tracks.len() as f32 * 32.0), Sense::hover());
                    let left = rect.left() + label_width;
                    let x_at = |time: u64| left + (time as f64 / duration as f64) as f32 * width;
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 0.0, Color32::from_rgb(24, 27, 33));
                    let ruler = Rect::from_min_max(pos2(left, rect.top()), pos2(rect.right(), rect.top() + 30.0));
                    let response = ui.interact(ruler, id.with("ruler"), Sense::click_and_drag());
                    if enabled && (response.clicked() || response.dragged()) {
                        if let Some(pointer) = response.interact_pointer_pos() {
                            seek = Some(time_at_x(pointer.x, left, width, duration));
                        }
                    }
                    let ticks = (width / 95.0).ceil().max(1.0) as u64;
                    for tick in 0..=ticks {
                        let time = (duration as f64 * tick as f64 / ticks as f64) as u64;
                        let x = x_at(time);
                        painter.line_segment([pos2(x, rect.top() + 22.0), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(48)));
                        painter.text(pos2(x + 3.0, rect.top() + 3.0), Align2::LEFT_TOP, format_time(time), FontId::monospace(11.0), Color32::LIGHT_GRAY);
                    }
                    for (row, track) in tracks.iter().enumerate() {
                        let top = rect.top() + 30.0 + row as f32 * 32.0;
                        painter.with_clip_rect(Rect::from_min_max(pos2(rect.left(), top), pos2(left - 4.0, top + 32.0)))
                            .text(pos2(rect.left() + 6.0, top + 15.0), Align2::LEFT_CENTER, &track.name, FontId::proportional(12.0), Color32::LIGHT_GRAY);
                        for (index, clip) in track.clips.iter().enumerate() {
                            let clip_left = x_at(clip.start.min(duration)).min(rect.right() - 5.0);
                            let clip_rect = Rect::from_min_max(pos2(clip_left, top + 3.0), pos2((x_at(clip.end.min(duration)).max(clip_left + 5.0)).min(rect.right()), top + 28.0));
                            if !ui.is_rect_visible(clip_rect) { continue; }
                            let selected = view.selected == Some((row, index));
                            painter.rect_filled(clip_rect, 2.0, if selected { Color32::from_rgb(180, 145, 55) } else { track.color });
                            if clip_rect.width() > 35.0 {
                                painter.with_clip_rect(clip_rect.shrink(2.0)).text(clip_rect.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, &clip.label, FontId::proportional(12.0), Color32::WHITE);
                            }
                            let response = ui.interact(clip_rect, id.with((row, index)), Sense::click());
                            if response.clicked() && enabled {
                                view.selected = Some((row, index));
                                seek = Some(clip.start);
                            }
                            response.on_hover_text(format!("{}\n{} — {}", clip.label, format_time(clip.start), format_time(clip.end)));
                        }
                    }
                    let x = x_at(studio.playback_ms.min(duration));
                    painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(2.0, Color32::from_rgb(245, 100, 90)));
                });
            if let Some(clip) = view.selected.and_then(|(row, index)| tracks.get(row)?.clips.get(index)) {
                ui.add(Label::new(format!("选中：{}  ·  {} — {}", clip.label, format_time(clip.start), format_time(clip.end))).truncate());
            }
        });
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
    use super::{
        adjacent_frame,
        frame_time_ms,
        time_at_x,
    };

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
