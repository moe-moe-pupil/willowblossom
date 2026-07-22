#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    collections::{
        hash_map::DefaultHasher,
        HashMap,
    },
    fs,
    hash::{
        Hash,
        Hasher,
    },
    io::{
        BufWriter,
        Write,
    },
    path::{
        Path,
        PathBuf,
    },
    process::Command,
    sync::Arc,
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
const MIN_DIALOGUE_MS: u64 = 1_350;
const MAX_DIALOGUE_MS: u64 = 4_875;
const HISTORY_DIALOGUE_GAP_MS: u64 = 135;
const DEFAULT_REPLAY_PATH: &str = ".data/willowblossom/replays/latest.willow-replay.json";
const DEFAULT_VIDEO_PATH: &str = ".data/willowblossom/replays/latest.mp4";
const ONNX_TTS_MODEL_DIR: &str = ".data/willowblossom/tts/kokoro-int8-multi-lang-v1_1";
const ONNX_TTS_SPEAKER_COUNT: i32 = 103;
const ONNX_TTS_FIRST_CHINESE_SPEAKER: i32 = 3;
const ONNX_TTS_CHINESE_SPEAKER_COUNT: i32 = 100;
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
    avatar: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avatar_data_url: Option<String>,
    visibility: Visibility,
    #[serde(default)]
    side: DialogueSide,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpeakerVoiceSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    voice_name: Option<String>,
    #[serde(default)]
    onnx_speaker_id: Option<i32>,
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
    installed_speech_voices: Vec<String>,
    speech_voices_loaded: bool,
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
    speaker_voice_settings: HashMap<u64, SpeakerVoiceSettings>,
    dialogue: Vec<ReplayDialogue>,
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

#[derive(Resource, Default)]
struct PreviewSpeechController {
    active_cue: Option<(u64, usize)>,
    prepared_signature: Option<u64>,
    onnx_cache: HashMap<(u64, usize), (Vec<u8>, f32)>,
    onnx_worker: Option<OnnxPreviewWorker>,
    onnx_failed: bool,
    audio_entity: Option<Entity>,
    #[cfg(windows)]
    windows_worker: Option<PreviewSpeechWorker>,
}

struct OnnxPreviewWorker {
    requests: Sender<OnnxPreviewRequest>,
    results: Receiver<OnnxPreviewResult>,
}

struct OnnxPreviewRequest {
    signature: u64,
    cue: (u64, usize),
    text: String,
    speaker_id: i32,
    speed: f32,
    volume: f32,
}

struct OnnxPreviewResult {
    signature: u64,
    cue: (u64, usize),
    wav: Result<Vec<u8>, String>,
    volume: f32,
}

#[cfg(windows)]
struct PreviewSpeechWorker {
    child: std::process::Child,
    input: std::process::ChildStdin,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum PreviewSpeechRequest<'a> {
    Speak {
        text: &'a str,
        language: &'static str,
        voice_name: Option<&'a str>,
        voice_slot: u64,
        pitch: i32,
        speech_rate: i32,
        volume: u8,
    },
    Stop,
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
            music_enabled: true,
            music_volume: 0.35,
            speech_enabled: true,
            speech_volume: 0.90,
            speech_settings_open: false,
            installed_speech_voices: Vec::new(),
            speech_voices_loaded: false,
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
                eprintln!("failed to prepare ONNX preview speech: {err}");
            }
        }
    }
    let active = (studio.mode == ReplayMode::Playing && studio.speech_enabled)
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
            Err(err) => eprintln!("failed to synthesize ONNX preview speech: {err}"),
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
        if let Err(err) = speech.stop_windows() {
            eprintln!("failed to stop replay preview speech: {err}");
        }
        speech.active_cue = cue;
    }
    let Some((_, _, line)) = active else { return };
    let settings = studio
        .replay
        .as_ref()
        .and_then(|replay| replay.speaker_voice_settings.get(&line.sender_id))
        .cloned()
        .unwrap_or_else(|| default_speaker_voice_settings(line.sender_id));
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
    } else if speech.onnx_failed || !onnx_tts_is_available() {
        if let Err(err) = speech.speak_windows(line, studio.speech_volume, &settings) {
            eprintln!("failed to play replay preview speech: {err}");
        }
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
        if self.prepared_signature.is_some() {
            self.onnx_worker = None;
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
        for (index, line) in replay.dialogue.iter().enumerate() {
            let settings = replay
                .speaker_voice_settings
                .get(&line.sender_id)
                .cloned()
                .unwrap_or_else(|| default_speaker_voice_settings(line.sender_id));
            let rate = fitted_speech_rate(
                settings.speech_rate,
                &line.text,
                line.duration_ms,
            );
            worker
                .requests
                .send(OnnxPreviewRequest {
                    signature,
                    cue: (replay.created_at_unix_ms, index),
                    text: line.text.clone(),
                    speaker_id: onnx_speaker_id(&settings, line.sender_id),
                    speed: onnx_speed(rate),
                    volume: (global_volume * settings.volume).clamp(0.0, 1.0),
                })
                .map_err(|err| format!("ONNX preview worker stopped: {err}"))?;
        }
        Ok(())
    }

    fn onnx_cue_ready(&self, signature: u64, cue: (u64, usize)) -> bool {
        self.prepared_signature == Some(signature) && self.onnx_cache.contains_key(&cue)
    }

    fn speak_windows(
        &mut self,
        line: &ReplayDialogue,
        global_volume: f32,
        settings: &SpeakerVoiceSettings,
    ) -> Result<(), String> {
        self.send(PreviewSpeechRequest::Speak {
            text: &line.text,
            language: dialogue_language(&line.text),
            voice_name: settings.voice_name.as_deref(),
            voice_slot: line.sender_id,
            pitch: settings.pitch,
            speech_rate: ssml_rate_percent(fitted_speech_rate(
                settings.speech_rate,
                &line.text,
                line.duration_ms,
            )),
            volume: (global_volume * settings.volume)
                .clamp(0.0, 1.0)
                .mul_add(100.0, 0.0)
                .round() as u8,
        })
    }

    fn stop_windows(&mut self) -> Result<(), String> {
        self.send_if_running(PreviewSpeechRequest::Stop)
    }

    #[cfg(windows)]
    fn send(&mut self, request: PreviewSpeechRequest<'_>) -> Result<(), String> {
        if self.windows_worker.is_none() {
            self.windows_worker = Some(start_preview_speech_worker()?);
        }
        self.send_if_running(request)
    }

    #[cfg(not(windows))]
    fn send(&mut self, _request: PreviewSpeechRequest<'_>) -> Result<(), String> {
        Err("replay preview speech currently requires Windows offline TTS".to_owned())
    }

    #[cfg(windows)]
    fn send_if_running(&mut self, request: PreviewSpeechRequest<'_>) -> Result<(), String> {
        let Some(worker) = self.windows_worker.as_mut() else { return Ok(()) };
        let encoded = preview_speech_wire_line(&request)?;
        if let Err(err) = worker
            .input
            .write_all(&encoded)
            .and_then(|_| worker.input.flush())
        {
            let _ = worker.child.kill();
            let _ = worker.child.wait();
            self.windows_worker = None;
            return Err(format!(
                "preview speech worker stopped: {err}"
            ));
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn send_if_running(&mut self, _request: PreviewSpeechRequest<'_>) -> Result<(), String> {
        Ok(())
    }
}

fn preview_speech_wire_line(request: &PreviewSpeechRequest<'_>) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(request)
        .map_err(|err| format!("unable to prepare preview speech: {err}"))?;
    let mut encoded = BASE64.encode(json).into_bytes();
    encoded.push(b'\n');
    Ok(encoded)
}

#[cfg(windows)]
impl Drop for PreviewSpeechController {
    fn drop(&mut self) {
        if let Some(worker) = self.windows_worker.as_mut() {
            let _ = worker.child.kill();
            let _ = worker.child.wait();
        }
    }
}

#[cfg(windows)]
fn start_preview_speech_worker() -> Result<PreviewSpeechWorker, String> {
    use std::process::Stdio;

    let script = r#"& { Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.SetOutputToDefaultAudioDevice(); while (($line = [Console]::In.ReadLine()) -ne $null) { try { $json = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($line)); $job = $json | ConvertFrom-Json; $s.SpeakAsyncCancelAll(); if ([string]$job.op -eq 'stop') { continue }; $selected = $false; if (-not [string]::IsNullOrWhiteSpace([string]$job.voice_name)) { $candidate = @($s.GetInstalledVoices() | Where-Object { $_.Enabled -and $_.VoiceInfo.Name -eq [string]$job.voice_name -and ([string]$job.language -ne 'zh-CN' -or $_.VoiceInfo.Culture.Name -eq 'zh-CN') }) | Select-Object -First 1; if ($null -ne $candidate) { $s.SelectVoice($candidate.VoiceInfo.Name); $selected = $true } }; if (-not $selected) { $culture = [Globalization.CultureInfo]::GetCultureInfo([string]$job.language); $voices = @($s.GetInstalledVoices($culture) | Where-Object { $_.Enabled }); if ($voices.Count -eq 0) { $voices = @($s.GetInstalledVoices() | Where-Object { $_.Enabled }) }; if ($voices.Count -eq 0) { continue }; $voice = $voices[[int64]$job.voice_slot % $voices.Count]; $s.SelectVoice($voice.VoiceInfo.Name) }; $s.Volume = [Math]::Max(0, [Math]::Min(100, [int]$job.volume)); $escaped = [Security.SecurityElement]::Escape([string]$job.text); $pitch = [int]$job.pitch; $speechRate = [int]$job.speech_rate; $language = [string]$job.language; $ssml = "<speak version='1.0' xml:lang='$language'><prosody pitch='$pitch%' rate='$speechRate%'>$escaped</prosody></speak>"; $null = $s.SpeakSsmlAsync($ssml) } catch {} }; $s.SpeakAsyncCancelAll(); $s.Dispose() }"#;
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_command_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|err| format!("unable to start Windows preview speech: {err}"))?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| "Windows preview speech did not open its input stream".to_owned())?;
    Ok(PreviewSpeechWorker { child, input })
}

fn onnx_tts_model_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(ONNX_TTS_MODEL_DIR)
}

fn onnx_tts_is_available() -> bool {
    let root = onnx_tts_model_dir();
    [
        "model.int8.onnx",
        "voices.bin",
        "lexicon-us-en.txt",
        "lexicon-zh.txt",
        "tokens.txt",
        "phone-zh.fst",
        "date-zh.fst",
        "number-zh.fst",
    ]
    .iter()
    .all(|name| root.join(name).is_file())
}

fn create_onnx_tts() -> Result<sherpa_onnx::OfflineTts, String> {
    use sherpa_onnx::{
        OfflineTtsConfig,
        OfflineTtsKokoroModelConfig,
        OfflineTtsModelConfig,
    };

    if !onnx_tts_is_available() {
        return Err(format!(
            "未找到中文 ONNX 语音模型：{ONNX_TTS_MODEL_DIR}"
        ));
    }
    let root = onnx_tts_model_dir();
    let path = |name: &str| root.join(name).to_string_lossy().into_owned();
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            kokoro: OfflineTtsKokoroModelConfig {
                model: Some(path("model.int8.onnx")),
                voices: Some(path("voices.bin")),
                tokens: Some(path("tokens.txt")),
                data_dir: Some(path("espeak-ng-data")),
                lexicon: Some(
                    ["lexicon-us-en.txt", "lexicon-zh.txt"]
                        .map(|name| path(name))
                        .join(","),
                ),
                ..Default::default()
            },
            num_threads: thread::available_parallelism()
                .map(|threads| threads.get().min(4) as i32)
                .unwrap_or(2),
            provider: Some("cpu".to_owned()),
            ..Default::default()
        },
        rule_fsts: Some(
            ["phone-zh.fst", "date-zh.fst", "number-zh.fst"]
                .map(|name| path(name))
                .join(","),
        ),
        max_num_sentences: 1,
        silence_scale: 0.12,
        ..Default::default()
    };
    sherpa_onnx::OfflineTts::create(&config)
        .ok_or_else(|| "无法加载 Kokoro 中英文 ONNX 语音模型".to_owned())
}

fn start_onnx_preview_worker() -> Result<OnnxPreviewWorker, String> {
    let tts = create_onnx_tts()?;
    let (request_tx, request_rx) = unbounded::<OnnxPreviewRequest>();
    let (result_tx, result_rx) = unbounded::<OnnxPreviewResult>();
    thread::Builder::new()
        .name("replay-onnx-preview".to_owned())
        .spawn(move || {
            while let Ok(request) = request_rx.recv() {
                let wav = generate_onnx_wav(
                    &tts,
                    &request.text,
                    request.speaker_id,
                    request.speed,
                );
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
        .map_err(|err| format!("无法启动 ONNX 预览线程：{err}"))?;
    Ok(OnnxPreviewWorker {
        requests: request_tx,
        results: result_rx,
    })
}

fn generate_onnx_wav(
    tts: &sherpa_onnx::OfflineTts,
    text: &str,
    speaker_id: i32,
    speed: f32,
) -> Result<Vec<u8>, String> {
    use sherpa_onnx::GenerationConfig;

    let audio = tts
        .generate_with_config(
            text,
            &GenerationConfig {
                sid: speaker_id.clamp(0, ONNX_TTS_SPEAKER_COUNT - 1),
                speed: speed.clamp(0.85, 1.45),
                silence_scale: 0.12,
                ..Default::default()
            },
            None::<fn(&[f32], f32) -> bool>,
        )
        .ok_or_else(|| "ONNX 未能生成语音".to_owned())?;
    let source_rate = tts.sample_rate().max(1) as u32;
    let samples = resample_f32_to_pcm16(audio.samples(), source_rate, 32_000);
    let data_bytes = samples.len().saturating_mul(2);
    let mut wav = Vec::with_capacity(44 + data_bytes);
    write_wav_header(
        &mut wav,
        32_000,
        1,
        16,
        data_bytes as u32,
    )?;
    for sample in samples {
        wav.write_all(&sample.to_le_bytes())
            .map_err(|err| format!("写入 ONNX WAV 失败：{err}"))?;
    }
    Ok(wav)
}

fn resample_f32_to_pcm16(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<i16> {
    if samples.is_empty() {
        return Vec::new();
    }
    let output_len = samples
        .len()
        .saturating_mul(target_rate as usize)
        .div_ceil(source_rate as usize);
    (0..output_len)
        .map(|index| {
            let position = index as f64 * source_rate as f64 / target_rate as f64;
            let left = position.floor() as usize;
            let right = (left + 1).min(samples.len() - 1);
            let fraction = (position - left as f64) as f32;
            pcm_i16(samples[left] + (samples[right] - samples[left]) * fraction)
        })
        .collect()
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
            "正在渲染视频 {}/{}（此阶段静音，完成后加入音乐和语音；Esc 取消）",
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
    ui.label("自定义整理要求");
    let prompt_width = ui.available_width().clamp(220.0, 560.0);
    ui.add(
        egui::TextEdit::multiline(&mut studio.deepseek_custom_prompt)
            .desired_rows(3)
            .desired_width(prompt_width)
            .hint_text("例如：重点列出未解决问题；保留角色和地点原名；措辞更简洁"),
    );
    let prompt_chars = studio.deepseek_custom_prompt.chars().count();
    if prompt_chars > DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS {
        ui.colored_label(
            egui::Color32::from_rgb(210, 90, 70),
            format!("已输入 {prompt_chars} 字；仅发送前 {DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS} 字"),
        );
    } else {
        ui.small(format!(
            "{prompt_chars}/{DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS} 字；只影响事实提要的格式与重点，不能扩大可见范围或续写剧情"
        ));
    }
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
                    &studio.deepseek_custom_prompt,
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
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut studio.music_enabled,
            "原创舒缓纯音乐",
        );
        ui.add_enabled(
            studio.music_enabled,
            egui::Slider::new(&mut studio.music_volume, 0.05..=0.60)
                .text("音量")
                .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
        );
    });
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut studio.speech_enabled,
            "角色语音（预览与导出，ONNX 中文离线 TTS）",
        );
        ui.add_enabled(
            studio.speech_enabled,
            egui::Slider::new(&mut studio.speech_volume, 0.20..=1.00)
                .text("语音音量")
                .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
        );
        if ui.button("角色语音设置…").clicked() {
            studio.speech_settings_open = true;
            if !studio.speech_voices_loaded {
                match installed_speech_voice_names() {
                    Ok(voices) => {
                        studio.installed_speech_voices = voices;
                        studio.speech_voices_loaded = true;
                        if let Some(replay) = studio.replay.as_mut() {
                            for settings in replay.speaker_voice_settings.values_mut() {
                                if settings.voice_name.as_ref().is_some_and(|voice| {
                                    !studio.installed_speech_voices.contains(voice)
                                }) {
                                    settings.voice_name = None;
                                }
                            }
                        }
                    },
                    Err(err) => studio.status = format!("读取 Windows 语音失败：{err}"),
                }
            }
        }
    });
    ui.small("台词默认使用本地 Kokoro 24 kHz ONNX（100 种中英文角色音色），模型不可用时回退到 Windows TTS。预览会随台词同步停止；导出逐帧阶段保持静音，最后一次性混合语音和音乐。语音仅处理已通过发布范围筛选的回放台词，不上传网络。");
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
            let speaker_positions = standees
                .iter()
                .map(|(transform, standee)| (standee.user_id, transform.translation))
                .collect::<HashMap<_, _>>();
            replay.camera = automatic_camera_track(
                base_camera,
                &replay.dialogue,
                replay.duration_ms,
                &speaker_positions,
            );
        }
        let summary_queued = studio.replay.as_ref().is_some_and(|replay| {
            queue_replay_summary(
                replay,
                deepseek_sender,
                deepseek_manager,
                &studio.deepseek_custom_prompt,
            )
            .is_ok()
        });
        if summary_queued {
            let _ = deepseek_manager.persist();
        }
        start_video_export(studio, capture_active, grids, windows);
    }
    ui.small("一键模式会自动完成：读取可见聊天 → 请求事实性制作提要 → 编排平滑镜头 → 逐帧渲染 → 生成角色语音 → FFmpeg 输出 MP4。");
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

fn speech_settings_window(ctx: &egui::Context, studio: &mut ReplayStudio) {
    let mut open = studio.speech_settings_open;
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
    let voices = studio.installed_speech_voices.clone();
    let mut settings_changed = false;

    egui::Window::new("角色语音设置")
        .id(egui::Id::new("replay-speaker-voice-settings"))
        .open(&mut open)
        .default_width(520.0)
        .max_width(620.0)
        .show(ctx, |ui| {
            ui.label("为每个角色分配 Kokoro 的独立中文音色（3–57 女声，58–102 男声），并调整基础语速和相对音量。自动加速已限制在清晰范围内，不会再用变调重采样强塞台词；设置同时用于播放预览和 MP4 导出。Windows 声音和音调仅用于 ONNX 不可用时的回退。");
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
                        let speaker_id = settings
                            .onnx_speaker_id
                            .get_or_insert_with(|| default_onnx_speaker_id(*sender_id));
                        if !(ONNX_TTS_FIRST_CHINESE_SPEAKER..ONNX_TTS_SPEAKER_COUNT)
                            .contains(speaker_id)
                        {
                            *speaker_id = default_onnx_speaker_id(*sender_id);
                        }
                        settings_changed |= ui
                            .add(egui::Slider::new(speaker_id, 3..=102).text("Kokoro 中文音色"))
                            .changed();
                        egui::ComboBox::from_id_salt(("replay-voice", sender_id))
                            .selected_text(settings.voice_name.as_deref().unwrap_or("自动分配"))
                            .width(300.0)
                            .show_ui(ui, |ui| {
                                settings_changed |= ui
                                    .selectable_value(&mut settings.voice_name, None, "自动分配")
                                    .changed();
                                for voice in &voices {
                                    settings_changed |= ui
                                        .selectable_value(
                                            &mut settings.voice_name,
                                            Some(voice.clone()),
                                            voice,
                                        )
                                        .changed();
                                }
                            });
                        settings_changed |= ui
                            .add(egui::Slider::new(&mut settings.pitch, -50..=20).text("回退音调"))
                            .changed();
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
        "已从现有聊天生成 {0} 条连续对话；切换间隔约 0.135 秒",
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
    custom_prompt: &str,
) -> Result<(), String> {
    if replay.dialogue.is_empty() {
        return Err("回放中没有可整理的台词".to_owned());
    }
    let sender = sender.ok_or_else(|| "DeepSeek 连接尚未就绪，请稍后重试".to_owned())?;
    let summary_key = replay_summary_key(replay);
    let message_count = replay.dialogue.len();
    if let Some(block) = replay_summary_block(replay, manager) {
        if block.pending {
            return Err("这版台词正在整理，请等待当前请求完成".to_owned());
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
        speaker_voice_settings: HashMap::new(),
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
    let Some((duration_ms, dialogue, speaker_voice_settings)) =
        studio.replay.as_ref().map(|replay| {
            (
                replay.duration_ms,
                replay.dialogue.clone(),
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
        speaker_voice_settings,
        dialogue,
        total_frames,
        next_frame: 0,
        capture_pending: false,
        pending_seconds: 0.0,
        warmup_frames: VIDEO_CAPTURE_WARMUP_FRAMES,
        failure: None,
        original_window_title,
        original_window_resizable,
    });
    studio.status =
        format!("正在逐帧渲染 {total_frames} 帧；截图阶段静音，全部帧完成后自动合成音乐和角色语音");
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

#[cfg(windows)]
fn check_speech_synthesizer() -> Result<(), String> {
    if onnx_tts_is_available() {
        return Ok(());
    }
    let script = r#"Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $voices = @($s.GetInstalledVoices() | Where-Object { $_.Enabled }); $s.Dispose(); if ($voices.Count -eq 0) { exit 2 }"#;
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    hide_command_window(&mut command);
    command
        .output()
        .map_err(|err| format!("无法启动 Windows 语音合成：{err}"))
        .and_then(|output| {
            output.status.success().then_some(()).ok_or_else(|| {
                "未找到可用的 Windows 语音。请在系统语言设置中安装语音，或关闭“角色语音”后导出"
                    .to_owned()
            })
        })
}

#[cfg(not(windows))]
fn check_speech_synthesizer() -> Result<(), String> {
    onnx_tts_is_available()
        .then_some(())
        .ok_or_else(|| "未找到中文 ONNX 语音模型；请关闭角色语音后导出".to_owned())
}

#[cfg(windows)]
fn installed_speech_voice_names() -> Result<Vec<String>, String> {
    let script = r#"[Console]::OutputEncoding = [Text.Encoding]::UTF8; Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.GetInstalledVoices() | Where-Object { $_.Enabled -and $_.VoiceInfo.Culture.Name -eq 'zh-CN' } | ForEach-Object { $_.VoiceInfo.Name }; $s.Dispose()"#;
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    hide_command_window(&mut command);
    let output = command
        .output()
        .map_err(|err| format!("无法启动 Windows 语音列表：{err}"))?;
    if !output.status.success() {
        return Err(format!(
            "Windows 语音列表退出码：{}",
            output.status
        ));
    }
    let mut voices = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    voices.sort();
    voices.dedup();
    if voices.is_empty() {
        Err(
            "未找到已安装的中文（zh-CN）Windows 语音，请先在 Windows 语言设置中安装中文语音"
                .to_owned(),
        )
    } else {
        Ok(voices)
    }
}

#[cfg(not(windows))]
fn installed_speech_voice_names() -> Result<Vec<String>, String> {
    Err("角色语音设置目前需要 Windows 离线 TTS".to_owned())
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
    dialogue: &[ReplayDialogue],
    speaker_voice_settings: &HashMap<u64, SpeakerVoiceSettings>,
) -> Result<(), String> {
    let frame_pattern = frames_path.join("frame_%06d.png");
    let soundtrack_path = frames_path.join("original-relaxing-soundtrack.wav");
    let narration_path = frames_path.join("character-narration.wav");
    if music_enabled {
        write_relaxing_soundtrack(
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
    language: &'static str,
    voice_name: Option<String>,
    voice_slot: u64,
    onnx_speaker_id: i32,
    pitch: i32,
    speech_rate: i32,
}

fn write_narration_track(
    path: &Path,
    working_directory: &Path,
    duration_ms: u64,
    volume: f32,
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
                text: line.text.clone(),
                output_path: working_directory
                    .join(format!("speech-{index:05}.wav"))
                    .to_string_lossy()
                    .into_owned(),
                language: dialogue_language(&line.text),
                voice_name: settings.voice_name.clone(),
                voice_slot: line.sender_id,
                onnx_speaker_id: onnx_speaker_id(&settings, line.sender_id),
                pitch: settings.pitch,
                speech_rate: ssml_rate_percent(fitted_speech_rate(
                    settings.speech_rate,
                    &line.text,
                    line.duration_ms,
                )),
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
    let volume = volume.clamp(0.0, 1.0);
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

fn speaker_voice_profile(sender_id: u64) -> (i32, i32) {
    const PITCHES: [i32; 8] = [-12, -6, -10, -4, -11, -7, -14, -5];
    const RATES: [i32; 8] = [18, 24, 14, 28, 10, 21, 16, 26];
    let profile = (sender_id as usize) % PITCHES.len();
    (PITCHES[profile], RATES[profile])
}

fn fitted_speech_rate(base_rate: i32, text: &str, duration_ms: u64) -> i32 {
    let estimated_ms = text.chars().fold(250_u64, |total, character| {
        let character_ms = match character {
            '。' | '！' | '？' | '!' | '?' | '；' | ';' => 280,
            '，' | ',' | '、' | '：' | ':' => 160,
            character if character.is_whitespace() => 15,
            character if is_cjk_character(character) => 250,
            _ => 45,
        };
        total.saturating_add(character_ms)
    });
    let speaking_window_ms = duration_ms.saturating_sub(120).max(1);
    let required_rate = estimated_ms
        .saturating_mul(100)
        .saturating_add(speaking_window_ms - 1)
        / speaking_window_ms;
    let required_percent_faster = required_rate.saturating_sub(100) as i32;
    base_rate.max(required_percent_faster).clamp(-30, 180)
}

fn ssml_rate_percent(relative_rate: i32) -> i32 { (100 + relative_rate).clamp(70, 280) }

fn default_speaker_voice_settings(sender_id: u64) -> SpeakerVoiceSettings {
    let (pitch, speech_rate) = speaker_voice_profile(sender_id);
    SpeakerVoiceSettings {
        voice_name: None,
        onnx_speaker_id: Some(default_onnx_speaker_id(sender_id)),
        pitch,
        speech_rate,
        volume: 1.0,
    }
}

fn onnx_speaker_id(settings: &SpeakerVoiceSettings, sender_id: u64) -> i32 {
    settings
        .onnx_speaker_id
        .filter(|speaker_id| {
            (ONNX_TTS_FIRST_CHINESE_SPEAKER..ONNX_TTS_SPEAKER_COUNT).contains(speaker_id)
        })
        .unwrap_or_else(|| default_onnx_speaker_id(sender_id))
}

fn default_onnx_speaker_id(sender_id: u64) -> i32 {
    ONNX_TTS_FIRST_CHINESE_SPEAKER + (sender_id % ONNX_TTS_CHINESE_SPEAKER_COUNT as u64) as i32
}

fn replay_voice_signature(replay: &ReplayFile, global_volume: f32) -> u64 {
    let mut hasher = DefaultHasher::new();
    replay.created_at_unix_ms.hash(&mut hasher);
    global_volume.to_bits().hash(&mut hasher);
    for line in &replay.dialogue {
        line.sender_id.hash(&mut hasher);
        line.duration_ms.hash(&mut hasher);
        line.text.hash(&mut hasher);
    }
    let mut settings = replay.speaker_voice_settings.iter().collect::<Vec<_>>();
    settings.sort_by_key(|(sender_id, _)| **sender_id);
    for (sender_id, voice) in settings {
        sender_id.hash(&mut hasher);
        voice.voice_name.hash(&mut hasher);
        voice.onnx_speaker_id.hash(&mut hasher);
        voice.pitch.hash(&mut hasher);
        voice.speech_rate.hash(&mut hasher);
        voice.volume.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

fn onnx_speed(relative_rate: i32) -> f32 { (1.0 + relative_rate as f32 / 200.0).clamp(0.85, 1.45) }

fn dialogue_language(text: &str) -> &'static str {
    if text.chars().any(is_cjk_character) {
        "zh-CN"
    } else {
        "en-US"
    }
}

fn synthesize_speech_batch(
    working_directory: &Path,
    jobs: &[SpeechSynthesisJob],
) -> Result<(), String> {
    if jobs.is_empty() {
        return Ok(());
    }
    let mut windows_jobs = Vec::new();
    if let Ok(tts) = create_onnx_tts() {
        for job in jobs {
            let wav = generate_onnx_wav(
                &tts,
                &job.text,
                job.onnx_speaker_id,
                onnx_speed(job.speech_rate - 100),
            )?;
            fs::write(&job.output_path, wav)
                .map_err(|err| format!("无法保存 ONNX 角色语音：{err}"))?;
        }
    } else {
        windows_jobs.extend(jobs);
    }
    if windows_jobs.is_empty() {
        Ok(())
    } else {
        synthesize_windows_speech_batch(working_directory, &windows_jobs)
    }
}

#[cfg(windows)]
fn synthesize_windows_speech_batch(
    working_directory: &Path,
    jobs: &[&SpeechSynthesisJob],
) -> Result<(), String> {
    if jobs.is_empty() {
        return Ok(());
    }
    let manifest_path = working_directory.join("speech-jobs.json");
    let manifest = serde_json::to_vec(&serde_json::json!({ "jobs": jobs }))
        .map_err(|err| format!("无法整理角色语音任务：{err}"))?;
    fs::write(&manifest_path, manifest).map_err(|err| format!("无法保存角色语音任务：{err}"))?;
    let script = r#"& { param($manifestPath) Add-Type -AssemblyName System.Speech; $manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json; $jobs = @($manifest.jobs); $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo -ArgumentList 32000,([System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen),([System.Speech.AudioFormat.AudioChannel]::Mono); foreach ($job in $jobs) { $selected = $false; if (-not [string]::IsNullOrWhiteSpace([string]$job.voice_name)) { $candidate = @($s.GetInstalledVoices() | Where-Object { $_.Enabled -and $_.VoiceInfo.Name -eq [string]$job.voice_name -and ([string]$job.language -ne 'zh-CN' -or $_.VoiceInfo.Culture.Name -eq 'zh-CN') }) | Select-Object -First 1; if ($null -ne $candidate) { $s.SelectVoice($candidate.VoiceInfo.Name); $selected = $true } }; if (-not $selected) { $culture = [Globalization.CultureInfo]::GetCultureInfo([string]$job.language); $voices = @($s.GetInstalledVoices($culture) | Where-Object { $_.Enabled }); if ($voices.Count -eq 0) { $voices = @($s.GetInstalledVoices() | Where-Object { $_.Enabled }) }; if ($voices.Count -eq 0) { throw 'No installed speech voice' }; $voice = $voices[[int64]$job.voice_slot % $voices.Count]; $s.SelectVoice($voice.VoiceInfo.Name) }; $s.SetOutputToWaveFile([string]$job.output_path, $format); $escaped = [Security.SecurityElement]::Escape([string]$job.text); $pitch = [int]$job.pitch; $speechRate = [int]$job.speech_rate; $language = [string]$job.language; $ssml = "<speak version='1.0' xml:lang='$language'><prosody pitch='$pitch%' rate='$speechRate%'>$escaped</prosody></speak>"; $s.SpeakSsml($ssml); $s.SetOutputToNull() }; $s.Dispose() }"#;
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .arg(&manifest_path);
    hide_command_window(&mut command);
    let output = command
        .output()
        .map_err(|err| format!("无法启动 Windows 语音合成：{err}"))?;
    if output.status.success() && jobs.iter().all(|job| Path::new(&job.output_path).exists()) {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if detail.is_empty() {
        format!(
            "Windows 语音合成退出码：{}",
            output.status
        )
    } else {
        detail
    })
}

#[cfg(not(windows))]
fn synthesize_windows_speech_batch(
    _working_directory: &Path,
    _jobs: &[&SpeechSynthesisJob],
) -> Result<(), String> {
    Err("当前系统不支持 Windows 离线语音合成".to_owned())
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

fn write_relaxing_soundtrack(path: &Path, duration_ms: u64, volume: f32) -> Result<(), String> {
    const SAMPLE_RATE: u32 = 32_000;
    const CHANNELS: u16 = 2;
    const BITS_PER_SAMPLE: u16 = 16;
    const CHORDS: [[f32; 3]; 4] = [
        [220.00, 261.63, 329.63],
        [174.61, 220.00, 261.63],
        [130.81, 164.81, 196.00],
        [196.00, 246.94, 293.66],
    ];
    let sample_count = duration_ms
        .saturating_mul(SAMPLE_RATE as u64)
        .saturating_add(999)
        / 1_000;
    let data_bytes = sample_count
        .saturating_mul(CHANNELS as u64)
        .saturating_mul((BITS_PER_SAMPLE / 8) as u64);
    if data_bytes > (u32::MAX - 36) as u64 {
        return Err("回放过长，无法生成 WAV 背景音乐".to_owned());
    }
    let file = fs::File::create(path).map_err(|err| format!("无法创建背景音乐：{err}"))?;
    let mut writer = BufWriter::new(file);
    write_wav_header(
        &mut writer,
        SAMPLE_RATE,
        CHANNELS,
        BITS_PER_SAMPLE,
        data_bytes as u32,
    )?;
    let duration_seconds = duration_ms as f32 / 1_000.0;
    let fade_in_seconds = (duration_seconds * 0.20).clamp(0.10, 3.0);
    let fade_out_seconds = (duration_seconds * 0.20).clamp(0.10, 4.0);
    let volume = volume.clamp(0.0, 1.0);
    for index in 0..sample_count {
        let time = index as f32 / SAMPLE_RATE as f32;
        let section = ((time / 8.0).floor() as usize) % CHORDS.len();
        let next_section = (section + 1) % CHORDS.len();
        let section_time = time % 8.0;
        let crossfade = smoothstep(((section_time - 7.0) / 1.0).clamp(0.0, 1.0));
        let current_pad = chord_wave(CHORDS[section], time, 0.0);
        let next_pad = chord_wave(CHORDS[next_section], time, 0.0);
        let current_pad_right = chord_wave(CHORDS[section], time, 0.08);
        let next_pad_right = chord_wave(CHORDS[next_section], time, 0.08);
        let pad_left = current_pad * (1.0 - crossfade) + next_pad * crossfade;
        let pad_right = current_pad_right * (1.0 - crossfade) + next_pad_right * crossfade;
        let arpeggio_step = ((time / 2.0).floor() as usize) % 3;
        let arpeggio_time = time % 2.0;
        let arpeggio_envelope = (std::f32::consts::PI * arpeggio_time / 2.0)
            .sin()
            .max(0.0)
            .powi(2);
        let arpeggio_frequency = CHORDS[section][arpeggio_step] * 2.0;
        let arpeggio_left = (std::f32::consts::TAU * arpeggio_frequency * time).sin();
        let arpeggio_right = (std::f32::consts::TAU * arpeggio_frequency * time + 0.18).sin();
        let breathing = 0.88 + 0.12 * (std::f32::consts::TAU * 0.06 * time).sin();
        let fade_in = (time / fade_in_seconds).clamp(0.0, 1.0);
        let fade_out = ((duration_seconds - time) / fade_out_seconds).clamp(0.0, 1.0);
        let envelope = smoothstep(fade_in) * smoothstep(fade_out) * breathing * volume;
        let left = (pad_left * 0.28 + arpeggio_left * arpeggio_envelope * 0.07) * envelope;
        let right = (pad_right * 0.28 + arpeggio_right * arpeggio_envelope * 0.07) * envelope;
        writer
            .write_all(&pcm_i16(left).to_le_bytes())
            .and_then(|_| writer.write_all(&pcm_i16(right).to_le_bytes()))
            .map_err(|err| format!("写入背景音乐失败：{err}"))?;
    }
    writer
        .flush()
        .map_err(|err| format!("完成背景音乐失败：{err}"))
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

fn chord_wave(frequencies: [f32; 3], time: f32, phase: f32) -> f32 {
    frequencies
        .into_iter()
        .enumerate()
        .map(|(index, frequency)| {
            (std::f32::consts::TAU * frequency * time + phase * (index + 1) as f32).sin()
        })
        .sum::<f32>()
        / 3.0
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

fn automatic_camera_track(
    base: &Transform,
    dialogue: &[ReplayDialogue],
    duration_ms: u64,
    speaker_positions: &HashMap<u64, Vec3>,
) -> Vec<ReplayCameraKeyframe> {
    let mut frames = Vec::with_capacity(dialogue.len().saturating_mul(3).saturating_add(2));
    frames.push(camera_keyframe(0, base));
    let mut current = base.clone();
    for (index, line) in dialogue.iter().enumerate() {
        frames.push(camera_keyframe(line.time_ms, &current));
        let line_end = line.time_ms.saturating_add(line.duration_ms);
        if let Some(target) = speaker_positions.get(&line.sender_id) {
            let arrival_ms = line
                .time_ms
                .saturating_add((line.duration_ms / 3).clamp(160, 360))
                .min(line_end);
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
            frames.push(camera_keyframe(arrival_ms, &focused));
            frames.push(camera_keyframe(line_end, &settled));
            current = settled;
        } else {
            let drifted = fallback_camera_drift(&current, line.sender_id, index);
            frames.push(camera_keyframe(line_end, &drifted));
            current = drifted;
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

fn fallback_camera_drift(current: &Transform, sender_id: u64, index: usize) -> Transform {
    let right = current.rotation * Vec3::X;
    let up = current.rotation * Vec3::Y;
    let forward = current.rotation * Vec3::NEG_Z;
    let horizontal = (cinematic_seed(sender_id, index) - 0.5) * 0.9;
    let vertical = (cinematic_seed(sender_id.rotate_left(17), index) - 0.5) * 0.35;
    let dolly = 0.18 + cinematic_seed(sender_id.rotate_left(31), index) * 0.32;
    let position = current.translation + right * horizontal + up * vertical + forward * dolly;
    let focus = current.translation + forward * 18.0 + right * horizontal * 0.7;
    Transform::from_translation(position).looking_at(focus, Vec3::Y)
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
        .saturating_div(2)
        .clamp(MIN_DIALOGUE_MS, MAX_DIALOGUE_MS)
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
    fn automatic_director_focuses_known_speakers() {
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
        let frames = automatic_camera_track(
            &base,
            &dialogue,
            5_430,
            &speaker_positions,
        );
        assert!(frames
            .windows(2)
            .all(|pair| pair[0].time_ms < pair[1].time_ms));
        assert_eq!(
            frames.first().unwrap().translation,
            base.translation.to_array()
        );
        for (arrival_ms, target) in [(710, speaker_positions[&1]), (3_390, speaker_positions[&2])] {
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
    fn automatic_director_drifts_when_speaker_has_no_standee() {
        let base = Transform::from_xyz(2.0, 3.0, 4.0);
        let dialogue = [test_dialogue(350, 1_200, DialogueSide::Right)];
        let frames = automatic_camera_track(&base, &dialogue, 1_550, &HashMap::new());
        assert_ne!(
            frames.last().unwrap().translation,
            base.translation.to_array()
        );
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
        assert_eq!(HISTORY_DIALOGUE_GAP_MS, 135);
        assert_eq!(MIN_DIALOGUE_MS, 1_350);
        assert_eq!(MAX_DIALOGUE_MS, 4_875);
    }

    #[test]
    fn speech_rate_is_valid_ssml_and_auto_fits_long_lines() {
        assert_eq!(ssml_rate_percent(18), 118);
        assert_eq!(ssml_rate_percent(-30), 70);
        assert_eq!(
            fitted_speech_rate(18, "短句", 4_875),
            18
        );
        assert!(fitted_speech_rate(18, &"很长的中文台词".repeat(8), 4_875) > 18);
        assert!((onnx_speed(18) - 1.09).abs() < 0.001);
        assert_eq!(onnx_speed(180), 1.45);
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
    fn preview_speech_wire_preserves_chinese_utf8() {
        let request = PreviewSpeechRequest::Speak {
            text: "萌萌打开了舱门。",
            language: "zh-CN",
            voice_name: None,
            voice_slot: 42,
            pitch: -18,
            speech_rate: 10,
            volume: 90,
        };

        let wire = preview_speech_wire_line(&request).unwrap();
        assert!(wire[..wire.len() - 1].iter().all(u8::is_ascii));
        let json = BASE64.decode(&wire[..wire.len() - 1]).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(value["text"], "萌萌打开了舱门。");
        assert_eq!(value["language"], "zh-CN");
    }

    #[test]
    fn speaker_voice_profiles_use_distinct_lower_pitches() {
        let profiles = (0..8).map(speaker_voice_profile).collect::<Vec<_>>();
        assert!(profiles.iter().all(|(pitch, _)| (-14..=-4).contains(pitch)));
        assert!(profiles.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn replay_voice_settings_are_optional_and_round_trip() {
        let old_json = r#"{"format_version":1,"title":"test","campaign_id":"c","created_at_unix_ms":1,"duration_ms":0,"audience":{"scope":"public"},"scene":{"voxels":[]},"camera":[],"dialogue":[]}"#;
        let mut replay: ReplayFile = serde_json::from_str(old_json).unwrap();
        assert!(replay.speaker_voice_settings.is_empty());
        replay
            .speaker_voice_settings
            .insert(42, SpeakerVoiceSettings {
                voice_name: Some("Test Voice".to_owned()),
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
            Some("Test Voice")
        );
        assert_eq!(settings.pitch, -25);
        assert_eq!(settings.onnx_speaker_id, Some(17));
        assert_eq!(settings.speech_rate, 12);
        assert_eq!(settings.volume, 0.75);
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
    fn original_relaxing_music_is_a_non_silent_stereo_wav() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("music.wav");
        write_relaxing_soundtrack(&path, 250, 0.35).unwrap();
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[22..24], &2_u16.to_le_bytes());
        assert!(bytes[44..].iter().any(|byte| *byte != 0));
    }

    #[test]
    #[ignore = "requires the downloaded Kokoro ONNX model"]
    fn kokoro_onnx_synthesizes_distinct_chinese_speakers() {
        let tts = create_onnx_tts().unwrap();
        assert_eq!(tts.sample_rate(), 24_000);
        assert_eq!(
            tts.num_speakers(),
            ONNX_TTS_SPEAKER_COUNT
        );
        let first = generate_onnx_wav(&tts, "你好，这是离线中文语音。", 3, 1.4).unwrap();
        let second = generate_onnx_wav(
            &tts,
            "你好，这是离线中文语音。",
            58,
            1.4,
        )
        .unwrap();
        assert_eq!(&first[0..4], b"RIFF");
        assert!(first.len() > 44);
        assert_ne!(first, second);
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "requires the downloaded ONNX model or installed Windows speech voices"]
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
            avatar: String::new(),
            avatar_data_url: None,
            visibility: Visibility::Public,
            side,
        }
    }
}
