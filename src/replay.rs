#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    collections::HashMap,
    fs,
    path::{
        Path,
        PathBuf,
    },
    process::Command,
    thread,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use base64::{
    engine::general_purpose::STANDARD as BASE64,
    Engine,
};
use bevy::{
    prelude::*,
    render::view::screenshot::{
        Screenshot,
        ScreenshotCaptured,
    },
    transform::TransformSystems,
    window::PrimaryWindow,
};
use bevy_egui::{
    egui,
    EguiContexts,
    EguiPrimaryContextPass,
};
use bevy_persistent::Persistent;
use crossbeam_channel::{
    bounded,
    Receiver,
};
use serde::{
    Deserialize,
    Serialize,
};
use tempfile::TempDir;
use tokio_tungstenite::tungstenite::protocol::Message;
use voxxelmaxx::prelude::*;

use crate::{
    deepseek::{
        DeepseekIOSender,
        DeepseekManager,
        DeepseekRequest,
        DeepseekSummaryBlock,
    },
    napcat::{
        CampaignMessage,
        NapcatMessageChainType,
        NapcatMessageManager,
        PlayerAccess,
        PlayerCharacter,
        Visibility,
    },
    ui,
    voxel::{
        cached_or_local_voxel_standee_path,
        TrpgVoxelGrid,
        VoxelViewportCamera,
    },
};

const REPLAY_FORMAT_VERSION: u32 = 1;
const CAMERA_SAMPLE_SECONDS: f32 = 0.1;
const MIN_DIALOGUE_MS: u64 = 1_800;
const MAX_DIALOGUE_MS: u64 = 6_500;
const HISTORY_DIALOGUE_GAP_MS: u64 = 180;
const DEFAULT_REPLAY_PATH: &str = ".data/willowblossom/replays/latest.willow-replay.json";
const DEFAULT_VIDEO_PATH: &str = ".data/willowblossom/replays/latest.mp4";
const VIDEO_CAPTURE_WARMUP_FRAMES: u8 = 3;
const VIDEO_CAPTURE_TIMEOUT_SECONDS: f32 = 30.0;

pub struct ReplayPlugin;

impl Plugin for ReplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ReplayStudio>()
            .init_resource::<ReplayVideoCaptureActive>()
            .add_systems(
                Update,
                (
                    record_replay,
                    advance_replay,
                    render_video_frames,
                    poll_video_encoding,
                ),
            )
            .add_systems(
                PostUpdate,
                apply_replay_camera.before(TransformSystems::Propagate),
            )
            .add_systems(
                EguiPrimaryContextPass,
                replay_studio_ui.after(ui::ui_system),
            );
    }
}

#[derive(Resource, Default)]
pub(crate) struct ReplayVideoCaptureActive(pub bool);

pub(crate) fn replay_video_capture_inactive(active: Res<ReplayVideoCaptureActive>) -> bool {
    !active.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "scope", content = "id", rename_all = "snake_case")]
enum ReplayAudience {
    Public,
    Party(String),
    Player(u64),
    Gm,
}

impl Default for ReplayAudience {
    fn default() -> Self { Self::Public }
}

impl ReplayAudience {
    fn label(&self) -> String {
        match self {
            Self::Public => "公开".to_owned(),
            Self::Party(id) => format!("队伍：{id}"),
            Self::Player(id) => format!("玩家：{id}"),
            Self::Gm => "GM（包含私密内容）".to_owned(),
        }
    }

    fn can_read(&self, message: &CampaignMessage, manager: &NapcatMessageManager) -> bool {
        let player_access = match self {
            Self::Player(player_id) => Some(manager.player_access_for_user(*player_id)),
            _ => None,
        };
        self.can_read_visibility(
            &message.visibility,
            player_access.as_ref(),
        )
    }

    fn can_read_visibility(
        &self,
        visibility: &Visibility,
        player_access: Option<&PlayerAccess>,
    ) -> bool {
        match self {
            Self::Public => matches!(visibility, Visibility::Public),
            Self::Party(party_id) => {
                matches!(visibility, Visibility::Public)
                    || matches!(visibility, Visibility::Party(id) if id == party_id)
            },
            Self::Player(_) => player_access.is_some_and(|access| access.can_read(visibility)),
            Self::Gm => PlayerAccess {
                is_gm: true,
                ..Default::default()
            }
            .can_read(visibility),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplayFile {
    format_version: u32,
    title: String,
    campaign_id: String,
    created_at_unix_ms: u64,
    duration_ms: u64,
    audience: ReplayAudience,
    scene: ReplayScene,
    camera: Vec<ReplayCameraKeyframe>,
    dialogue: Vec<ReplayDialogue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ReplayScene {
    voxels: Vec<ReplayVoxel>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct ReplayVoxel {
    position: [i32; 3],
    material: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplayCameraKeyframe {
    time_ms: u64,
    translation: [f32; 3],
    rotation: [f32; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplayDialogue {
    time_ms: u64,
    duration_ms: u64,
    sender_id: u64,
    name: String,
    role: String,
    text: String,
    avatar: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avatar_data_url: Option<String>,
    visibility: Visibility,
    #[serde(default)]
    side: DialogueSide,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DialogueSide {
    Left,
    #[default]
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayMode {
    Idle,
    Recording,
    Playing,
    Paused,
}

impl Default for ReplayMode {
    fn default() -> Self { Self::Idle }
}

#[derive(Resource)]
struct ReplayStudio {
    mode: ReplayMode,
    replay: Option<ReplayFile>,
    audience: ReplayAudience,
    record_elapsed_ms: u64,
    playback_ms: u64,
    playback_speed: f32,
    camera_sample_accumulator: f32,
    message_counts: HashMap<String, usize>,
    pre_playback_scene: Option<ReplayScene>,
    video_path: String,
    video_fps: u32,
    project_export_path: String,
    project_import_path: String,
    video_render: Option<VideoRenderJob>,
    video_encoding: Option<VideoEncodingJob>,
    status: String,
    panel_open: bool,
}

struct VideoRenderJob {
    id: u64,
    frames: TempDir,
    output_path: PathBuf,
    fps: u32,
    total_frames: u64,
    next_frame: u64,
    capture_pending: bool,
    pending_seconds: f32,
    warmup_frames: u8,
    failure: Option<String>,
    original_window_title: String,
    original_window_resizable: bool,
}

struct VideoEncodingJob {
    _frames: TempDir,
    output_path: PathBuf,
    result: Receiver<Result<(), String>>,
}

impl Default for ReplayStudio {
    fn default() -> Self {
        Self {
            mode: ReplayMode::Idle,
            replay: None,
            audience: ReplayAudience::Public,
            record_elapsed_ms: 0,
            playback_ms: 0,
            playback_speed: 1.0,
            camera_sample_accumulator: 0.0,
            message_counts: HashMap::new(),
            pre_playback_scene: None,
            video_path: DEFAULT_VIDEO_PATH.to_owned(),
            video_fps: 15,
            project_export_path: DEFAULT_REPLAY_PATH.to_owned(),
            project_import_path: DEFAULT_REPLAY_PATH.to_owned(),
            video_render: None,
            video_encoding: None,
            status: "尚未创建回放".to_owned(),
            panel_open: true,
        }
    }
}

fn record_replay(
    time: Res<Time>,
    manager: Res<Persistent<NapcatMessageManager>>,
    camera: Query<&Transform, With<VoxelViewportCamera>>,
    mut studio: ResMut<ReplayStudio>,
) {
    if studio.mode != ReplayMode::Recording {
        return;
    }

    let delta_seconds = time.delta_secs();
    studio.record_elapsed_ms = studio
        .record_elapsed_ms
        .saturating_add((delta_seconds * 1_000.0).round() as u64);
    studio.camera_sample_accumulator += delta_seconds;

    if studio.camera_sample_accumulator >= CAMERA_SAMPLE_SECONDS {
        studio.camera_sample_accumulator %= CAMERA_SAMPLE_SECONDS;
        let record_elapsed_ms = studio.record_elapsed_ms;
        if let (Ok(transform), Some(replay)) = (camera.single(), studio.replay.as_mut()) {
            replay.camera.push(camera_keyframe(
                record_elapsed_ms,
                transform,
            ));
        }
    }

    let audience = studio.audience.clone();
    let record_elapsed_ms = studio.record_elapsed_ms;
    let targets = manager.messages.keys().cloned().collect::<Vec<_>>();
    let mut captured = Vec::new();
    for target_id in targets {
        let messages = &manager.messages[&target_id];
        let seen = studio
            .message_counts
            .get(&target_id)
            .copied()
            .unwrap_or_default();
        for message in messages.iter().skip(seen) {
            let campaign_message = manager.campaign_message_for_target(&target_id, message);
            if audience.can_read(&campaign_message, &manager) {
                if let Some(dialogue) = dialogue_from_message(
                    &campaign_message,
                    &manager,
                    record_elapsed_ms,
                ) {
                    captured.push(dialogue);
                }
            }
        }
        studio.message_counts.insert(target_id, messages.len());
    }
    let record_elapsed_ms = studio.record_elapsed_ms;
    if let Some(replay) = studio.replay.as_mut() {
        replay.dialogue.extend(captured);
        replay.duration_ms = record_elapsed_ms;
    }
}

fn advance_replay(time: Res<Time>, mut studio: ResMut<ReplayStudio>) {
    if studio.mode != ReplayMode::Playing {
        return;
    }
    let Some(duration_ms) = studio.replay.as_ref().map(|replay| replay.duration_ms) else {
        studio.mode = ReplayMode::Idle;
        return;
    };
    let delta_ms = (time.delta_secs() * studio.playback_speed * 1_000.0).round() as u64;
    studio.playback_ms = studio.playback_ms.saturating_add(delta_ms).min(duration_ms);
    if studio.playback_ms >= duration_ms {
        studio.mode = ReplayMode::Paused;
    }
}

fn render_video_frames(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut studio: ResMut<ReplayStudio>,
    mut capture_active: ResMut<ReplayVideoCaptureActive>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Some(job) = studio.video_render.as_mut() else {
        return;
    };

    if keys.just_pressed(KeyCode::Escape) {
        job.failure = Some("视频导出已取消".to_owned());
    }
    if job.capture_pending {
        job.pending_seconds += time.delta_secs();
        if job.pending_seconds >= VIDEO_CAPTURE_TIMEOUT_SECONDS {
            job.failure = Some("等待画面截图超时".to_owned());
            job.capture_pending = false;
        }
    }

    if let Some(error) = job.failure.clone() {
        finish_video_capture(
            &mut studio,
            &mut capture_active,
            &mut grids,
            &mut windows,
            Err(error),
        );
        return;
    }
    if job.capture_pending {
        return;
    }
    if job.warmup_frames > 0 {
        job.warmup_frames -= 1;
        return;
    }
    if job.next_frame >= job.total_frames {
        finish_video_capture(
            &mut studio,
            &mut capture_active,
            &mut grids,
            &mut windows,
            Ok(()),
        );
        return;
    }

    let job_id = job.id;
    let frame_index = job.next_frame;
    let frame_path = job.frames.path().join(frame_file_name(frame_index));
    let playback_ms = frame_time_ms(frame_index, job.fps);
    let total_frames = job.total_frames;
    job.capture_pending = true;
    job.pending_seconds = 0.0;
    studio.playback_ms = playback_ms;
    if let Ok(mut window) = windows.single_mut() {
        window.title = format!(
            "正在渲染视频 {}/{}（Esc 取消）",
            frame_index + 1,
            total_frames
        );
    }

    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, mut studio: ResMut<ReplayStudio>| {
            let save_result = captured
                .image
                .clone()
                .try_into_dynamic()
                .map_err(|err| err.to_string())
                .and_then(|image| {
                    image
                        .to_rgb8()
                        .save(&frame_path)
                        .map_err(|err| err.to_string())
                });
            let Some(job) = studio.video_render.as_mut().filter(|job| job.id == job_id) else {
                return;
            };
            job.capture_pending = false;
            job.pending_seconds = 0.0;
            match save_result {
                Ok(()) => job.next_frame += 1,
                Err(err) => job.failure = Some(format!("保存视频帧失败：{err}")),
            }
        },
    );
}

fn finish_video_capture(
    studio: &mut ReplayStudio,
    capture_active: &mut ReplayVideoCaptureActive,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
    result: Result<(), String>,
) {
    let Some(job) = studio.video_render.take() else {
        return;
    };
    capture_active.0 = false;
    if let Ok(mut window) = windows.single_mut() {
        window.title = job.original_window_title;
        window.resizable = job.original_window_resizable;
    }
    stop_playback(studio, grids);

    if let Err(err) = result {
        studio.status = err;
        return;
    }

    let frames_path = job.frames.path().to_owned();
    let output_path = job.output_path.clone();
    let fps = job.fps;
    let (sender, receiver) = bounded(1);
    thread::spawn(move || {
        let result = encode_video_frames(&frames_path, &output_path, fps);
        let _ = sender.send(result);
    });
    studio.status = format!(
        "正在使用 FFmpeg 编码 {}",
        job.output_path.display()
    );
    studio.video_encoding = Some(VideoEncodingJob {
        _frames: job.frames,
        output_path: job.output_path,
        result: receiver,
    });
}

fn poll_video_encoding(mut studio: ResMut<ReplayStudio>) {
    let Some(job) = studio.video_encoding.as_ref() else {
        return;
    };
    let Ok(result) = job.result.try_recv() else {
        return;
    };
    let output_path = job.output_path.clone();
    studio.video_encoding = None;
    studio.status = match result {
        Ok(()) => format!(
            "MP4 视频已导出到 {}",
            output_path.display()
        ),
        Err(err) => format!("视频编码失败：{err}"),
    };
}

fn apply_replay_camera(
    studio: Res<ReplayStudio>,
    mut camera: Query<&mut Transform, With<VoxelViewportCamera>>,
) {
    if !matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    ) {
        return;
    }
    let Some(replay) = studio.replay.as_ref() else { return };
    let Some(transform) = interpolated_camera(&replay.camera, studio.playback_ms) else {
        return;
    };
    if let Ok(mut camera) = camera.single_mut() {
        *camera = transform;
    }
}

fn replay_studio_ui(
    mut contexts: EguiContexts,
    manager: Res<Persistent<NapcatMessageManager>>,
    deepseek_sender: Option<Res<DeepseekIOSender>>,
    mut deepseek_manager: ResMut<Persistent<DeepseekManager>>,
    mut studio: ResMut<ReplayStudio>,
    camera: Query<&Transform, With<VoxelViewportCamera>>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut capture_active: ResMut<ReplayVideoCaptureActive>,
    mut avatar_textures: Local<HashMap<String, egui::TextureHandle>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };

    if !capture_active.0 {
        egui::Area::new(egui::Id::new("replay-studio-button"))
            .anchor(
                egui::Align2::RIGHT_TOP,
                egui::vec2(-12.0, 44.0),
            )
            .show(ctx, |ui| {
                if ui.button("🎬 回放").clicked() {
                    studio.panel_open = !studio.panel_open;
                }
            });
    }

    if studio.panel_open && !capture_active.0 {
        let mut open = studio.panel_open;
        egui::Window::new("TRPG 回放工作室")
            .id(egui::Id::new("trpg-replay-studio"))
            .open(&mut open)
            .default_width(390.0)
            .show(ctx, |ui| {
                replay_controls(
                    ui,
                    &manager,
                    deepseek_sender.as_deref(),
                    &mut deepseek_manager,
                    &mut studio,
                    &camera,
                    &mut grids,
                    &mut windows,
                    &mut capture_active,
                )
            });
        studio.panel_open = open;
    }

    if matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    ) {
        if let Some(dialogue) = studio
            .replay
            .as_ref()
            .and_then(|replay| active_dialogue(&replay.dialogue, studio.playback_ms))
            .cloned()
        {
            dialogue_overlay(ctx, &dialogue, &mut avatar_textures);
        }
    }
}

fn replay_controls(
    ui: &mut egui::Ui,
    manager: &NapcatMessageManager,
    deepseek_sender: Option<&DeepseekIOSender>,
    deepseek_manager: &mut Persistent<DeepseekManager>,
    studio: &mut ReplayStudio,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
    capture_active: &mut ReplayVideoCaptureActive,
) {
    ui.label("记录体素场景、自由镜头和可见对话，并在应用内确定性回放。");
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("发布范围");
        egui::ComboBox::from_id_salt("replay-audience")
            .selected_text(studio.audience.label())
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut studio.audience,
                    ReplayAudience::Public,
                    "公开",
                );
                if let Some(group) = manager.current_group() {
                    let mut parties = group.parties.keys().cloned().collect::<Vec<_>>();
                    parties.sort();
                    for party in parties {
                        ui.selectable_value(
                            &mut studio.audience,
                            ReplayAudience::Party(party.clone()),
                            format!("队伍：{party}"),
                        );
                    }
                    let mut players = group
                        .players
                        .iter()
                        .filter_map(|id| id.parse::<u64>().ok())
                        .collect::<Vec<_>>();
                    players.sort_unstable();
                    for player in players {
                        ui.selectable_value(
                            &mut studio.audience,
                            ReplayAudience::Player(player),
                            format!("玩家：{player}"),
                        );
                    }
                }
                ui.selectable_value(
                    &mut studio.audience,
                    ReplayAudience::Gm,
                    "GM（包含私密内容）",
                );
            });
    });
    if studio.audience == ReplayAudience::Gm {
        ui.colored_label(
            egui::Color32::from_rgb(210, 90, 70),
            "GM 回放可能包含私聊、隐藏队伍和系统内容，请勿公开发布。",
        );
    }

    ui.horizontal(|ui| match studio.mode {
        ReplayMode::Recording => {
            ui.label(format!(
                "● 录制中 {}",
                format_time(studio.record_elapsed_ms)
            ));
            if ui.button("停止录制").clicked() {
                stop_recording(studio);
            }
        },
        ReplayMode::Playing | ReplayMode::Paused => {
            if ui
                .button(if studio.mode == ReplayMode::Playing { "暂停" } else { "继续" })
                .clicked()
            {
                studio.mode = if studio.mode == ReplayMode::Playing {
                    ReplayMode::Paused
                } else {
                    ReplayMode::Playing
                };
            }
            if ui.button("停止回放").clicked() {
                stop_playback(studio, grids);
            }
        },
        ReplayMode::Idle => {
            if ui.button("开始录制").clicked() {
                start_recording(studio, manager, camera, grids);
            }
            if ui.button("从现有聊天生成").clicked() {
                build_from_history(studio, manager, camera, grids);
            }
        },
    });

    if let Some(replay) = studio.replay.as_ref() {
        ui.label(format!(
            "{} · {} · {} 个镜头帧 · {} 条对话 · {} 个体素",
            replay.title,
            format_time(replay.duration_ms),
            replay.camera.len(),
            replay.dialogue.len(),
            replay.scene.voxels.len(),
        ));
    }

    if !matches!(studio.mode, ReplayMode::Recording) && studio.replay.is_some() {
        let duration = studio.replay.as_ref().unwrap().duration_ms.max(1);
        ui.horizontal(|ui| {
            if !matches!(
                studio.mode,
                ReplayMode::Playing | ReplayMode::Paused
            ) && ui.button("▶ 播放").clicked()
            {
                start_playback(studio, grids);
            }
            ui.add(
                egui::Slider::new(&mut studio.playback_ms, 0..=duration)
                    .show_value(false)
                    .text("时间轴"),
            );
            ui.label(format!(
                "{} / {}",
                format_time(studio.playback_ms),
                format_time(duration)
            ));
        });
        ui.horizontal(|ui| {
            ui.label("速度");
            for speed in [0.5, 1.0, 2.0, 4.0] {
                ui.selectable_value(
                    &mut studio.playback_speed,
                    speed,
                    format!("{speed}×"),
                );
            }
        });
    }

    ui.separator();
    ui.heading("DeepSeek 制作提要");
    ui.small("使用 deepseek-v4-pro 思考模式，只整理当前发布范围内已经说过的内容，供 DM 审核；不会续写剧情、改写玩家台词或读取其他队伍的隐藏内容。");
    if ui
        .add_enabled(
            studio
                .replay
                .as_ref()
                .is_some_and(|replay| !replay.dialogue.is_empty()),
            egui::Button::new("整理现有台词"),
        )
        .clicked()
    {
        studio.status = match studio
            .replay
            .as_ref()
            .ok_or_else(|| "请先从现有聊天生成回放".to_owned())
            .and_then(|replay| {
                queue_replay_summary(
                    replay,
                    deepseek_sender,
                    deepseek_manager,
                )
            }) {
            Ok(()) => {
                if let Err(err) = deepseek_manager.persist() {
                    format!("DeepSeek 请求已发送，但保存请求状态失败：{err}")
                } else {
                    "DeepSeek 正在整理可见台词；完成后会在这里显示制作提要".to_owned()
                }
            },
            Err(err) => format!("DeepSeek 整理失败：{err}"),
        };
    }
    if let Some(replay) = studio.replay.as_ref() {
        if let Some(block) = replay_summary_block(replay, deepseek_manager) {
            if block.pending {
                ui.spinner();
                ui.small("正在整理……");
            } else if let Some(error) = &block.error {
                ui.colored_label(
                    egui::Color32::from_rgb(210, 90, 70),
                    error,
                );
            } else if !block.latest.trim().is_empty() {
                ui.group(|ui| {
                    ui.label("DM 制作提要（不会自动写入台词）");
                    ui.label(&block.latest);
                });
            }
        }
    }
    ui.small("镜头使用 DM 录制的自由镜头轨迹；从聊天生成时使用当前镜头。台词显示时长和切换间隔由本地确定性排版器控制。");

    ui.separator();
    ui.heading("导出 MP4 视频");
    ui.label("视频路径");
    ui.text_edit_singleline(&mut studio.video_path);
    ui.horizontal(|ui| {
        ui.label("帧率");
        for (fps, label) in [
            (12, "12 FPS 极速"),
            (15, "15 FPS 快速"),
            (24, "24 FPS 电影"),
            (30, "30 FPS 流畅"),
            (60, "60 FPS 高质量"),
        ] {
            ui.selectable_value(&mut studio.video_fps, fps, label);
        }
    });
    if let Some(replay) = studio.replay.as_ref() {
        ui.small(format!(
            "预计渲染 {} 帧，视频时长 {}",
            video_frame_count(replay.duration_ms, studio.video_fps),
            format_time(replay.duration_ms)
        ));
    }
    let can_export_video = studio.replay.is_some()
        && studio.video_encoding.is_none()
        && studio.mode != ReplayMode::Recording;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                can_export_video,
                egui::Button::new("渲染并导出 MP4"),
            )
            .clicked()
        {
            start_video_export(studio, capture_active, grids, windows);
        }
    });
    let can_auto_direct = manager.active_campaign_id().is_some()
        && studio.mode == ReplayMode::Idle
        && studio.video_render.is_none()
        && studio.video_encoding.is_none();
    if ui
        .add_enabled(
            can_auto_direct,
            egui::Button::new("DeepSeek 整理 + 自动导演并导出"),
        )
        .on_hover_text("从所选发布范围的现有聊天重建回放，生成确定性镜头轨迹，并立即开始导出 MP4。DeepSeek 仅生成制作提要。")
        .clicked()
    {
        build_from_history(studio, manager, camera, grids);
        if let (Some(replay), Ok(base_camera)) = (studio.replay.as_mut(), camera.single()) {
            replay.camera = automatic_camera_track(
                base_camera,
                &replay.dialogue,
                replay.duration_ms,
            );
        }
        let summary_queued = studio.replay.as_ref().is_some_and(|replay| {
            queue_replay_summary(replay, deepseek_sender, deepseek_manager).is_ok()
        });
        if summary_queued {
            let _ = deepseek_manager.persist();
        }
        start_video_export(studio, capture_active, grids, windows);
    }
    ui.small("一键模式会自动完成：读取可见聊天 → 请求事实性制作提要 → 编排平滑镜头 → 逐帧渲染 → FFmpeg 输出 MP4。");
    ui.small("导出时会隐藏编辑器界面，逐帧渲染台词层，再用 FFmpeg 编码 H.264 视频。按 Esc 可取消逐帧渲染。");

    ui.collapsing(
        "回放项目数据（JSON，可选）",
        |ui| {
            ui.label("项目导出路径");
            ui.text_edit_singleline(&mut studio.project_export_path);
            if ui
                .add_enabled(
                    studio.replay.is_some(),
                    egui::Button::new("保存回放项目"),
                )
                .clicked()
            {
                studio.status = match studio
                    .replay
                    .as_ref()
                    .ok_or_else(|| "没有可保存的回放".to_owned())
                    .and_then(|replay| export_replay(replay, &studio.project_export_path))
                {
                    Ok(()) => format!(
                        "回放项目已保存到 {}",
                        studio.project_export_path
                    ),
                    Err(err) => format!("项目保存失败：{err}"),
                };
            }
            ui.label("项目导入路径");
            ui.text_edit_singleline(&mut studio.project_import_path);
            if ui.button("载入回放项目").clicked() {
                match import_replay(&studio.project_import_path) {
                    Ok(replay) => {
                        stop_playback(studio, grids);
                        studio.playback_ms = 0;
                        studio.audience = replay.audience.clone();
                        studio.status = format!("已载入项目：{}", replay.title);
                        studio.replay = Some(replay);
                    },
                    Err(err) => studio.status = format!("项目载入失败：{err}"),
                }
            }
        },
    );
    if !studio.status.is_empty() {
        ui.small(studio.status.as_str());
    }
}

fn start_recording(
    studio: &mut ReplayStudio,
    manager: &NapcatMessageManager,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
) {
    let Some(campaign_id) = manager.active_campaign_id() else {
        studio.status = "请先选择跑团组（战役）".to_owned();
        return;
    };
    let scene = grids
        .single_mut()
        .map(|grid| capture_scene(&grid))
        .unwrap_or_default();
    let mut replay = new_replay(
        manager,
        campaign_id,
        studio.audience.clone(),
        scene,
    );
    if let Ok(transform) = camera.single() {
        replay.camera.push(camera_keyframe(0, transform));
    }
    studio.message_counts = manager
        .messages
        .iter()
        .map(|(target, messages)| (target.clone(), messages.len()))
        .collect();
    studio.record_elapsed_ms = 0;
    studio.camera_sample_accumulator = 0.0;
    studio.playback_ms = 0;
    studio.replay = Some(replay);
    studio.mode = ReplayMode::Recording;
    studio.status = "开始录制；只会收录所选发布范围可见的消息".to_owned();
}

fn stop_recording(studio: &mut ReplayStudio) {
    if let Some(replay) = studio.replay.as_mut() {
        replay.duration_ms = studio.record_elapsed_ms.max(
            replay
                .dialogue
                .iter()
                .map(|line| line.time_ms.saturating_add(line.duration_ms))
                .max()
                .unwrap_or_default(),
        );
    }
    studio.mode = ReplayMode::Idle;
    studio.playback_ms = 0;
    studio.status = "录制已停止，可以预览或导出".to_owned();
}

fn build_from_history(
    studio: &mut ReplayStudio,
    manager: &NapcatMessageManager,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
) {
    let Some(campaign_id) = manager.active_campaign_id() else {
        studio.status = "请先选择跑团组（战役）".to_owned();
        return;
    };
    let scene = grids
        .single_mut()
        .map(|grid| capture_scene(&grid))
        .unwrap_or_default();
    let mut replay = new_replay(
        manager,
        campaign_id.clone(),
        studio.audience.clone(),
        scene,
    );
    let mut visible = manager
        .messages
        .iter()
        .flat_map(|(target, messages)| {
            messages
                .iter()
                .map(|message| manager.campaign_message_for_target(target, message))
                .collect::<Vec<_>>()
        })
        .filter(|message| message.campaign_id == campaign_id)
        .filter(|message| studio.audience.can_read(message, manager))
        .filter(|message| !message.text.trim().is_empty())
        .collect::<Vec<_>>();
    visible.sort_by_key(|message| message.time);
    let mut timeline_ms: u64 = 350;
    for message in &visible {
        if let Some(dialogue) = dialogue_from_message(message, manager, timeline_ms) {
            timeline_ms = dialogue
                .time_ms
                .saturating_add(dialogue.duration_ms)
                .saturating_add(HISTORY_DIALOGUE_GAP_MS);
            replay.dialogue.push(dialogue);
        }
    }
    replay.duration_ms = replay
        .dialogue
        .iter()
        .map(|line| line.time_ms.saturating_add(line.duration_ms))
        .max()
        .unwrap_or(5_000);
    if let Ok(transform) = camera.single() {
        replay.camera.push(camera_keyframe(0, transform));
        replay.camera.push(camera_keyframe(
            replay.duration_ms,
            transform,
        ));
    }
    studio.playback_ms = 0;
    studio.replay = Some(replay);
    studio.status = format!(
        "已从现有聊天生成 {0} 条连续对话；切换间隔约 0.18 秒",
        visible.len()
    );
}

fn replay_summary_key(replay: &ReplayFile) -> String {
    let audience = match &replay.audience {
        ReplayAudience::Public => "public".to_owned(),
        ReplayAudience::Party(id) => format!("party:{id}"),
        ReplayAudience::Player(id) => format!("player:{id}"),
        ReplayAudience::Gm => "gm".to_owned(),
    };
    format!(
        "replay:{}:{audience}",
        replay.campaign_id
    )
}

fn replay_summary_block<'a>(
    replay: &ReplayFile,
    manager: &'a DeepseekManager,
) -> Option<&'a DeepseekSummaryBlock> {
    let message_count = replay.dialogue.len();
    manager
        .summaries
        .get(&replay_summary_key(replay))?
        .blocks
        .iter()
        .find(|block| block.message_count == message_count)
}

fn queue_replay_summary(
    replay: &ReplayFile,
    sender: Option<&DeepseekIOSender>,
    manager: &mut DeepseekManager,
) -> Result<(), String> {
    if replay.dialogue.is_empty() {
        return Err("回放中没有可整理的台词".to_owned());
    }
    let sender = sender.ok_or_else(|| "DeepSeek 连接尚未就绪，请稍后重试".to_owned())?;
    let summary_key = replay_summary_key(replay);
    let message_count = replay.dialogue.len();
    if let Some(block) = replay_summary_block(replay, manager) {
        if block.pending || !block.latest.trim().is_empty() {
            return Err("这版台词已经整理过；新增台词后可以再次整理".to_owned());
        }
    }
    let text = replay
        .dialogue
        .iter()
        .map(|line| format!("{}：{}", line.name, line.text.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    let request = serde_json::to_string(&DeepseekRequest::Summary {
        target_id: summary_key.clone(),
        message_count,
        text,
    })
    .map(Message::text)
    .map_err(|err| err.to_string())?;
    sender.0.try_send(request).map_err(|err| err.to_string())?;
    manager
        .summaries
        .entry(summary_key)
        .or_default()
        .upsert_block(DeepseekSummaryBlock {
            latest: String::new(),
            message_count,
            pending: true,
            error: None,
        });
    Ok(())
}

fn new_replay(
    manager: &NapcatMessageManager,
    campaign_id: String,
    audience: ReplayAudience,
    scene: ReplayScene,
) -> ReplayFile {
    let title = manager
        .current_trpg_group
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("TRPG 回放")
        .to_owned();
    ReplayFile {
        format_version: REPLAY_FORMAT_VERSION,
        title,
        campaign_id,
        created_at_unix_ms: unix_time_ms(),
        duration_ms: 0,
        audience,
        scene,
        camera: Vec::new(),
        dialogue: Vec::new(),
    }
}

fn start_playback(
    studio: &mut ReplayStudio,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
) {
    let Some(scene) = studio.replay.as_ref().map(|replay| replay.scene.clone()) else {
        return;
    };
    if let Ok(mut grid) = grids.single_mut() {
        studio.pre_playback_scene = Some(capture_scene(&grid));
        apply_scene(&mut grid, &scene);
    }
    studio.playback_ms = studio.playback_ms.min(
        studio
            .replay
            .as_ref()
            .map(|replay| replay.duration_ms)
            .unwrap_or_default(),
    );
    studio.mode = ReplayMode::Playing;
    studio.status = "正在回放；停止后会恢复当前体素场景".to_owned();
}

fn stop_playback(studio: &mut ReplayStudio, grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>) {
    if let Some(scene) = studio.pre_playback_scene.take() {
        if let Ok(mut grid) = grids.single_mut() {
            apply_scene(&mut grid, &scene);
        }
    }
    if matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    ) {
        studio.mode = ReplayMode::Idle;
    }
    studio.playback_ms = 0;
}

fn start_video_export(
    studio: &mut ReplayStudio,
    capture_active: &mut ReplayVideoCaptureActive,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) {
    let Some(duration_ms) = studio.replay.as_ref().map(|replay| replay.duration_ms) else {
        studio.status = "没有可渲染的回放".to_owned();
        return;
    };
    let output_path = match normalized_video_path(&studio.video_path) {
        Ok(path) => path,
        Err(err) => {
            studio.status = format!("视频路径无效：{err}");
            return;
        },
    };
    if let Err(err) = check_ffmpeg() {
        studio.status = err;
        return;
    }
    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        if let Err(err) = fs::create_dir_all(parent) {
            studio.status = format!("无法创建视频目录：{err}");
            return;
        }
    }
    let temp_root = Path::new(".data")
        .join("willowblossom")
        .join("video-render-cache");
    if let Err(err) = fs::create_dir_all(&temp_root) {
        studio.status = format!("无法创建视频帧缓存目录：{err}");
        return;
    }
    let frames = match tempfile::Builder::new()
        .prefix("render-")
        .tempdir_in(&temp_root)
    {
        Ok(frames) => frames,
        Err(err) => {
            studio.status = format!("无法创建视频帧缓存：{err}");
            return;
        },
    };

    if matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    ) {
        stop_playback(studio, grids);
    }
    start_playback(studio, grids);
    studio.mode = ReplayMode::Paused;
    studio.playback_ms = 0;
    let fps = studio.video_fps.clamp(1, 120);
    let total_frames = video_frame_count(duration_ms, fps);
    let Ok(mut window) = windows.single_mut() else {
        stop_playback(studio, grids);
        studio.status = "找不到主窗口，无法渲染视频".to_owned();
        return;
    };
    let original_window_title = window.title.clone();
    let original_window_resizable = window.resizable;
    window.resizable = false;
    window.title = format!("正在准备视频渲染（共 {total_frames} 帧）");
    capture_active.0 = true;
    studio.video_render = Some(VideoRenderJob {
        id: unix_time_ms(),
        frames,
        output_path,
        fps,
        total_frames,
        next_frame: 0,
        capture_pending: false,
        pending_seconds: 0.0,
        warmup_frames: VIDEO_CAPTURE_WARMUP_FRAMES,
        failure: None,
        original_window_title,
        original_window_resizable,
    });
    studio.status = format!("正在逐帧渲染 {total_frames} 帧");
}

fn video_frame_count(duration_ms: u64, fps: u32) -> u64 {
    (duration_ms.saturating_mul(fps as u64).saturating_add(999) / 1_000).max(1)
}

fn frame_time_ms(frame_index: u64, fps: u32) -> u64 {
    frame_index.saturating_mul(1_000) / fps.max(1) as u64
}

fn frame_file_name(frame_index: u64) -> String { format!("frame_{frame_index:06}.png") }

fn normalized_video_path(path: &str) -> Result<PathBuf, String> {
    let mut path = normalized_path(path)?;
    if path.extension().is_none() {
        path.set_extension("mp4");
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("mp4"))
    {
        return Err("视频文件必须使用 .mp4 扩展名".to_owned());
    }
    Ok(path)
}

fn check_ffmpeg() -> Result<(), String> {
    let mut command = Command::new("ffmpeg");
    command.arg("-version");
    hide_command_window(&mut command);
    command
        .output()
        .map_err(|_| "未找到 FFmpeg，请安装 FFmpeg 并将 ffmpeg 加入 PATH".to_owned())
        .and_then(|output| {
            output
                .status
                .success()
                .then_some(())
                .ok_or_else(|| "FFmpeg 无法启动，请检查安装".to_owned())
        })
}

fn encode_video_frames(frames_path: &Path, output_path: &Path, fps: u32) -> Result<(), String> {
    let frame_pattern = frames_path.join("frame_%06d.png");
    let temporary_output = temporary_video_output_path(output_path);
    let mut command = Command::new("ffmpeg");
    command.args(ffmpeg_arguments(fps));
    command.arg(frame_pattern);
    command.args([
        "-vf",
        "pad=ceil(iw/2)*2:ceil(ih/2)*2",
        "-c:v",
        "libx264",
        "-preset",
        "medium",
        "-crf",
        "18",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
    ]);
    command.arg(&temporary_output);
    hide_command_window(&mut command);
    let output = command
        .output()
        .map_err(|err| format!("无法启动 FFmpeg：{err}"))?;
    if output.status.success() {
        if output_path.exists() {
            fs::remove_file(output_path).map_err(|err| format!("无法替换已有视频：{err}"))?;
        }
        return fs::rename(&temporary_output, output_path)
            .map_err(|err| format!("无法完成视频文件：{err}"));
    }
    let _ = fs::remove_file(&temporary_output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail = stderr
        .lines()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ");
    Err(if tail.is_empty() {
        format!("FFmpeg 退出码：{}", output.status)
    } else {
        tail
    })
}

fn temporary_video_output_path(output_path: &Path) -> PathBuf {
    let file_stem = output_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("replay");
    output_path.with_file_name(format!(
        ".{file_stem}.rendering-{}.mp4",
        unix_time_ms()
    ))
}

fn ffmpeg_arguments(fps: u32) -> Vec<String> {
    vec![
        "-y".to_owned(),
        "-hide_banner".to_owned(),
        "-loglevel".to_owned(),
        "error".to_owned(),
        "-framerate".to_owned(),
        fps.max(1).to_string(),
        "-start_number".to_owned(),
        "0".to_owned(),
        "-i".to_owned(),
    ]
}

fn hide_command_window(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
}

fn capture_scene(grid: &Grid<u8>) -> ReplayScene {
    let voxels = grid
        .iter()
        .flat_map(|(chunk_position, chunk)| {
            prism(IVec3::ZERO, DIMS).filter_map(move |local| {
                let material = chunk[local];
                (material != 0).then_some(ReplayVoxel {
                    position: (*chunk_position * DIMS + local).to_array(),
                    material,
                })
            })
        })
        .collect();
    ReplayScene { voxels }
}

fn apply_scene(grid: &mut Mut<Grid<u8>>, scene: &ReplayScene) {
    let occupied = grid
        .iter()
        .flat_map(|(chunk_position, chunk)| {
            prism(IVec3::ZERO, DIMS)
                .filter(|local| chunk[*local] != 0)
                .map(|local| *chunk_position * DIMS + local)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for cell in occupied {
        grid.set(cell, 0);
    }
    for voxel in &scene.voxels {
        grid.set(
            IVec3::from_array(voxel.position),
            voxel.material,
        );
    }
}

fn camera_keyframe(time_ms: u64, transform: &Transform) -> ReplayCameraKeyframe {
    ReplayCameraKeyframe {
        time_ms,
        translation: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
    }
}

fn automatic_camera_track(
    base: &Transform,
    dialogue: &[ReplayDialogue],
    duration_ms: u64,
) -> Vec<ReplayCameraKeyframe> {
    let mut frames = Vec::with_capacity(dialogue.len().saturating_mul(2).saturating_add(2));
    frames.push(camera_keyframe(0, base));
    let camera_right = base.rotation * Vec3::X;
    let camera_forward = base.rotation * Vec3::NEG_Z;
    for (index, line) in dialogue.iter().enumerate() {
        let side = match line.side {
            DialogueSide::Left => -1.0,
            DialogueSide::Right => 1.0,
        };
        let mut shot = base.clone();
        shot.translation += camera_right * side * 0.45;
        if index % 3 == 1 {
            shot.translation += camera_forward * 0.20;
        }
        shot.rotation = Quat::from_rotation_y(-side * 0.025) * base.rotation;
        frames.push(camera_keyframe(line.time_ms, &shot));
        frames.push(camera_keyframe(
            line.time_ms.saturating_add(line.duration_ms),
            &shot,
        ));
    }
    frames.push(camera_keyframe(duration_ms, base));
    frames.sort_by_key(|frame| frame.time_ms);
    frames.dedup_by(|right, left| {
        if right.time_ms == left.time_ms {
            *left = right.clone();
            true
        } else {
            false
        }
    });
    frames
}

fn interpolated_camera(frames: &[ReplayCameraKeyframe], time_ms: u64) -> Option<Transform> {
    let first = frames.first()?;
    let next_index = frames.partition_point(|frame| frame.time_ms <= time_ms);
    if next_index == 0 {
        return Some(frame_transform(first));
    }
    if next_index >= frames.len() {
        return Some(frame_transform(frames.last().unwrap()));
    }
    let left = &frames[next_index - 1];
    let right = &frames[next_index];
    let span = right.time_ms.saturating_sub(left.time_ms).max(1);
    let t = time_ms.saturating_sub(left.time_ms) as f32 / span as f32;
    let left_rotation = Quat::from_array(left.rotation).normalize();
    let right_rotation = Quat::from_array(right.rotation).normalize();
    Some(Transform {
        translation: Vec3::from_array(left.translation)
            .lerp(Vec3::from_array(right.translation), t),
        rotation: left_rotation.slerp(right_rotation, t),
        ..default()
    })
}

fn frame_transform(frame: &ReplayCameraKeyframe) -> Transform {
    Transform {
        translation: Vec3::from_array(frame.translation),
        rotation: Quat::from_array(frame.rotation).normalize(),
        ..default()
    }
}

fn dialogue_from_message(
    message: &CampaignMessage,
    manager: &NapcatMessageManager,
    time_ms: u64,
) -> Option<ReplayDialogue> {
    let text = message.text.trim();
    if text.is_empty() {
        return None;
    }
    let is_gm = manager
        .current_group()
        .is_some_and(|group| group.gm_users.contains(&message.sender_id));
    let character = manager
        .player_characters
        .get(&message.sender_id.to_string())
        .or_else(|| {
            if is_gm {
                None
            } else {
                message
                    .character_id
                    .as_ref()
                    .and_then(|character_id| manager.player_characters.get(character_id))
            }
        });
    let (name, role, avatar) = dialogue_identity(message, character);
    let avatar = resolve_character_image_source(manager, &avatar);
    let side = speaker_side(is_gm);
    Some(ReplayDialogue {
        time_ms,
        duration_ms: dialogue_duration_ms(text),
        sender_id: message.sender_id,
        name,
        role,
        text: text.to_owned(),
        avatar,
        avatar_data_url: None,
        visibility: message.visibility.clone(),
        side,
    })
}

fn resolve_character_image_source(manager: &NapcatMessageManager, source: &str) -> String {
    let source = source.trim();
    if source.is_empty() {
        return String::new();
    }
    if Path::new(source).exists() {
        return source.to_owned();
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        if let Some(local_path) = manager
            .messages
            .values()
            .flatten()
            .flat_map(|message| &message.data.message)
            .find_map(|segment| match &segment.variant {
                NapcatMessageChainType::Image { data }
                    if data.url.trim() == source && Path::new(data.local_path.trim()).exists() =>
                {
                    Some(data.local_path.trim().to_owned())
                },
                _ => None,
            })
        {
            return local_path;
        }
    }
    source.to_owned()
}

fn speaker_side(is_gm: bool) -> DialogueSide {
    if is_gm {
        DialogueSide::Left
    } else {
        DialogueSide::Right
    }
}

fn dialogue_identity(
    message: &CampaignMessage,
    character: Option<&PlayerCharacter>,
) -> (String, String, String) {
    let Some(character) = character else {
        return (
            message.sender_name.clone(),
            String::new(),
            String::new(),
        );
    };
    let name = if character.nickname.trim().is_empty() {
        if character.name.trim().is_empty() {
            message.sender_name.clone()
        } else {
            character.name.clone()
        }
    } else {
        character.nickname.clone()
    };
    let role = if !character.name.trim().is_empty() && character.name.trim() != name.trim() {
        character.name.clone()
    } else {
        String::new()
    };
    (name, role, character.image.clone())
}

fn dialogue_duration_ms(text: &str) -> u64 {
    let reading_ms = text.chars().fold(700_u64, |total, character| {
        let character_ms = match character {
            '。' | '！' | '？' | '!' | '?' | '；' | ';' => 220,
            '，' | ',' | '、' | '：' | ':' => 100,
            character if character.is_whitespace() => 15,
            character if is_cjk_character(character) => 105,
            _ => 45,
        };
        total.saturating_add(character_ms)
    });
    reading_ms.clamp(MIN_DIALOGUE_MS, MAX_DIALOGUE_MS)
}

fn is_cjk_character(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2FA1F
    )
}

fn active_dialogue(dialogue: &[ReplayDialogue], time_ms: u64) -> Option<&ReplayDialogue> {
    dialogue
        .iter()
        .rev()
        .find(|line| time_ms >= line.time_ms && time_ms < line.time_ms + line.duration_ms)
}

fn dialogue_overlay(
    ctx: &egui::Context,
    dialogue: &ReplayDialogue,
    textures: &mut HashMap<String, egui::TextureHandle>,
) {
    let screen = ctx.content_rect();
    let box_height = (screen.height() * 0.27).clamp(150.0, 290.0);
    let margin = (screen.width() * 0.035).clamp(24.0, 70.0);
    let dialogue_rect = egui::Rect::from_min_max(
        egui::pos2(
            screen.left() + margin,
            screen.bottom() - margin - box_height,
        ),
        egui::pos2(
            screen.right() - margin,
            screen.bottom() - margin,
        ),
    );
    let layer = egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("replay-dialogue"),
    );
    let painter = ctx.layer_painter(layer);
    let accent = speaker_accent(dialogue.sender_id);
    painter.rect_filled(
        dialogue_rect,
        0.0,
        egui::Color32::from_white_alpha(235),
    );
    painter.rect_stroke(
        dialogue_rect,
        0.0,
        egui::Stroke::new(2.0, egui::Color32::from_gray(25)),
        egui::StrokeKind::Inside,
    );

    let name_width = 225.0;
    let name_left = match dialogue.side {
        DialogueSide::Left => dialogue_rect.left() + 40.0,
        DialogueSide::Right => dialogue_rect.right() - name_width - 40.0,
    };
    let name_rect = egui::Rect::from_min_size(
        egui::pos2(name_left, dialogue_rect.top() - 102.0),
        egui::vec2(name_width, 66.0),
    );
    let avatar_width = (screen.width() * 0.24).clamp(200.0, 430.0);
    let avatar_left = name_rect.center().x - avatar_width * 0.5;
    let avatar_rect = egui::Rect::from_min_max(
        egui::pos2(avatar_left, screen.top() + 20.0),
        egui::pos2(
            avatar_left + avatar_width,
            name_rect.top() - 8.0,
        ),
    );
    if let Some(texture) = replay_avatar_texture(ctx, dialogue, textures) {
        let size = texture.size_vec2();
        let scale = (avatar_rect.width() / size.x)
            .min(avatar_rect.height() / size.y)
            .max(0.0);
        let fitted_size = size * scale;
        let fitted_rect = egui::Rect::from_min_size(
            egui::pos2(
                avatar_rect.center().x - fitted_size.x * 0.5,
                avatar_rect.bottom() - fitted_size.y,
            ),
            fitted_size,
        );
        painter.image(
            texture.id(),
            fitted_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        let radius = 72.0_f32
            .min(avatar_rect.width() * 0.30)
            .min(avatar_rect.height().max(1.0) * 0.45);
        let center = egui::pos2(
            avatar_rect.center().x,
            avatar_rect.bottom() - radius,
        );
        painter.circle_filled(center, radius, accent);
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            dialogue.name.chars().next().unwrap_or('角'),
            egui::FontId::proportional(54.0),
            egui::Color32::WHITE,
        );
    }

    painter.rect_filled(
        name_rect,
        0.0,
        egui::Color32::from_white_alpha(245),
    );
    painter.rect_stroke(
        name_rect,
        0.0,
        egui::Stroke::new(2.0, egui::Color32::from_gray(25)),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [name_rect.left_top(), name_rect.right_top()],
        egui::Stroke::new(6.0, accent),
    );
    painter.text(
        name_rect.center(),
        egui::Align2::CENTER_CENTER,
        &dialogue.name,
        egui::FontId::proportional(34.0),
        egui::Color32::from_gray(25),
    );
    if !dialogue.role.trim().is_empty() {
        let role_width = 180.0;
        let role_left = match dialogue.side {
            DialogueSide::Left => name_rect.left() + 20.0,
            DialogueSide::Right => name_rect.right() - role_width - 20.0,
        };
        let role_rect = egui::Rect::from_min_size(
            egui::pos2(role_left, name_rect.bottom()),
            egui::vec2(role_width, 38.0),
        );
        painter.rect_filled(role_rect, 0.0, accent);
        painter.text(
            role_rect.center(),
            egui::Align2::CENTER_CENTER,
            &dialogue.role,
            egui::FontId::proportional(21.0),
            egui::Color32::WHITE,
        );
    }
    let text_width = (dialogue_rect.width() - avatar_width * 0.45 - 90.0).max(240.0);
    let text_left = match dialogue.side {
        DialogueSide::Left => dialogue_rect.left() + avatar_width * 0.45 + 60.0,
        DialogueSide::Right => dialogue_rect.left() + 70.0,
    };
    let galley = painter.layout(
        dialogue.text.clone(),
        egui::FontId::proportional((screen.width() / 52.0).clamp(25.0, 42.0)),
        egui::Color32::from_gray(35),
        text_width,
    );
    painter.galley(
        egui::pos2(
            text_left,
            dialogue_rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        egui::Color32::from_gray(35),
    );
}

fn speaker_accent(sender_id: u64) -> egui::Color32 {
    const PALETTE: [(u8, u8, u8); 8] = [
        (67, 126, 181),
        (194, 91, 137),
        (45, 145, 137),
        (202, 132, 48),
        (126, 99, 181),
        (73, 145, 88),
        (196, 96, 70),
        (51, 139, 177),
    ];
    let mixed = sender_id ^ sender_id.rotate_right(23) ^ sender_id.rotate_left(17);
    let (red, green, blue) = PALETTE[mixed as usize % PALETTE.len()];
    egui::Color32::from_rgb(red, green, blue)
}

fn replay_avatar_texture(
    ctx: &egui::Context,
    dialogue: &ReplayDialogue,
    textures: &mut HashMap<String, egui::TextureHandle>,
) -> Option<egui::TextureHandle> {
    let key = dialogue
        .avatar_data_url
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(dialogue.avatar.as_str())
        .trim();
    if key.is_empty() {
        return None;
    }
    if let Some(texture) = textures.get(key) {
        return Some(texture.clone());
    }
    let bytes = if let Some((_, encoded)) = key.split_once(";base64,") {
        BASE64.decode(encoded).ok()?
    } else {
        let path = cached_or_local_voxel_standee_path(key).ok()?;
        fs::read(path).ok()?
    };
    let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    let texture = ctx.load_texture(
        format!("replay-avatar:{key}"),
        color_image,
        egui::TextureOptions::LINEAR,
    );
    textures.insert(key.to_owned(), texture.clone());
    Some(texture)
}

fn export_replay(replay: &ReplayFile, path: &str) -> Result<(), String> {
    let mut portable = replay.clone();
    for dialogue in &mut portable.dialogue {
        if dialogue.avatar_data_url.is_none() {
            dialogue.avatar_data_url = avatar_data_url(&dialogue.avatar);
        }
    }
    let bytes = serde_json::to_vec(&portable).map_err(|err| err.to_string())?;
    let path = normalized_path(path)?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(&path, bytes).map_err(|err| err.to_string())
}

fn import_replay(path: &str) -> Result<ReplayFile, String> {
    let path = normalized_path(path)?;
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    let replay: ReplayFile = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
    if replay.format_version != REPLAY_FORMAT_VERSION {
        return Err(format!(
            "不支持的回放版本 {}（当前支持 {}）",
            replay.format_version, REPLAY_FORMAT_VERSION
        ));
    }
    Ok(replay)
}

fn avatar_data_url(source: &str) -> Option<String> {
    let source = source.trim();
    if source.is_empty() {
        return None;
    }
    let path = cached_or_local_voxel_standee_path(source).ok()?;
    let bytes = fs::read(&path).ok()?;
    let mime = match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    Some(format!(
        "data:{mime};base64,{}",
        BASE64.encode(bytes)
    ))
}

fn normalized_path(path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("路径不能为空".to_owned());
    }
    Ok(PathBuf::from(path))
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn format_time(time_ms: u64) -> String {
    let seconds = time_ms / 1_000;
    format!(
        "{:02}:{:02}",
        seconds / 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_replay_excludes_private_dialogue() {
        assert!(ReplayAudience::Public.can_read_visibility(&Visibility::Public, None));
        assert!(!ReplayAudience::Public.can_read_visibility(&Visibility::Player(7), None));
        assert!(
            !ReplayAudience::Public.can_read_visibility(
                &Visibility::Party("split-a".to_owned()),
                None
            )
        );
    }

    #[test]
    fn party_replay_only_includes_its_party_and_public() {
        let audience = ReplayAudience::Party("split-a".to_owned());
        assert!(audience.can_read_visibility(&Visibility::Public, None));
        assert!(audience.can_read_visibility(
            &Visibility::Party("split-a".to_owned()),
            None
        ));
        assert!(!audience.can_read_visibility(
            &Visibility::Party("split-b".to_owned()),
            None
        ));
    }

    #[test]
    fn player_and_gm_replays_follow_access_rules() {
        let access = PlayerAccess {
            player_id: 7,
            party_id: Some("split-a".to_owned()),
            ..Default::default()
        };
        let player = ReplayAudience::Player(7);
        assert!(player.can_read_visibility(&Visibility::Player(7), Some(&access)));
        assert!(player.can_read_visibility(
            &Visibility::Party("split-a".to_owned()),
            Some(&access),
        ));
        assert!(!player.can_read_visibility(&Visibility::Player(8), Some(&access)));
        assert!(ReplayAudience::Gm.can_read_visibility(&Visibility::Gm, None));
        assert!(ReplayAudience::Gm.can_read_visibility(&Visibility::System, None));
    }

    #[test]
    fn camera_interpolation_is_smooth_and_clamped() {
        let frames = vec![
            ReplayCameraKeyframe {
                time_ms: 0,
                translation: [0.0, 0.0, 0.0],
                rotation: Quat::IDENTITY.to_array(),
            },
            ReplayCameraKeyframe {
                time_ms: 1_000,
                translation: [10.0, 0.0, 0.0],
                rotation: Quat::from_rotation_y(1.0).to_array(),
            },
        ];
        assert_eq!(
            interpolated_camera(&frames, 500).unwrap().translation.x,
            5.0
        );
        assert_eq!(
            interpolated_camera(&frames, 2_000).unwrap().translation.x,
            10.0
        );
    }

    #[test]
    fn automatic_director_tracks_speaker_side_and_returns_to_base() {
        let base = Transform::from_xyz(2.0, 3.0, 4.0);
        let dialogue = [
            test_dialogue(350, 2_400, DialogueSide::Left),
            test_dialogue(3_030, 2_400, DialogueSide::Right),
        ];
        let frames = automatic_camera_track(&base, &dialogue, 5_430);
        assert!(frames
            .windows(2)
            .all(|pair| pair[0].time_ms < pair[1].time_ms));
        assert_eq!(
            frames.first().unwrap().translation,
            base.translation.to_array()
        );
        assert_eq!(
            frames.last().unwrap().translation,
            base.translation.to_array()
        );
        let left = frames.iter().find(|frame| frame.time_ms == 350).unwrap();
        let right = frames.iter().find(|frame| frame.time_ms == 3_030).unwrap();
        assert!(left.translation[0] < base.translation.x);
        assert!(right.translation[0] > base.translation.x);
    }

    #[test]
    fn dialogue_duration_is_readable_and_bounded() {
        assert_eq!(
            dialogue_duration_ms("短句"),
            MIN_DIALOGUE_MS
        );
        assert_eq!(
            dialogue_duration_ms(&"长".repeat(1_000)),
            MAX_DIALOGUE_MS
        );
        assert!(dialogue_duration_ms("这是需要认真阅读的一句中文。") > MIN_DIALOGUE_MS);
        assert!(
            dialogue_duration_ms("等等！发生什么了？") > dialogue_duration_ms("等等发生什么了")
        );
        assert!(
            dialogue_duration_ms(
                "最近上班没以前忙碌了，做独立游戏的间隔可以让ai大人继续维护老项目了哈哈哈"
            ) < 6_000
        );
    }

    #[test]
    fn dm_is_composed_left_and_player_right() {
        assert_eq!(speaker_side(true), DialogueSide::Left);
        assert_eq!(speaker_side(false), DialogueSide::Right);
    }

    #[test]
    fn speaker_colors_are_stable_and_distinguish_people() {
        assert_eq!(
            speaker_accent(1_670_426_821),
            speaker_accent(1_670_426_821)
        );
        assert_ne!(
            speaker_accent(1_670_426_821),
            speaker_accent(2_383_680_235)
        );
    }

    #[test]
    fn video_timeline_uses_exact_frame_rate() {
        assert_eq!(video_frame_count(1_000, 30), 30);
        assert_eq!(video_frame_count(1, 30), 1);
        assert_eq!(frame_time_ms(15, 30), 500);
        assert_eq!(frame_file_name(12), "frame_000012.png");
    }

    #[test]
    fn historical_dialogue_transition_is_brief() {
        assert!(HISTORY_DIALOGUE_GAP_MS < 500);
    }

    #[test]
    fn ffmpeg_arguments_describe_numbered_png_input() {
        let arguments = ffmpeg_arguments(30);
        assert_eq!(arguments, [
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-framerate",
            "30",
            "-start_number",
            "0",
            "-i",
        ]);
    }

    fn test_dialogue(time_ms: u64, duration_ms: u64, side: DialogueSide) -> ReplayDialogue {
        ReplayDialogue {
            time_ms,
            duration_ms,
            sender_id: 1,
            name: "测试".to_owned(),
            role: String::new(),
            text: "台词".to_owned(),
            avatar: String::new(),
            avatar_data_url: None,
            visibility: Visibility::Public,
            side,
        }
    }
}
