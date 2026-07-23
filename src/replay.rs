#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    collections::{
        hash_map::DefaultHasher,
        HashMap,
        HashSet,
        VecDeque,
    },
    fs,
    hash::{
        Hash,
        Hasher,
    },
    io::{
        BufRead,
        BufReader,
        BufWriter,
        Write,
    },
    path::{
        Path,
        PathBuf,
    },
    process::{
        Child,
        ChildStdin,
        ChildStdout,
        Command,
        Stdio,
    },
    sync::{
        atomic::{
            AtomicU64,
            Ordering,
        },
        Arc,
        OnceLock,
    },
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
    unbounded,
    Receiver,
    Sender,
};
use rand::RngExt;
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
        DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS,
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
        VoxelPlayerStandee,
        VoxelViewportCamera,
    },
};

const REPLAY_FORMAT_VERSION: u32 = 1;
const CAMERA_SAMPLE_SECONDS: f32 = 0.1;
const MIN_DIALOGUE_MS: u64 = 2_700;
const MAX_DIALOGUE_MS: u64 = 9_750;
const HISTORY_DIALOGUE_GAP_MS: u64 = 270;
const DEFAULT_REPLAY_PATH: &str = ".data/willowblossom/replays/latest.willow-replay.json";
const DEFAULT_VIDEO_PATH: &str = ".data/willowblossom/replays/latest.mp4";
const BACKGROUND_MUSIC_PATH: &str = "assets/audio/jrpg2-piano.mp3";
const SPARK_TTS_RUNTIME_DIR: &str = ".data/willowblossom/tts/spark";
const SHORT_UTTERANCE_MAX_UNITS: usize = 6;
const SHORT_UTTERANCE_SPEED_CAP: f32 = 1.10;
const SHORT_UTTERANCE_HEAD_PAD_MS: u64 = 80;
const SHORT_UTTERANCE_TAIL_PAD_MS: u64 = 180;
const VIDEO_CAPTURE_WARMUP_FRAMES: u8 = 3;
const VIDEO_CAPTURE_TIMEOUT_SECONDS: f32 = 30.0;

pub struct ReplayPlugin;

impl Plugin for ReplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ReplayStudio>()
            .init_resource::<ReplayVideoCaptureActive>()
            .init_resource::<PreviewSpeechController>()
            .add_systems(
                Update,
                (
                    record_replay,
                    advance_replay,
                    preview_replay_speech.after(advance_replay),
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
    #[serde(default = "default_master_speech_speed")]
    master_speech_speed: f32,
    #[serde(default = "default_master_dialogue_duration")]
    master_dialogue_duration: f32,
    #[serde(default)]
    speaker_voice_settings: HashMap<u64, SpeakerVoiceSettings>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_text: Option<String>,
    avatar: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avatar_data_url: Option<String>,
    visibility: Visibility,
    #[serde(default)]
    side: DialogueSide,
}

#[derive(Debug, Clone, Deserialize)]
struct DirectorPlan {
    dialogue: Vec<DirectorCue>,
}

#[derive(Debug, Clone, Deserialize)]
struct DirectorCue {
    index: usize,
    text: String,
    #[serde(default)]
    speech_text: String,
    shot: DirectorShot,
    motion: DirectorMotion,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DirectorShot {
    SpeakerClose,
    SpeakerMedium,
    SpeakerWide,
    Establishing,
    Environment,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DirectorMotion {
    Static,
    DollyIn,
    DollyOut,
    DriftLeft,
    DriftRight,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpeakerVoiceSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    voice_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    emotion: Option<String>,
    #[serde(default)]
    onnx_speaker_id: Option<i32>,
    #[serde(default)]
    pitch: i32,
    speech_rate: i32,
    volume: f32,
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
    record_camera_enabled: bool,
    deepseek_director_enabled: bool,
    director_response_hash: Option<u64>,
    director_request_pending: bool,
    auto_export_after_director: bool,
    camera_sample_accumulator: f32,
    message_counts: HashMap<String, usize>,
    pre_playback_scene: Option<ReplayScene>,
    video_path: String,
    video_fps: u32,
    music_enabled: bool,
    music_volume: f32,
    speech_enabled: bool,
    speech_volume: f32,
    speech_settings_open: bool,
    deepseek_custom_prompt: String,
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
    duration_ms: u64,
    music_enabled: bool,
    music_volume: f32,
    speech_enabled: bool,
    speech_volume: f32,
    master_speech_speed: f32,
    speaker_voice_settings: HashMap<u64, SpeakerVoiceSettings>,
    dialogue: Vec<ReplayDialogue>,
    total_frames: u64,
    next_frame: u64,
    capture_pending: bool,
    pending_seconds: f32,
    warmup_frames: u8,
    monitor_music_started: bool,
    monitor_music_entity: Option<Entity>,
    failure: Option<String>,
    original_window_title: String,
    original_window_resizable: bool,
}

struct VideoEncodingJob {
    _frames: TempDir,
    output_path: PathBuf,
    result: Receiver<Result<(), String>>,
}

#[derive(Resource, Default)]
struct PreviewSpeechController {
    active_cue: Option<(u64, usize)>,
    prepared_signature: Option<u64>,
    onnx_cache: HashMap<(u64, usize), (Vec<u8>, f32)>,
    onnx_worker: Option<OnnxPreviewWorker>,
    onnx_failed: bool,
    audio_entity: Option<Entity>,
}

struct OnnxPreviewWorker {
    requests: Sender<OnnxPreviewRequest>,
    results: Receiver<OnnxPreviewResult>,
    latest_signature: Arc<AtomicU64>,
}

struct OnnxPreviewRequest {
    signature: u64,
    cue: (u64, usize),
    text: String,
    speaker: String,
    emotion: String,
    speed: f32,
    volume: f32,
}

struct OnnxPreviewResult {
    signature: u64,
    cue: (u64, usize),
    wav: Result<Vec<u8>, String>,
    volume: f32,
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
            record_camera_enabled: false,
            deepseek_director_enabled: false,
            director_response_hash: None,
            director_request_pending: false,
            auto_export_after_director: false,
            camera_sample_accumulator: 0.0,
            message_counts: HashMap::new(),
            pre_playback_scene: None,
            video_path: DEFAULT_VIDEO_PATH.to_owned(),
            video_fps: 15,
            music_enabled: true,
            music_volume: 0.65,
            speech_enabled: true,
            speech_volume: 1.25,
            speech_settings_open: false,
            deepseek_custom_prompt: String::new(),
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
    standees: Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
    mut studio: ResMut<ReplayStudio>,
) {
    if studio.mode != ReplayMode::Recording {
        return;
    }

    let delta_seconds = time.delta_secs();
    studio.record_elapsed_ms = studio
        .record_elapsed_ms
        .saturating_add((delta_seconds * 1_000.0).round() as u64);
    if studio.record_camera_enabled {
        studio.camera_sample_accumulator += delta_seconds;
    }

    if studio.record_camera_enabled && studio.camera_sample_accumulator >= CAMERA_SAMPLE_SECONDS {
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
    let speaker_positions = standee_positions(&standees);
    let mut next_turn_ms = studio
        .replay
        .as_ref()
        .and_then(|replay| replay.dialogue.last())
        .map(|line| {
            line.time_ms
                .saturating_add(line.duration_ms)
                .saturating_add(HISTORY_DIALOGUE_GAP_MS)
        })
        .unwrap_or(350);
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
                    next_turn_ms,
                ) {
                    next_turn_ms = dialogue
                        .time_ms
                        .saturating_add(dialogue.duration_ms)
                        .saturating_add(HISTORY_DIALOGUE_GAP_MS);
                    captured.push(dialogue);
                }
            }
        }
        studio.message_counts.insert(target_id, messages.len());
    }
    let record_elapsed_ms = studio.record_elapsed_ms;
    let record_camera_enabled = studio.record_camera_enabled;
    if let Some(replay) = studio.replay.as_mut() {
        let captured_any = !captured.is_empty();
        replay.dialogue.extend(captured);
        retain_dialogue_with_standees(&mut replay.dialogue, &speaker_positions);
        spatially_order_dialogue_turns(&mut replay.dialogue, &speaker_positions);
        let dialogue_end = retime_dialogue_turns(&mut replay.dialogue);
        replay.duration_ms = if record_camera_enabled {
            record_elapsed_ms.max(dialogue_end)
        } else {
            dialogue_end
        };
        if !record_camera_enabled {
            extend_replay_for_speech(replay);
        }
        if captured_any && !record_camera_enabled {
            if let Ok(base) = camera.single() {
                replay.camera = turn_based_camera_track(
                    base,
                    &replay.dialogue,
                    replay.duration_ms,
                    &speaker_positions,
                );
            }
        }
    }
}

fn advance_replay(
    time: Res<Time>,
    speech: Res<PreviewSpeechController>,
    mut studio: ResMut<ReplayStudio>,
) {
    if studio.mode != ReplayMode::Playing {
        return;
    }
    let Some(duration_ms) = studio.replay.as_ref().map(|replay| replay.duration_ms) else {
        studio.mode = ReplayMode::Idle;
        return;
    };
    let delta_ms = (time.delta_secs() * studio.playback_speed * 1_000.0).round() as u64;
    let proposed_ms = studio.playback_ms.saturating_add(delta_ms).min(duration_ms);
    if studio.speech_enabled && onnx_tts_is_available() && !speech.onnx_failed {
        if let Some(replay) = studio.replay.as_ref() {
            let current = active_dialogue_index(&replay.dialogue, studio.playback_ms);
            let proposed = active_dialogue_index(&replay.dialogue, proposed_ms);
            if proposed != current {
                if let Some(index) = proposed {
                    let signature = replay_voice_signature(replay, studio.speech_volume);
                    if !speech.onnx_cue_ready(
                        signature,
                        (replay.created_at_unix_ms, index),
                    ) {
                        return;
                    }
                }
            } else if studio.playback_ms == 0 {
                if let Some(index) = proposed {
                    let signature = replay_voice_signature(replay, studio.speech_volume);
                    if !speech.onnx_cue_ready(
                        signature,
                        (replay.created_at_unix_ms, index),
                    ) {
                        return;
                    }
                }
            }
        }
    }
    studio.playback_ms = proposed_ms;
    if studio.playback_ms >= duration_ms {
        studio.mode = ReplayMode::Paused;
    }
}

fn preview_replay_speech(
    mut commands: Commands,
    studio: Res<ReplayStudio>,
    mut speech: ResMut<PreviewSpeechController>,
    mut audio_sources: ResMut<Assets<AudioSource>>,
) {
    if studio.speech_enabled && onnx_tts_is_available() {
        if let Some(replay) = studio.replay.as_ref() {
            if let Err(err) = speech.prepare_onnx_replay(replay, studio.speech_volume) {
                speech.onnx_failed = true;
                eprintln!("failed to prepare Spark-TTS preview speech: {err}");
            }
        }
    }
    let active = ((studio.mode == ReplayMode::Playing || studio.video_render.is_some())
        && studio.speech_enabled)
        .then(|| {
            studio.replay.as_ref().and_then(|replay| {
                active_dialogue_index(&replay.dialogue, studio.playback_ms).map(|index| {
                    (
                        replay.created_at_unix_ms,
                        index,
                        &replay.dialogue[index],
                    )
                })
            })
        })
        .flatten();
    let cue = active.map(|(replay_id, index, _)| (replay_id, index));
    let results = speech
        .onnx_worker
        .as_ref()
        .map(|worker| worker.results.try_iter().collect::<Vec<_>>())
        .unwrap_or_default();
    let mut newly_ready_current = false;
    for result in results {
        if Some(result.signature) != speech.prepared_signature {
            continue;
        }
        match result.wav {
            Ok(wav) => {
                newly_ready_current |= Some(result.cue) == cue;
                speech.onnx_cache.insert(result.cue, (wav, result.volume));
            },
            Err(err) => {
                speech.onnx_failed = true;
                eprintln!("failed to synthesize Spark-TTS preview speech: {err}");
            },
        }
    }
    let cue_changed = speech.active_cue != cue;
    if !cue_changed && !newly_ready_current {
        return;
    }

    if cue_changed {
        if let Some(entity) = speech.audio_entity.take() {
            commands.entity(entity).try_despawn();
        }
        speech.active_cue = cue;
    }
    let Some((_, _, _line)) = active else { return };
    let active_cue = cue.expect("active dialogue has a cue");
    if let Some((wav, volume)) = speech.onnx_cache.get(&active_cue).cloned() {
        let source = audio_sources.add(AudioSource {
            bytes: Arc::from(wav),
        });
        speech.audio_entity = Some(
            commands
                .spawn((
                    AudioPlayer::new(source),
                    PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(volume)),
                ))
                .id(),
        );
    }
}

impl PreviewSpeechController {
    fn prepare_onnx_replay(
        &mut self,
        replay: &ReplayFile,
        global_volume: f32,
    ) -> Result<(), String> {
        let signature = replay_voice_signature(replay, global_volume);
        if self.prepared_signature == Some(signature) {
            return Ok(());
        }
        if self.onnx_worker.is_none() {
            self.onnx_worker = Some(start_onnx_preview_worker()?);
        }
        self.prepared_signature = Some(signature);
        self.onnx_cache.clear();
        self.active_cue = None;
        let worker = self
            .onnx_worker
            .as_ref()
            .expect("ONNX worker was initialized");
        worker.latest_signature.store(signature, Ordering::Release);
        for (index, line) in replay.dialogue.iter().enumerate() {
            let settings = replay
                .speaker_voice_settings
                .get(&line.sender_id)
                .cloned()
                .unwrap_or_else(|| default_speaker_voice_settings(line.sender_id));
            worker
                .requests
                .send(OnnxPreviewRequest {
                    signature,
                    cue: (replay.created_at_unix_ms, index),
                    text: speech_text_for_line(line),
                    speaker: resolved_emotivoice_speaker(
                        settings.voice_name.as_deref(),
                        line.sender_id,
                    ),
                    emotion: resolved_emotivoice_emotion(settings.emotion.as_deref()).to_owned(),
                    speed: combined_onnx_speed(
                        settings.speech_rate,
                        replay.master_speech_speed,
                    ),
                    volume: (global_volume * settings.volume).max(0.0),
                })
                .map_err(|err| format!("Spark-TTS preview worker stopped: {err}"))?;
        }
        Ok(())
    }

    fn onnx_cue_ready(&self, signature: u64, cue: (u64, usize)) -> bool {
        self.prepared_signature == Some(signature) && self.onnx_cache.contains_key(&cue)
    }
}

fn onnx_tts_is_available() -> bool {
    emotivoice_python_path().is_file()
        && emotivoice_worker_path().is_file()
        && emotivoice_source_path().is_dir()
        && emotivoice_model_path().is_dir()
        && emotivoice_voice_bank_path().join("profiles.json").is_file()
        && SPARK_TTS_VOICE_PROFILES
            .iter()
            .all(|(profile, _)| {
                emotivoice_voice_bank_path()
                    .join(format!("{profile}.wav"))
                    .is_file()
            })
}

fn emotivoice_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(SPARK_TTS_RUNTIME_DIR)
}

fn emotivoice_python_path() -> PathBuf {
    let root = emotivoice_root().join(".venv");
    if cfg!(windows) {
        root.join("Scripts").join("python.exe")
    } else {
        root.join("bin").join("python")
    }
}

fn emotivoice_source_path() -> PathBuf { emotivoice_root().join("Spark-TTS") }

fn emotivoice_model_path() -> PathBuf { emotivoice_root().join("Spark-TTS-0.5B") }

fn emotivoice_voice_bank_path() -> PathBuf { emotivoice_root().join("voice-bank") }

fn emotivoice_worker_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("sparktts_worker.py")
}

struct SparkTts {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    cache: TempDir,
    sequence: u64,
}

impl Drop for SparkTts {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn create_onnx_tts() -> Result<SparkTts, String> {
    if !onnx_tts_is_available() {
        return Err(
            "未安装 Spark-TTS 中文运行环境或 32 音色库；请运行 scripts/setup_sparktts.ps1"
                .to_owned(),
        );
    }
    let cache_root = emotivoice_root().join("worker-cache");
    fs::create_dir_all(&cache_root)
        .map_err(|err| format!("无法创建 Spark-TTS 缓存目录：{err}"))?;
    let cache = tempfile::Builder::new()
        .prefix("worker-")
        .tempdir_in(&cache_root)
        .map_err(|err| format!("无法创建 Spark-TTS 临时目录：{err}"))?;
    let log = fs::File::create(cache.path().join("sparktts.log"))
        .map_err(|err| format!("无法创建 Spark-TTS 日志：{err}"))?;
    let mut command = Command::new(emotivoice_python_path());
    command
        .arg(emotivoice_worker_path())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env(
            "SPARK_TTS_SOURCE",
            emotivoice_source_path(),
        )
        .env("SPARK_TTS_MODEL", emotivoice_model_path())
        .env("SPARK_TTS_VOICE_BANK", emotivoice_voice_bank_path())
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    hide_command_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|err| format!("无法启动 Spark-TTS 中文进程：{err}"))?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| "Spark-TTS 没有打开输入流".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Spark-TTS 没有打开输出流".to_owned())?;
    let mut output = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        if output
            .read_line(&mut line)
            .map_err(|err| format!("读取 Spark-TTS 启动状态失败：{err}"))?
            == 0
        {
            return Err("Spark-TTS 在模型加载完成前退出，请查看 worker-cache 中的日志".to_owned());
        }
        let Ok(status) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if status.get("ready").and_then(|value| value.as_bool()) == Some(true) {
            break;
        }
        if status.get("ready").and_then(|value| value.as_bool()) == Some(false) {
            return Err(status["error"]
                .as_str()
                .unwrap_or("Spark-TTS 中文模型加载失败")
                .to_owned());
        }
    }
    Ok(SparkTts {
        child,
        input,
        output,
        cache,
        sequence: 0,
    })
}

impl SparkTts {
    fn synthesize(
        &mut self,
        text: &str,
        speaker: &str,
        emotion: &str,
        speed: f32,
    ) -> Result<Vec<u8>, String> {
        let normalized_text = emotivoice_model_text(text);
        if normalized_text.is_empty() {
            return Err("台词中没有可朗读的中文文字".to_owned());
        }
        let raw_path = self
            .cache
            .path()
            .join(format!("raw-{:06}.wav", self.sequence));
        let output_path = self.cache.path().join(format!(
            "voice-{:06}.wav",
            self.sequence
        ));
        self.sequence = self.sequence.saturating_add(1);
        let request = serde_json::json!({
            "text": normalized_text,
            "speaker": speaker,
            "emotion": emotion,
            "speed": speed.max(0.10),
            "output_path": raw_path,
        });
        serde_json::to_writer(&mut self.input, &request)
            .and_then(|_| self.input.write_all(b"\n").map_err(serde_json::Error::io))
            .and_then(|_| self.input.flush().map_err(serde_json::Error::io))
            .map_err(|err| format!("发送 Spark-TTS 台词失败：{err}"))?;
        let mut response = String::new();
        self.output
            .read_line(&mut response)
            .map_err(|err| format!("读取 Spark-TTS 结果失败：{err}"))?;
        let response: serde_json::Value = serde_json::from_str(&response)
            .map_err(|err| format!("Spark-TTS 返回了无效结果：{err}"))?;
        if response.get("ok").and_then(|value| value.as_bool()) != Some(true) {
            return Err(response["error"]
                .as_str()
                .unwrap_or("Spark-TTS 中文合成失败")
                .to_owned());
        }
        let mut command = Command::new("ffmpeg");
        let tempo_filter = emotivoice_audio_filter(&normalized_text, speed);
        command
            .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(&raw_path)
            .args([
                "-af",
                &tempo_filter,
                "-ar",
                "32000",
                "-ac",
                "1",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&output_path);
        hide_command_window(&mut command);
        let output = command
            .output()
            .map_err(|err| format!("无法转换 Spark-TTS 角色语音：{err}"))?;
        let _ = fs::remove_file(&raw_path);
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        let wav =
            fs::read(&output_path).map_err(|err| format!("无法读取 Spark-TTS WAV：{err}"))?;
        let _ = fs::remove_file(&output_path);
        Ok(wav)
    }
}

fn ffmpeg_atempo_filter(speed: f32) -> String {
    let mut remaining = if speed.is_finite() { speed.max(0.10) } else { 1.0 };
    let mut factors = Vec::new();
    while remaining > 2.0 {
        factors.push(2.0);
        remaining /= 2.0;
    }
    while remaining < 0.5 {
        factors.push(0.5);
        remaining /= 0.5;
    }
    factors.push(remaining);
    factors
        .into_iter()
        .map(|factor| format!("atempo={factor:.6}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn speech_unit_count(text: &str) -> usize {
    text.chars()
        .filter(|character| is_cjk_character(*character) || character.is_ascii_alphanumeric())
        .count()
}

fn is_short_utterance(text: &str) -> bool {
    let units = speech_unit_count(text);
    units > 0 && units <= SHORT_UTTERANCE_MAX_UNITS
}

fn protect_repeated_short_phrase(text: &str) -> String {
    let mut characters = text.chars().collect::<Vec<_>>();
    let ending = characters
        .last()
        .copied()
        .filter(|character| matches!(character, '。' | '！' | '？' | '.' | '!' | '?'));
    if ending.is_some() {
        characters.pop();
    }
    let half = characters.len() / 2;
    if characters.len() >= 4
        && characters.len() <= SHORT_UTTERANCE_MAX_UNITS
        && characters.len() % 2 == 0
        && characters.iter().all(|character| is_cjk_character(*character))
        && characters[..half] == characters[half..]
    {
        characters.insert(half, '，');
    }
    if let Some(ending) = ending {
        characters.push(ending);
    }
    characters.into_iter().collect()
}

fn emotivoice_model_text(text: &str) -> String {
    let mut normalized = protect_repeated_short_phrase(&normalize_tts_text(text));
    if is_short_utterance(&normalized)
        && !normalized.chars().next_back().is_some_and(|character| {
            matches!(
                character,
                '。' | '！' | '？' | '.' | '!' | '?'
            )
        })
    {
        normalized.push('。');
    }
    normalized
}

fn effective_emotivoice_speed(text: &str, configured_speed: f32) -> f32 {
    let configured_speed =
        if configured_speed.is_finite() { configured_speed.max(0.10) } else { 1.0 };
    if is_short_utterance(text) {
        configured_speed.min(SHORT_UTTERANCE_SPEED_CAP)
    } else {
        configured_speed
    }
}

fn emotivoice_audio_filter(text: &str, configured_speed: f32) -> String {
    let tempo = ffmpeg_atempo_filter(effective_emotivoice_speed(
        text,
        configured_speed,
    ));
    if is_short_utterance(text) {
        format!(
            "adelay={SHORT_UTTERANCE_HEAD_PAD_MS},{tempo},apad=pad_dur={:.3}",
            SHORT_UTTERANCE_TAIL_PAD_MS as f32 / 1_000.0
        )
    } else {
        tempo
    }
}

fn start_onnx_preview_worker() -> Result<OnnxPreviewWorker, String> {
    let (request_tx, request_rx) = unbounded::<OnnxPreviewRequest>();
    let (result_tx, result_rx) = unbounded::<OnnxPreviewResult>();
    let latest_signature = Arc::new(AtomicU64::new(0));
    let worker_signature = Arc::clone(&latest_signature);
    thread::Builder::new()
        .name("replay-sparktts-preview".to_owned())
        .spawn(move || {
            let mut tts = create_onnx_tts();
            while let Ok(request) = request_rx.recv() {
                if request.signature != worker_signature.load(Ordering::Acquire) {
                    continue;
                }
                let wav = tts.as_mut().map_err(|err| err.clone()).and_then(|tts| {
                    tts.synthesize(
                        &request.text,
                        &request.speaker,
                        &request.emotion,
                        request.speed,
                    )
                });
                if request.signature != worker_signature.load(Ordering::Acquire) {
                    continue;
                }
                if result_tx
                    .send(OnnxPreviewResult {
                        signature: request.signature,
                        cue: request.cue,
                        wav,
                        volume: request.volume,
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .map_err(|err| format!("无法启动 Spark-TTS 预览线程：{err}"))?;
    Ok(OnnxPreviewWorker {
        requests: request_tx,
        results: result_rx,
        latest_signature,
    })
}

fn render_video_frames(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut audio_sources: ResMut<Assets<AudioSource>>,
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
        if let Some(entity) = job.monitor_music_entity.take() {
            commands.entity(entity).try_despawn();
        }
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
    if !job.monitor_music_started {
        job.monitor_music_started = true;
        if job.music_enabled {
            let monitor_path = job.frames.path().join("render-monitor-music.wav");
            match write_jrpg_soundtrack(
                &monitor_path,
                job.duration_ms,
                job.music_volume,
            )
            .and_then(|_| {
                fs::read(&monitor_path).map_err(|err| format!("无法读取渲染监听音乐：{err}"))
            }) {
                Ok(wav) => {
                    let source = audio_sources.add(AudioSource {
                        bytes: Arc::from(wav),
                    });
                    job.monitor_music_entity = Some(
                        commands
                            .spawn((
                                AudioPlayer::new(source),
                                PlaybackSettings::DESPAWN,
                            ))
                            .id(),
                    );
                },
                Err(err) => eprintln!("failed to start render monitor music: {err}"),
            }
        }
    }
    if job.next_frame >= job.total_frames {
        if let Some(entity) = job.monitor_music_entity.take() {
            commands.entity(entity).try_despawn();
        }
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
            "正在渲染视频 {}/{}（同步监听音乐和角色语音；Esc 取消）",
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
    let duration_ms = job.duration_ms;
    let music_enabled = job.music_enabled;
    let music_volume = job.music_volume;
    let speech_enabled = job.speech_enabled;
    let speech_volume = job.speech_volume;
    let master_speech_speed = job.master_speech_speed;
    let speaker_voice_settings = job.speaker_voice_settings;
    let dialogue = job.dialogue;
    let (sender, receiver) = bounded(1);
    thread::spawn(move || {
        let result = encode_video_frames(
            &frames_path,
            &output_path,
            fps,
            duration_ms,
            music_enabled,
            music_volume,
            speech_enabled,
            speech_volume,
            master_speech_speed,
            &dialogue,
            &speaker_voice_settings,
        );
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
    standees: Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
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
        let max_window_width = (ctx.content_rect().width() - 32.0).clamp(360.0, 620.0);
        egui::Window::new("TRPG 回放工作室")
            .id(egui::Id::new("trpg-replay-studio"))
            .open(&mut open)
            .default_width(390.0)
            .min_width(360.0)
            .max_width(max_window_width)
            .show(ctx, |ui| {
                ui.set_max_width(max_window_width);
                replay_controls(
                    ui,
                    &manager,
                    deepseek_sender.as_deref(),
                    &mut deepseek_manager,
                    &mut studio,
                    &camera,
                    &standees,
                    &mut grids,
                    &mut windows,
                    &mut capture_active,
                )
            });
        studio.panel_open = open;
    }

    if studio.speech_settings_open && !capture_active.0 {
        speech_settings_window(ctx, &mut studio);
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
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
    capture_active: &mut ReplayVideoCaptureActive,
) {
    ui.label("记录体素场景和可见对话，并在应用内确定性回放。");
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
    ui.checkbox(
        &mut studio.record_camera_enabled,
        "录制 DM 自由镜头（默认关闭，点击后才采集）",
    )
    .on_hover_text("关闭时只记录场景和台词，不持续采集你的镜头移动。");

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
                build_from_history(studio, manager, camera, standees, grids);
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
    ui.heading("DeepSeek 视频导演");
    ui.small("使用 deepseek-v4-pro 思考模式，润色当前发布范围内且场景中有立牌的玩家台词，并为每句选择镜头。回合内会从最早发言者开始，再按立牌距离依次访问最近玩家；镜头从台词第一刻起持续对准说话者。它还会生成只供 Spark-TTS 使用的中文谐音读法，画面字幕仍显示正常原文。不会读取其他队伍的隐藏内容，也不得新增剧情事实。");
    ui.checkbox(
        &mut studio.deepseek_director_enabled,
        "允许 DeepSeek 润色台词并控制镜头",
    );
    ui.label("自定义导演要求");
    let prompt_width = ui.available_width().clamp(220.0, 560.0);
    ui.add(
        egui::TextEdit::multiline(&mut studio.deepseek_custom_prompt)
            .desired_rows(3)
            .desired_width(prompt_width)
            .hint_text("例如：保留角色口癖；战斗段落使用快速切镜；安静段落多用环境远景"),
    );
    let prompt_chars = studio.deepseek_custom_prompt.chars().count();
    if prompt_chars > DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS {
        ui.colored_label(
            egui::Color32::from_rgb(210, 90, 70),
            format!("已输入 {prompt_chars} 字；仅发送前 {DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS} 字"),
        );
    } else {
        ui.small(format!(
            "{prompt_chars}/{DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS} 字；可影响措辞和镜头风格，不能扩大可见范围或新增剧情"
        ));
    }
    if ui
        .add_enabled(
            studio.deepseek_director_enabled
                && studio
                    .replay
                    .as_ref()
                    .is_some_and(|replay| !replay.dialogue.is_empty()),
            egui::Button::new("生成并应用导演方案"),
        )
        .clicked()
    {
        studio.director_response_hash = None;
        studio.status = match studio
            .replay
            .as_ref()
            .ok_or_else(|| "请先从现有聊天生成回放".to_owned())
            .and_then(|replay| {
                queue_replay_director(
                    replay,
                    deepseek_sender,
                    deepseek_manager,
                    &studio.deepseek_custom_prompt,
                    standees,
                )
            }) {
            Ok(()) => {
                studio.director_request_pending = true;
                if let Err(err) = deepseek_manager.persist() {
                    format!("DeepSeek 请求已发送，但保存请求状态失败：{err}")
                } else {
                    "DeepSeek 正在润色台词并设计逐句镜头；返回后会自动应用".to_owned()
                }
            },
            Err(err) => {
                studio.director_request_pending = false;
                format!("DeepSeek 导演请求失败：{err}")
            },
        };
    }
    if let Some(replay) = studio.replay.as_ref() {
        if let Some(block) = replay_summary_block(replay, deepseek_manager) {
            if block.pending {
                ui.spinner();
                ui.small("正在生成导演方案……");
            } else if let Some(error) = &block.error {
                ui.colored_label(
                    egui::Color32::from_rgb(210, 90, 70),
                    error,
                );
            } else if !block.latest.trim().is_empty() {
                ui.group(|ui| {
                    ui.label("DeepSeek 导演方案（已验证后自动应用）");
                });
                ui.collapsing(
                    "查看 DeepSeek API 原始响应",
                    |ui| {
                        ui.monospace(&block.latest);
                        if ui.button("复制原始响应").clicked() {
                            ui.ctx().copy_text(block.latest.clone());
                        }
                        ui.small("显示 API 返回的 message.content；不会显示 API 密钥或思考过程。");
                    },
                );
            }
        }
    }
    let director_applied = match apply_ready_director_plan(
        studio,
        deepseek_manager,
        camera,
        standees,
    ) {
        Ok(applied) => applied,
        Err(err) => {
            studio.auto_export_after_director = false;
            studio.status = format!("DeepSeek 导演方案无效：{err}");
            false
        },
    };
    if director_applied && studio.auto_export_after_director {
        studio.auto_export_after_director = false;
        start_video_export(studio, capture_active, grids, windows);
    }
    ui.small("只有开启“录制 DM 自由镜头”才会持续采集镜头。DeepSeek 导演开启后会等待 API 返回，再应用润色台词和镜头决策。");
    ui.small(
        "较长回放会自动分批交给 DeepSeek，再按原台词顺序合并；单个批次若被截断会继续拆分重试。",
    );

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
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut studio.music_enabled,
            "CC0 二次元 JRPG 钢琴音乐",
        );
        ui.add_enabled(
            studio.music_enabled,
            egui::Slider::new(&mut studio.music_volume, 0.05..=1.50)
                .text("音量")
                .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
        );
    });
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut studio.speech_enabled,
            "角色语音（预览与导出，Spark-TTS 中文离线语音）",
        );
        ui.add_enabled(
            studio.speech_enabled,
            egui::Slider::new(&mut studio.speech_volume, 0.20..=2.00)
                .text("语音音量")
                .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
        );
        if let Some(replay) = studio.replay.as_mut() {
            ui.add_enabled_ui(studio.speech_enabled, |ui| {
                ui.label("整体语速");
                let changed = ui
                    .add(
                        egui::DragValue::new(&mut replay.master_speech_speed)
                            .speed(0.05)
                            .range(0.10..=f32::INFINITY)
                            .fixed_decimals(2)
                            .suffix("×"),
                    )
                    .on_hover_text("同时调整所有角色在播放预览和 MP4 导出中的语速；没有上限。")
                    .changed();
                if changed {
                    extend_replay_for_speech(replay);
                }
            });
        }
        if let Some(replay) = studio.replay.as_mut() {
            let previous_duration = replay.master_dialogue_duration;
            ui.label("整体台词停留");
            let changed = ui
                .add(
                    egui::DragValue::new(&mut replay.master_dialogue_duration)
                        .speed(0.05)
                        .range(0.10..=f32::INFINITY)
                        .fixed_decimals(2)
                        .suffix("×"),
                )
                .on_hover_text("统一缩放字幕停留时间、台词间隔和导演镜头时间；没有上限。")
                .changed();
            if changed {
                let new_duration = replay.master_dialogue_duration;
                retime_replay(replay, previous_duration, new_duration);
                extend_replay_for_speech(replay);
                studio.playback_ms = studio.playback_ms.min(replay.duration_ms);
            }
        }
        if ui.button("角色语音设置…").clicked() {
            studio.speech_settings_open = true;
        }
    });
    ui.small("整体语速默认 1.30×；长台词不会自动加速，而会自动延长字幕和镜头时间，确保角色保持固定语速。六个字以内的极短台词会自动使用较自然的短句语速和首尾保护，避免吞字，不改变角色音色或音调。整体台词停留默认 1.00×，可无上限延长字幕、间隔和对应镜头。预览与导出共用同一条时间线和 Spark-TTS 中文语音。DeepSeek 另行生成只供发音使用的中文谐音文本，画面仍显示正常中英文原文。所有语音均在本机生成，不上传网络。");
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
    let has_applied_director_plan =
        studio.replay.is_some() && studio.director_response_hash.is_some();
    let can_auto_direct = can_start_director_export(
        studio,
        manager.active_campaign_id().is_some(),
    );
    let director_export_tooltip = if has_applied_director_plan {
        "当前 DeepSeek 导演方案已经应用；点击后直接导出，不会重复请求 API。"
    } else {
        "从所选发布范围的聊天重建回放，等待 DeepSeek 返回润色台词和逐句镜头方案，验证并应用后再导出 MP4。"
    };
    if ui
        .add_enabled(
            can_auto_direct,
            egui::Button::new("DeepSeek 导演并导出"),
        )
        .on_hover_text(director_export_tooltip)
        .clicked()
    {
        if has_applied_director_plan {
            start_video_export(studio, capture_active, grids, windows);
            return;
        }
        if matches!(
            studio.mode,
            ReplayMode::Playing | ReplayMode::Paused
        ) {
            stop_playback(studio, grids);
        }
        build_from_history(studio, manager, camera, standees, grids);
        let director_queued = studio.replay.as_ref().is_some_and(|replay| {
            queue_replay_director(
                replay,
                deepseek_sender,
                deepseek_manager,
                &studio.deepseek_custom_prompt,
                standees,
            )
            .is_ok()
        });
        if director_queued {
            studio.director_response_hash = None;
            studio.director_request_pending = true;
            studio.auto_export_after_director = true;
            studio.status = "已发送 DeepSeek 导演请求；收到并应用有效方案后自动开始导出".to_owned();
            let _ = deepseek_manager.persist();
        } else {
            studio.director_request_pending = false;
            studio.auto_export_after_director = false;
            studio.status = "无法发送 DeepSeek 导演请求，请检查 API 连接".to_owned();
        }
    }
    ui.small("一键模式会自动完成：读取可见聊天 → DeepSeek 润色逐句台词并选择镜头 → 本地验证和生成平滑轨迹 → 逐帧渲染 → FFmpeg 输出 MP4。");
    ui.small("导出时会隐藏编辑器界面，逐帧渲染台词层，再用 FFmpeg 编码 H.264/AAC 视频。按 Esc 可取消逐帧渲染。");

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
                    Ok(mut replay) => {
                        normalize_dialogue_sides(&mut replay, manager);
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

fn can_start_director_export(studio: &ReplayStudio, has_active_campaign: bool) -> bool {
    studio.deepseek_director_enabled
        && studio.mode != ReplayMode::Recording
        && studio.video_render.is_none()
        && studio.video_encoding.is_none()
        && !studio.director_request_pending
        && ((studio.replay.is_some() && studio.director_response_hash.is_some())
            || has_active_campaign)
}

fn speech_settings_window(ctx: &egui::Context, studio: &mut ReplayStudio) {
    let mut open = studio.speech_settings_open;
    let installed_speakers = installed_emotivoice_speakers();
    let speakers = studio
        .replay
        .as_ref()
        .map(|replay| {
            let mut speakers = Vec::<(u64, String, String)>::new();
            for line in &replay.dialogue {
                if !speakers.iter().any(|(id, ..)| *id == line.sender_id) {
                    speakers.push((
                        line.sender_id,
                        line.name.clone(),
                        line.role.clone(),
                    ));
                }
            }
            speakers
        })
        .unwrap_or_default();
    let mut settings_changed = false;

    egui::Window::new("角色语音设置")
        .id(egui::Id::new("replay-speaker-voice-settings"))
        .open(&mut open)
        .default_width(520.0)
        .max_width(620.0)
        .show(ctx, |ui| {
            ui.label(format!(
                "Spark-TTS 已加载 {} 个稳定中文角色音色（16 个男声、16 个女声）。每个角色的固定参考音色同时用于播放预览和 MP4 导出，不会因台词长短而改变。",
                installed_speakers.len()
            ));
            if installed_speakers.is_empty() {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "未找到已安装音色目录，暂时只显示推荐音色。",
                );
            }
            if speakers.is_empty() {
                ui.label("请先录制回放或从现有聊天生成回放。");
                return;
            }
            egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                for (sender_id, name, role) in &speakers {
                    let defaults = default_speaker_voice_settings(*sender_id);
                    let settings = studio
                        .replay
                        .as_mut()
                        .expect("speaker list requires a replay")
                        .speaker_voice_settings
                        .entry(*sender_id)
                        .or_insert_with(|| defaults.clone());
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.heading(name);
                            if !role.trim().is_empty() {
                                ui.label(role.as_str());
                            }
                            ui.small(format!("QQ {sender_id}"));
                        });
                        let current_speaker =
                            resolved_emotivoice_speaker(settings.voice_name.as_deref(), *sender_id);
                        if settings.voice_name.as_deref() != Some(current_speaker.as_str()) {
                            settings.voice_name = Some(current_speaker.clone());
                            settings_changed = true;
                        }
                        let mut selected_speaker = current_speaker.clone();
                        egui::ComboBox::from_id_salt(("replay-emotivoice-speaker", sender_id))
                            .selected_text(emotivoice_speaker_label(&selected_speaker))
                            .height(360.0)
                            .show_ui(ui, |ui| {
                                ui.strong("男声");
                                for (speaker, label) in SPARK_TTS_VOICE_PROFILES {
                                    if !speaker.starts_with("spark-m") {
                                        continue;
                                    }
                                    ui.selectable_value(
                                        &mut selected_speaker,
                                        speaker.to_owned(),
                                        label,
                                    );
                                }
                                ui.separator();
                                ui.strong("女声");
                                for (speaker, label) in SPARK_TTS_VOICE_PROFILES {
                                    if !speaker.starts_with("spark-f") {
                                            continue;
                                    }
                                    ui.selectable_value(
                                        &mut selected_speaker,
                                        speaker.to_owned(),
                                        label,
                                    );
                                }
                            });
                        if ui
                            .add_enabled(
                                !installed_speakers.is_empty(),
                                egui::Button::new("随机音色"),
                            )
                            .on_hover_text("从 32 个稳定中文角色音色中随机选择")
                            .clicked()
                        {
                            if let Some(random_speaker) = random_emotivoice_speaker(
                                installed_speakers,
                                &current_speaker,
                            ) {
                                selected_speaker = random_speaker;
                            }
                        }
                        if selected_speaker != current_speaker {
                            settings.voice_name = Some(selected_speaker);
                            settings_changed = true;
                        }
                        settings_changed |= ui
                            .add(
                                egui::Slider::new(&mut settings.speech_rate, -30..=180)
                                    .text("基础语速")
                                    .custom_formatter(|value, _| format!("{value:+.0}%")),
                            )
                            .changed();
                        settings_changed |= ui
                            .add(
                                egui::Slider::new(&mut settings.volume, 0.20..=1.20)
                                    .text("相对音量")
                                    .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
                            )
                            .changed();
                        if ui.button("恢复该角色默认值").clicked() {
                            *settings = defaults;
                            settings_changed = true;
                        }
                    });
                }
            });
        });
    studio.speech_settings_open = open;
    if settings_changed {
        if let Some(replay) = studio.replay.as_mut() {
            extend_replay_for_speech(replay);
        }
        studio.status = "角色语音设置已更新，将用于下一句预览和视频导出".to_owned();
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
    if studio.record_camera_enabled {
        if let Ok(transform) = camera.single() {
            replay.camera.push(camera_keyframe(0, transform));
        }
    }
    studio.message_counts = manager
        .messages
        .iter()
        .map(|(target, messages)| (target.clone(), messages.len()))
        .collect();
    studio.record_elapsed_ms = 0;
    studio.camera_sample_accumulator = 0.0;
    studio.playback_ms = 0;
    studio.director_request_pending = false;
    studio.director_response_hash = None;
    studio.auto_export_after_director = false;
    studio.replay = Some(replay);
    studio.mode = ReplayMode::Recording;
    studio.status = if studio.record_camera_enabled {
        "开始录制场景、可见消息和 DM 自由镜头".to_owned()
    } else {
        "开始录制场景和可见消息；DM 镜头采集保持关闭".to_owned()
    };
}

fn stop_recording(studio: &mut ReplayStudio) {
    if let Some(replay) = studio.replay.as_mut() {
        let dialogue_end = replay
            .dialogue
            .last()
            .map(|line| line.time_ms.saturating_add(line.duration_ms))
            .unwrap_or(5_000);
        replay.duration_ms = if studio.record_camera_enabled {
            studio.record_elapsed_ms.max(dialogue_end)
        } else {
            dialogue_end
        };
        extend_replay_for_speech(replay);
    }
    studio.mode = ReplayMode::Idle;
    studio.playback_ms = 0;
    studio.status = "录制已停止，可以预览或导出".to_owned();
}

fn build_from_history(
    studio: &mut ReplayStudio,
    manager: &NapcatMessageManager,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
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
    let speaker_positions = standee_positions(standees);
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
    let ignored_count =
        retain_dialogue_with_standees(&mut replay.dialogue, &speaker_positions);
    spatially_order_dialogue_turns(&mut replay.dialogue, &speaker_positions);
    replay.duration_ms = retime_dialogue_turns(&mut replay.dialogue);
    extend_replay_for_speech(&mut replay);
    if let Ok(transform) = camera.single() {
        replay.camera = turn_based_camera_track(
            transform,
            &replay.dialogue,
            replay.duration_ms,
            &speaker_positions,
        );
    }
    studio.playback_ms = 0;
    studio.director_request_pending = false;
    studio.director_response_hash = None;
    studio.auto_export_after_director = false;
    let dialogue_count = replay.dialogue.len();
    studio.replay = Some(replay);
    studio.status = format!(
        "已生成 {dialogue_count} 个玩家回合；同轮按立牌距离就近切换，已忽略 {ignored_count} 句无场景立牌发言"
    );
}

fn replay_director_key(replay: &ReplayFile) -> String {
    let audience = match &replay.audience {
        ReplayAudience::Public => "public".to_owned(),
        ReplayAudience::Party(id) => format!("party:{id}"),
        ReplayAudience::Player(id) => format!("player:{id}"),
        ReplayAudience::Gm => "gm".to_owned(),
    };
    format!(
        "replay-director:{}:{audience}",
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
        .get(&replay_director_key(replay))?
        .blocks
        .iter()
        .find(|block| block.message_count == message_count)
}

fn queue_replay_director(
    replay: &ReplayFile,
    sender: Option<&DeepseekIOSender>,
    manager: &mut DeepseekManager,
    custom_prompt: &str,
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
) -> Result<(), String> {
    if replay.dialogue.is_empty() {
        return Err("回放中没有可整理的台词".to_owned());
    }
    let sender = sender.ok_or_else(|| "DeepSeek 连接尚未就绪，请稍后重试".to_owned())?;
    let summary_key = replay_director_key(replay);
    let message_count = replay.dialogue.len();
    if let Some(block) = replay_summary_block(replay, manager) {
        if block.pending {
            return Err("这版台词正在整理，请等待当前请求完成".to_owned());
        }
    }
    let visible_standees = standees
        .iter()
        .map(|(_, standee)| standee.user_id)
        .collect::<std::collections::HashSet<_>>();
    let missing_standee_count = replay
        .dialogue
        .iter()
        .filter(|line| !visible_standees.contains(&line.sender_id))
        .count();
    if missing_standee_count > 0 {
        return Err(format!(
            "回放中有 {missing_standee_count} 句找不到说话者立牌；请重新从当前场景聊天生成回放"
        ));
    }
    let dialogue = replay
        .dialogue
        .iter()
        .enumerate()
        .map(|(index, line)| {
            serde_json::json!({
                "index": index,
                "speaker_id": line.sender_id.to_string(),
                "name": line.name,
                "role": line.role,
                "text": line.text.trim(),
                "has_character_model": true,
            })
        })
        .collect::<Vec<_>>();
    let text = serde_json::to_string(&serde_json::json!({ "dialogue": dialogue }))
        .map_err(|err| err.to_string())?;
    let request = serde_json::to_string(&DeepseekRequest::Director {
        target_id: summary_key.clone(),
        message_count,
        text,
        custom_prompt: custom_prompt
            .chars()
            .take(DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS)
            .collect(),
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

fn apply_ready_director_plan(
    studio: &mut ReplayStudio,
    manager: &DeepseekManager,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
) -> Result<bool, String> {
    if !studio.director_request_pending {
        return Ok(false);
    }
    let Some(replay) = studio.replay.as_ref() else {
        studio.director_request_pending = false;
        return Ok(false);
    };
    let Some(block) = replay_summary_block(replay, manager) else {
        return Ok(false);
    };
    if block.pending {
        return Ok(false);
    }
    if let Some(error) = &block.error {
        studio.director_request_pending = false;
        studio.auto_export_after_director = false;
        return Err(error.clone());
    }
    let raw = block.latest.trim();
    if raw.is_empty() {
        return Ok(false);
    }
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    let response_hash = hasher.finish();
    if studio.director_response_hash == Some(response_hash) {
        return Ok(false);
    }
    studio.director_response_hash = Some(response_hash);
    studio.director_request_pending = false;

    let plan = parse_director_plan(raw)?;
    let replay = studio
        .replay
        .as_mut()
        .ok_or_else(|| "回放在应用导演方案前已被移除".to_owned())?;
    if plan.dialogue.len() != replay.dialogue.len() {
        studio.auto_export_after_director = false;
        return Err(format!(
            "方案包含 {} 句，但回放需要 {} 句",
            plan.dialogue.len(),
            replay.dialogue.len()
        ));
    }
    let mut cues = plan.dialogue;
    cues.sort_by_key(|cue| cue.index);
    if cues
        .iter()
        .enumerate()
        .any(|(expected, cue)| cue.index != expected)
    {
        studio.auto_export_after_director = false;
        return Err("方案必须恰好包含每个原始台词 index，且不能重复".to_owned());
    }
    for (line, cue) in replay.dialogue.iter_mut().zip(&cues) {
        let text = cue.text.trim();
        if text.is_empty() {
            studio.auto_export_after_director = false;
            return Err(format!(
                "第 {} 句润色结果为空",
                cue.index
            ));
        }
        if text.chars().count() > 500 {
            studio.auto_export_after_director = false;
            return Err(format!(
                "第 {} 句超过 500 字",
                cue.index
            ));
        }
        if cue.speech_text.chars().count() > 700 {
            studio.auto_export_after_director = false;
            return Err(format!(
                "第 {} 句 TTS 中文读音超过 700 字",
                cue.index
            ));
        }
        line.text = text.to_owned();
        line.speech_text = Some(if cue.speech_text.trim().is_empty() {
            chinese_tts_fallback(text)
        } else {
            cue.speech_text.trim().to_owned()
        });
        line.duration_ms = scaled_dialogue_duration_ms(text, replay.master_dialogue_duration);
    }
    let mut timeline_ms = 350_u64;
    for line in &mut replay.dialogue {
        line.time_ms = timeline_ms;
        timeline_ms = line
            .time_ms
            .saturating_add(line.duration_ms)
            .saturating_add(HISTORY_DIALOGUE_GAP_MS);
    }
    replay.duration_ms = replay
        .dialogue
        .last()
        .map(|line| line.time_ms.saturating_add(line.duration_ms))
        .unwrap_or(5_000);
    extend_replay_for_speech(replay);

    let base = camera
        .single()
        .ok()
        .cloned()
        .or_else(|| replay.camera.first().map(frame_transform))
        .ok_or_else(|| "找不到可用的导演基础镜头".to_owned())?;
    let speaker_positions = standee_positions(standees);
    replay.camera = director_camera_track(
        &base,
        &replay.dialogue,
        &cues,
        replay.duration_ms,
        &speaker_positions,
    );
    studio.playback_ms = 0;
    studio.status = format!(
        "已应用 DeepSeek 导演方案：{} 句润色台词、{} 个镜头帧",
        replay.dialogue.len(),
        replay.camera.len()
    );
    Ok(true)
}

fn parse_director_plan(raw: &str) -> Result<DirectorPlan, String> {
    let trimmed = raw.trim();
    let json = if trimmed.starts_with("```") {
        trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .and_then(|body| body.strip_suffix("```"))
            .unwrap_or(trimmed)
            .trim()
    } else {
        trimmed
    };
    serde_json::from_str(json).map_err(|err| format!("无法解析导演 JSON：{err}"))
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
        master_speech_speed: default_master_speech_speed(),
        master_dialogue_duration: default_master_dialogue_duration(),
        speaker_voice_settings: HashMap::new(),
    }
}

fn start_playback(
    studio: &mut ReplayStudio,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
) {
    if let Some(replay) = studio.replay.as_mut() {
        extend_replay_for_speech(replay);
    }
    let Some(scene) = studio.replay.as_ref().map(|replay| replay.scene.clone()) else {
        return;
    };
    if let Ok(mut grid) = grids.single_mut() {
        studio.pre_playback_scene = Some(capture_scene(&grid));
        apply_scene(&mut grid, &scene);
    }
    studio.playback_ms = 0;
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
    if let Some(replay) = studio.replay.as_mut() {
        extend_replay_for_speech(replay);
    }
    let Some((duration_ms, dialogue, master_speech_speed, speaker_voice_settings)) =
        studio.replay.as_ref().map(|replay| {
            (
                replay.duration_ms,
                replay.dialogue.clone(),
                replay.master_speech_speed,
                replay.speaker_voice_settings.clone(),
            )
        })
    else {
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
    if studio.speech_enabled && !dialogue.is_empty() {
        if let Err(err) = check_speech_synthesizer() {
            studio.status = err;
            return;
        }
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
        duration_ms,
        music_enabled: studio.music_enabled,
        music_volume: studio.music_volume,
        speech_enabled: studio.speech_enabled,
        speech_volume: studio.speech_volume,
        master_speech_speed,
        speaker_voice_settings,
        dialogue,
        total_frames,
        next_frame: 0,
        capture_pending: false,
        pending_seconds: 0.0,
        warmup_frames: VIDEO_CAPTURE_WARMUP_FRAMES,
        monitor_music_started: false,
        monitor_music_entity: None,
        failure: None,
        original_window_title,
        original_window_resizable,
    });
    studio.status = format!(
        "正在逐帧渲染 {total_frames} 帧；当前同步监听音乐和角色语音，完成后生成干净的 MP4 音轨"
    );
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

fn check_speech_synthesizer() -> Result<(), String> {
    onnx_tts_is_available().then_some(()).ok_or_else(|| {
        "未安装 Spark-TTS 中文运行环境或 32 音色库；请运行 scripts/setup_sparktts.ps1"
            .to_owned()
    })
}

fn encode_video_frames(
    frames_path: &Path,
    output_path: &Path,
    fps: u32,
    duration_ms: u64,
    music_enabled: bool,
    music_volume: f32,
    speech_enabled: bool,
    speech_volume: f32,
    master_speech_speed: f32,
    dialogue: &[ReplayDialogue],
    speaker_voice_settings: &HashMap<u64, SpeakerVoiceSettings>,
) -> Result<(), String> {
    let frame_pattern = frames_path.join("frame_%06d.png");
    let soundtrack_path = frames_path.join("jrpg2-piano-soundtrack.wav");
    let narration_path = frames_path.join("character-narration.wav");
    if music_enabled {
        write_jrpg_soundtrack(
            &soundtrack_path,
            duration_ms,
            music_volume,
        )?;
    }
    let speech_enabled = speech_enabled && !dialogue.is_empty();
    if speech_enabled {
        write_narration_track(
            &narration_path,
            frames_path,
            duration_ms,
            speech_volume,
            master_speech_speed,
            dialogue,
            speaker_voice_settings,
        )?;
    }
    let temporary_output = temporary_video_output_path(output_path);
    let mut command = Command::new("ffmpeg");
    command.args(ffmpeg_arguments(fps));
    command.arg(frame_pattern);
    let mut next_input = 1;
    let music_input = if music_enabled {
        command.arg("-i").arg(&soundtrack_path);
        let input = next_input;
        next_input += 1;
        Some(input)
    } else {
        None
    };
    let speech_input = if speech_enabled {
        command.arg("-i").arg(&narration_path);
        let input = next_input;
        Some(input)
    } else {
        None
    };
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
    match (music_input, speech_input) {
        (Some(music), Some(speech)) => {
            command.args([
                "-filter_complex",
                &format!(
                    "[{music}:a][{speech}:a]amix=inputs=2:duration=longest:normalize=0:dropout_transition=0[aout]"
                ),
                "-map",
                "0:v:0",
                "-map",
                "[aout]",
            ]);
        },
        (Some(audio), None) | (None, Some(audio)) => {
            command.args(["-map", "0:v:0", "-map", &format!("{audio}:a:0")]);
        },
        (None, None) => {},
    }
    if music_enabled || speech_enabled {
        command.args(["-c:a", "aac", "-b:a", "160k", "-shortest"]);
    }
    command.arg(&temporary_output);
    hide_command_window(&mut command);
    let output = command
        .output()
        .map_err(|err| format!("无法启动 FFmpeg：{err}"))?;
    if output.status.success() {
        if music_enabled || speech_enabled {
            if let Err(err) = verify_audio_signal(&temporary_output) {
                let _ = fs::remove_file(&temporary_output);
                return Err(err);
            }
        }
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

fn verify_audio_signal(video_path: &Path) -> Result<(), String> {
    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-i"]).arg(video_path).args([
        "-map", "0:a:0", "-t", "30", "-ac", "1", "-ar", "8000", "-f", "s16le", "pipe:1",
    ]);
    hide_command_window(&mut command);
    let output = command
        .output()
        .map_err(|err| format!("无法检查视频音轨：{err}"))?;
    if !output.status.success() {
        return Err("视频音轨缺失或无法解码；未保存静音视频".to_owned());
    }
    let has_signal = output
        .stdout
        .chunks_exact(2)
        .any(|sample| i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs() > 32);
    has_signal
        .then_some(())
        .ok_or_else(|| "视频音轨完全静音；未保存静音视频".to_owned())
}

struct SynthesizedSpeechCue {
    start_sample: u64,
    output_samples: u64,
    samples: Vec<i16>,
    side: DialogueSide,
    volume: f32,
}

#[derive(Serialize)]
struct SpeechSynthesisJob {
    text: String,
    output_path: String,
    speaker: String,
    emotion: String,
    onnx_speed: f32,
    duration_ms: u64,
}

fn write_narration_track(
    path: &Path,
    working_directory: &Path,
    duration_ms: u64,
    volume: f32,
    master_speech_speed: f32,
    dialogue: &[ReplayDialogue],
    speaker_settings: &HashMap<u64, SpeakerVoiceSettings>,
) -> Result<(), String> {
    const SAMPLE_RATE: u32 = 32_000;
    const CHANNELS: u16 = 2;
    const BITS_PER_SAMPLE: u16 = 16;
    let sample_count = duration_ms
        .saturating_mul(SAMPLE_RATE as u64)
        .saturating_add(999)
        / 1_000;
    let data_bytes = sample_count
        .saturating_mul(CHANNELS as u64)
        .saturating_mul((BITS_PER_SAMPLE / 8) as u64);
    if data_bytes > (u32::MAX - 36) as u64 {
        return Err("回放过长，无法生成 WAV 角色语音".to_owned());
    }

    let speech_jobs = dialogue
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let settings = speaker_settings
                .get(&line.sender_id)
                .cloned()
                .unwrap_or_else(|| default_speaker_voice_settings(line.sender_id));
            SpeechSynthesisJob {
                text: speech_text_for_line(line),
                output_path: working_directory
                    .join(format!("speech-{index:05}.wav"))
                    .to_string_lossy()
                    .into_owned(),
                speaker: resolved_emotivoice_speaker(
                    settings.voice_name.as_deref(),
                    line.sender_id,
                ),
                emotion: resolved_emotivoice_emotion(settings.emotion.as_deref()).to_owned(),
                onnx_speed: combined_onnx_speed(
                    settings.speech_rate,
                    master_speech_speed,
                ),
                duration_ms: line.duration_ms,
            }
        })
        .collect::<Vec<_>>();
    synthesize_speech_batch(working_directory, &speech_jobs)?;

    let mut cues = Vec::with_capacity(dialogue.len());
    for (index, line) in dialogue.iter().enumerate() {
        let wav_path = working_directory.join(format!("speech-{index:05}.wav"));
        let samples = read_pcm16_mono_wav(&wav_path)?;
        if samples.is_empty() {
            return Err(format!("角色 {} 的语音为空", line.name));
        }
        let max_output_samples = line.duration_ms.saturating_mul(SAMPLE_RATE as u64) / 1_000;
        cues.push(SynthesizedSpeechCue {
            start_sample: line.time_ms.saturating_mul(SAMPLE_RATE as u64) / 1_000,
            output_samples: (samples.len() as u64).min(max_output_samples).max(1),
            samples,
            side: line.side,
            volume: speaker_settings
                .get(&line.sender_id)
                .map(|settings| settings.volume)
                .unwrap_or(1.0),
        });
    }
    cues.sort_by_key(|cue| cue.start_sample);

    let file = fs::File::create(path).map_err(|err| format!("无法创建角色语音轨道：{err}"))?;
    let mut writer = BufWriter::new(file);
    write_wav_header(
        &mut writer,
        SAMPLE_RATE,
        CHANNELS,
        BITS_PER_SAMPLE,
        data_bytes as u32,
    )?;
    let volume = volume.max(0.0);
    let mut cue_index = 0_usize;
    for sample_index in 0..sample_count {
        while cue_index + 1 < cues.len() && sample_index >= cues[cue_index + 1].start_sample {
            cue_index += 1;
        }
        let sample = cues
            .get(cue_index)
            .filter(|cue| sample_index >= cue.start_sample)
            .and_then(|cue| {
                let local = sample_index - cue.start_sample;
                (local < cue.output_samples).then(|| {
                    let source_index = local;
                    let fade_samples = 96_u64.min(cue.output_samples / 2).max(1);
                    let fade_in = (local as f32 / fade_samples as f32).clamp(0.0, 1.0);
                    let fade_out =
                        ((cue.output_samples - local) as f32 / fade_samples as f32).clamp(0.0, 1.0);
                    let envelope = smoothstep(fade_in) * smoothstep(fade_out);
                    (cue.samples[source_index as usize] as f32 / i16::MAX as f32)
                        * envelope
                        * volume
                        * cue.volume
                })
            })
            .unwrap_or(0.0);
        let (left_pan, right_pan) = cues
            .get(cue_index)
            .map(|cue| match cue.side {
                DialogueSide::Left => (1.0, 0.82),
                DialogueSide::Right => (0.82, 1.0),
            })
            .unwrap_or((1.0, 1.0));
        writer
            .write_all(&pcm_i16(sample * left_pan).to_le_bytes())
            .and_then(|_| writer.write_all(&pcm_i16(sample * right_pan).to_le_bytes()))
            .map_err(|err| format!("写入角色语音轨道失败：{err}"))?;
    }
    writer
        .flush()
        .map_err(|err| format!("完成角色语音轨道失败：{err}"))
}

const SPARK_TTS_VOICE_PROFILES: [(&str, &str); 32] = [
    ("spark-m01", "男声 01 · 深沉"),
    ("spark-m02", "男声 02 · 厚重"),
    ("spark-m03", "男声 03 · 沉稳"),
    ("spark-m04", "男声 04 · 冷静"),
    ("spark-m05", "男声 05 · 温和"),
    ("spark-m06", "男声 06 · 硬朗"),
    ("spark-m07", "男声 07 · 可靠"),
    ("spark-m08", "男声 08 · 清朗"),
    ("spark-m09", "男声 09 · 青年"),
    ("spark-m10", "男声 10 · 成熟"),
    ("spark-m11", "男声 11 · 克制"),
    ("spark-m12", "男声 12 · 机敏"),
    ("spark-m13", "男声 13 · 悠然"),
    ("spark-m14", "男声 14 · 严肃"),
    ("spark-m15", "男声 15 · 亲切"),
    ("spark-m16", "男声 16 · 明快"),
    ("spark-f01", "女声 01 · 温柔"),
    ("spark-f02", "女声 02 · 沉静"),
    ("spark-f03", "女声 03 · 从容"),
    ("spark-f04", "女声 04 · 知性"),
    ("spark-f05", "女声 05 · 可靠"),
    ("spark-f06", "女声 06 · 清澈"),
    ("spark-f07", "女声 07 · 成熟"),
    ("spark-f08", "女声 08 · 克制"),
    ("spark-f09", "女声 09 · 活泼"),
    ("spark-f10", "女声 10 · 明快"),
    ("spark-f11", "女声 11 · 灵动"),
    ("spark-f12", "女声 12 · 亲切"),
    ("spark-f13", "女声 13 · 坚定"),
    ("spark-f14", "女声 14 · 轻盈"),
    ("spark-f15", "女声 15 · 稚气"),
    ("spark-f16", "女声 16 · 元气"),
];

const EMOTIVOICE_EMOTIONS: [&str; 7] = ["普通", "开心", "悲伤", "生气", "惊讶", "厌恶", "恐惧"];

fn installed_emotivoice_speakers() -> &'static [String] {
    static INSTALLED_SPEAKERS: OnceLock<Vec<String>> = OnceLock::new();
    INSTALLED_SPEAKERS
        .get_or_init(|| {
            if emotivoice_voice_bank_path().join("profiles.json").is_file() {
                SPARK_TTS_VOICE_PROFILES
                    .iter()
                    .map(|(id, _)| (*id).to_owned())
                    .collect()
            } else {
                Vec::new()
            }
        })
        .as_slice()
}

fn random_emotivoice_speaker(speakers: &[String], current: &str) -> Option<String> {
    let available_count = speakers
        .iter()
        .filter(|speaker| speaker.as_str() != current)
        .count();
    if available_count == 0 {
        return speakers.first().cloned();
    }
    let selected_index = rand::rng().random_range(0..available_count);
    speakers
        .iter()
        .filter(|speaker| speaker.as_str() != current)
        .nth(selected_index)
        .cloned()
}

fn emotivoice_speaker_label(speaker: &str) -> String {
    SPARK_TTS_VOICE_PROFILES
        .iter()
        .find_map(|(id, label)| (*id == speaker).then_some((*label).to_owned()))
        .unwrap_or_else(|| format!("音色 {speaker}"))
}

fn default_emotivoice_speaker(sender_id: u64) -> &'static str {
    let index = (sender_id as usize) % SPARK_TTS_VOICE_PROFILES.len();
    SPARK_TTS_VOICE_PROFILES[index].0
}

fn resolved_emotivoice_speaker(configured: Option<&str>, sender_id: u64) -> String {
    configured
        .map(str::trim)
        .filter(|speaker| {
            SPARK_TTS_VOICE_PROFILES
                .iter()
                .any(|(profile, _)| profile == speaker)
        })
        .map(str::to_owned)
        .unwrap_or_else(|| default_emotivoice_speaker(sender_id).to_owned())
}

fn resolved_emotivoice_emotion(configured: Option<&str>) -> &'static str {
    configured
        .and_then(|configured| {
            EMOTIVOICE_EMOTIONS
                .iter()
                .copied()
                .find(|emotion| *emotion == configured.trim())
        })
        .unwrap_or("普通")
}

fn speaker_voice_profile(sender_id: u64) -> (i32, i32) {
    const RATES: [i32; 8] = [18, 24, 14, 28, 10, 21, 16, 26];
    let profile = (sender_id as usize) % RATES.len();
    (0, RATES[profile])
}

fn estimated_speech_duration_ms(text: &str) -> u64 {
    text.chars().fold(250_u64, |total, character| {
        let character_ms = match character {
            '。' | '！' | '？' | '!' | '?' | '；' | ';' => 280,
            '，' | ',' | '、' | '：' | ':' => 160,
            character if character.is_whitespace() => 15,
            character if is_cjk_character(character) => 250,
            _ => 45,
        };
        total.saturating_add(character_ms)
    })
}

fn minimum_speech_window_ms(text: &str, relative_rate: i32, master_speed: f32) -> u64 {
    let speed = effective_emotivoice_speed(
        text,
        combined_onnx_speed(relative_rate, master_speed),
    ) as f64;
    let padding_ms = if is_short_utterance(text) {
        SHORT_UTTERANCE_HEAD_PAD_MS.saturating_add(SHORT_UTTERANCE_TAIL_PAD_MS)
    } else {
        0
    };
    ((estimated_speech_duration_ms(text) as f64 * 1.20 / speed).ceil() as u64)
        .saturating_add(250)
        .saturating_add(padding_ms)
}

fn stretched_replay_time(time_ms: u64, segments: &[(u64, u64, u64, u64)]) -> u64 {
    let mut accumulated_extension = 0_u64;
    for &(old_start, old_end, new_start, new_end) in segments {
        if time_ms < old_start {
            return time_ms.saturating_add(accumulated_extension);
        }
        if time_ms <= old_end {
            let old_duration = old_end.saturating_sub(old_start).max(1);
            let new_duration = new_end.saturating_sub(new_start);
            let elapsed = time_ms.saturating_sub(old_start);
            return new_start.saturating_add(
                ((elapsed as u128 * new_duration as u128) / old_duration as u128) as u64,
            );
        }
        accumulated_extension = new_end.saturating_sub(old_end);
    }
    time_ms.saturating_add(accumulated_extension)
}

fn extend_replay_for_speech(replay: &mut ReplayFile) -> bool {
    let mut accumulated_extension = 0_u64;
    let mut segments = Vec::with_capacity(replay.dialogue.len());
    let mut updated_lines = Vec::with_capacity(replay.dialogue.len());

    for line in &replay.dialogue {
        let settings = replay
            .speaker_voice_settings
            .get(&line.sender_id)
            .cloned()
            .unwrap_or_else(|| default_speaker_voice_settings(line.sender_id));
        let required_duration = minimum_speech_window_ms(
            &speech_text_for_line(line),
            settings.speech_rate,
            replay.master_speech_speed,
        );
        let new_duration = line.duration_ms.max(required_duration);
        let new_start = line.time_ms.saturating_add(accumulated_extension);
        let old_end = line.time_ms.saturating_add(line.duration_ms);
        let new_end = new_start.saturating_add(new_duration);
        segments.push((
            line.time_ms,
            old_end,
            new_start,
            new_end,
        ));
        updated_lines.push((new_start, new_duration));
        accumulated_extension =
            accumulated_extension.saturating_add(new_duration.saturating_sub(line.duration_ms));
    }

    if accumulated_extension == 0 {
        return false;
    }

    for (line, (new_start, new_duration)) in replay.dialogue.iter_mut().zip(updated_lines) {
        line.time_ms = new_start;
        line.duration_ms = new_duration;
    }
    for frame in &mut replay.camera {
        frame.time_ms = stretched_replay_time(frame.time_ms, &segments);
    }
    replay.duration_ms = stretched_replay_time(replay.duration_ms, &segments);
    true
}

fn default_master_speech_speed() -> f32 { 1.30 }

fn normalized_master_speech_speed(speed: f32) -> f32 {
    if speed.is_finite() && speed > 0.0 {
        speed.max(0.10)
    } else {
        default_master_speech_speed()
    }
}

fn default_master_dialogue_duration() -> f32 { 1.0 }

fn normalized_master_dialogue_duration(duration: f32) -> f32 {
    if duration.is_finite() && duration > 0.0 {
        duration.max(0.10)
    } else {
        default_master_dialogue_duration()
    }
}

fn scaled_millis(value: u64, ratio: f64) -> u64 {
    ((value as f64 * ratio).round().clamp(0.0, u64::MAX as f64)) as u64
}

fn scaled_dialogue_duration_ms(text: &str, master_duration: f32) -> u64 {
    scaled_millis(
        dialogue_duration_ms(text),
        normalized_master_dialogue_duration(master_duration) as f64,
    )
}

fn retime_replay(replay: &mut ReplayFile, previous: f32, requested: f32) {
    let previous = normalized_master_dialogue_duration(previous);
    let requested = normalized_master_dialogue_duration(requested);
    replay.master_dialogue_duration = requested;
    let ratio = requested as f64 / previous as f64;
    for line in &mut replay.dialogue {
        line.time_ms = scaled_millis(line.time_ms, ratio);
        line.duration_ms = scaled_millis(line.duration_ms, ratio).max(1);
    }
    for frame in &mut replay.camera {
        frame.time_ms = scaled_millis(frame.time_ms, ratio);
    }
    replay.duration_ms = scaled_millis(replay.duration_ms, ratio).max(1);
}

fn combined_onnx_speed(relative_rate: i32, master_speed: f32) -> f32 {
    onnx_speed(relative_rate) * normalized_master_speech_speed(master_speed)
}

fn default_speaker_voice_settings(sender_id: u64) -> SpeakerVoiceSettings {
    let (pitch, speech_rate) = speaker_voice_profile(sender_id);
    SpeakerVoiceSettings {
        voice_name: Some(default_emotivoice_speaker(sender_id).to_owned()),
        emotion: Some("普通".to_owned()),
        onnx_speaker_id: None,
        pitch,
        speech_rate,
        volume: 1.0,
    }
}

fn replay_voice_signature(replay: &ReplayFile, global_volume: f32) -> u64 {
    let mut hasher = DefaultHasher::new();
    replay.created_at_unix_ms.hash(&mut hasher);
    global_volume.to_bits().hash(&mut hasher);
    replay.master_speech_speed.to_bits().hash(&mut hasher);
    replay.master_dialogue_duration.to_bits().hash(&mut hasher);
    for line in &replay.dialogue {
        line.sender_id.hash(&mut hasher);
        line.duration_ms.hash(&mut hasher);
        line.text.hash(&mut hasher);
        line.speech_text.hash(&mut hasher);
    }
    let mut settings = replay.speaker_voice_settings.iter().collect::<Vec<_>>();
    settings.sort_by_key(|(sender_id, _)| **sender_id);
    for (sender_id, voice) in settings {
        sender_id.hash(&mut hasher);
        voice.voice_name.hash(&mut hasher);
        voice.emotion.hash(&mut hasher);
        voice.onnx_speaker_id.hash(&mut hasher);
        voice.pitch.hash(&mut hasher);
        voice.speech_rate.hash(&mut hasher);
        voice.volume.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

fn onnx_speed(relative_rate: i32) -> f32 { (1.0 + relative_rate as f32 / 200.0).clamp(0.85, 1.45) }

fn synthesize_speech_batch(
    working_directory: &Path,
    jobs: &[SpeechSynthesisJob],
) -> Result<(), String> {
    if jobs.is_empty() {
        return Ok(());
    }
    let _ = working_directory;
    let mut tts = create_onnx_tts()?;
    for job in jobs {
        let max_samples = job.duration_ms.saturating_mul(32_000) / 1_000;
        let wav = tts.synthesize(
            &job.text,
            &job.speaker,
            &job.emotion,
            job.onnx_speed,
        )?;
        fs::write(&job.output_path, wav)
            .map_err(|err| format!("无法保存 Spark-TTS 角色语音：{err}"))?;
        let samples = read_pcm16_mono_wav(Path::new(&job.output_path))?;
        if samples.len() as u64 > max_samples {
            return Err(format!(
                "Spark-TTS 生成的语音超过 {} 毫秒；请提高整体语速或延长整体台词停留",
                job.duration_ms
            ));
        }
    }
    Ok(())
}

fn read_pcm16_mono_wav(path: &Path) -> Result<Vec<i16>, String> {
    let bytes = fs::read(path).map_err(|err| format!("无法读取角色语音 WAV：{err}"))?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("角色语音不是有效的 WAV 文件".to_owned());
    }
    let mut offset = 12_usize;
    let mut valid_format = false;
    let mut audio_data = None;
    while offset + 8 <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_size =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let data_start = offset + 8;
        let data_end = data_start.saturating_add(chunk_size);
        if data_end > bytes.len() {
            return Err("角色语音 WAV 数据不完整".to_owned());
        }
        if chunk_id == b"fmt " && chunk_size >= 16 {
            let audio_format =
                u16::from_le_bytes(bytes[data_start..data_start + 2].try_into().unwrap());
            let channels =
                u16::from_le_bytes(bytes[data_start + 2..data_start + 4].try_into().unwrap());
            let sample_rate =
                u32::from_le_bytes(bytes[data_start + 4..data_start + 8].try_into().unwrap());
            let bits =
                u16::from_le_bytes(bytes[data_start + 14..data_start + 16].try_into().unwrap());
            valid_format =
                audio_format == 1 && channels == 1 && sample_rate == 32_000 && bits == 16;
        } else if chunk_id == b"data" {
            audio_data = Some(&bytes[data_start..data_end]);
        }
        offset = data_end.saturating_add(chunk_size % 2);
    }
    if !valid_format {
        return Err("角色语音 WAV 必须是 32 kHz、16 位、单声道 PCM".to_owned());
    }
    let audio_data = audio_data.ok_or_else(|| "角色语音 WAV 缺少音频数据".to_owned())?;
    Ok(audio_data
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
        .collect())
}

fn write_jrpg_soundtrack(path: &Path, duration_ms: u64, volume: f32) -> Result<(), String> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join(BACKGROUND_MUSIC_PATH);
    if !source.is_file() {
        return Err(format!(
            "缺少 CC0 JRPG 背景音乐：{}",
            source.display()
        ));
    }
    let duration_seconds = (duration_ms.max(50) as f64 / 1_000.0).max(0.05);
    let fade_seconds = (duration_seconds * 0.15).clamp(0.05, 2.0);
    let fade_out_start = (duration_seconds - fade_seconds).max(0.0);
    let filter = format!(
        "volume={:.4},afade=t=in:st=0:d={fade_seconds:.3},afade=t=out:st={fade_out_start:.3}:d={fade_seconds:.3}",
        volume.max(0.0)
    );
    let mut command = Command::new("ffmpeg");
    command
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-stream_loop",
            "-1",
            "-i",
        ])
        .arg(&source)
        .args([
            "-t",
            &format!("{duration_seconds:.3}"),
            "-af",
            &filter,
            "-ar",
            "32000",
            "-ac",
            "2",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(path);
    hide_command_window(&mut command);
    let output = command
        .output()
        .map_err(|err| format!("无法启动 FFmpeg 处理 JRPG 背景音乐：{err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "无法生成 JRPG 背景音乐：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn write_wav_header(
    writer: &mut impl Write,
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    data_bytes: u32,
) -> Result<(), String> {
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    let block_align = channels * bits_per_sample / 8;
    writer
        .write_all(b"RIFF")
        .and_then(|_| writer.write_all(&(36 + data_bytes).to_le_bytes()))
        .and_then(|_| writer.write_all(b"WAVEfmt "))
        .and_then(|_| writer.write_all(&16_u32.to_le_bytes()))
        .and_then(|_| writer.write_all(&1_u16.to_le_bytes()))
        .and_then(|_| writer.write_all(&channels.to_le_bytes()))
        .and_then(|_| writer.write_all(&sample_rate.to_le_bytes()))
        .and_then(|_| writer.write_all(&byte_rate.to_le_bytes()))
        .and_then(|_| writer.write_all(&block_align.to_le_bytes()))
        .and_then(|_| writer.write_all(&bits_per_sample.to_le_bytes()))
        .and_then(|_| writer.write_all(b"data"))
        .and_then(|_| writer.write_all(&data_bytes.to_le_bytes()))
        .map_err(|err| format!("写入 WAV 文件头失败：{err}"))
}

fn smoothstep(value: f32) -> f32 { value * value * (3.0 - 2.0 * value) }

fn pcm_i16(value: f32) -> i16 { (value.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16 }

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

fn spatially_order_dialogue_turns(
    dialogue: &mut Vec<ReplayDialogue>,
    speaker_positions: &HashMap<u64, Vec3>,
) {
    if dialogue.len() < 2 {
        return;
    }
    let mut queues = HashMap::<u64, VecDeque<ReplayDialogue>>::new();
    for line in dialogue.drain(..) {
        queues.entry(line.sender_id).or_default().push_back(line);
    }
    let mut ordered = Vec::with_capacity(queues.values().map(VecDeque::len).sum());
    while queues.values().any(|queue| !queue.is_empty()) {
        let mut unplayed = queues
            .iter()
            .filter(|(_, queue)| !queue.is_empty())
            .map(|(speaker_id, _)| *speaker_id)
            .collect::<HashSet<_>>();
        let Some(mut current_speaker) = unplayed.iter().copied().min_by_key(|speaker_id| {
            queues
                .get(speaker_id)
                .and_then(|queue| queue.front())
                .map(|line| (line.time_ms, line.sender_id))
                .unwrap_or((u64::MAX, *speaker_id))
        }) else {
            break;
        };
        while unplayed.remove(&current_speaker) {
            if let Some(line) = queues
                .get_mut(&current_speaker)
                .and_then(VecDeque::pop_front)
            {
                ordered.push(line);
            }
            let Some(current_position) = speaker_positions.get(&current_speaker) else {
                break;
            };
            let Some(next_speaker) = unplayed.iter().copied().min_by(|left, right| {
                let left_distance = speaker_positions
                    .get(left)
                    .map(|position| current_position.distance_squared(*position))
                    .unwrap_or(f32::INFINITY);
                let right_distance = speaker_positions
                    .get(right)
                    .map(|position| current_position.distance_squared(*position))
                    .unwrap_or(f32::INFINITY);
                left_distance
                    .total_cmp(&right_distance)
                    .then_with(|| left.cmp(right))
            }) else {
                break;
            };
            current_speaker = next_speaker;
        }
    }
    *dialogue = ordered;
}

fn retain_dialogue_with_standees(
    dialogue: &mut Vec<ReplayDialogue>,
    speaker_positions: &HashMap<u64, Vec3>,
) -> usize {
    let previous_len = dialogue.len();
    dialogue.retain(|line| speaker_positions.contains_key(&line.sender_id));
    previous_len.saturating_sub(dialogue.len())
}

fn retime_dialogue_turns(dialogue: &mut [ReplayDialogue]) -> u64 {
    let mut timeline_ms = 350_u64;
    for line in dialogue {
        line.time_ms = timeline_ms;
        timeline_ms = line
            .time_ms
            .saturating_add(line.duration_ms)
            .saturating_add(HISTORY_DIALOGUE_GAP_MS);
    }
    timeline_ms
        .saturating_sub(HISTORY_DIALOGUE_GAP_MS)
        .max(5_000)
}

fn standee_positions(
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
) -> HashMap<u64, Vec3> {
    standees
        .iter()
        .map(|(transform, standee)| (standee.user_id, transform.translation))
        .collect()
}

fn turn_based_camera_track(
    base: &Transform,
    dialogue: &[ReplayDialogue],
    duration_ms: u64,
    speaker_positions: &HashMap<u64, Vec3>,
) -> Vec<ReplayCameraKeyframe> {
    let mut frames = Vec::with_capacity(dialogue.len().saturating_mul(3).saturating_add(2));
    let mut current = dialogue
        .first()
        .and_then(|line| speaker_positions.get(&line.sender_id))
        .map(|target| {
            speaker_camera_shot(
                base,
                *target,
                dialogue[0].sender_id,
                0,
                0.0,
            )
        })
        .unwrap_or_else(|| base.clone());
    frames.push(camera_keyframe(0, &current));
    for (index, line) in dialogue.iter().enumerate() {
        let line_end = line.time_ms.saturating_add(line.duration_ms);
        if let Some(target) = speaker_positions.get(&line.sender_id) {
            let focused = speaker_camera_shot(
                base,
                *target,
                line.sender_id,
                index,
                0.0,
            );
            let settled = speaker_camera_shot(
                base,
                *target,
                line.sender_id,
                index,
                1.0,
            );
            if line.time_ms > 0 {
                frames.push(camera_keyframe(
                    line.time_ms.saturating_sub(1),
                    &current,
                ));
            }
            frames.push(camera_keyframe(line.time_ms, &focused));
            frames.push(camera_keyframe(line_end, &settled));
            current = settled;
        }
    }
    if frames
        .last()
        .is_some_and(|frame| frame.time_ms < duration_ms)
    {
        frames.push(camera_keyframe(duration_ms, &current));
    }
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

fn director_camera_track(
    base: &Transform,
    dialogue: &[ReplayDialogue],
    cues: &[DirectorCue],
    duration_ms: u64,
    speaker_positions: &HashMap<u64, Vec3>,
) -> Vec<ReplayCameraKeyframe> {
    let mut current = base.clone();
    if let (Some(line), Some(cue)) = (dialogue.first(), cues.first()) {
        if let Some(target) = speaker_positions.get(&line.sender_id) {
            current = director_speaker_shot(
                base,
                *target,
                line.sender_id,
                0,
                resolved_speaker_shot(cue.shot),
                cue.motion,
                0.0,
            );
        }
    }
    let mut frames = vec![camera_keyframe(0, &current)];
    for (index, (line, cue)) in dialogue.iter().zip(cues).enumerate() {
        let line_end = line.time_ms.saturating_add(line.duration_ms);
        let (arrival, settled) = if let Some(target) = speaker_positions.get(&line.sender_id) {
            let speaker_shot = resolved_speaker_shot(cue.shot);
            let desired_arrival = director_speaker_shot(
                base,
                *target,
                line.sender_id,
                index,
                speaker_shot,
                cue.motion,
                0.0,
            );
            let arrival = desired_arrival;
            let desired_settled = director_speaker_shot(
                &arrival,
                *target,
                line.sender_id,
                index,
                speaker_shot,
                cue.motion,
                1.0,
            );
            let settle_limit = (line.duration_ms as f32 / 1_000.0 * 0.12).clamp(0.2, 0.65);
            let settled = limit_camera_travel(&arrival, &desired_settled, settle_limit);
            (arrival, settled)
        } else {
            continue;
        };
        if line.time_ms > 0 {
            frames.push(camera_keyframe(
                line.time_ms.saturating_sub(1),
                &current,
            ));
        }
        frames.push(camera_keyframe(line.time_ms, &arrival));
        frames.push(camera_keyframe(line_end, &settled));
        current = settled;
    }
    if frames
        .last()
        .is_some_and(|frame| frame.time_ms < duration_ms)
    {
        frames.push(camera_keyframe(duration_ms, &current));
    }
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

fn resolved_speaker_shot(shot: DirectorShot) -> DirectorShot {
    match shot {
        DirectorShot::SpeakerClose | DirectorShot::SpeakerMedium | DirectorShot::SpeakerWide => {
            shot
        },
        DirectorShot::Establishing | DirectorShot::Environment => DirectorShot::SpeakerMedium,
    }
}

fn director_speaker_shot(
    base: &Transform,
    target: Vec3,
    sender_id: u64,
    index: usize,
    shot: DirectorShot,
    motion: DirectorMotion,
    progress: f32,
) -> Transform {
    let seed = cinematic_seed(sender_id, index);
    let angle = seed * std::f32::consts::TAU;
    let mut approach = base.translation - target;
    approach.y = 0.0;
    let approach = approach
        .try_normalize()
        .unwrap_or_else(|| Vec3::new(angle.cos(), 0.0, angle.sin()));
    let base_distance = match shot {
        DirectorShot::SpeakerClose => 3.0,
        DirectorShot::SpeakerMedium => 5.0,
        DirectorShot::SpeakerWide => 8.0,
        DirectorShot::Establishing => 12.0,
        DirectorShot::Environment => 10.0,
    };
    let lateral = Vec3::new(-approach.z, 0.0, approach.x);
    let distance_delta = match motion {
        DirectorMotion::DollyIn => -0.35 * progress,
        DirectorMotion::DollyOut => 0.35 * progress,
        _ => 0.0,
    };
    let lateral_delta = match motion {
        DirectorMotion::DriftLeft => -0.30 * progress,
        DirectorMotion::DriftRight => 0.30 * progress,
        _ => 0.0,
    };
    let height = match shot {
        DirectorShot::SpeakerClose => 1.25,
        DirectorShot::SpeakerMedium => 1.55,
        DirectorShot::SpeakerWide => 2.2,
        DirectorShot::Establishing | DirectorShot::Environment => 3.4,
    };
    let position = target
        + approach * (base_distance + distance_delta)
        + lateral * lateral_delta
        + Vec3::Y * height;
    Transform::from_translation(position).looking_at(target + Vec3::Y * 0.35, Vec3::Y)
}

fn limit_camera_travel(current: &Transform, desired: &Transform, max_distance: f32) -> Transform {
    let offset = desired.translation - current.translation;
    Transform {
        translation: current.translation + offset.clamp_length_max(max_distance.max(0.0)),
        rotation: desired.rotation,
        scale: desired.scale,
    }
}

fn speaker_camera_shot(
    base: &Transform,
    target: Vec3,
    sender_id: u64,
    index: usize,
    settle: f32,
) -> Transform {
    let seed = cinematic_seed(sender_id, index);
    let angle = seed * std::f32::consts::TAU;
    let mut approach = base.translation - target;
    approach.y = 0.0;
    let approach = approach
        .try_normalize()
        .unwrap_or_else(|| Vec3::new(angle.cos(), 0.0, angle.sin()));
    let lateral = Vec3::new(-approach.z, 0.0, approach.x);
    let distance = 5.2 + cinematic_seed(sender_id.rotate_left(11), index) * 1.8 - settle * 0.25;
    let shoulder = (cinematic_seed(sender_id.rotate_left(23), index) - 0.5) * 1.2;
    let height = 1.15 + cinematic_seed(sender_id.rotate_left(37), index) * 0.75;
    let position =
        target + approach * distance + lateral * (shoulder + settle * 0.12) + Vec3::Y * height;
    Transform::from_translation(position).looking_at(target + Vec3::Y * 0.25, Vec3::Y)
}

fn cinematic_seed(sender_id: u64, index: usize) -> f32 {
    let mut value = sender_id
        .wrapping_add((index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .wrapping_add(0xa076_1d64_78bd_642f);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    (value as u32) as f32 / u32::MAX as f32
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
    let is_gm = manager.is_gm_user(message.sender_id);
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
        speech_text: None,
        avatar,
        avatar_data_url: None,
        visibility: message.visibility.clone(),
        side,
    })
}

fn normalize_dialogue_sides(replay: &mut ReplayFile, manager: &NapcatMessageManager) {
    for dialogue in &mut replay.dialogue {
        dialogue.side = speaker_side(manager.is_gm_user(dialogue.sender_id));
    }
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
    let reading_ms = text.chars().fold(350_u64, |total, character| {
        let character_ms = match character {
            '。' | '！' | '？' | '!' | '?' | '；' | ';' => 110,
            '，' | ',' | '、' | '：' | ':' => 50,
            character if character.is_whitespace() => 8,
            character if is_cjk_character(character) => 53,
            _ => 23,
        };
        total.saturating_add(character_ms)
    });
    reading_ms
        .saturating_mul(3)
        .clamp(MIN_DIALOGUE_MS, MAX_DIALOGUE_MS)
}

fn speech_text_for_line(line: &ReplayDialogue) -> String {
    chinese_tts_fallback(
        line.speech_text
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .unwrap_or(&line.text),
    )
}

fn chinese_digit(character: char) -> Option<&'static str> {
    Some(match character {
        '0' => "零",
        '1' => "一",
        '2' => "二",
        '3' => "三",
        '4' => "四",
        '5' => "五",
        '6' => "六",
        '7' => "七",
        '8' => "八",
        '9' => "九",
        _ => return None,
    })
}

fn chinese_integer_reading(digits: &str) -> String {
    fn section_reading(value: u16, omit_leading_one: bool) -> String {
        const DIGITS: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
        const UNITS: [&str; 4] = ["", "十", "百", "千"];
        let mut output = String::new();
        let mut pending_zero = false;
        for position in (0..4).rev() {
            let divisor = 10_u16.pow(position);
            let digit = value / divisor % 10;
            if digit == 0 {
                pending_zero |= !output.is_empty() && value % divisor != 0;
                continue;
            }
            if pending_zero {
                output.push('零');
                pending_zero = false;
            }
            if !(digit == 1 && position == 1 && output.is_empty() && omit_leading_one) {
                output.push_str(DIGITS[digit as usize]);
            }
            output.push_str(UNITS[position as usize]);
        }
        output
    }

    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        return "零".to_owned();
    }
    if trimmed.len() > 12 {
        return digits.chars().filter_map(chinese_digit).collect();
    }

    let mut sections = Vec::new();
    let mut end = trimmed.len();
    while end > 0 {
        let start = end.saturating_sub(4);
        sections.push(trimmed[start..end].parse::<u16>().unwrap_or_default());
        end = start;
    }

    const SECTION_UNITS: [&str; 4] = ["", "万", "亿", "万亿"];
    let mut output = String::new();
    let mut pending_zero = false;
    for index in (0..sections.len()).rev() {
        let section = sections[index];
        if section == 0 {
            pending_zero |= !output.is_empty();
            continue;
        }
        if !output.is_empty() && (pending_zero || section < 1_000) {
            output.push('零');
        }
        let omit_leading_one = output.is_empty();
        output.push_str(&section_reading(
            section,
            omit_leading_one,
        ));
        output.push_str(SECTION_UNITS[index]);
        pending_zero = false;
    }
    output
}

fn chinese_tts_fallback(text: &str) -> String {
    fn latin_reading(character: char) -> Option<&'static str> {
        Some(match character.to_ascii_uppercase() {
            'A' => "诶",
            'B' => "比",
            'C' => "西",
            'D' => "迪",
            'E' => "伊",
            'F' => "艾弗",
            'G' => "吉",
            'H' => "艾尺",
            'I' => "艾",
            'J' => "杰",
            'K' => "开",
            'L' => "艾勒",
            'M' => "艾姆",
            'N' => "恩",
            'O' => "欧",
            'P' => "屁",
            'Q' => "丘",
            'R' => "阿尔",
            'S' => "艾丝",
            'T' => "踢",
            'U' => "优",
            'V' => "维",
            'W' => "达布流",
            'X' => "艾克斯",
            'Y' => "歪",
            'Z' => "贼德",
            _ => return None,
        })
    }

    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len() + 16);
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        if character.is_ascii_digit() {
            let start = index;
            while index < characters.len() && characters[index].is_ascii_digit() {
                index += 1;
            }
            output.push_str(&chinese_integer_reading(
                &characters[start..index].iter().collect::<String>(),
            ));
            continue;
        }
        if character.is_ascii_alphabetic() {
            let start = index;
            while index < characters.len() && characters[index].is_ascii_alphabetic() {
                index += 1;
            }
            let word = characters[start..index].iter().collect::<String>();
            if word.eq_ignore_ascii_case("steam") {
                output.push_str("斯地母");
            } else {
                for letter in word.chars() {
                    if let Some(reading) = latin_reading(letter) {
                        output.push_str(reading);
                    }
                }
            }
            continue;
        }
        if let Some(reading) = latin_reading(character) {
            output.push_str(reading);
            index += 1;
            continue;
        }
        let reading = match character {
            '%' => "百分号",
            '&' => "和",
            '+' => "加",
            '=' => "等于",
            _ => "",
        };
        if reading.is_empty() {
            output.push(character);
        } else {
            output.push_str(reading);
        }
        index += 1;
    }
    output
}

fn normalize_tts_text(text: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Script {
        Cjk,
        Latin,
        Other,
    }

    let mut output = String::with_capacity(text.len() + 8);
    let mut previous_script = Script::Other;
    let mut pending_space = false;
    for character in text.chars() {
        let script = if is_cjk_character(character) {
            Script::Cjk
        } else if character.is_ascii_alphanumeric() || matches!(character, '\'' | '-') {
            Script::Latin
        } else {
            Script::Other
        };
        if matches!(
            (previous_script, script),
            (Script::Cjk, Script::Latin) | (Script::Latin, Script::Cjk)
        ) && output
            .chars()
            .next_back()
            .is_some_and(|previous| is_cjk_character(previous) || previous.is_ascii_alphanumeric())
        {
            output.push(' ');
        }
        match character {
            character if is_cjk_character(character) || character.is_ascii_alphanumeric() => {
                if pending_space && !output.ends_with(' ') && !output.ends_with('，') {
                    output.push(' ');
                }
                output.push(character);
                pending_space = false;
            },
            '\'' | '-' => {
                output.push(character);
                pending_space = false;
            },
            '。' | '！' | '？' | '，' | '、' | '：' | '；' | '.' | '!' | '?' | ',' | ':' | ';' =>
            {
                output.push(character);
                pending_space = false;
            },
            character if character.is_whitespace() => pending_space = true,
            _ => {
                if !output.ends_with('，') && !output.is_empty() {
                    output.push('，');
                }
                pending_space = false;
            },
        }
        if script != Script::Other {
            previous_script = script;
        }
    }
    output.trim().trim_matches('，').trim().to_owned()
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
    active_dialogue_index(dialogue, time_ms).map(|index| &dialogue[index])
}

fn active_dialogue_index(dialogue: &[ReplayDialogue], time_ms: u64) -> Option<usize> {
    dialogue.iter().rposition(|line| {
        time_ms >= line.time_ms && time_ms < line.time_ms.saturating_add(line.duration_ms)
    })
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
    fn turn_camera_focuses_known_speakers_at_line_start() {
        let base = Transform::from_xyz(2.0, 3.0, 4.0);
        let mut dialogue = [
            test_dialogue(350, 2_400, DialogueSide::Left),
            test_dialogue(3_030, 2_400, DialogueSide::Right),
        ];
        dialogue[1].sender_id = 2;
        let speaker_positions = HashMap::from([
            (1, Vec3::new(-8.0, 1.0, 2.0)),
            (2, Vec3::new(9.0, 1.0, -3.0)),
        ]);
        let frames = turn_based_camera_track(
            &base,
            &dialogue,
            5_430,
            &speaker_positions,
        );
        assert!(
            frames
                .windows(2)
                .all(|pair| pair[0].time_ms < pair[1].time_ms)
        );
        for (arrival_ms, target) in [(350, speaker_positions[&1]), (3_030, speaker_positions[&2])] {
            let frame = frames
                .iter()
                .find(|frame| frame.time_ms == arrival_ms)
                .unwrap();
            let shot = frame_transform(frame);
            let forward = shot.rotation * Vec3::NEG_Z;
            let to_speaker = (target + Vec3::Y * 0.25 - shot.translation).normalize();
            assert!(forward.dot(to_speaker) > 0.99);
        }
    }

    #[test]
    fn turn_order_visits_the_nearest_unplayed_standee_in_each_round() {
        let mut dialogue = Vec::new();
        for (index, sender_id) in [1, 2, 3, 1, 2, 3].into_iter().enumerate() {
            let mut line = test_dialogue(
                index as u64 * 1_000,
                600,
                DialogueSide::Right,
            );
            line.sender_id = sender_id;
            line.text = format!("{sender_id}:{index}");
            dialogue.push(line);
        }
        let positions = HashMap::from([
            (1, Vec3::ZERO),
            (2, Vec3::new(10.0, 0.0, 0.0)),
            (3, Vec3::new(2.0, 0.0, 0.0)),
        ]);

        spatially_order_dialogue_turns(&mut dialogue, &positions);

        assert_eq!(
            dialogue
                .iter()
                .map(|line| line.sender_id)
                .collect::<Vec<_>>(),
            vec![1, 3, 2, 1, 3, 2]
        );
        assert_eq!(
            dialogue
                .iter()
                .filter(|line| line.sender_id == 1)
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["1:0", "1:3"]
        );
    }

    #[test]
    fn dialogue_without_a_scene_standee_is_removed() {
        let mut dialogue = vec![
            test_dialogue(0, 600, DialogueSide::Right),
            test_dialogue(1_000, 600, DialogueSide::Right),
        ];
        dialogue[1].sender_id = 2;

        let ignored = retain_dialogue_with_standees(
            &mut dialogue,
            &HashMap::from([(2, Vec3::ZERO)]),
        );

        assert_eq!(ignored, 1);
        assert_eq!(dialogue.len(), 1);
        assert_eq!(dialogue[0].sender_id, 2);
    }

    #[test]
    fn deepseek_director_shot_focuses_the_selected_speaker() {
        let base = Transform::from_xyz(2.0, 3.0, 4.0);
        let dialogue = [test_dialogue(350, 2_700, DialogueSide::Right)];
        let cues = [DirectorCue {
            index: 0,
            text: "打开舱门。".to_owned(),
            speech_text: "打开舱门。".to_owned(),
            shot: DirectorShot::SpeakerClose,
            motion: DirectorMotion::DollyIn,
        }];
        let target = Vec3::new(8.0, 1.0, -3.0);
        let frames = director_camera_track(
            &base,
            &dialogue,
            &cues,
            3_050,
            &HashMap::from([(1, target)]),
        );
        let shot = frame_transform(frames.last().unwrap());
        let forward = shot.rotation * Vec3::NEG_Z;
        let to_speaker = (target + Vec3::Y * 0.35 - shot.translation).normalize();
        assert!(forward.dot(to_speaker) > 0.99);
        assert!(frames.iter().any(|frame| frame.time_ms == 350));
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
        let long_line = dialogue_duration_ms(
            "最近上班没以前忙碌了，做独立游戏的间隔可以让ai大人继续维护老项目了哈哈哈",
        );
        assert!(long_line > 6_000);
        assert!(long_line <= MAX_DIALOGUE_MS);
    }

    #[test]
    fn tts_text_normalization_separates_scripts_and_replaces_unknown_symbols() {
        assert_eq!(
            normalize_tts_text("你好AI大人，test测试🙂OK"),
            "你好 AI 大人，test 测试，OK"
        );
        assert_eq!(
            normalize_tts_text("  你好   world  "),
            "你好 world"
        );
        assert_eq!(
            normalize_tts_text("测试❓中文"),
            "测试，中文"
        );
        assert_eq!(
            chinese_tts_fallback("Steam上的AI有10个方案，另有21个备用方案"),
            "斯地母上的诶艾有十个方案，另有二十一个备用方案"
        );
        assert_eq!(
            chinese_integer_reading("101"),
            "一百零一"
        );
        assert_eq!(
            chinese_integer_reading("10010"),
            "一万零一十"
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
        assert_eq!(HISTORY_DIALOGUE_GAP_MS, 270);
        assert_eq!(MIN_DIALOGUE_MS, 2_700);
        assert_eq!(MAX_DIALOGUE_MS, 9_750);
    }

    #[test]
    fn emotivoice_keeps_configured_speed_and_extends_long_lines() {
        let configured_speed = combined_onnx_speed(18, 1.30);
        assert!(minimum_speech_window_ms("短句", 18, 1.30) < MIN_DIALOGUE_MS);
        assert!(minimum_speech_window_ms(&"很长的中文台词".repeat(8), 18, 1.30) > 4_875);
        assert!((configured_speed - combined_onnx_speed(18, 1.30)).abs() < f32::EPSILON);
        assert_eq!(
            emotivoice_model_text("可以可以"),
            "可以，可以。"
        );
        assert_eq!(
            emotivoice_model_text("可以吗？"),
            "可以吗？"
        );
        assert_eq!(
            emotivoice_model_text("侦测魔法"),
            "侦测魔法。"
        );
        assert_eq!(
            emotivoice_model_text("不要不要！"),
            "不要，不要！"
        );
        assert_eq!(
            emotivoice_model_text("好好"),
            "好好。"
        );
        assert!(
            (effective_emotivoice_speed("可以可以", configured_speed) - 1.10).abs() < f32::EPSILON
        );
        assert!(
            (effective_emotivoice_speed(
                "这是一句足够长的正常台词",
                configured_speed
            ) - configured_speed)
                .abs()
                < f32::EPSILON
        );
        assert_eq!(
            emotivoice_audio_filter("可以可以。", configured_speed),
            "adelay=80,atempo=1.100000,apad=pad_dur=0.180"
        );
        let stretched = [(350, 1_000, 350, 2_000), (1_270, 2_000, 2_270, 3_000)];
        assert_eq!(
            stretched_replay_time(1_000, &stretched),
            2_000
        );
        assert_eq!(
            stretched_replay_time(1_135, &stretched),
            2_135
        );
        assert_eq!(
            stretched_replay_time(2_500, &stretched),
            3_500
        );
        assert!((onnx_speed(18) - 1.09).abs() < 0.001);
        assert_eq!(onnx_speed(180), 1.45);
        assert_eq!(default_master_speech_speed(), 1.30);
        assert!((combined_onnx_speed(18, 1.30) - 1.417).abs() < 0.001);
        assert!((combined_onnx_speed(18, 3.0) - 3.27).abs() < 0.001);
        assert_eq!(
            ffmpeg_atempo_filter(3.0),
            "atempo=2.000000,atempo=1.500000"
        );
        assert_eq!(
            ffmpeg_atempo_filter(0.1),
            "atempo=0.500000,atempo=0.500000,atempo=0.500000,atempo=0.800000"
        );
        assert_eq!(
            normalized_master_speech_speed(20.0),
            20.0
        );
        assert_eq!(
            normalized_master_speech_speed(f32::NAN),
            1.30
        );
        assert_eq!(default_master_dialogue_duration(), 1.0);
        assert_eq!(
            normalized_master_dialogue_duration(250.0),
            250.0
        );
        assert_eq!(scaled_millis(4_000, 2.5), 10_000);
        assert_eq!(
            scaled_dialogue_duration_ms("短句", 2.0),
            MIN_DIALOGUE_MS * 2
        );
    }

    #[test]
    fn replay_audio_defaults_are_clearly_audible() {
        let studio = ReplayStudio::default();
        assert_eq!(studio.music_volume, 0.65);
        assert_eq!(studio.speech_volume, 1.25);
    }

    #[test]
    fn director_plan_parses_strict_json_and_markdown_fallback() {
        let json = r#"{"dialogue":[{"index":0,"text":"AI打开1个舱门。","speech_text":"诶艾打开一个舱门。","shot":"speaker_close","motion":"dolly_in"}]}"#;
        let direct = parse_director_plan(json).unwrap();
        let fenced = parse_director_plan(&format!("```json\n{json}\n```")).unwrap();
        assert_eq!(direct.dialogue.len(), 1);
        assert_eq!(
            fenced.dialogue[0].text,
            "AI打开1个舱门。"
        );
        assert_eq!(
            fenced.dialogue[0].speech_text,
            "诶艾打开一个舱门。"
        );
        assert!(parse_director_plan(r#"{"dialogue":[{"index":0}]}"#).is_err());
    }

    #[test]
    fn preview_cue_tracks_the_visible_dialogue_only() {
        let dialogue = [
            test_dialogue(100, 900, DialogueSide::Left),
            test_dialogue(1_100, 900, DialogueSide::Right),
        ];

        assert_eq!(
            active_dialogue_index(&dialogue, 99),
            None
        );
        assert_eq!(
            active_dialogue_index(&dialogue, 100),
            Some(0)
        );
        assert_eq!(
            active_dialogue_index(&dialogue, 999),
            Some(0)
        );
        assert_eq!(
            active_dialogue_index(&dialogue, 1_000),
            None
        );
        assert_eq!(
            active_dialogue_index(&dialogue, 1_100),
            Some(1)
        );
    }

    #[test]
    fn director_export_is_available_during_preview_and_reuses_an_applied_plan() {
        let mut studio = ReplayStudio::default();
        studio.deepseek_director_enabled = true;
        studio.mode = ReplayMode::Paused;
        assert!(can_start_director_export(&studio, true));

        let replay_json = r#"{"format_version":1,"title":"test","campaign_id":"c","created_at_unix_ms":1,"duration_ms":0,"audience":{"scope":"public"},"scene":{"voxels":[]},"camera":[],"dialogue":[]}"#;
        studio.replay = Some(serde_json::from_str(replay_json).unwrap());
        studio.director_response_hash = Some(7);
        assert!(can_start_director_export(
            &studio, false
        ));

        studio.director_request_pending = true;
        assert!(!can_start_director_export(
            &studio, true
        ));
        studio.director_request_pending = false;
        studio.mode = ReplayMode::Recording;
        assert!(!can_start_director_export(
            &studio, true
        ));
    }

    #[test]
    fn visible_and_spoken_dialogue_text_remain_separate() {
        let mut dialogue = test_dialogue(0, 2_700, DialogueSide::Right);
        dialogue.text = "AI领域有1个方案。".to_owned();
        dialogue.speech_text = Some("诶艾领域有一个方案。".to_owned());
        assert_eq!(dialogue.text, "AI领域有1个方案。");
        assert_eq!(
            speech_text_for_line(&dialogue),
            "诶艾领域有一个方案。"
        );
    }

    #[test]
    fn speaker_voice_profiles_use_stable_spark_tts_profiles() {
        let profiles = (0..8).map(speaker_voice_profile).collect::<Vec<_>>();
        assert!(profiles.iter().all(|(pitch, _)| *pitch == 0));
        assert_eq!(SPARK_TTS_VOICE_PROFILES.len(), 32);
        assert_eq!(default_emotivoice_speaker(0), "spark-m01");
        assert_eq!(default_emotivoice_speaker(1), "spark-m02");
        assert_ne!(
            default_emotivoice_speaker(0),
            default_emotivoice_speaker(3)
        );
        assert_eq!(
            resolved_emotivoice_emotion(None),
            "普通"
        );
        assert_eq!(
            random_emotivoice_speaker(
                &["spark-m01".to_owned(), "spark-f01".to_owned()],
                "spark-m01"
            ),
            Some("spark-f01".to_owned())
        );
        assert_eq!(random_emotivoice_speaker(&[], "spark-m01"), None);
        assert_eq!(
            resolved_emotivoice_speaker(Some("9000"), 0),
            "spark-m01"
        );
    }

    #[test]
    fn replay_voice_settings_are_optional_and_round_trip() {
        let old_json = r#"{"format_version":1,"title":"test","campaign_id":"c","created_at_unix_ms":1,"duration_ms":0,"audience":{"scope":"public"},"scene":{"voxels":[]},"camera":[],"dialogue":[]}"#;
        let mut replay: ReplayFile = serde_json::from_str(old_json).unwrap();
        assert!(replay.speaker_voice_settings.is_empty());
        assert_eq!(replay.master_speech_speed, 1.30);
        assert_eq!(replay.master_dialogue_duration, 1.0);
        replay.master_speech_speed = 1.15;
        replay.master_dialogue_duration = 2.75;
        replay
            .speaker_voice_settings
            .insert(42, SpeakerVoiceSettings {
                voice_name: Some("spark-m01".to_owned()),
                emotion: Some("开心".to_owned()),
                onnx_speaker_id: Some(17),
                pitch: -25,
                speech_rate: 12,
                volume: 0.75,
            });

        let restored: ReplayFile =
            serde_json::from_str(&serde_json::to_string(&replay).unwrap()).unwrap();
        let settings = &restored.speaker_voice_settings[&42];
        assert_eq!(
            settings.voice_name.as_deref(),
            Some("spark-m01")
        );
        assert_eq!(
            settings.emotion.as_deref(),
            Some("开心")
        );
        assert_eq!(settings.pitch, -25);
        assert_eq!(settings.onnx_speaker_id, Some(17));
        assert_eq!(settings.speech_rate, 12);
        assert_eq!(settings.volume, 0.75);
        assert_eq!(restored.master_speech_speed, 1.15);
        assert_eq!(restored.master_dialogue_duration, 2.75);
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

    #[test]
    fn cc0_jrpg_music_is_a_non_silent_stereo_wav() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("music.wav");
        write_jrpg_soundtrack(&path, 250, 0.35).unwrap();
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[22..24], &2_u16.to_le_bytes());
        assert!(bytes[44..].iter().any(|byte| *byte != 0));
    }

    #[test]
    #[ignore = "requires the installed Spark-TTS Chinese runtime"]
    fn spark_tts_synthesizes_distinct_chinese_character_voices() {
        let mut tts = create_onnx_tts().unwrap();
        let chinese = "你好诶艾，我在维护跑团回放。";
        let first = tts.synthesize(chinese, "spark-m01", "普通", 1.4).unwrap();
        let second = tts.synthesize(chinese, "spark-f01", "开心", 1.4).unwrap();
        let unrestricted_fast = tts
            .synthesize(chinese, "spark-m01", "普通", 3.0)
            .unwrap();
        assert_eq!(&first[0..4], b"RIFF");
        assert!(first.len() > 44);
        assert!(unrestricted_fast.len() > 44);
        assert!(unrestricted_fast.len() < first.len());
        assert_ne!(first, second);
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "requires the installed Spark-TTS Chinese runtime"]
    fn tts_assigns_distinct_speaker_profiles() {
        let directory = tempfile::tempdir().unwrap();
        let mut first_line = test_dialogue(0, 1_350, DialogueSide::Left);
        first_line.text = "这是同一句角色语音测试。".to_owned();
        let mut second_line = test_dialogue(1_485, 1_350, DialogueSide::Right);
        second_line.sender_id = 2;
        second_line.text = first_line.text.clone();
        write_narration_track(
            &directory.path().join("narration.wav"),
            directory.path(),
            2_835,
            0.90,
            default_master_speech_speed(),
            &[first_line, second_line],
            &HashMap::new(),
        )
        .unwrap();
        let first = directory.path().join("speech-00000.wav");
        let second = directory.path().join("speech-00001.wav");
        let first_samples = read_pcm16_mono_wav(&first).unwrap();
        assert!(!first_samples.is_empty());
        assert!(
            first_samples.len() <= 32_000 * 1_350 / 1_000,
            "synthesized {} samples for a 1350 ms cue",
            first_samples.len()
        );
        assert_ne!(
            fs::read(first).unwrap(),
            fs::read(second).unwrap()
        );
    }

    #[test]
    #[ignore = "requires FFmpeg, FFprobe, and an offline speech backend"]
    fn encoded_replay_mixes_music_and_character_speech() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..10 {
            image::RgbImage::from_pixel(64, 64, image::Rgb([24, 30, 36]))
                .save(directory.path().join(frame_file_name(index)))
                .unwrap();
        }
        let output = directory.path().join("speech-music-test.mp4");
        let mut dialogue = test_dialogue(0, 900, DialogueSide::Right);
        dialogue.text = "你好，这是角色语音。".to_owned();
        encode_video_frames(
            directory.path(),
            &output,
            10,
            1_000,
            true,
            0.35,
            true,
            0.90,
            default_master_speech_speed(),
            &[dialogue],
            &HashMap::new(),
        )
        .unwrap();
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
            ])
            .arg(output)
            .output()
            .unwrap();
        assert!(probe.status.success());
        assert_eq!(
            String::from_utf8_lossy(&probe.stdout).trim(),
            "aac"
        );
    }

    fn test_dialogue(time_ms: u64, duration_ms: u64, side: DialogueSide) -> ReplayDialogue {
        ReplayDialogue {
            time_ms,
            duration_ms,
            sender_id: 1,
            name: "测试".to_owned(),
            role: String::new(),
            text: "台词".to_owned(),
            speech_text: None,
            avatar: String::new(),
            avatar_data_url: None,
            visibility: Visibility::Public,
            side,
        }
    }
}
