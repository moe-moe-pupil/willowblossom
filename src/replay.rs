#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    collections::{
        hash_map::DefaultHasher,
        BTreeSet,
        HashMap,
        HashSet,
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

use avian3d::prelude::{
    AngularVelocity,
    LinearVelocity,
};
use base64::{
    engine::general_purpose::STANDARD as BASE64,
    Engine,
};
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    render::view::screenshot::{
        Screenshot,
        ScreenshotCaptured,
    },
    transform::TransformSystems,
    window::PrimaryWindow,
};
use bevy_egui::{
    egui::{
        self,
        DragValue,
        Ui,
    },
    EguiContexts,
    EguiPrimaryContextPass,
};
use bevy_persistent::{
    Persistent,
    StorageFormat,
};
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
use serde_json::{
    json,
    to_string as json_to_string,
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
        ReplayMessageSnapshot,
        Visibility,
    },
    ui,
    voxel::{
        cached_or_local_voxel_standee_path,
        TrpgVoxelGrid,
        VoxelEditMode,
        VoxelEditorState,
        VoxelGeometryDirtyChunks,
        VoxelPhysicsBody,
        VoxelPlayerStandee,
        VoxelPlayerStandeeSynced,
        VoxelPossessionState,
        VoxelReplayOcclusionFade,
        VoxelSpaceship,
        VoxelSpaceshipControlState,
        VoxelSpaceshipDocked,
        VoxelSpaceshipNeedsRebuild,
        VoxelSpaceshipOccupancyCache,
        VoxelToolGunDragState,
        VoxelViewportCamera,
        DEFAULT_VOXEL_OCCLUSION_CAST_END_HEIGHT_CELLS,
        DEFAULT_VOXEL_OCCLUSION_CAST_END_WIDTH_CELLS,
        DEFAULT_VOXEL_OCCLUSION_CAST_HEIGHT_CELLS,
        DEFAULT_VOXEL_OCCLUSION_CAST_WIDTH_CELLS,
        MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
        MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
        VOXEL_SIZE,
    },
};

const REPLAY_FORMAT_VERSION: u32 = 3;
const LEGACY_REPLAY_FORMAT_VERSION: u32 = 1;
const AREA_REPLAY_FORMAT_VERSION: u32 = 2;
const DEFAULT_AREA_RADIUS_CELLS: u32 = 12;
const AREA_BLOCK_TURN_LIMIT: usize = 3;
const CAMERA_SAMPLE_SECONDS: f32 = 0.1;
const SHIP_TRAJECTORY_SAMPLE_SECONDS: f32 = 0.1;
const MIN_DIALOGUE_MS: u64 = 1_500;
const MAX_DIALOGUE_MS: u64 = 9_750;
/// Lines at or under this duration are treated as quick table exchanges: they
/// are never stretched to fit synthesized audio and never block playback on
/// it, so a short GM prompt does not stall the turn for seconds.
const SHORT_DIALOGUE_MAX_MS: u64 = 2_500;
const HISTORY_DIALOGUE_GAP_MS: u64 = 270;
const DEFAULT_REPLAY_PATH: &str = ".data/willowblossom/replays/latest.willow-replay.json";
const DIRECTOR_CACHE_FINGERPRINT_VERSION: &str = "deepseek-director-v4";
const DEFAULT_VIDEO_PATH: &str = ".data/willowblossom/replays/latest.mp4";
const BACKGROUND_MUSIC_DIRECTORY: &str = "assets/audio";
const BACKGROUND_MUSIC_EXTENSIONS: &[&str] = &["mp3", "wav", "ogg", "flac", "m4a", "aac"];
const EMOTIVOICE_RUNTIME_DIR: &str = ".data/willowblossom/tts/emotivoice";
const EMOTIVOICE_SPEECH_CACHE_VERSION: u32 = 1;
const SHORT_UTTERANCE_MAX_UNITS: usize = 6;
const SHORT_UTTERANCE_SPEED_CAP: f32 = 1.10;
const SHORT_UTTERANCE_HEAD_PAD_MS: u64 = 80;
const SHORT_UTTERANCE_TAIL_PAD_MS: u64 = 180;
const VIDEO_CAPTURE_WARMUP_FRAMES: u8 = 3;
const VIDEO_CAPTURE_TIMEOUT_SECONDS: f32 = 30.0;
const REPLAY_PLAYING_STATUS: &str = "正在回放；停止后会恢复当前体素场景";
const REPLAY_SPEECH_PREPARING_STATUS: &str = "正在准备当前台词语音；语音就绪后字幕与声音会同时开始";
const REPLAY_SPEECH_MAX_WAIT_SECONDS: f32 = 5.0;
const DEFAULT_DIRECTED_CAMERA_DISTANCE_SCALE: f32 = 1.5;
const MIN_DIRECTED_CAMERA_DISTANCE_SCALE: f32 = 0.5;
const MAX_DIRECTED_CAMERA_DISTANCE_SCALE: f32 = 4.0;
const DEFAULT_DIRECTED_CAMERA_YAW_DEGREES: f32 = 0.0;
const MIN_DIRECTED_CAMERA_YAW_DEGREES: f32 = -60.0;
const MAX_DIRECTED_CAMERA_YAW_DEGREES: f32 = 60.0;
const DEFAULT_CAMERA_TRANSITION_CURVE: f32 = 2.0;
const MIN_CAMERA_TRANSITION_CURVE: f32 = 1.0;
const MAX_CAMERA_TRANSITION_CURVE: f32 = 4.0;
const FOCUS_TRANSITION_MS: u64 = 900;
const STANDEE_POSITION_SAMPLE_SECONDS: f32 = 0.1;
const MIN_MOVEMENT_SEGMENT_MS: u64 = 400;
const MAX_MOVEMENT_SEGMENT_MS: u64 = 2_500;
/// Camera reposition that is still shown as a smooth dolly instead of a cut.
/// Larger subject moves (the standee walked across the map) snap with a cut so
/// the camera never glides through empty space away from the standee.
const CAMERA_DOLLY_MAX_DISTANCE: f32 = 3.0;
const PLAYER_MOVEMENT_SAMPLE_SECONDS: f32 = 0.1;
const LIVE_REPLAY_EDIT_SAMPLE_SECONDS: f32 = 0.1;
const MIN_LIVE_DIALOGUE_MS: u64 = 100;
const MAX_LIVE_DIALOGUE_MS: u64 = 120_000;
const DEFAULT_PLAYER_MOVEMENT_CURVE: f32 = 0.75;
const MIN_PLAYER_MOVEMENT_CURVE: f32 = 0.0;
const MAX_PLAYER_MOVEMENT_CURVE: f32 = 1.0;
const MAX_PERSISTED_MOVEMENT_SESSIONS: usize = 256;
const MOVEMENT_HISTORY_PERSIST_SECONDS: f32 = 0.5;
const MAX_PERSISTED_SHIP_TRAJECTORY_SESSIONS: usize = 128;
const SHIP_TRAJECTORY_HISTORY_PERSIST_SECONDS: f32 = 0.5;

pub struct ReplayPlugin {
    runtime_enabled: bool,
}

impl ReplayPlugin {
    pub fn new(runtime_enabled: bool) -> Self { Self { runtime_enabled } }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ReplayCameraApplied;

impl Plugin for ReplayPlugin {
    fn build(&self, app: &mut App) {
        let voice_favorites = Persistent::<ReplayVoiceFavorites>::builder()
            .name("replay_voice_favorites")
            .format(StorageFormat::Toml)
            .path(
                Path::new(".data")
                    .join("willowblossom")
                    .join("replay_voice_favorites.toml"),
            )
            .default(ReplayVoiceFavorites::default())
            .revertible(true)
            .revert_to_default_on_deserialization_errors(true)
            .build()
            .expect("failed to initialize replay voice favorites");
        let player_movement_history = Persistent::<ReplayPlayerMovementHistory>::builder()
            .name("replay_player_movement_history")
            .format(StorageFormat::Toml)
            .path(
                Path::new(".data")
                    .join("willowblossom")
                    .join("replay_player_movements.toml"),
            )
            .default(ReplayPlayerMovementHistory::default())
            .revertible(true)
            .revert_to_default_on_deserialization_errors(true)
            .build()
            .expect("failed to initialize replay player movement history");
        let ship_trajectory_history = Persistent::<ReplayShipTrajectoryHistory>::builder()
            .name("replay_ship_trajectory_history")
            .format(StorageFormat::Toml)
            .path(
                Path::new(".data")
                    .join("willowblossom")
                    .join("replay_ship_trajectories.toml"),
            )
            .default(ReplayShipTrajectoryHistory::default())
            .revertible(true)
            .revert_to_default_on_deserialization_errors(true)
            .build()
            .expect("failed to initialize replay ship trajectory history");
        app.init_resource::<ReplayStudio>()
            .init_resource::<ReplayVideoCaptureActive>()
            .init_resource::<PreviewSpeechController>()
            .init_resource::<ReplaySnapshotTracker>()
            .init_resource::<ReplayMovementHistoryRecorder>()
            .init_resource::<ReplaySceneRecorder>()
            .init_resource::<ReplayShipTrajectoryRecorder>()
            .insert_resource(voice_favorites)
            .insert_resource(player_movement_history)
            .insert_resource(ship_trajectory_history);

        if !self.runtime_enabled {
            return;
        }

        app.add_systems(
            Update,
            (
                snapshot_new_replay_messages.after(VoxelPlayerStandeeSynced),
                record_player_movement_history.after(VoxelPlayerStandeeSynced),
                record_ship_trajectory_history.after(VoxelPlayerStandeeSynced),
                record_replay
                    .after(snapshot_new_replay_messages)
                    .after(VoxelPlayerStandeeSynced),
                apply_replay_terrain_changes
                    .after(advance_replay)
                    .after(render_video_frames)
                    .before(crate::voxel::rebuild_voxel_geometry),
                apply_replay_ship_hull_changes
                    .after(advance_replay)
                    .after(render_video_frames)
                    .before(crate::voxel::rebuild_dirty_voxel_spaceships),
                advance_replay,
                preview_replay_speech
                    .after(advance_replay)
                    .after(record_replay),
                render_video_frames,
                poll_video_encoding,
            ),
        )
        .add_systems(
            PostUpdate,
            (
                capture_live_replay_edits,
                apply_replay_standee_positions,
                apply_replay_ship_positions,
                apply_replay_camera,
            )
                .chain()
                .in_set(ReplayCameraApplied)
                .before(TransformSystems::Propagate),
        )
        .add_systems(
            EguiPrimaryContextPass,
            replay_studio_ui.after(ui::ui_system),
        );
    }
}

#[derive(Resource, Default)]
pub(crate) struct ReplayVideoCaptureActive(pub bool);

#[derive(Resource, Default)]
pub(crate) struct ReplaySnapshotTracker {
    initialized: bool,
    message_counts: HashMap<String, usize>,
}

pub(crate) fn replay_video_capture_inactive(active: Res<ReplayVideoCaptureActive>) -> bool {
    !active.0
}

pub(crate) fn replay_mouse_interaction_inactive(studio: Res<ReplayStudio>) -> bool {
    !replay_blocks_mouse_interaction(&studio)
}

fn replay_blocks_mouse_interaction(studio: &ReplayStudio) -> bool {
    (matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    ) && !studio.live_editing_enabled)
        || studio.video_render.is_some()
}

fn snapshot_new_replay_messages(
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
    standees: Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
    mut tracker: ResMut<ReplaySnapshotTracker>,
) {
    if !tracker.initialized {
        tracker.message_counts = manager
            .messages
            .iter()
            .map(|(target, messages)| (target.clone(), messages.len()))
            .collect();
        tracker.initialized = true;
        return;
    }

    let positions = standees
        .iter()
        .map(|(transform, standee)| {
            (
                standee.user_id,
                (transform.translation / VOXEL_SIZE)
                    .round()
                    .as_ivec3()
                    .to_array(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut next_line_id = manager
        .replay_snapshots
        .values()
        .flat_map(|snapshots| snapshots.iter().flatten())
        .map(|snapshot| snapshot.line_id)
        .max()
        .unwrap_or_default()
        .saturating_add(1)
        .max(1);
    let mut targets = manager.messages.keys().cloned().collect::<Vec<_>>();
    targets.sort();
    let mut additions = Vec::new();
    for target in targets {
        let messages = &manager.messages[&target];
        let seen = tracker
            .message_counts
            .get(&target)
            .copied()
            .unwrap_or_default()
            .min(messages.len());
        for (index, message) in messages.iter().enumerate().skip(seen) {
            let sender_id = manager.replay_message_sender_id(message);
            if let Some(position_cells) = positions.get(&sender_id).copied() {
                let campaign_message = manager.campaign_message_for_target(&target, message);
                additions.push((
                    target.clone(),
                    index,
                    ReplayMessageSnapshot {
                        line_id: next_line_id,
                        turn_index: replay_message_turn_index(
                            &manager,
                            &campaign_message.campaign_id,
                            sender_id,
                        ),
                        position_cells,
                    },
                ));
                next_line_id = next_line_id.saturating_add(1);
            }
        }
        tracker
            .message_counts
            .insert(target.clone(), messages.len());
    }

    if additions.is_empty() {
        return;
    }
    for (target, index, snapshot) in additions {
        let snapshots = manager.replay_snapshots.entry(target).or_default();
        if snapshots.len() <= index {
            snapshots.resize(index + 1, None);
        }
        snapshots[index] = Some(snapshot);
    }
    let _ = manager.persist();
}

fn replay_message_turn_index(
    manager: &NapcatMessageManager,
    campaign_id: &str,
    sender_id: u64,
) -> u32 {
    let mut groups = manager
        .trpg_groups
        .values()
        .filter(|group| group.campaign_id.trim() == campaign_id);
    let Some(group) = groups.next() else { return 0 };
    if groups.next().is_some() {
        return 0;
    }
    group
        .player_turns
        .get(&sender_id.to_string())
        .map(|turn| turn.turns_passed)
        .unwrap_or(group.world_turn)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "scope", content = "id", rename_all = "snake_case")]
enum ReplayAudience {
    Public,
    Party(String),
    Player(u64),
    All,
    Gm,
}

impl Default for ReplayAudience {
    fn default() -> Self { Self::All }
}

impl ReplayAudience {
    fn label(&self) -> String {
        match self {
            Self::Public => "公开".to_owned(),
            Self::Party(id) => format!("队伍：{id}"),
            Self::Player(id) => format!("玩家：{id}"),
            Self::All => "全部 / All".to_owned(),
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
            Self::All | Self::Gm => true,
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
    #[serde(default = "default_directed_camera_distance_scale")]
    camera_distance_scale: f32,
    #[serde(default = "default_directed_camera_yaw_degrees")]
    camera_yaw_degrees: f32,
    #[serde(default = "default_camera_transition_curve")]
    camera_transition_curve: f32,
    #[serde(default = "default_player_movement_curve")]
    player_movement_curve: f32,
    #[serde(default)]
    player_movements: Vec<ReplayPlayerMovement>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    authored_player_movements: BTreeSet<u64>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    authored_ship_trajectories: BTreeSet<String>,
    #[serde(default)]
    player_movement_history_cursor_unix_ms: u64,
    #[serde(default)]
    ship_trajectories: Vec<ReplayShipTrajectory>,
    #[serde(default)]
    standee_positions: Vec<ReplayStandeePosition>,
    #[serde(default = "default_ship_motion_speed")]
    ship_motion_speed: f32,
    #[serde(default = "default_dialogue_waits_for_ship_motion")]
    dialogue_waits_for_ship_motion: bool,
    #[serde(default)]
    terrain_changes: Vec<ReplayTerrainChange>,
    #[serde(default)]
    ship_hull_changes: Vec<ReplayShipHullChange>,
    #[serde(default)]
    ship_trajectory_history_cursor_unix_ms: u64,
    dialogue: Vec<ReplayDialogue>,
    #[serde(default)]
    area_blocks: Vec<ReplayAreaBlock>,
    /// Explicit DM line dragging takes precedence over historical GM chronology.
    #[serde(default)]
    manual_dialogue_order: bool,
    #[serde(default = "default_area_radius_cells")]
    area_radius_cells: u32,
    #[serde(default = "default_master_speech_speed")]
    master_speech_speed: f32,
    #[serde(default = "default_master_dialogue_duration")]
    master_dialogue_duration: f32,
    #[serde(default)]
    speaker_voice_settings: HashMap<u64, SpeakerVoiceSettings>,
}

#[derive(Debug, Clone)]
struct ReplayGenerationSettings {
    area_radius_cells: u32,
    master_speech_speed: f32,
    master_dialogue_duration: f32,
    speaker_voice_settings: HashMap<u64, SpeakerVoiceSettings>,
}

impl ReplayGenerationSettings {
    fn from_replay(replay: &ReplayFile) -> Self {
        Self {
            area_radius_cells: replay.area_radius_cells,
            master_speech_speed: replay.master_speech_speed,
            master_dialogue_duration: replay.master_dialogue_duration,
            speaker_voice_settings: replay.speaker_voice_settings.clone(),
        }
    }

    fn apply_to(self, replay: &mut ReplayFile) {
        replay.area_radius_cells = self.area_radius_cells;
        replay.master_speech_speed = self.master_speech_speed;
        replay.master_dialogue_duration = self.master_dialogue_duration;
        let speaker_ids = replay
            .dialogue
            .iter()
            .map(|line| line.sender_id)
            .collect::<HashSet<_>>();
        for (sender_id, settings) in self.speaker_voice_settings {
            if speaker_ids.contains(&sender_id) {
                replay.speaker_voice_settings.insert(sender_id, settings);
            }
        }
    }
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
struct ReplayPlayerMovement {
    user_id: u64,
    keyframes: Vec<ReplayPlayerMovementKeyframe>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ReplayPlayerMovementKeyframe {
    time_ms: u64,
    position_cells: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplayShipTrajectory {
    ship_id: String,
    ship_name: String,
    keyframes: Vec<ReplayShipKeyframe>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ReplayShipKeyframe {
    time_ms: u64,
    translation: [f32; 3],
    rotation: [f32; 4],
}

/// One world-space sample of a player standee during recording, so playback
/// moves the standee exactly where it was (including on board a moving ship)
/// instead of converting or estimating.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ReplayStandeePosition {
    time_ms: u64,
    user_id: u64,
    position: [f32; 3],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ReplayTerrainChange {
    time_ms: u64,
    position: [i32; 3],
    material: u8,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplayShipHullChange {
    time_ms: u64,
    ship_id: String,
    position: [i32; 3],
    material: u8,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReplayGridCellEvent {
    pub(crate) cell: IVec3,
    pub(crate) material: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplayShipHullCellEvent {
    pub(crate) ship_id: String,
    pub(crate) cell: IVec3,
    pub(crate) material: u8,
}

/// Per-frame buffer used by the voxel editor to report terrain and ship hull
/// cell mutations while a replay is being recorded. `record_replay` drains it
/// once per frame and stamps the events with the current record time.
#[derive(Default, Resource)]
pub(crate) struct ReplaySceneRecorder {
    pub(crate) grid_cells: Vec<ReplayGridCellEvent>,
    pub(crate) ship_hull_cells: Vec<ReplayShipHullCellEvent>,
}

pub(crate) fn record_replay_grid_cell(
    recorder: Option<&mut ReplaySceneRecorder>,
    cell: IVec3,
    material: u8,
) {
    if let Some(recorder) = recorder {
        recorder
            .grid_cells
            .push(ReplayGridCellEvent { cell, material });
    }
}

pub(crate) fn record_replay_ship_hull_cell(
    recorder: Option<&mut ReplaySceneRecorder>,
    ship_id: &str,
    cell: IVec3,
    material: u8,
) {
    if let Some(recorder) = recorder {
        if !ship_id.is_empty() {
            recorder.ship_hull_cells.push(ReplayShipHullCellEvent {
                ship_id: ship_id.to_owned(),
                cell,
                material,
            });
        }
    }
}

impl ReplaySceneRecorder {
    pub(crate) fn clear(&mut self) {
        self.grid_cells.clear();
        self.ship_hull_cells.clear();
    }
}

/// Bundled persistent histories passed from the replay UI system to the plain
/// UI helpers, keeping the system parameter count within Bevy's limit.
#[derive(SystemParam)]
struct ReplayHistoryParams<'w> {
    player_movement_history: ResMut<'w, Persistent<ReplayPlayerMovementHistory>>,
    ship_trajectory_history: ResMut<'w, Persistent<ReplayShipTrajectoryHistory>>,
}

#[derive(SystemParam)]
struct ReplayLiveEditParams<'w, 's> {
    possession: Res<'w, VoxelPossessionState>,
    ship_control: Res<'w, VoxelSpaceshipControlState>,
    editor: ResMut<'w, VoxelEditorState>,
    ships: Query<
        'w,
        's,
        (
            &'static VoxelSpaceship,
            &'static Transform,
        ),
        (
            Without<VoxelPlayerStandee>,
            Without<VoxelViewportCamera>,
        ),
    >,
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ReplayPlayerMovementHistory {
    #[serde(default)]
    sessions: Vec<PersistedPlayerMovementSession>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedPlayerMovementSession {
    campaign_id: String,
    user_id: u64,
    #[serde(default)]
    turn_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start_after_source_time: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start_after_sender_id: Option<u64>,
    #[serde(default)]
    start_delay_ms: u64,
    keyframes: Vec<PersistedPlayerMovementKeyframe>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PersistedPlayerMovementKeyframe {
    source_unix_ms: u64,
    position_cells: [f32; 3],
}

/// Continuously persisted ship trajectory history, mirroring the player
/// movement history: ships are sampled whenever a campaign is active, with no
/// need to press "开始录制", and replays anchor the samples onto their dialogue
/// timeline by real message time.
#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ReplayShipTrajectoryHistory {
    #[serde(default)]
    sessions: Vec<PersistedShipTrajectorySession>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedShipTrajectorySession {
    campaign_id: String,
    ship_id: String,
    ship_name: String,
    #[serde(default)]
    turn_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start_after_source_time: Option<u64>,
    #[serde(default)]
    start_delay_ms: u64,
    keyframes: Vec<PersistedShipKeyframe>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PersistedShipKeyframe {
    source_unix_ms: u64,
    translation: [f32; 3],
    rotation: [f32; 4],
}

/// Live bookkeeping for `record_ship_trajectory_history`; the session index
/// cache is rebuilt whenever the session list is trimmed.
#[derive(Resource, Default)]
pub(crate) struct ReplayShipTrajectoryRecorder {
    sessions: HashMap<(String, String), usize>,
    sample_accumulators: HashMap<(String, String), f32>,
    /// Latest sampled pose for ships that have not moved yet. A session is
    /// only opened once the pose changes, so parked ships never record a
    /// trajectory.
    pending_poses: HashMap<(String, String), PersistedShipKeyframe>,
    persist_accumulator: f32,
}

pub(crate) fn clear_campaign_replay_ship_trajectory_history(
    history: &mut ReplayShipTrajectoryHistory,
    campaign_id: &str,
) -> usize {
    let previous_len = history.sessions.len();
    history
        .sessions
        .retain(|session| session.campaign_id != campaign_id);
    previous_len - history.sessions.len()
}

pub(crate) fn clear_campaign_replay_movement_history(
    history: &mut ReplayPlayerMovementHistory,
    campaign_id: &str,
) -> usize {
    let previous_len = history.sessions.len();
    history
        .sessions
        .retain(|session| session.campaign_id != campaign_id);
    previous_len - history.sessions.len()
}

pub(crate) fn clear_player_replay_movement_history(
    history: &mut ReplayPlayerMovementHistory,
    user_id: u64,
) -> usize {
    let previous_len = history.sessions.len();
    history
        .sessions
        .retain(|session| session.user_id != user_id);
    previous_len - history.sessions.len()
}

/// Clears every replay artifact of a campaign: the persisted player-movement
/// and ship-trajectory histories, the in-memory studio replay (including its
/// generated camera frames), message snapshot tracking, and the live recorder
/// caches. Returns the number of removed trajectory sessions.
pub(crate) fn clear_campaign_replay_data(
    studio: &mut ReplayStudio,
    snapshot_tracker: &mut ReplaySnapshotTracker,
    movement_recorder: &mut ReplayMovementHistoryRecorder,
    ship_recorder: &mut ReplayShipTrajectoryRecorder,
    player_history: &mut ReplayPlayerMovementHistory,
    ship_history: &mut ReplayShipTrajectoryHistory,
    campaign_id: &str,
) -> usize {
    let removed_player = clear_campaign_replay_movement_history(player_history, campaign_id);
    let removed_ships = clear_campaign_replay_ship_trajectory_history(ship_history, campaign_id);

    studio.mode = ReplayMode::Idle;
    studio.replay = None;
    studio.playback_ms = 0;
    studio.record_elapsed_ms = 0;
    studio.message_counts.clear();
    studio.pending_terrain_changes.clear();
    studio.pending_ship_hull_changes.clear();
    studio.pre_playback_scene = None;
    studio.live_editing_enabled = false;
    studio.live_movement_punch_in = None;
    studio.live_ship_punch_in = None;
    studio.recorded_possession_user_id = None;
    studio.recorded_player_movement_index = None;
    studio.director_request_pending = false;
    studio.director_response_hash = None;
    studio.auto_export_after_director = false;
    if let Some(job) = studio.video_render.as_mut() {
        job.failure = Some("测试进度已清空，视频导出已取消".to_owned());
    }
    studio.status = format!(
        "回放数据已清空：移除 {removed_player} 段玩家移动和 {removed_ships} 艘飞船轨迹，当前回放与镜头已重置"
    );

    snapshot_tracker.initialized = false;
    snapshot_tracker.message_counts.clear();
    movement_recorder.active_session = None;
    movement_recorder.session_index = None;
    movement_recorder.sample_accumulator = 0.0;
    movement_recorder.persist_accumulator = 0.0;
    ship_recorder.sessions.clear();
    ship_recorder.sample_accumulators.clear();
    ship_recorder.pending_poses.clear();
    ship_recorder.persist_accumulator = 0.0;

    removed_player.saturating_add(removed_ships)
}

#[derive(Resource, Default)]
pub(crate) struct ReplayMovementHistoryRecorder {
    active_session: Option<(String, u64)>,
    session_index: Option<usize>,
    sample_accumulator: f32,
    persist_accumulator: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplayDialogue {
    time_ms: u64,
    duration_ms: u64,
    /// A GM-authored exact subtitle window. Speech fitting must not stretch it.
    #[serde(default)]
    duration_locked: bool,
    sender_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    camera_focus_id: Option<u64>,
    name: String,
    role: String,
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_text: Option<String>,
    #[serde(default = "default_true")]
    speech_enabled: bool,
    #[serde(default = "default_line_speech_rate")]
    speech_rate: f32,
    #[serde(default = "default_line_speech_volume")]
    speech_volume: f32,
    avatar: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avatar_data_url: Option<String>,
    visibility: Visibility,
    #[serde(default)]
    side: DialogueSide,
    #[serde(default)]
    line_id: u64,
    #[serde(default)]
    source_time: u64,
    #[serde(default)]
    turn_index: u32,
    #[serde(default)]
    position_cells: [i32; 3],
    #[serde(default)]
    area: String,
    #[serde(default = "default_true")]
    included: bool,
    #[serde(default)]
    snapshot_recorded: bool,
    #[serde(default)]
    metadata_estimated: bool,
    #[serde(default)]
    forwarded: bool,
}

fn replay_dialogue_is_playable(line: &ReplayDialogue) -> bool {
    line.included && line.time_ms != u64::MAX
}

fn replay_dialogue_has_saved_position(line: &ReplayDialogue) -> bool {
    line.snapshot_recorded && (!line.metadata_estimated || line.position_cells != [0; 3])
}

#[derive(Debug, Clone, Copy)]
struct ReplayDialogueDrag {
    line_id: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct ReplayAreaBlock {
    id: u64,
    area: String,
    line_ids: Vec<u64>,
}

const fn default_area_radius_cells() -> u32 { DEFAULT_AREA_RADIUS_CELLS }

const fn default_true() -> bool { true }

const fn default_line_speech_rate() -> f32 { 1.0 }

const fn default_line_speech_volume() -> f32 { 1.0 }

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ReplayVoiceFavorite {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sender_id: Option<u64>,
    settings: SpeakerVoiceSettings,
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
struct ReplayVoiceFavorites {
    #[serde(default)]
    favorites: Vec<ReplayVoiceFavorite>,
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

#[derive(Debug, Clone)]
struct ReplayMovementPunchIn {
    user_id: u64,
    start_ms: u64,
    original_duration_ms: u64,
    last_sample_ms: Option<u64>,
    samples: Vec<ReplayStandeePosition>,
}

#[derive(Debug, Clone)]
struct ReplayShipPunchIn {
    ship_id: String,
    ship_name: String,
    start_ms: u64,
    original_duration_ms: u64,
    last_sample_ms: Option<u64>,
    samples: Vec<ReplayShipKeyframe>,
}

impl Default for ReplayMode {
    fn default() -> Self { Self::Idle }
}

#[derive(Resource)]
pub(crate) struct ReplayStudio {
    mode: ReplayMode,
    replay: Option<ReplayFile>,
    audience: ReplayAudience,
    record_elapsed_ms: u64,
    playback_ms: u64,
    playback_speed: f32,
    camera_distance_scale: f32,
    camera_yaw_degrees: f32,
    camera_transition_curve: f32,
    player_movement_curve: f32,
    record_camera_enabled: bool,
    director_response_hash: Option<u64>,
    director_request_pending: bool,
    auto_export_after_director: bool,
    camera_sample_accumulator: f32,
    standee_sample_accumulator: f32,
    player_movement_sample_accumulator: f32,
    recorded_possession_user_id: Option<u64>,
    recorded_player_movement_index: Option<usize>,
    turn_playback_enabled: bool,
    speech_wait_cue: Option<(u64, usize)>,
    speech_wait_elapsed_seconds: f32,
    speech_bypass_cues: HashSet<(u64, usize)>,
    /// Unlocks normal viewport tools while preview playback remains active.
    /// Replay playback still suppresses campaign persistence.
    live_editing_enabled: bool,
    live_movement_user_id: Option<u64>,
    live_ship_id: Option<String>,
    live_movement_punch_in: Option<ReplayMovementPunchIn>,
    live_ship_punch_in: Option<ReplayShipPunchIn>,
    timeline_revision: u64,
    scene_revision: u64,
    message_counts: HashMap<String, usize>,
    pending_terrain_changes: Vec<(u64, IVec3, u8)>,
    pending_ship_hull_changes: Vec<(u64, String, IVec3, u8)>,
    pre_playback_scene: Option<ReplayScene>,
    video_path: String,
    video_fps: u32,
    music_enabled: bool,
    music_volume: f32,
    music_file: Option<PathBuf>,
    music_files: Vec<PathBuf>,
    speech_enabled: bool,
    speech_volume: f32,
    speech_settings_open: bool,
    voice_search: String,
    voice_favorite_name_drafts: HashMap<u64, String>,
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
    music_file: Option<PathBuf>,
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
    onnx_queued: HashSet<(u64, usize)>,
    onnx_attempts: HashMap<(u64, usize), u8>,
    onnx_failures: HashMap<(u64, usize), String>,
    pending_generation_lines: HashSet<(u64, u64)>,
    generation_cues: HashSet<(u64, usize)>,
    onnx_worker: Option<OnnxPreviewWorker>,
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
        let music_files = discover_background_music();
        let music_file = music_files.first().cloned();
        Self {
            mode: ReplayMode::Idle,
            replay: None,
            audience: ReplayAudience::All,
            record_elapsed_ms: 0,
            playback_ms: 0,
            playback_speed: 1.0,
            camera_distance_scale: default_directed_camera_distance_scale(),
            camera_yaw_degrees: default_directed_camera_yaw_degrees(),
            camera_transition_curve: default_camera_transition_curve(),
            player_movement_curve: default_player_movement_curve(),
            record_camera_enabled: false,
            director_response_hash: None,
            director_request_pending: false,
            auto_export_after_director: false,
            camera_sample_accumulator: 0.0,
            standee_sample_accumulator: 0.0,
            player_movement_sample_accumulator: 0.0,
            recorded_possession_user_id: None,
            recorded_player_movement_index: None,
            turn_playback_enabled: false,
            speech_wait_cue: None,
            speech_wait_elapsed_seconds: 0.0,
            speech_bypass_cues: HashSet::new(),
            live_editing_enabled: false,
            live_movement_user_id: None,
            live_ship_id: None,
            live_movement_punch_in: None,
            live_ship_punch_in: None,
            timeline_revision: 0,
            scene_revision: 0,
            message_counts: HashMap::new(),
            pending_terrain_changes: Vec::new(),
            pending_ship_hull_changes: Vec::new(),
            pre_playback_scene: None,
            video_path: DEFAULT_VIDEO_PATH.to_owned(),
            video_fps: 15,
            music_enabled: music_file.is_some(),
            music_volume: 0.65,
            music_file,
            music_files,
            speech_enabled: true,
            speech_volume: 1.25,
            speech_settings_open: false,
            voice_search: String::new(),
            voice_favorite_name_drafts: HashMap::new(),
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

fn record_player_movement_history(
    time: Res<Time>,
    manager: Res<Persistent<NapcatMessageManager>>,
    possession: Res<VoxelPossessionState>,
    studio: Res<ReplayStudio>,
    standees: Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
    mut history: ResMut<Persistent<ReplayPlayerMovementHistory>>,
    mut recorder: ResMut<ReplayMovementHistoryRecorder>,
) {
    // Live replay editing has its own punch-in recorder. Never let those
    // temporary preview poses leak into the campaign's continuous history.
    let active_session = if replay_scene_dynamics_active(&studio) {
        None
    } else {
        manager
            .active_campaign_id()
            .zip(possession.active_user_id)
            .filter(|_| !possession.movement_is_completed())
    };
    let session_changed = recorder.active_session != active_session;
    if session_changed {
        if recorder.active_session.is_some() {
            if let Err(err) = history.persist() {
                eprintln!("failed to persist replay player movement history: {err}");
            }
        }
        recorder.active_session = active_session.clone();
        recorder.session_index = None;
        recorder.sample_accumulator = 0.0;
        recorder.persist_accumulator = 0.0;
    } else if active_session.is_some() {
        recorder.sample_accumulator += time.delta_secs();
        recorder.persist_accumulator += time.delta_secs();
    }

    let sample_due =
        session_changed || recorder.sample_accumulator >= PLAYER_MOVEMENT_SAMPLE_SECONDS;
    let Some((campaign_id, user_id)) = active_session.filter(|_| sample_due) else {
        return;
    };
    let Some(position) = standees.iter().find_map(|(transform, standee)| {
        (standee.user_id == user_id).then_some(transform.translation)
    }) else {
        return;
    };
    recorder.sample_accumulator %= PLAYER_MOVEMENT_SAMPLE_SECONDS;

    let index = recorder.session_index.unwrap_or_else(|| {
        if history.sessions.len() >= MAX_PERSISTED_MOVEMENT_SESSIONS {
            let remove_count = history.sessions.len() + 1 - MAX_PERSISTED_MOVEMENT_SESSIONS;
            history.sessions.drain(..remove_count);
        }
        let turn_index = movement_turn_index(&manager, user_id);
        history.sessions.push(PersistedPlayerMovementSession {
            turn_index,
            start_after_source_time: latest_player_line_time(
                &manager,
                &campaign_id,
                user_id,
                turn_index,
            ),
            start_after_sender_id: Some(user_id),
            start_delay_ms: 0,
            campaign_id,
            user_id,
            keyframes: Vec::new(),
        });
        history.sessions.len() - 1
    });
    recorder.session_index = Some(index);
    let keyframe = PersistedPlayerMovementKeyframe {
        source_unix_ms: unix_time_ms(),
        position_cells: (position / VOXEL_SIZE).to_array(),
    };
    let session = &mut history.sessions[index];
    if let Some(last) = session
        .keyframes
        .last_mut()
        .filter(|last| last.source_unix_ms == keyframe.source_unix_ms)
    {
        *last = keyframe;
    } else {
        session.keyframes.push(keyframe);
    }

    if recorder.persist_accumulator >= MOVEMENT_HISTORY_PERSIST_SECONDS {
        recorder.persist_accumulator %= MOVEMENT_HISTORY_PERSIST_SECONDS;
        if let Err(err) = history.persist() {
            eprintln!("failed to persist replay player movement history: {err}");
        }
    }
}

/// Continuously samples every voxel spaceship pose into the persisted
/// `ReplayShipTrajectoryHistory` while a campaign is active, regardless of
/// whether a replay is being recorded. The recorder is paused while a replay
/// is playing or rendering, because ship transforms are then replay-driven.
fn record_ship_trajectory_history(
    time: Res<Time>,
    manager: Res<Persistent<NapcatMessageManager>>,
    studio: Res<ReplayStudio>,
    control: Res<VoxelSpaceshipControlState>,
    drag: Res<VoxelToolGunDragState>,
    spaceships: Query<(Entity, &VoxelSpaceship, &Transform), Without<VoxelViewportCamera>>,
    mut history: ResMut<Persistent<ReplayShipTrajectoryHistory>>,
    mut recorder: ResMut<ReplayShipTrajectoryRecorder>,
) {
    let active_campaign = if replay_scene_dynamics_active(&studio) {
        None
    } else {
        manager.active_campaign_id()
    };
    if active_campaign.is_none() {
        recorder.sessions.clear();
        recorder.sample_accumulators.clear();
        recorder.pending_poses.clear();
        return;
    }
    let campaign_id = active_campaign.unwrap();
    recorder.persist_accumulator += time.delta_secs();
    for (entity, ship, transform) in &spaceships {
        let key = (campaign_id.clone(), ship.id.clone());
        let accumulator = recorder
            .sample_accumulators
            .entry(key.clone())
            .or_insert(0.0);
        *accumulator += time.delta_secs();
        if *accumulator < SHIP_TRAJECTORY_SAMPLE_SECONDS {
            continue;
        }
        *accumulator %= SHIP_TRAJECTORY_SAMPLE_SECONDS;
        let keyframe = PersistedShipKeyframe {
            source_unix_ms: unix_time_ms(),
            translation: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
        };
        // Only the GM's own manipulation of a ship is recorded: keyboard
        // driving or the tool-gun drag. Physics drift, docking sync, or other
        // ships following the carrier stay out of the replay.
        let gm_manipulated = ship_is_gm_manipulated(&control, &drag, entity, &ship.id);
        let moved = |left: [f32; 3], right: [f32; 3]| left != right;
        if let Some(&session_index) = recorder.sessions.get(&key) {
            let session = &mut history.sessions[session_index];
            session.ship_name = ship.name.clone();
            if gm_manipulated {
                let unchanged = session.keyframes.last().is_some_and(|last| {
                    !moved(last.translation, keyframe.translation)
                        && last.rotation == keyframe.rotation
                });
                if !unchanged {
                    session.keyframes.push(keyframe);
                }
            }
            continue;
        }
        // No session yet: remember the parked pose, but only open a session
        // once the GM moves the ship, so stationary and physics-driven motion
        // stay unrecorded.
        let pending_pose = {
            let pending = recorder
                .pending_poses
                .entry(key.clone())
                .or_insert(keyframe);
            if !moved(
                pending.translation,
                keyframe.translation,
            ) && pending.rotation == keyframe.rotation
            {
                *pending = keyframe;
                continue;
            }
            if !gm_manipulated {
                *pending = keyframe;
                continue;
            }
            *pending
        };
        let session_index = find_or_create_ship_trajectory_session(
            &mut history,
            &mut recorder,
            &campaign_id,
            ship,
            &manager,
        );
        recorder.sessions.insert(key.clone(), session_index);
        recorder.pending_poses.remove(&key);
        let session = &mut history.sessions[session_index];
        session.ship_name = ship.name.clone();
        session.keyframes.push(pending_pose);
        if moved(
            pending_pose.translation,
            keyframe.translation,
        ) || pending_pose.rotation != keyframe.rotation
        {
            session.keyframes.push(keyframe);
        }
    }
    if recorder.persist_accumulator >= SHIP_TRAJECTORY_HISTORY_PERSIST_SECONDS {
        recorder.persist_accumulator %= SHIP_TRAJECTORY_HISTORY_PERSIST_SECONDS;
        if let Err(err) = history.persist() {
            eprintln!("failed to persist replay ship trajectory history: {err}");
        }
    }
}

fn ship_is_gm_manipulated(
    control: &VoxelSpaceshipControlState,
    drag: &VoxelToolGunDragState,
    entity: Entity,
    ship_id: &str,
) -> bool {
    control.driving_ship_id.as_deref() == Some(ship_id) || drag.target == Some(entity)
}

fn find_or_create_ship_trajectory_session(
    history: &mut ReplayShipTrajectoryHistory,
    recorder: &mut ReplayShipTrajectoryRecorder,
    campaign_id: &str,
    ship: &VoxelSpaceship,
    manager: &Persistent<NapcatMessageManager>,
) -> usize {
    if let Some(index) = history
        .sessions
        .iter()
        .position(|session| session.campaign_id == campaign_id && session.ship_id == ship.id)
    {
        return index;
    }
    if history.sessions.len() >= MAX_PERSISTED_SHIP_TRAJECTORY_SESSIONS {
        history.sessions.remove(0);
        recorder.sessions = history
            .sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
                (
                    (
                        session.campaign_id.clone(),
                        session.ship_id.clone(),
                    ),
                    index,
                )
            })
            .collect();
    }
    let world_turn = manager
        .current_group()
        .map(|group| group.world_turn)
        .unwrap_or_default();
    history.sessions.push(PersistedShipTrajectorySession {
        campaign_id: campaign_id.to_owned(),
        ship_id: ship.id.clone(),
        ship_name: ship.name.clone(),
        turn_index: world_turn,
        start_after_source_time: latest_campaign_line_time(manager, campaign_id),
        start_delay_ms: 0,
        keyframes: Vec::new(),
    });
    history.sessions.len() - 1
}

fn latest_campaign_line_time(manager: &NapcatMessageManager, campaign_id: &str) -> Option<u64> {
    manager
        .messages
        .iter()
        .flat_map(|(target, messages)| {
            messages
                .iter()
                .map(move |message| manager.campaign_message_for_target(target, message))
        })
        .filter(|message| message.campaign_id == campaign_id)
        .map(|message| message.time)
        .max()
}

fn movement_turn_index(manager: &NapcatMessageManager, user_id: u64) -> u32 {
    manager
        .current_group()
        .and_then(|group| group.player_turns.get(&user_id.to_string()))
        .map(|turn| turn.turns_passed)
        .or_else(|| manager.current_group().map(|group| group.world_turn))
        .unwrap_or_default()
}

fn latest_player_line_time(
    manager: &NapcatMessageManager,
    campaign_id: &str,
    user_id: u64,
    turn_index: u32,
) -> Option<u64> {
    let mut matching_turn = Vec::new();
    let mut any_turn = Vec::new();
    for (target, messages) in &manager.messages {
        for (index, message) in messages.iter().enumerate() {
            let message = manager.campaign_message_for_target(target, message);
            if message.campaign_id != campaign_id || message.sender_id != user_id {
                continue;
            }
            any_turn.push(message.time);
            if manager
                .replay_snapshots
                .get(target)
                .and_then(|snapshots| snapshots.get(index))
                .and_then(Option::as_ref)
                .is_some_and(|snapshot| snapshot.turn_index == turn_index)
            {
                matching_turn.push(message.time);
            }
        }
    }
    matching_turn
        .into_iter()
        .max()
        .or_else(|| any_turn.into_iter().max())
}

fn record_replay(
    time: Res<Time>,
    manager: Res<Persistent<NapcatMessageManager>>,
    possession: Res<VoxelPossessionState>,
    camera: Query<&Transform, With<VoxelViewportCamera>>,
    standees: Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
    mut studio: ResMut<ReplayStudio>,
    mut speech: ResMut<PreviewSpeechController>,
    mut scene_recorder: ResMut<ReplaySceneRecorder>,
) {
    if studio.mode != ReplayMode::Recording {
        return;
    }

    let delta_seconds = time.delta_secs();
    if studio.record_elapsed_ms == 0 {
        // First recording frame: drop any terrain events recorded before
        // "开始录制" was pressed; ship trajectories come from the continuous
        // `ReplayShipTrajectoryHistory` instead.
        scene_recorder.clear();
    }
    studio.record_elapsed_ms = studio
        .record_elapsed_ms
        .saturating_add((delta_seconds * 1_000.0).round() as u64);
    let speaker_positions = standee_positions(&standees);
    record_possessed_player_movement(
        &mut studio,
        possession
            .active_user_id
            .filter(|_| !possession.movement_is_completed()),
        &speaker_positions,
        delta_seconds,
    );
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
    studio.standee_sample_accumulator += delta_seconds;
    if studio.standee_sample_accumulator >= STANDEE_POSITION_SAMPLE_SECONDS {
        studio.standee_sample_accumulator %= STANDEE_POSITION_SAMPLE_SECONDS;
        let record_elapsed_ms = studio.record_elapsed_ms;
        if let Some(replay) = studio.replay.as_mut() {
            for (transform, standee) in &standees {
                replay.standee_positions.push(ReplayStandeePosition {
                    time_ms: record_elapsed_ms,
                    user_id: standee.user_id,
                    position: transform.translation.to_array(),
                });
            }
        }
    }
    let drained_grid_cells = std::mem::take(&mut scene_recorder.grid_cells);
    let drained_ship_hull_cells = std::mem::take(&mut scene_recorder.ship_hull_cells);
    let source_unix_ms = unix_time_ms();
    studio.pending_terrain_changes.extend(
        drained_grid_cells.into_iter().map(|event| {
            (
                source_unix_ms,
                event.cell,
                event.material,
            )
        }),
    );
    studio.pending_ship_hull_changes.extend(
        drained_ship_hull_cells.into_iter().map(|event| {
            (
                source_unix_ms,
                event.ship_id,
                event.cell,
                event.material,
            )
        }),
    );

    let audience = studio.audience.clone();
    let replay_campaign_id = studio
        .replay
        .as_ref()
        .map(|replay| replay.campaign_id.clone())
        .unwrap_or_default();
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
        for (index, message) in messages.iter().enumerate().skip(seen) {
            let campaign_message = manager.campaign_message_for_target(&target_id, message);
            if replay_message_is_eligible(
                &audience,
                &replay_campaign_id,
                &campaign_message,
                &manager,
            ) {
                let persisted_snapshot = manager
                    .replay_snapshots
                    .get(&target_id)
                    .and_then(|snapshots| snapshots.get(index))
                    .and_then(Option::as_ref);
                let estimated_snapshot = persisted_snapshot.is_none().then(|| {
                    estimated_replay_snapshot(
                        &campaign_message,
                        &manager,
                        &speaker_positions,
                    )
                });
                if let Some(dialogue) = dialogue_from_message(
                    &campaign_message,
                    &manager,
                    next_turn_ms,
                    persisted_snapshot.unwrap_or_else(|| estimated_snapshot.as_ref().unwrap()),
                    persisted_snapshot.is_none(),
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
        let captured_messages = captured
            .iter()
            .map(|line| {
                (
                    line.sender_id,
                    line.source_time,
                    line.text.clone(),
                )
            })
            .collect::<HashSet<_>>();
        replay.dialogue.extend(captured);
        deduplicate_broadcast_dialogue(&mut replay.dialogue, &manager);
        assign_replay_line_ids(&mut replay.dialogue);
        auto_group_replay_areas(replay);
        rebuild_area_blocks(replay);
        let dialogue_end = compile_area_block_timeline(replay);
        replay.duration_ms = if record_camera_enabled || !replay.player_movements.is_empty() {
            record_elapsed_ms.max(dialogue_end)
        } else {
            dialogue_end
        };
        if captured_any && !record_camera_enabled {
            if let Ok(base) = camera.single() {
                let obstacles = ReplayCameraObstacles::from_scene(&replay.scene);
                replay.camera = turn_based_camera_track(
                    base,
                    &replay.dialogue,
                    replay.duration_ms,
                    &speaker_positions,
                    replay.camera_distance_scale,
                    replay.camera_yaw_degrees,
                    &obstacles,
                );
            }
        }
        if captured_any {
            speech.pending_generation_lines.extend(
                replay
                    .dialogue
                    .iter()
                    .filter(|line| {
                        captured_messages.contains(&(
                            line.sender_id,
                            line.source_time,
                            line.text.clone(),
                        ))
                    })
                    .map(|line| (replay.created_at_unix_ms, line.line_id)),
            );
        }
    }
}

/// Captures viewport edits against the replay playhead. Voxel mutations arrive
/// through the same recorder used by normal recording, so explosions and hull
/// edits retain the canonical voxel scale and implementation.
fn capture_live_replay_edits(
    mut studio: ResMut<ReplayStudio>,
    mut scene_recorder: ResMut<ReplaySceneRecorder>,
    standees: Query<
        (&Transform, &VoxelPlayerStandee),
        (
            With<VoxelPlayerStandee>,
            Without<VoxelViewportCamera>,
        ),
    >,
    ships: Query<
        (&VoxelSpaceship, &Transform),
        (
            Without<VoxelPlayerStandee>,
            Without<VoxelViewportCamera>,
        ),
    >,
) {
    let playback_active = matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    );
    if !playback_active || !studio.live_editing_enabled || studio.video_render.is_some() {
        if studio.mode != ReplayMode::Recording {
            scene_recorder.clear();
        }
        return;
    }

    let playback_ms = studio.playback_ms;
    let grid_cells = std::mem::take(&mut scene_recorder.grid_cells);
    let hull_cells = std::mem::take(&mut scene_recorder.ship_hull_cells);
    let scene_change_count = grid_cells.len().saturating_add(hull_cells.len());
    if scene_change_count > 0 {
        if let Some(replay) = studio.replay.as_mut() {
            replay
                .terrain_changes
                .extend(
                    grid_cells.into_iter().map(|event| ReplayTerrainChange {
                        time_ms: playback_ms,
                        position: event.cell.to_array(),
                        material: event.material,
                        enabled: true,
                    }),
                );
            replay
                .ship_hull_changes
                .extend(
                    hull_cells.into_iter().map(|event| ReplayShipHullChange {
                        time_ms: playback_ms,
                        ship_id: event.ship_id,
                        position: event.cell.to_array(),
                        material: event.material,
                        enabled: true,
                    }),
                );
            replay.terrain_changes.sort_by_key(|change| change.time_ms);
            replay
                .ship_hull_changes
                .sort_by_key(|change| change.time_ms);
        }
        studio.scene_revision = studio.scene_revision.wrapping_add(1);
        studio.status = format!(
            "已在 {} 写入 {scene_change_count} 个场景体素变化；可在现场编辑器中禁用或恢复",
            format_time(playback_ms),
        );
    }

    if let Some(punch_in) = studio.live_movement_punch_in.as_mut() {
        if live_replay_sample_due(punch_in.last_sample_ms, playback_ms) {
            if playback_ms < punch_in.last_sample_ms.unwrap_or_default() {
                punch_in
                    .samples
                    .retain(|sample| sample.time_ms <= playback_ms);
            }
            if let Some(position) = standees.iter().find_map(|(transform, standee)| {
                (standee.user_id == punch_in.user_id).then_some(transform.translation)
            }) {
                push_live_standee_sample(
                    &mut punch_in.samples,
                    playback_ms,
                    punch_in.user_id,
                    position,
                );
                punch_in.last_sample_ms = Some(playback_ms);
            }
        }
    }

    if let Some(punch_in) = studio.live_ship_punch_in.as_mut() {
        if live_replay_sample_due(punch_in.last_sample_ms, playback_ms) {
            if playback_ms < punch_in.last_sample_ms.unwrap_or_default() {
                punch_in
                    .samples
                    .retain(|sample| sample.time_ms <= playback_ms);
            }
            if let Some(transform) = ships
                .iter()
                .find_map(|(ship, transform)| (ship.id == punch_in.ship_id).then_some(*transform))
            {
                push_live_ship_sample(
                    &mut punch_in.samples,
                    playback_ms,
                    transform,
                );
                punch_in.last_sample_ms = Some(playback_ms);
            }
        }
    }
}

fn live_replay_sample_due(last_sample_ms: Option<u64>, playback_ms: u64) -> bool {
    last_sample_ms.is_none_or(|last| {
        playback_ms < last
            || playback_ms.saturating_sub(last)
                >= (LIVE_REPLAY_EDIT_SAMPLE_SECONDS * 1_000.0).round() as u64
    })
}

fn push_live_standee_sample(
    samples: &mut Vec<ReplayStandeePosition>,
    time_ms: u64,
    user_id: u64,
    position: Vec3,
) {
    let sample = ReplayStandeePosition {
        time_ms,
        user_id,
        position: position.to_array(),
    };
    if let Some(last) = samples.last_mut().filter(|last| last.time_ms == time_ms) {
        *last = sample;
    } else {
        samples.push(sample);
    }
}

fn push_live_ship_sample(
    samples: &mut Vec<ReplayShipKeyframe>,
    time_ms: u64,
    transform: Transform,
) {
    let sample = ReplayShipKeyframe {
        time_ms,
        translation: transform.translation.to_array(),
        rotation: transform.rotation.normalize().to_array(),
    };
    if let Some(last) = samples.last_mut().filter(|last| last.time_ms == time_ms) {
        *last = sample;
    } else {
        samples.push(sample);
    }
}

fn record_possessed_player_movement(
    studio: &mut ReplayStudio,
    active_user_id: Option<u64>,
    player_positions: &HashMap<u64, Vec3>,
    delta_seconds: f32,
) {
    let possession_changed = studio.recorded_possession_user_id != active_user_id;
    if possession_changed {
        if let (Some(previous_user_id), Some(index)) = (
            studio.recorded_possession_user_id,
            studio.recorded_player_movement_index,
        ) {
            if let (Some(position), Some(replay)) = (
                player_positions.get(&previous_user_id).copied(),
                studio.replay.as_mut(),
            ) {
                if let Some(movement) = replay.player_movements.get_mut(index) {
                    push_player_movement_keyframe(
                        movement,
                        studio.record_elapsed_ms,
                        position,
                    );
                }
            }
        }
        studio.recorded_possession_user_id = active_user_id;
        studio.recorded_player_movement_index = None;
        studio.player_movement_sample_accumulator = 0.0;
    } else if active_user_id.is_some() {
        studio.player_movement_sample_accumulator += delta_seconds;
    }

    let sample_due = possession_changed
        || studio.player_movement_sample_accumulator >= PLAYER_MOVEMENT_SAMPLE_SECONDS;
    let Some(user_id) = active_user_id.filter(|_| sample_due) else { return };
    let Some(position) = player_positions.get(&user_id).copied() else { return };
    studio.player_movement_sample_accumulator %= PLAYER_MOVEMENT_SAMPLE_SECONDS;

    let Some(replay) = studio.replay.as_mut() else { return };
    replay.player_movement_history_cursor_unix_ms = unix_time_ms();
    let index = studio.recorded_player_movement_index.unwrap_or_else(|| {
        replay.player_movements.push(ReplayPlayerMovement {
            user_id,
            keyframes: Vec::new(),
        });
        replay.player_movements.len() - 1
    });
    studio.recorded_player_movement_index = Some(index);
    push_player_movement_keyframe(
        &mut replay.player_movements[index],
        studio.record_elapsed_ms,
        position,
    );
}

fn push_player_movement_keyframe(
    movement: &mut ReplayPlayerMovement,
    time_ms: u64,
    position: Vec3,
) {
    let keyframe = ReplayPlayerMovementKeyframe {
        time_ms,
        position_cells: (position / VOXEL_SIZE).to_array(),
    };
    if let Some(last) = movement
        .keyframes
        .last_mut()
        .filter(|last| last.time_ms == time_ms)
    {
        *last = keyframe;
    } else {
        movement.keyframes.push(keyframe);
    }
}

fn interpolated_ship_pose(trajectory: &ReplayShipTrajectory, time_ms: u64) -> Option<(Vec3, Quat)> {
    let keyframes = &trajectory.keyframes;
    let first = keyframes.first()?;
    if time_ms <= first.time_ms {
        return Some((
            Vec3::from_array(first.translation),
            Quat::from_array(first.rotation),
        ));
    }
    let last = keyframes.last()?;
    if time_ms >= last.time_ms {
        return Some((
            Vec3::from_array(last.translation),
            Quat::from_array(last.rotation),
        ));
    }
    let right_index = keyframes.partition_point(|keyframe| keyframe.time_ms <= time_ms);
    let left = &keyframes[right_index - 1];
    let right = &keyframes[right_index];
    let fraction = ((time_ms - left.time_ms) as f32 / (right.time_ms - left.time_ms).max(1) as f32)
        .clamp(0.0, 1.0);
    Some((
        Vec3::from_array(left.translation).lerp(
            Vec3::from_array(right.translation),
            fraction,
        ),
        Quat::from_array(left.rotation).slerp(
            Quat::from_array(right.rotation),
            fraction,
        ),
    ))
}

fn replay_speech_wait_is_blocking(
    studio: &mut ReplayStudio,
    cue: (u64, usize),
    delta_seconds: f32,
) -> bool {
    if studio.speech_wait_cue != Some(cue) {
        studio.speech_wait_cue = Some(cue);
        studio.speech_wait_elapsed_seconds = 0.0;
    }
    studio.speech_wait_elapsed_seconds += delta_seconds.max(0.0);
    if studio.speech_wait_elapsed_seconds < REPLAY_SPEECH_MAX_WAIT_SECONDS {
        return true;
    }
    studio.speech_bypass_cues.insert(cue);
    false
}

fn advance_replay(
    time: Res<Time>,
    speech: Res<PreviewSpeechController>,
    mut studio: ResMut<ReplayStudio>,
    audio_sinks: Query<&AudioSink>,
    audio_players: Query<(), With<AudioPlayer>>,
) {
    if studio.mode != ReplayMode::Playing {
        return;
    }
    let Some(duration_ms) = studio.replay.as_ref().map(|replay| replay.duration_ms) else {
        studio.mode = ReplayMode::Idle;
        return;
    };
    let delta_ms = (time.delta_secs() * studio.playback_speed * 1_000.0).round() as u64;
    let recording_take =
        studio.live_movement_punch_in.is_some() || studio.live_ship_punch_in.is_some();
    let proposed_ms = studio.playback_ms.saturating_add(delta_ms);
    let proposed_ms = if recording_take { proposed_ms } else { proposed_ms.min(duration_ms) };
    let mut waiting_for_speech = None;
    let mut waiting_for_audio = None;
    if !recording_take && studio.speech_enabled && onnx_tts_is_available() {
        if let Some(replay) = studio.replay.as_ref() {
            let current = active_dialogue_index(&replay.dialogue, studio.playback_ms);
            let proposed = active_dialogue_index(&replay.dialogue, proposed_ms);
            let quick_exchange = |index: usize| {
                replay.dialogue.get(index).is_some_and(|line| {
                    !line.speech_enabled || line.duration_ms <= SHORT_DIALOGUE_MAX_MS
                })
            };
            if let Some(index) = current {
                if !quick_exchange(index) {
                    let signature = replay_voice_signature(replay, studio.speech_volume);
                    let cue = (replay.created_at_unix_ms, index);
                    if !speech.onnx_cue_finished(signature, cue) {
                        waiting_for_speech = Some(cue);
                    } else if proposed != current
                        && speech.cue_audio_is_pending(cue, &audio_sinks, &audio_players)
                    {
                        waiting_for_audio = Some(cue);
                    }
                }
            }
            if proposed != current {
                if let Some(index) = proposed {
                    if !quick_exchange(index) {
                        let signature = replay_voice_signature(replay, studio.speech_volume);
                        if !speech.onnx_cue_finished(
                            signature,
                            (replay.created_at_unix_ms, index),
                        ) {
                            waiting_for_speech = Some((replay.created_at_unix_ms, index));
                        }
                    }
                }
            }
        }
    }
    if let Some(waiting_cue) = waiting_for_speech.or(waiting_for_audio) {
        if replay_speech_wait_is_blocking(
            &mut studio,
            waiting_cue,
            time.delta_secs(),
        ) {
            studio.status = REPLAY_SPEECH_PREPARING_STATUS.to_owned();
            return;
        }
        studio.status = "当前台词语音准备超时，已跳过语音并继续回放".to_owned();
    } else {
        studio.speech_wait_cue = None;
        studio.speech_wait_elapsed_seconds = 0.0;
        if studio.status == REPLAY_SPEECH_PREPARING_STATUS {
            studio.status = REPLAY_PLAYING_STATUS.to_owned();
        }
    }
    if !recording_take && studio.turn_playback_enabled {
        if let Some(replay) = studio.replay.as_ref() {
            let turns = replay_turns(replay);
            if let Some(turn) = turns
                .iter()
                .find(|turn| studio.playback_ms < turn.end_ms)
                .filter(|turn| proposed_ms >= turn.end_ms)
            {
                studio.playback_ms = turn.end_ms;
                studio.mode = ReplayMode::Paused;
                let speakers = if turn.speaker_names.is_empty() {
                    String::new()
                } else {
                    format!("（{}）", turn.speaker_names.join("、"))
                };
                studio.status = format!(
                    "已暂停：第 {} 段{speakers}结束；点击“继续”进入下一回合",
                    turn.ordinal
                );
                return;
            }
        }
    }
    studio.playback_ms = proposed_ms;
    if recording_take {
        // Live DM movement can append footage beyond the original ending.
        // Keep the seek range and saved project duration in sync with the take.
        if let Some(replay) = studio.replay.as_mut() {
            replay.duration_ms = replay.duration_ms.max(proposed_ms);
        }
    } else if studio.playback_ms >= duration_ms {
        studio.mode = ReplayMode::Paused;
    }
}

fn preview_replay_speech(
    mut commands: Commands,
    mut studio: ResMut<ReplayStudio>,
    mut speech: ResMut<PreviewSpeechController>,
    mut audio_sources: ResMut<Assets<AudioSource>>,
) {
    let pending_generation_line_ids = studio
        .replay
        .as_ref()
        .map(replay_speech_generation_line_ids)
        .unwrap_or_default();
    let mut generation_request_consumed = false;
    if studio.speech_enabled && onnx_tts_is_available() {
        if let Some(replay) = studio.replay.as_ref() {
            let requested = replay_speech_preparation_indices(replay, studio.playback_ms);
            match speech.prepare_onnx_replay(
                replay,
                studio.speech_volume,
                &requested,
                &pending_generation_line_ids,
            ) {
                Ok(()) => generation_request_consumed = true,
                Err(err) => {
                    studio.status = format!("角色语音预览失败：{err}");
                    eprintln!("failed to prepare EmotiVoice preview speech: {err}");
                },
            }
        }
    } else {
        generation_request_consumed = true;
    }
    if generation_request_consumed {
        let current_replay_id = studio
            .replay
            .as_ref()
            .map(|replay| replay.created_at_unix_ms);
        speech
            .pending_generation_lines
            .retain(|(replay_id, line_id)| {
                Some(*replay_id) != current_replay_id
                    || !pending_generation_line_ids.contains(line_id)
            });
    }
    let active = ((studio.mode == ReplayMode::Playing || studio.video_render.is_some())
        && studio.speech_enabled)
        .then(|| {
            studio.replay.as_ref().and_then(|replay| {
                active_dialogue_index(&replay.dialogue, studio.playback_ms)
                    .filter(|index| replay.dialogue[*index].speech_enabled)
                    .map(|index| (replay.created_at_unix_ms, index))
            })
        })
        .flatten()
        .filter(|cue| !studio.speech_bypass_cues.contains(cue));
    let cue = active;
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
                speech.generation_cues.remove(&result.cue);
                speech.onnx_failures.remove(&result.cue);
                speech.onnx_cache.insert(result.cue, (wav, result.volume));
            },
            Err(err) => {
                speech.onnx_queued.remove(&result.cue);
                let attempts = speech.onnx_attempts.entry(result.cue).or_default();
                *attempts = attempts.saturating_add(1);
                if *attempts >= 2 {
                    speech.generation_cues.remove(&result.cue);
                    speech.onnx_failures.insert(result.cue, err.clone());
                    studio.status = format!("一条角色语音生成失败，其他台词将继续：{err}");
                } else {
                    studio.status = format!("角色语音通道中断，正在自动重试：{err}");
                }
                eprintln!("failed to synthesize EmotiVoice preview speech: {err}");
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
    let Some(active_cue) = active else { return };
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
        requested_indices: &[usize],
        generation_line_ids: &HashSet<u64>,
    ) -> Result<(), String> {
        let signature = replay_voice_signature(replay, global_volume);
        if self.prepared_signature != Some(signature) {
            self.prepared_signature = Some(signature);
            self.onnx_cache.clear();
            self.onnx_queued.clear();
            self.onnx_attempts.clear();
            self.onnx_failures.clear();
            self.generation_cues.clear();
            self.active_cue = None;
            if let Some(worker) = self.onnx_worker.as_ref() {
                worker.latest_signature.store(signature, Ordering::Release);
            }
        }
        for &index in requested_indices {
            let Some(line) = replay.dialogue.get(index) else {
                continue;
            };
            let cue = (replay.created_at_unix_ms, index);
            if !replay_dialogue_is_playable(line)
                || !line.speech_enabled
                || self.onnx_cache.contains_key(&cue)
                || self.onnx_failures.contains_key(&cue)
            {
                continue;
            }
            let settings = replay
                .speaker_voice_settings
                .get(&line.sender_id)
                .cloned()
                .unwrap_or_else(|| default_speaker_voice_settings(line.sender_id));
            let text = speech_text_for_line(line);
            let speaker = resolved_emotivoice_speaker(
                settings.voice_name.as_deref(),
                line.sender_id,
            );
            let emotion = resolved_emotivoice_emotion(settings.emotion.as_deref()).to_owned();
            let speed = line_onnx_speed(
                line,
                settings.speech_rate,
                replay.master_speech_speed,
            );
            let volume = (global_volume
                * settings.volume
                * normalized_line_speech_volume(line.speech_volume))
            .max(0.0);
            if let Some(wav) = read_speech_cache(&text, &speaker, &emotion, speed) {
                self.onnx_cache.insert(cue, (wav, volume));
                continue;
            }
            if !speech_generation_requested(
                line.line_id,
                cue,
                generation_line_ids,
                &self.generation_cues,
            ) {
                continue;
            }
            if self.onnx_queued.contains(&cue) {
                continue;
            }
            if self.onnx_worker.is_none() {
                self.onnx_worker = Some(start_onnx_preview_worker()?);
                self.onnx_worker
                    .as_ref()
                    .expect("ONNX worker was initialized")
                    .latest_signature
                    .store(signature, Ordering::Release);
            }
            self.generation_cues.insert(cue);
            let send_result = self
                .onnx_worker
                .as_ref()
                .expect("ONNX worker was initialized")
                .requests
                .send(OnnxPreviewRequest {
                    signature,
                    cue,
                    text,
                    speaker,
                    emotion,
                    speed,
                    volume,
                });
            if let Err(err) = send_result {
                // Dropping the disconnected sender lets the next frame create
                // a fresh worker instead of permanently repeating send errors.
                self.onnx_worker = None;
                return Err(format!(
                    "EmotiVoice preview worker stopped: {err}"
                ));
            }
            self.onnx_queued.insert(cue);
        }
        Ok(())
    }

    fn onnx_cue_finished(&self, signature: u64, cue: (u64, usize)) -> bool {
        self.prepared_signature == Some(signature)
            && (self.onnx_cache.contains_key(&cue)
                || self.onnx_failures.contains_key(&cue)
                || !self.onnx_queued.contains(&cue))
    }

    fn cue_audio_is_pending(
        &self,
        cue: (u64, usize),
        audio_sinks: &Query<&AudioSink>,
        audio_players: &Query<(), With<AudioPlayer>>,
    ) -> bool {
        if !self.onnx_cache.contains_key(&cue) {
            return false;
        }
        if self.active_cue != Some(cue) {
            return true;
        }
        let Some(entity) = self.audio_entity else {
            return true;
        };
        if let Ok(sink) = audio_sinks.get(entity) {
            return !sink.empty();
        }
        audio_players.get(entity).is_ok()
    }

    fn preparation_progress(
        &self,
        replay: &ReplayFile,
        global_volume: f32,
    ) -> (usize, usize, usize) {
        let total = replay
            .dialogue
            .iter()
            .filter(|line| replay_dialogue_is_playable(line))
            .count();
        if self.prepared_signature
            != Some(replay_voice_signature(
                replay,
                global_volume,
            ))
        {
            return (0, 0, total);
        }
        let ready = replay
            .dialogue
            .iter()
            .enumerate()
            .filter(|(index, line)| {
                replay_dialogue_is_playable(line)
                    && self
                        .onnx_cache
                        .contains_key(&(replay.created_at_unix_ms, *index))
            })
            .count();
        let failed = replay
            .dialogue
            .iter()
            .enumerate()
            .filter(|(index, line)| {
                replay_dialogue_is_playable(line)
                    && self
                        .onnx_failures
                        .contains_key(&(replay.created_at_unix_ms, *index))
            })
            .count();
        (ready, failed, total)
    }
}

fn replay_message_is_eligible(
    audience: &ReplayAudience,
    campaign_id: &str,
    message: &CampaignMessage,
    manager: &NapcatMessageManager,
) -> bool {
    message.campaign_id == campaign_id && audience.can_read(message, manager)
}

fn replay_speech_progress_ui(
    ui: &mut egui::Ui,
    studio: &ReplayStudio,
    speech: &PreviewSpeechController,
) {
    let Some(replay) = studio.replay.as_ref() else { return };
    let (ready, failed, total) = speech.preparation_progress(replay, studio.speech_volume);
    let generation_active = !speech.generation_cues.is_empty()
        || speech
            .pending_generation_lines
            .iter()
            .any(|(replay_id, _)| *replay_id == replay.created_at_unix_ms);
    let processed = ready.saturating_add(failed);
    let progress = if total == 0 { 1.0 } else { processed as f32 / total as f32 };
    let text = if !studio.speech_enabled {
        format!("语音预生成已暂停：{ready}/{total}")
    } else if !onnx_tts_is_available() {
        format!("语音预生成不可用：{ready}/{total}")
    } else if failed > 0 && processed == total {
        format!("语音预生成完成：成功 {ready}/{total}，失败 {failed}")
    } else if failed > 0 {
        format!("正在生成角色语音：成功 {ready}/{total}，失败 {failed}")
    } else if ready == total {
        format!("语音缓存已就绪：{ready}/{total}")
    } else if generation_active || processed < total {
        format!("正在生成角色语音：{ready}/{total}")
    } else {
        format!("语音缓存状态：{ready}/{total}")
    };
    ui.add(
        egui::ProgressBar::new(progress)
            .desired_width(ui.available_width())
            .text(text),
    );
}

fn onnx_tts_is_available() -> bool {
    emotivoice_python_path().is_file()
        && emotivoice_worker_path().is_file()
        && emotivoice_source_path().is_dir()
}

fn emotivoice_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(EMOTIVOICE_RUNTIME_DIR)
}

fn emotivoice_python_path() -> PathBuf {
    let root = emotivoice_root().join(".venv313");
    if cfg!(windows) {
        root.join("Scripts").join("python.exe")
    } else {
        root.join("bin").join("python")
    }
}

fn emotivoice_source_path() -> PathBuf { emotivoice_root().join("EmotiVoice") }

fn emotivoice_speech_cache_path(text: &str, speaker: &str, emotion: &str, speed: f32) -> PathBuf {
    let mut hash = 0xcbf29ce484222325_u64;
    for part in [
        EMOTIVOICE_SPEECH_CACHE_VERSION.to_le_bytes().as_slice(),
        emotivoice_model_text(text).as_bytes(),
        speaker.as_bytes(),
        emotion.as_bytes(),
        speed.to_bits().to_le_bytes().as_slice(),
    ] {
        for byte in (part.len() as u64).to_le_bytes().iter().chain(part) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    emotivoice_root()
        .join("speech-cache")
        .join(format!("{hash:016x}.wav"))
}

fn read_speech_cache(text: &str, speaker: &str, emotion: &str, speed: f32) -> Option<Vec<u8>> {
    let path = emotivoice_speech_cache_path(text, speaker, emotion, speed);
    let bytes = fs::read(&path).ok()?;
    if bytes.len() >= 44 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        Some(bytes)
    } else {
        let _ = fs::remove_file(path);
        None
    }
}

fn cache_speech(text: &str, speaker: &str, emotion: &str, speed: f32, wav: &[u8]) {
    let path = emotivoice_speech_cache_path(text, speaker, emotion, speed);
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(mut temporary) = tempfile::NamedTempFile::new_in(parent) else { return };
    if temporary.write_all(wav).is_ok() && temporary.flush().is_ok() {
        let _ = temporary.persist_noclobber(path);
    }
}

fn emotivoice_worker_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("emotivoice_worker.py")
}

struct EmotiVoiceTts {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    cache: TempDir,
    sequence: u64,
}

impl Drop for EmotiVoiceTts {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn create_onnx_tts() -> Result<EmotiVoiceTts, String> {
    if !onnx_tts_is_available() {
        return Err(
            "未安装 EmotiVoice 中文运行环境；请运行 scripts/setup_emotivoice.ps1".to_owned(),
        );
    }
    let cache_root = emotivoice_root().join("worker-cache");
    fs::create_dir_all(&cache_root)
        .map_err(|err| format!("无法创建 EmotiVoice 缓存目录：{err}"))?;
    let cache = tempfile::Builder::new()
        .prefix("worker-")
        .tempdir_in(&cache_root)
        .map_err(|err| format!("无法创建 EmotiVoice 临时目录：{err}"))?;
    let log = fs::File::create(cache.path().join("emotivoice.log"))
        .map_err(|err| format!("无法创建 EmotiVoice 日志：{err}"))?;
    let mut command = Command::new(emotivoice_python_path());
    command
        .arg(emotivoice_worker_path())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env(
            "EMOTIVOICE_SOURCE",
            emotivoice_source_path(),
        )
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    hide_command_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|err| format!("无法启动 EmotiVoice 中文进程：{err}"))?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| "EmotiVoice 没有打开输入流".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "EmotiVoice 没有打开输出流".to_owned())?;
    let mut output = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        if output
            .read_line(&mut line)
            .map_err(|err| format!("读取 EmotiVoice 启动状态失败：{err}"))?
            == 0
        {
            return Err("EmotiVoice 在模型加载完成前退出，请查看 worker-cache 中的日志".to_owned());
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
                .unwrap_or("EmotiVoice 中文模型加载失败")
                .to_owned());
        }
    }
    Ok(EmotiVoiceTts {
        child,
        input,
        output,
        cache,
        sequence: 0,
    })
}

impl EmotiVoiceTts {
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
            .map_err(|err| format!("发送 EmotiVoice 台词失败：{err}"))?;
        let mut response = String::new();
        self.output
            .read_line(&mut response)
            .map_err(|err| format!("读取 EmotiVoice 结果失败：{err}"))?;
        let response: serde_json::Value = serde_json::from_str(&response)
            .map_err(|err| format!("EmotiVoice 返回了无效结果：{err}"))?;
        if response.get("ok").and_then(|value| value.as_bool()) != Some(true) {
            return Err(response["error"]
                .as_str()
                .unwrap_or("EmotiVoice 中文合成失败")
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
            .map_err(|err| format!("无法转换 EmotiVoice 角色语音：{err}"))?;
        let _ = fs::remove_file(&raw_path);
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        let wav =
            fs::read(&output_path).map_err(|err| format!("无法读取 EmotiVoice WAV：{err}"))?;
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
    let ending = characters.last().copied().filter(|character| {
        matches!(
            character,
            '。' | '！' | '？' | '.' | '!' | '?'
        )
    });
    if ending.is_some() {
        characters.pop();
    }
    let half = characters.len() / 2;
    if characters.len() >= 4
        && characters.len() <= SHORT_UTTERANCE_MAX_UNITS
        && characters.len() % 2 == 0
        && characters
            .iter()
            .all(|character| is_cjk_character(*character))
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
        .name("replay-emotivoice-preview".to_owned())
        .spawn(move || {
            let mut tts = create_onnx_tts();
            while let Ok(request) = request_rx.recv() {
                if request.signature != worker_signature.load(Ordering::Acquire) {
                    continue;
                }
                let wav = tts.as_mut().map_err(|err| err.clone()).and_then(|tts| {
                    let wav = tts.synthesize(
                        &request.text,
                        &request.speaker,
                        &request.emotion,
                        request.speed,
                    )?;
                    cache_speech(
                        &request.text,
                        &request.speaker,
                        &request.emotion,
                        request.speed,
                        &wav,
                    );
                    Ok(wav)
                });
                if wav
                    .as_ref()
                    .is_err_and(|err| tts_worker_connection_error(err))
                {
                    // A dead Python pipe cannot recover. Drop it now so the
                    // controller's automatic retry gets a fresh process.
                    tts = create_onnx_tts();
                }
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
        .map_err(|err| format!("无法启动 EmotiVoice 预览线程：{err}"))?;
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
        if let Some(music_file) = job.music_file.as_deref() {
            let monitor_path = job.frames.path().join("render-monitor-music.wav");
            match write_background_music_track(
                music_file,
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
    let music_file = job.music_file;
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
            music_file.as_deref(),
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
    mut fade: Option<ResMut<VoxelReplayOcclusionFade>>,
    mut camera: Query<
        &mut Transform,
        (
            With<VoxelViewportCamera>,
            Without<VoxelPlayerStandee>,
        ),
    >,
    standees: Query<
        (&Transform, &VoxelPlayerStandee),
        (
            With<VoxelPlayerStandee>,
            Without<VoxelViewportCamera>,
        ),
    >,
) {
    if let Some(fade) = fade.as_mut() {
        fade.active = false;
        fade.targets.clear();
    }
    if !matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    ) {
        return;
    }
    if studio.live_movement_punch_in.is_some() || studio.live_ship_punch_in.is_some() {
        // A punch-in take uses the live possession camera. The deterministic
        // authored camera resumes as soon as the take is saved or cancelled.
        return;
    }
    let Some(replay) = studio.replay.as_ref() else { return };
    let Some(transform) = interpolated_camera(
        &replay.camera,
        studio.playback_ms,
        replay.camera_transition_curve,
    ) else {
        return;
    };
    if let Ok(mut camera) = camera.single_mut() {
        *camera = transform;
        if let Some(fade) = fade.as_mut() {
            set_replay_occlusion_targets(
                fade,
                transform.translation,
                standees.iter().map(|(transform, _)| transform.translation),
            );
        }
    }
}

fn set_replay_occlusion_targets(
    fade: &mut VoxelReplayOcclusionFade,
    camera: Vec3,
    targets: impl IntoIterator<Item = Vec3>,
) {
    fade.camera = camera;
    fade.targets.clear();
    fade.targets.extend(targets);
    fade.active = !fade.targets.is_empty();
}

fn tts_worker_connection_error(error: &str) -> bool {
    [
        "发送 EmotiVoice 台词失败",
        "读取 EmotiVoice 结果失败",
        "EmotiVoice 返回了无效结果",
        "模型加载完成前退出",
    ]
    .iter()
    .any(|marker| error.contains(marker))
}

fn replay_studio_ui(
    mut contexts: EguiContexts,
    manager: Res<Persistent<NapcatMessageManager>>,
    deepseek_sender: Option<Res<DeepseekIOSender>>,
    mut deepseek_manager: ResMut<Persistent<DeepseekManager>>,
    mut studio: ResMut<ReplayStudio>,
    mut voice_favorites: ResMut<Persistent<ReplayVoiceFavorites>>,
    mut replay_histories: ReplayHistoryParams,
    speech: Res<PreviewSpeechController>,
    camera: Query<&Transform, With<VoxelViewportCamera>>,
    standees: Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut capture_active: ResMut<ReplayVideoCaptureActive>,
    mut occlusion_fade: Option<ResMut<VoxelReplayOcclusionFade>>,
    mut live_edits: ReplayLiveEditParams,
    mut avatar_textures: Local<HashMap<String, egui::TextureHandle>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let mut occlusion_opacity = occlusion_fade.as_ref().map_or(0.0, |fade| fade.opacity);
    let mut occlusion_cast_width_cells = occlusion_fade.as_ref().map_or(
        DEFAULT_VOXEL_OCCLUSION_CAST_WIDTH_CELLS,
        |fade| fade.cast_width_cells,
    );
    let mut occlusion_cast_height_cells = occlusion_fade.as_ref().map_or(
        DEFAULT_VOXEL_OCCLUSION_CAST_HEIGHT_CELLS,
        |fade| fade.cast_height_cells,
    );
    let mut occlusion_cast_end_width_cells = occlusion_fade.as_ref().map_or(
        DEFAULT_VOXEL_OCCLUSION_CAST_END_WIDTH_CELLS,
        |fade| fade.cast_end_width_cells,
    );
    let mut occlusion_cast_end_height_cells = occlusion_fade.as_ref().map_or(
        DEFAULT_VOXEL_OCCLUSION_CAST_END_HEIGHT_CELLS,
        |fade| fade.cast_end_height_cells,
    );
    let mut occlusion_debug_gizmo = occlusion_fade.as_ref().is_some_and(|fade| fade.debug_gizmo);

    if !capture_active.0 {
        egui::Area::new(egui::Id::new("replay-studio-button"))
            .anchor(
                egui::Align2::RIGHT_TOP,
                egui::vec2(-12.0, 44.0),
            )
            .show(ctx, |ui| {
                if ui.button("🎬 回放").clicked() {
                    studio.panel_open = true;
                    ui::raise_and_expand_window(
                        ui.ctx(),
                        egui::Id::new("trpg-replay-studio"),
                    );
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
            .max_height((ctx.content_rect().height() - 32.0).max(320.0))
            .show(ctx, |ui| {
                ui.set_max_width(max_window_width);
                egui::ScrollArea::vertical()
                    .id_salt("trpg-replay-studio-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        replay_controls(
                            ui,
                            &manager,
                            deepseek_sender.as_deref(),
                            &mut deepseek_manager,
                            &mut studio,
                            &voice_favorites,
                            &mut replay_histories.player_movement_history,
                            &mut replay_histories.ship_trajectory_history,
                            &speech,
                            &camera,
                            &standees,
                            &mut grids,
                            &mut windows,
                            &mut capture_active,
                            &mut live_edits,
                            &mut occlusion_opacity,
                            &mut occlusion_cast_width_cells,
                            &mut occlusion_cast_height_cells,
                            &mut occlusion_cast_end_width_cells,
                            &mut occlusion_cast_end_height_cells,
                            &mut occlusion_debug_gizmo,
                        )
                    });
            });
        studio.panel_open = open;
    }
    if let Some(fade) = occlusion_fade.as_mut() {
        fade.opacity = occlusion_opacity.clamp(0.0, 1.0);
        fade.cast_width_cells = occlusion_cast_width_cells.clamp(
            MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
            MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
        );
        fade.cast_height_cells = occlusion_cast_height_cells.clamp(
            MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
            MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
        );
        fade.cast_end_width_cells = occlusion_cast_end_width_cells.clamp(
            MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
            MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
        );
        fade.cast_end_height_cells = occlusion_cast_end_height_cells.clamp(
            MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
            MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
        );
        fade.debug_gizmo = occlusion_debug_gizmo;
    }

    if studio.speech_settings_open && !capture_active.0 {
        speech_settings_window(ctx, &mut studio, &mut voice_favorites);
    }

    if matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    ) {
        if let Some((index, dialogue)) = studio.replay.as_ref().and_then(|replay| {
            active_dialogue_index(&replay.dialogue, studio.playback_ms)
                .map(|index| (index, replay.dialogue[index].clone()))
        }) {
            if replay_dialogue_is_ready_for_display(
                &studio,
                &speech,
                index,
                onnx_tts_is_available(),
            ) {
                dialogue_overlay(ctx, &dialogue, &mut avatar_textures);
            }
        }
    }
}

fn replay_controls(
    ui: &mut egui::Ui,
    manager: &NapcatMessageManager,
    deepseek_sender: Option<&DeepseekIOSender>,
    deepseek_manager: &mut Persistent<DeepseekManager>,
    studio: &mut ReplayStudio,
    voice_favorites: &ReplayVoiceFavorites,
    player_movement_history: &mut Persistent<ReplayPlayerMovementHistory>,
    ship_trajectory_history: &mut Persistent<ReplayShipTrajectoryHistory>,
    speech: &PreviewSpeechController,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
    capture_active: &mut ReplayVideoCaptureActive,
    live_edits: &mut ReplayLiveEditParams,
    occlusion_opacity: &mut f32,
    occlusion_cast_width_cells: &mut f32,
    occlusion_cast_height_cells: &mut f32,
    occlusion_cast_end_width_cells: &mut f32,
    occlusion_cast_end_height_cells: &mut f32,
    occlusion_debug_gizmo: &mut bool,
) {
    ui.label("记录体素场景和可见对话，并在应用内确定性回放。");
    ui.small("飞船轨迹、地形变化和玩家接管移动都会常态化自动记录，无需先点击“开始录制”；回放生成后新录到的内容会在按“播放”时自动并入时间轴。");
    if matches!(
        &studio.audience,
        ReplayAudience::All | ReplayAudience::Gm
    ) {
        ui.group(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(220, 80, 65),
                "隐私警告：“全部 / All”包含队伍隐藏消息、玩家私聊、GM 与系统内容。使用 DeepSeek 导演会把当前勾选的这些台词上传到 DeepSeek；分享项目或视频前请先逐句检查。",
            );
        });
    }
    ui.separator();
    ui.collapsing("录制、镜头与遮挡设置", |ui| {
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
                        ReplayAudience::All,
                        "全部 / All（GM 可见的全部内容）",
                    );
                });
        });
        ui.checkbox(
            &mut studio.record_camera_enabled,
            "录制 DM 自由镜头（默认关闭，点击后才采集）",
        )
        .on_hover_text("关闭时只记录场景和台词，不持续采集你的镜头移动。");
        let mut requested_camera_distance = studio.camera_distance_scale;
        let camera_distance_changed = ui
            .horizontal(|ui| {
                ui.label("自动导演镜头距离");
                ui.add(
                    egui::DragValue::new(&mut requested_camera_distance)
                        .speed(0.05)
                        .range(
                            MIN_DIRECTED_CAMERA_DISTANCE_SCALE..=MAX_DIRECTED_CAMERA_DISTANCE_SCALE,
                        )
                        .fixed_decimals(2)
                        .suffix("×"),
                )
                .on_hover_text("调整自动生成和 DeepSeek 导演镜头与当前说话玩家之间的距离。")
                .changed()
            })
            .inner;
        if camera_distance_changed {
            studio.camera_distance_scale =
                normalized_directed_camera_distance_scale(requested_camera_distance);
            let camera_distance_scale = studio.camera_distance_scale;
            if let Some(replay) = studio.replay.as_mut() {
                let speaker_positions = standee_positions(standees);
                rescale_replay_camera_distance(
                    replay,
                    camera_distance_scale,
                    &speaker_positions,
                );
            }
            studio.status = format!(
                "自动导演镜头距离已设为 {:.2}×",
                studio.camera_distance_scale
            );
        }
        let mut requested_camera_yaw = studio.camera_yaw_degrees;
        let camera_yaw_changed = ui
            .horizontal(|ui| {
                ui.label("焦点镜头水平旋转");
                ui.add(
                    egui::DragValue::new(&mut requested_camera_yaw)
                        .speed(1.0)
                        .range(
                            MIN_DIRECTED_CAMERA_YAW_DEGREES
                                ..=MAX_DIRECTED_CAMERA_YAW_DEGREES,
                        )
                        .fixed_decimals(1)
                        .suffix("°"),
                )
                .on_hover_text(
                    "围绕当前焦点水平旋转自动生成和 DeepSeek 导演镜头；镜头仍对准玩家并保持在同一拍摄侧。",
                )
                .changed()
            })
            .inner;
        if camera_yaw_changed {
            let requested_camera_yaw =
                normalized_directed_camera_yaw_degrees(requested_camera_yaw);
            let mut applied_camera_yaw = requested_camera_yaw;
            if let Some(replay) = studio.replay.as_mut() {
                let speaker_positions = standee_positions(standees);
                applied_camera_yaw = rotate_replay_camera_yaw(
                    replay,
                    requested_camera_yaw,
                    &speaker_positions,
                );
            }
            studio.camera_yaw_degrees = applied_camera_yaw;
            studio.status = format!(
                "焦点镜头水平旋转已设为 {:.1}°",
                studio.camera_yaw_degrees
            );
        }
        let mut requested_transition_curve = studio.camera_transition_curve;
        let transition_curve_changed = ui
            .horizontal(|ui| {
                ui.label("焦点切换缓动曲线");
                ui.add(
                    egui::Slider::new(
                        &mut requested_transition_curve,
                        MIN_CAMERA_TRANSITION_CURVE..=MAX_CAMERA_TRANSITION_CURVE,
                    )
                    .step_by(0.1)
                    .fixed_decimals(1),
                )
                .on_hover_text("1.0 为匀速；数值越高，切换玩家时镜头起步和停下越柔和。")
                .changed()
            })
            .inner;
        if transition_curve_changed {
            studio.camera_transition_curve =
                normalized_camera_transition_curve(requested_transition_curve);
            if let Some(replay) = studio.replay.as_mut() {
                replay.camera_transition_curve = studio.camera_transition_curve;
            }
            studio.status = format!(
                "焦点切换缓动曲线已设为 {:.1}",
                studio.camera_transition_curve
            );
        }
        let mut requested_player_movement_curve = studio.player_movement_curve;
        let player_movement_curve_changed = ui
            .horizontal(|ui| {
                ui.label("玩家移动平滑曲线");
                ui.add(
                    egui::Slider::new(
                        &mut requested_player_movement_curve,
                        MIN_PLAYER_MOVEMENT_CURVE..=MAX_PLAYER_MOVEMENT_CURVE,
                    )
                    .step_by(0.05)
                    .fixed_decimals(2),
                )
                .on_hover_text("0 为逐点直线移动；1 为最平滑的轨迹曲线。")
                .changed()
            })
            .inner;
        if player_movement_curve_changed {
            studio.player_movement_curve =
                normalized_player_movement_curve(requested_player_movement_curve);
            if let Some(replay) = studio.replay.as_mut() {
                replay.player_movement_curve = studio.player_movement_curve;
            }
            studio.status = format!(
                "玩家移动平滑曲线已设为 {:.2}",
                studio.player_movement_curve
            );
        }
        ui.add(
            egui::Slider::new(
                occlusion_cast_width_cells,
                MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS..=MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
            )
            .text("镜头端宽度（体素）")
            .integer(),
        )
        .on_hover_text("射线方盒在回放镜头位置的起始宽度。");
        ui.add(
            egui::Slider::new(
                occlusion_cast_height_cells,
                MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS..=MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
            )
            .text("镜头端高度（体素）")
            .integer(),
        )
        .on_hover_text("射线方盒在回放镜头位置的起始高度。");
        ui.add(
            egui::Slider::new(
                occlusion_cast_end_width_cells,
                MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS..=MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
            )
            .text("玩家端宽度（体素）")
            .integer(),
        )
        .on_hover_text("方盒沿距离线性缩小，在玩家目标位置达到此宽度。");
        ui.add(
            egui::Slider::new(
                occlusion_cast_end_height_cells,
                MIN_VOXEL_OCCLUSION_CAST_SIZE_CELLS..=MAX_VOXEL_OCCLUSION_CAST_SIZE_CELLS,
            )
            .text("玩家端高度（体素）")
            .integer(),
        )
        .on_hover_text("方盒沿距离线性缩小，在玩家目标位置达到此高度。");
        ui.add(
            egui::Slider::new(occlusion_opacity, 0.0..=1.0)
                .text("方盒内体素不透明度")
                .fixed_decimals(2),
        )
        .on_hover_text("0 为完全透明，1 为完全不透明；中间值使用真实半透明混合。");
        ui.checkbox(
            occlusion_debug_gizmo,
            "显示剔除射线调试框",
        )
        .on_hover_text("显示镜头到每名玩家的中心线和宽高方盒。");
    });

    ui.horizontal(|ui| match studio.mode {
        ReplayMode::Recording => {
            ui.label(format!(
                "● 录制中 {}",
                format_time(studio.record_elapsed_ms)
            ));
            if ui.button("停止录制").clicked() {
                stop_recording(studio, ship_trajectory_history);
            }
        },
        ReplayMode::Playing | ReplayMode::Paused => {
            let completed = replay_has_completed(studio);
            if ui
                .button(if studio.mode == ReplayMode::Playing {
                    "暂停"
                } else if completed {
                    "重播"
                } else {
                    "继续"
                })
                .clicked()
            {
                studio.mode = if studio.mode == ReplayMode::Playing {
                    ReplayMode::Paused
                } else {
                    if completed {
                        studio.playback_ms = 0;
                    }
                    ReplayMode::Playing
                };
                if studio.mode == ReplayMode::Playing {
                    studio.status = REPLAY_PLAYING_STATUS.to_owned();
                }
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
                build_from_history(
                    studio,
                    manager,
                    voice_favorites,
                    player_movement_history,
                    ship_trajectory_history,
                    camera,
                    standees,
                    grids,
                );
            }
        },
    });

    if let Some(replay) = studio.replay.as_ref() {
        let movement_frame_count = replay
            .player_movements
            .iter()
            .map(|movement| movement.keyframes.len())
            .sum::<usize>();
        let ship_count = replay.ship_trajectories.len();
        let terrain_change_count = replay.terrain_changes.len();
        let hull_change_count = replay.ship_hull_changes.len();
        ui.label(format!(
            "{} · {} · {} 个镜头帧 · {} 帧玩家移动 · {} 艘飞船轨迹 · {} 条地形变化 · {} 条船体变化 · {} 条对话 · {} 个体素",
            replay.title,
            format_time(replay.duration_ms),
            replay.camera.len(),
            movement_frame_count,
            ship_count,
            terrain_change_count,
            hull_change_count,
            replay.dialogue.len(),
            replay.scene.voxels.len(),
        ));
    }
    replay_live_edit_panel(ui, studio, live_edits, standees);
    ui.add_enabled_ui(!replay_has_live_take(studio), |ui| {
        replay_dialogue_editor(ui, studio, camera);
        replay_movement_timing_editor(
            ui,
            studio,
            player_movement_history,
            camera,
        );
        replay_ship_trajectory_editor(
            ui,
            studio,
            ship_trajectory_history,
            camera,
        );
    });
    if !matches!(studio.mode, ReplayMode::Recording) && studio.replay.is_some() {
        let duration = studio.replay.as_ref().unwrap().duration_ms.max(1);
        ui.horizontal(|ui| {
            if !matches!(
                studio.mode,
                ReplayMode::Playing | ReplayMode::Paused
            ) && ui.button("▶ 播放").clicked()
            {
                prepare_replay_movement_for_playback(
                    studio,
                    player_movement_history,
                    ship_trajectory_history,
                );
                start_playback(studio, grids);
            }
            ui.add_enabled(
                studio.live_movement_punch_in.is_none() && studio.live_ship_punch_in.is_none(),
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
        replay_speech_progress_ui(ui, studio, speech);
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
        ui.checkbox(
            &mut studio.turn_playback_enabled,
            "按回合播放（每回合结束暂停，如同 GM 带团）",
        );
        if let Some(replay) = studio.replay.as_ref() {
            let turns = replay_turns(replay);
            if !turns.is_empty() {
                let current_turn = current_replay_turn(&turns, studio.playback_ms);
                ui.horizontal(|ui| {
                    let previous = current_turn
                        .and_then(|index| index.checked_sub(1))
                        .or_else(|| previous_replay_turn(&turns, studio.playback_ms));
                    if ui
                        .add_enabled(
                            previous.is_some() && !replay_has_live_take(studio),
                            egui::Button::new("⏮ 上一回合"),
                        )
                        .clicked()
                    {
                        if let Some(index) = previous {
                            jump_replay_to_turn(studio, &turns, index);
                        }
                    }
                    let next = next_replay_turn(&turns, studio.playback_ms);
                    if ui
                        .add_enabled(
                            next.is_some() && !replay_has_live_take(studio),
                            egui::Button::new("下一回合 ⏭"),
                        )
                        .clicked()
                    {
                        if let Some(index) = next {
                            jump_replay_to_turn(studio, &turns, index);
                        }
                    }
                    if let Some(index) = current_turn {
                        let turn = &turns[index];
                        ui.label(format!(
                            "第 {} 段 {}（{}）",
                            turn.ordinal,
                            turn.label(),
                            turn.speaker_names
                                .first()
                                .map(String::as_str)
                                .unwrap_or("等待中"),
                        ));
                    } else {
                        ui.label("尚未进入回合时间轴");
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    for (index, turn) in turns.iter().enumerate() {
                        let selected = current_turn == Some(index);
                        if ui
                            .selectable_label(
                                selected,
                                format!("{}.{}", turn.ordinal, turn.turn_index),
                            )
                            .clicked()
                        {
                            jump_replay_to_turn(studio, &turns, index);
                        }
                    }
                });
            }
        }
        ui.add_enabled_ui(!replay_has_live_take(studio), |ui| {
            ui.collapsing("飞船移动节奏", |ui| {
                ui.small(
                    "飞船机动会按此速度压缩播放；关闭“下一句等待”后，机动会移到所有台词之后。",
                );
                let mut speed = studio
                    .replay
                    .as_ref()
                    .map(|replay| replay.ship_motion_speed)
                    .unwrap_or_else(default_ship_motion_speed);
                let mut waits = studio
                    .replay
                    .as_ref()
                    .map(|replay| replay.dialogue_waits_for_ship_motion)
                    .unwrap_or_else(default_dialogue_waits_for_ship_motion);
                let speed_changed = ui
                    .add(
                        egui::Slider::new(&mut speed, 0.5..=10.0)
                            .step_by(0.5)
                            .text("飞船移动速度×"),
                    )
                    .changed();
                let waits_changed = ui
                    .checkbox(&mut waits, "下一句台词等待飞船移动结束")
                    .changed();
                if speed_changed || waits_changed {
                    if let Some(replay) = studio.replay.as_mut() {
                        replay.ship_motion_speed = speed;
                        replay.dialogue_waits_for_ship_motion = waits;
                        rebuild_replay_timeline_with_ships(replay, ship_trajectory_history, camera);
                    }
                    studio.playback_ms = 0;
                    studio.status = "飞船移动节奏已更新".to_owned();
                }
            });
        });
    }

    ui.separator();
    if replay_has_live_take(studio) {
        ui.small("请先保存或取消本次录制，再调整时间轴、应用导演方案、保存/载入项目或导出视频。");
    }
    ui.add_enabled_ui(!replay_has_live_take(studio), |ui| {
    ui.collapsing("DeepSeek 视频导演", |ui| {
    ui.small("DeepSeek 可直接润色当前回放中已勾选的台词，并为每句选择固定构图或缓慢推拉；选择“全部 / All”时，上传内容也会包含队伍隐藏消息、玩家私聊、GM 与系统台词。三人及以上时，本地镜头会优先放在队伍正面的中央位置，避免从侧面拍摄时角色互相遮挡；镜头只平滑移动位置，不环绕角色。它还会生成只供 EmotiVoice 使用的中文谐音读法，画面字幕仍显示正常原文，并且不得新增剧情事实。");
    let visible_standees = standees
        .iter()
        .map(|(transform, standee)| (standee.user_id, transform.translation))
        .collect::<HashMap<_, _>>();
    let missing_director_lines = studio
        .replay
        .as_ref()
        .map(|replay| missing_director_dialogue(replay, &visible_standees))
        .unwrap_or_default();
    if !missing_director_lines.is_empty() {
        ui.group(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(210, 90, 70),
                format!(
                    "以下 {} 句找不到可用于导演镜头的说话者立牌：",
                    missing_director_lines.len()
                ),
            );
            for (_, description) in &missing_director_lines {
                ui.label(format!("• {description}"));
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("快速修复：排除上述台词").clicked() {
                    let missing_indices = missing_director_lines
                        .iter()
                        .map(|(index, _)| *index)
                        .collect::<Vec<_>>();
                    if let Some(replay) = studio.replay.as_mut() {
                        for index in &missing_indices {
                            if let Some(line) = replay.dialogue.get_mut(*index) {
                                line.included = false;
                            }
                        }
                        recompile_edited_replay_dialogue(replay);
                        extend_replay_for_speech(replay);
                    }
                    studio.director_request_pending = false;
                    studio.director_response_hash = None;
                    studio.auto_export_after_director = false;
                    studio.status = format!(
                        "已排除 {} 句缺少立牌的台词；原文仍保留，可在台词编辑中重新勾选",
                        missing_indices.len()
                    );
                }
                if ui.button("从当前聊天重建回放").clicked() {
                    build_from_history(
                        studio,
                        manager,
                        voice_favorites,
                        player_movement_history,
                        ship_trajectory_history,
                        camera,
                        standees,
                        grids,
                    );
                }
            });
            ui.small("排除只会取消这些台词的“使用”勾选，不会删除台词。重建会用当前聊天和场景替换现有回放编辑。");
        });
    }
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
    let saved_director_available = studio
        .replay
        .as_ref()
        .and_then(|replay| {
            replay_director_request(
                replay,
                &studio.deepseek_custom_prompt,
                standees,
            )
            .ok()
            .map(|request| saved_director_block(replay, deepseek_manager, &request).is_some())
        })
        .unwrap_or(false);
    let director_button_text = if saved_director_available {
        "应用已保存的导演方案"
    } else {
        "生成并应用导演方案"
    };
    if ui
        .add_enabled(
            studio
                .replay
                .as_ref()
                .is_some_and(|replay| {
                    replay
                        .dialogue
                        .iter()
                        .any(replay_dialogue_is_playable)
                }),
            egui::Button::new(director_button_text),
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
            Ok(source) => {
                studio.director_request_pending = true;
                match source {
                    DirectorPlanSource::Saved => {
                        "已找到完全匹配的已保存 DeepSeek 导演方案，正在应用；未请求 API".to_owned()
                    },
                    DirectorPlanSource::Api => {
                        if let Err(err) = deepseek_manager.persist() {
                            format!("DeepSeek 请求已发送，但保存请求状态失败：{err}")
                        } else {
                            "DeepSeek 正在润色台词并设计逐句镜头；返回后会自动应用".to_owned()
                        }
                    },
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
        start_video_export(
            studio,
            capture_active,
            grids,
            windows,
            player_movement_history,
            ship_trajectory_history,
        );
    }
    ui.small("只有开启“录制 DM 自由镜头”才会持续采集镜头。DeepSeek 导演开启后会等待 API 返回，再应用润色台词和镜头决策。");
    ui.small(
        "较长回放会自动分批交给 DeepSeek，再按原台词顺序合并；单个批次若被截断会继续拆分重试。",
    );
    });

    ui.separator();
    ui.heading("导出 MP4 视频");
    ui.collapsing("视频、背景音乐与角色语音设置", |ui| {
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
        let music_available = studio
            .music_file
            .as_ref()
            .is_some_and(|path| path.is_file());
        ui.add_enabled_ui(music_available, |ui| {
            ui.checkbox(&mut studio.music_enabled, "本地 BGM");
        });
        ui.add_enabled(
            studio.music_enabled && music_available,
            egui::Slider::new(&mut studio.music_volume, 0.05..=1.50)
                .text("音量")
                .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
        );
    });
    ui.horizontal(|ui| {
        ui.label("曲目");
        ui.add_enabled_ui(!studio.music_files.is_empty(), |ui| {
            egui::ComboBox::from_id_salt("replay-background-music")
                .selected_text(
                    studio
                        .music_file
                        .as_deref()
                        .and_then(Path::file_name)
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "未选择".to_owned()),
                )
                .show_ui(ui, |ui| {
                    for path in &studio.music_files {
                        let label = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        ui.selectable_value(
                            &mut studio.music_file,
                            Some(path.clone()),
                            label,
                        );
                    }
                });
        });
        if ui.button("重新扫描").clicked() {
            refresh_background_music(studio);
        }
        if ui.button("打开文件夹").clicked() {
            studio.status = match open_background_music_directory() {
                Ok(()) => format!(
                    "已打开 {}",
                    background_music_directory().display()
                ),
                Err(err) => err,
            };
        }
    });
    if studio.music_files.is_empty() {
        ui.small("BGM 文件夹为空；加入音乐后点击“重新扫描”。");
    }
    ui.small(
        "从 assets/audio 加载 MP3、WAV、OGG、FLAC、M4A 或 AAC；音乐只在本地读取，不由 AI 生成。",
    );
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut studio.speech_enabled,
            "角色语音（预览与导出，EmotiVoice 固定中文音色）",
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
                ui.add(
                    egui::DragValue::new(&mut replay.master_speech_speed)
                        .speed(0.05)
                        .range(0.10..=f32::INFINITY)
                        .fixed_decimals(2)
                        .suffix("×"),
                )
                .on_hover_text(
                    "只调整所有角色在播放预览和 MP4 导出中的语速，不改变时间轴；没有上限。",
                );
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
                studio.playback_ms = studio.playback_ms.min(replay.duration_ms);
            }
        }
        if ui.button("角色语音设置…").clicked() {
            studio.speech_settings_open = true;
            ui::raise_and_expand_window(
                ui.ctx(),
                egui::Id::new("replay-speaker-voice-settings"),
            );
        }
    });
    ui.small("每条台词进入回放后会立即排队生成并缓存角色语音，不需要等待下一条消息或点击播放。预览与 MP4 导出共用缓存。");
    ui.small("整体语速默认 1.10×，调整语速或单个角色音色时不会改变时间轴。EmotiVoice 使用固定说话人 ID，声音会更机械，但同一玩家跨台词保持一致且生成更快。需要改变字幕、间隔和镜头时长时，请使用“整体台词停留”。预览与导出共用同一条时间线和语音缓存。DeepSeek 另行生成只供发音使用的中文谐音文本，画面仍显示正常中英文原文。所有语音均在本机生成，不上传网络。");
    if let Some(replay) = studio.replay.as_ref() {
        ui.small(format!(
            "预计渲染 {} 帧，视频时长 {}",
            video_frame_count(replay.duration_ms, studio.video_fps),
            format_time(replay.duration_ms)
        ));
    }
    });
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
            start_video_export(
                studio,
                capture_active,
                grids,
                windows,
                player_movement_history,
                ship_trajectory_history,
            );
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
            start_video_export(
                studio,
                capture_active,
                grids,
                windows,
                player_movement_history,
                ship_trajectory_history,
            );
            return;
        }
        if matches!(
            studio.mode,
            ReplayMode::Playing | ReplayMode::Paused
        ) {
            stop_playback(studio, grids);
        }
        build_from_history(
            studio,
            manager,
            voice_favorites,
            player_movement_history,
            ship_trajectory_history,
            camera,
            standees,
            grids,
        );
        let director_result = studio
            .replay
            .as_ref()
            .ok_or_else(|| "无法从当前聊天生成回放".to_owned())
            .and_then(|replay| {
                queue_replay_director(
                    replay,
                    deepseek_sender,
                    deepseek_manager,
                    &studio.deepseek_custom_prompt,
                    standees,
                )
            });
        match director_result {
            Ok(source) => {
                studio.director_response_hash = None;
                studio.director_request_pending = true;
                studio.auto_export_after_director = true;
                studio.status = match source {
                    DirectorPlanSource::Saved => {
                        "已找到完全匹配的已保存 DeepSeek 导演方案；正在应用后自动导出，未请求 API"
                            .to_owned()
                    },
                    DirectorPlanSource::Api => {
                        let _ = deepseek_manager.persist();
                        "已发送 DeepSeek 导演请求；收到并应用有效方案后自动开始导出".to_owned()
                    },
                };
            },
            Err(err) => {
                studio.director_request_pending = false;
                studio.auto_export_after_director = false;
                studio.status = format!("DeepSeek 导演请求失败：{err}");
            },
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
                match import_replay(&studio.project_import_path, Some(manager)) {
                    Ok(replay) => {
                        stop_playback(studio, grids);
                        studio.playback_ms = 0;
                        studio.audience = replay.audience.clone();
                        studio.camera_distance_scale = replay.camera_distance_scale;
                        studio.camera_yaw_degrees = replay.camera_yaw_degrees;
                        studio.camera_transition_curve = replay.camera_transition_curve;
                        studio.player_movement_curve = replay.player_movement_curve;
                        studio.status = format!("已载入项目：{}", replay.title);
                        studio.replay = Some(replay);
                    },
                    Err(err) => studio.status = format!("项目载入失败：{err}"),
                }
            }
        },
    );
    });
    if !studio.status.is_empty() {
        ui.small(studio.status.as_str());
    }
}

fn can_start_director_export(studio: &ReplayStudio, has_active_campaign: bool) -> bool {
    studio.mode != ReplayMode::Recording
        && !replay_has_live_take(studio)
        && studio.video_render.is_none()
        && studio.video_encoding.is_none()
        && !studio.director_request_pending
        && ((studio.replay.is_some() && studio.director_response_hash.is_some())
            || has_active_campaign)
}

fn speech_settings_window(
    ctx: &egui::Context,
    studio: &mut ReplayStudio,
    voice_favorites: &mut Persistent<ReplayVoiceFavorites>,
) {
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
    let mut pending_status = None;

    egui::Window::new("角色语音设置")
        .id(egui::Id::new("replay-speaker-voice-settings"))
        .open(&mut open)
        .default_width(520.0)
        .max_width(620.0)
        .show(ctx, |ui| {
            ui.label(format!(
                "官方 EmotiVoice 音色目录共 {} 个音色。同一玩家始终使用同一说话人 ID；收藏会保存到本机，并在从现有聊天生成时按 QQ 角色自动应用。",
                emotivoice_voice_profiles().len()
            ));
            if installed_speakers.is_empty() {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "未找到已安装的 EmotiVoice 运行环境；仍可配置音色，安装后即可试听和导出。",
                );
            }
            ui.horizontal(|ui| {
                ui.label("搜索音色");
                ui.text_edit_singleline(&mut studio.voice_search)
                    .on_hover_text("可按音色 ID、姓名、性别或说明筛选；留空显示全部音色");
                if !studio.voice_search.is_empty() && ui.button("清空").clicked() {
                    studio.voice_search.clear();
                }
            });
            ui.collapsing(
                format!("收藏管理（{}）", voice_favorites.favorites.len()),
                |ui| {
                    if voice_favorites.favorites.is_empty() {
                        ui.label("尚未收藏角色语音设置。");
                    }
                    let mut delete_index = None;
                    for (index, favorite) in voice_favorites.favorites.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} · {} · {} · 语速 {:+}% · 音量 {:.0}%",
                                favorite.name,
                                emotivoice_speaker_label(
                                    favorite.settings.voice_name.as_deref().unwrap_or_default()
                                ),
                                resolved_emotivoice_emotion(
                                    favorite.settings.emotion.as_deref()
                                ),
                                favorite.settings.speech_rate,
                                favorite.settings.volume * 100.0,
                            ));
                            if ui.small_button("删除").clicked() {
                                delete_index = Some(index);
                            }
                        });
                    }
                    if let Some(index) = delete_index {
                        voice_favorites.favorites.remove(index);
                        pending_status = Some(match voice_favorites.persist() {
                            Ok(()) => "语音收藏已删除并保存".to_owned(),
                            Err(err) => format!("删除了语音收藏，但保存失败：{err}"),
                        });
                    }
                },
            );
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
                                let query = studio.voice_search.trim().to_lowercase();
                                ui.strong("男声");
                                for profile in emotivoice_voice_profiles() {
                                    if profile.gender != "M"
                                        || !emotivoice_voice_matches(profile, &query)
                                    {
                                        continue;
                                    }
                                    ui.selectable_value(
                                        &mut selected_speaker,
                                        profile.id.to_owned(),
                                        emotivoice_profile_label(profile),
                                    )
                                    .on_hover_text(profile.description);
                                }
                                ui.separator();
                                ui.strong("女声");
                                for profile in emotivoice_voice_profiles() {
                                    if profile.gender != "F"
                                        || !emotivoice_voice_matches(profile, &query)
                                    {
                                        continue;
                                    }
                                    ui.selectable_value(
                                        &mut selected_speaker,
                                        profile.id.to_owned(),
                                        emotivoice_profile_label(profile),
                                    )
                                    .on_hover_text(profile.description);
                                }
                            });
                        if ui
                            .add_enabled(
                                !installed_speakers.is_empty(),
                                egui::Button::new("随机音色"),
                            )
                            .on_hover_text("从固定 EmotiVoice 中文角色音色中随机选择")
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
                        ui.horizontal(|ui| {
                            ui.label("情绪");
                            egui::ComboBox::from_id_salt(("replay-emotivoice-emotion", sender_id))
                                .selected_text(resolved_emotivoice_emotion(
                                    settings.emotion.as_deref(),
                                ))
                                .show_ui(ui, |ui| {
                                    for emotion in EMOTIVOICE_EMOTIONS {
                                        settings_changed |= ui
                                            .selectable_value(
                                                settings
                                                    .emotion
                                                    .get_or_insert_with(|| "普通".to_owned()),
                                                emotion.to_owned(),
                                                emotion,
                                            )
                                            .changed();
                                    }
                                });
                        });
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
                        ui.horizontal(|ui| {
                            ui.menu_button("应用收藏", |ui| {
                                if voice_favorites.favorites.is_empty() {
                                    ui.label("尚无收藏");
                                }
                                for favorite in &voice_favorites.favorites {
                                    if ui.button(favorite.name.as_str()).clicked() {
                                        *settings = favorite.settings.clone();
                                        settings_changed = true;
                                        ui.close();
                                    }
                                }
                            });
                            let draft = studio
                                .voice_favorite_name_drafts
                                .entry(*sender_id)
                                .or_default();
                            ui.add(
                                egui::TextEdit::singleline(draft)
                                    .hint_text("收藏名称")
                                    .desired_width(180.0),
                            );
                            if ui.button("收藏当前设置").clicked() {
                                let favorite_name = if draft.trim().is_empty() {
                                    format!("{name} · {}", emotivoice_speaker_label(
                                        settings.voice_name.as_deref().unwrap_or_default()
                                    ))
                                } else {
                                    draft.trim().to_owned()
                                };
                                let favorite = ReplayVoiceFavorite {
                                    name: favorite_name.clone(),
                                    sender_id: Some(*sender_id),
                                    settings: settings.clone(),
                                };
                                if let Some(existing_index) = voice_favorites
                                    .favorites
                                    .iter()
                                    .position(|item| item.name.eq_ignore_ascii_case(&favorite_name))
                                {
                                    voice_favorites.favorites.remove(existing_index);
                                }
                                voice_favorites.favorites.push(favorite);
                                *draft = favorite_name;
                                pending_status = Some(match voice_favorites.persist() {
                                    Ok(()) => {
                                        "语音收藏已保存；从现有聊天生成时会自动应用".to_owned()
                                    },
                                    Err(err) => format!("已添加语音收藏，但保存失败：{err}"),
                                });
                            }
                        });
                        if ui.button("恢复该角色默认值").clicked() {
                            *settings = defaults;
                            settings_changed = true;
                        }
                    });
                }
            });
        });
    studio.speech_settings_open = open;
    if let Some(status) = pending_status {
        studio.status = status;
    } else if settings_changed {
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
        studio.camera_distance_scale,
        studio.camera_yaw_degrees,
        studio.camera_transition_curve,
        studio.player_movement_curve,
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
    studio.standee_sample_accumulator = 0.0;
    studio.player_movement_sample_accumulator = 0.0;
    studio.pending_terrain_changes.clear();
    studio.pending_ship_hull_changes.clear();
    studio.recorded_possession_user_id = None;
    studio.recorded_player_movement_index = None;
    studio.live_editing_enabled = false;
    studio.live_movement_punch_in = None;
    studio.live_ship_punch_in = None;
    studio.playback_ms = 0;
    studio.director_request_pending = false;
    studio.director_response_hash = None;
    studio.auto_export_after_director = false;
    studio.replay = Some(replay);
    studio.mode = ReplayMode::Recording;
    studio.status = if studio.record_camera_enabled {
        "开始录制场景、可见消息和 DM 自由镜头；飞船轨迹与地形变化由常态化记录自动并入".to_owned()
    } else {
        "开始录制场景和可见消息；飞船轨迹与地形变化由常态化记录自动并入，DM 镜头采集保持关闭"
            .to_owned()
    };
}

fn stop_recording(
    studio: &mut ReplayStudio,
    ship_trajectory_history: &ReplayShipTrajectoryHistory,
) {
    if let Some(replay) = studio.replay.as_mut() {
        let cutoff_unix_ms = replay
            .ship_trajectory_history_cursor_unix_ms
            .max(replay.created_at_unix_ms);
        let imported_ships = compile_scene_dynamics_timeline(
            replay,
            ship_trajectory_history,
            cutoff_unix_ms,
            &studio.pending_terrain_changes,
            &studio.pending_ship_hull_changes,
        );
        replay.ship_trajectory_history_cursor_unix_ms = unix_time_ms();
        extend_replay_for_speech(replay);
        let dialogue_end = replay
            .dialogue
            .last()
            .map(|line| line.time_ms.saturating_add(line.duration_ms))
            .unwrap_or(5_000);
        replay.duration_ms = if studio.record_camera_enabled || !replay.player_movements.is_empty()
        {
            studio
                .record_elapsed_ms
                .max(dialogue_end)
                .max(replay.duration_ms)
        } else {
            dialogue_end.max(replay.duration_ms)
        };
        studio.status = if imported_ships > 0 {
            format!(
                "录制已停止，已把 {imported_ships} 艘飞船的常态化轨迹并入时间轴，可以预览或导出"
            )
        } else {
            "录制已停止，可以预览或导出".to_owned()
        };
        return;
    }
    studio.mode = ReplayMode::Idle;
    studio.playback_ms = 0;
    studio.status = "录制已停止，可以预览或导出".to_owned();
}

fn build_from_history(
    studio: &mut ReplayStudio,
    manager: &NapcatMessageManager,
    voice_favorites: &ReplayVoiceFavorites,
    player_movement_history: &ReplayPlayerMovementHistory,
    ship_trajectory_history: &ReplayShipTrajectoryHistory,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
) {
    let Some(campaign_id) = manager.active_campaign_id() else {
        studio.status = "请先选择跑团组（战役）".to_owned();
        return;
    };
    let previous_settings = studio
        .replay
        .as_ref()
        .map(ReplayGenerationSettings::from_replay);
    let retained_previous_settings = previous_settings.is_some();
    let scene = grids
        .single_mut()
        .map(|grid| capture_scene(&grid))
        .unwrap_or_default();
    let mut replay = new_replay(
        manager,
        campaign_id.clone(),
        studio.audience.clone(),
        scene,
        studio.camera_distance_scale,
        studio.camera_yaw_degrees,
        studio.camera_transition_curve,
        studio.player_movement_curve,
    );
    let speaker_positions = standee_positions(standees);
    let mut visible = manager
        .messages
        .iter()
        .flat_map(|(target, messages)| {
            messages
                .iter()
                .enumerate()
                .map(|(index, message)| {
                    (
                        manager.campaign_message_for_target(target, message),
                        manager
                            .replay_snapshots
                            .get(target)
                            .and_then(|snapshots| snapshots.get(index))
                            .and_then(Option::as_ref)
                            .copied(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .filter(|(message, _)| {
            replay_message_is_eligible(
                &studio.audience,
                &campaign_id,
                message,
                manager,
            )
        })
        .filter(|(message, _)| !message.text.trim().is_empty())
        .collect::<Vec<_>>();
    visible.sort_by_key(|(message, _)| message.time);
    let estimated_count = visible
        .iter()
        .filter(|(_, snapshot)| snapshot.is_none())
        .count();
    let mut timeline_ms: u64 = 350;
    for (message, snapshot) in &visible {
        let estimated_snapshot;
        let (snapshot, metadata_estimated) = if let Some(snapshot) = snapshot.as_ref() {
            (snapshot, false)
        } else {
            estimated_snapshot = estimated_replay_snapshot(message, manager, &speaker_positions);
            (&estimated_snapshot, true)
        };
        if let Some(dialogue) = dialogue_from_message(
            message,
            manager,
            timeline_ms,
            snapshot,
            metadata_estimated,
        ) {
            timeline_ms = dialogue
                .time_ms
                .saturating_add(dialogue.duration_ms)
                .saturating_add(HISTORY_DIALOGUE_GAP_MS);
            replay.dialogue.push(dialogue);
        }
    }
    deduplicate_broadcast_dialogue(&mut replay.dialogue, manager);
    assign_replay_line_ids(&mut replay.dialogue);
    let favorite_count = apply_favorite_voice_settings(&mut replay, voice_favorites);
    if let Some(settings) = previous_settings {
        settings.apply_to(&mut replay);
    }
    auto_group_replay_areas(&mut replay);
    rebuild_area_blocks(&mut replay);
    compile_scene_dynamics_timeline(
        &mut replay,
        ship_trajectory_history,
        0,
        &[],
        &[],
    );
    replay.ship_trajectory_history_cursor_unix_ms = unix_time_ms();
    extend_replay_for_speech(&mut replay);
    replay.player_movements = replay_player_movements_from_history(
        player_movement_history,
        &campaign_id,
        &replay.dialogue,
    );
    replay.player_movement_history_cursor_unix_ms = unix_time_ms();
    if let Some(movement_end) = replay
        .player_movements
        .iter()
        .filter_map(|movement| movement.keyframes.last())
        .map(|frame| frame.time_ms)
        .max()
    {
        replay.duration_ms = replay.duration_ms.max(movement_end);
    }
    if let Some(trajectory_end) = replay
        .ship_trajectories
        .iter()
        .filter_map(|trajectory| trajectory.keyframes.last())
        .map(|frame| frame.time_ms)
        .max()
    {
        replay.duration_ms = replay.duration_ms.max(trajectory_end);
    }
    if let Ok(transform) = camera.single() {
        let obstacles = ReplayCameraObstacles::from_scene(&replay.scene);
        replay.camera = turn_based_camera_track(
            transform,
            &replay.dialogue,
            replay.duration_ms,
            &speaker_positions,
            replay.camera_distance_scale,
            replay.camera_yaw_degrees,
            &obstacles,
        );
    }
    studio.playback_ms = 0;
    studio.director_request_pending = false;
    studio.director_response_hash = None;
    studio.auto_export_after_director = false;
    let dialogue_count = replay.dialogue.len();
    let movement_frame_count = replay
        .player_movements
        .iter()
        .map(|movement| movement.keyframes.len())
        .sum::<usize>();
    studio.replay = Some(replay);
    let favorite_status = if favorite_count == 0 {
        String::new()
    } else {
        format!("；已自动应用 {favorite_count} 个角色的语音收藏")
    };
    let retained_settings_status = if retained_previous_settings {
        "；已保留当前回放的语速、台词停留和角色语音设置"
    } else {
        ""
    };
    studio.status = format!(
        "已生成 {dialogue_count} 句区域回放台词和 {movement_frame_count} 帧接管移动；其中 {estimated_count} 句旧消息使用当前立牌位置或原点估算，可由 DM 编辑{favorite_status}{retained_settings_status}"
    );
}

fn apply_favorite_voice_settings(
    replay: &mut ReplayFile,
    voice_favorites: &ReplayVoiceFavorites,
) -> usize {
    let speaker_ids = replay
        .dialogue
        .iter()
        .map(|line| line.sender_id)
        .collect::<HashSet<_>>();
    for favorite in &voice_favorites.favorites {
        let Some(sender_id) = favorite.sender_id.filter(|id| speaker_ids.contains(id)) else {
            continue;
        };
        replay
            .speaker_voice_settings
            .insert(sender_id, favorite.settings.clone());
    }
    replay.speaker_voice_settings.len()
}

fn replay_director_key(replay: &ReplayFile) -> String {
    let audience = match &replay.audience {
        ReplayAudience::Public => "public".to_owned(),
        ReplayAudience::Party(id) => format!("party:{id}"),
        ReplayAudience::Player(id) => format!("player:{id}"),
        ReplayAudience::All => "all".to_owned(),
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
    let message_count = replay
        .dialogue
        .iter()
        .filter(|line| replay_dialogue_is_playable(line))
        .count();
    manager
        .summaries
        .get(&replay_director_key(replay))?
        .blocks
        .iter()
        .find(|block| block.message_count == message_count)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectorPlanSource {
    Saved,
    Api,
}

struct ReplayDirectorRequest {
    summary_key: String,
    message_count: usize,
    text: String,
    custom_prompt: String,
    fingerprint: String,
}

fn director_fingerprint_key(summary_key: &str, message_count: usize) -> String {
    format!("{summary_key}:dialogue-count:{message_count}")
}

fn replay_edit_fingerprint_state(replay: &ReplayFile) -> Result<String, String> {
    json_to_string(&json!({
        "dialogue": &replay.dialogue,
        "blocks": &replay.area_blocks,
        "manual_dialogue_order": replay.manual_dialogue_order,
        "duration_ms": replay.duration_ms,
        "camera": &replay.camera,
        "camera_settings": (
            replay.camera_distance_scale,
            replay.camera_yaw_degrees,
            replay.camera_transition_curve,
        ),
        "player_movements": &replay.player_movements,
        "authored_player_movements": &replay.authored_player_movements,
        "ship_trajectories": &replay.ship_trajectories,
        "authored_ship_trajectories": &replay.authored_ship_trajectories,
        "standee_positions": &replay.standee_positions,
        "terrain_changes": &replay.terrain_changes,
        "ship_hull_changes": &replay.ship_hull_changes,
        "timing_settings": (
            replay.master_dialogue_duration,
            replay.master_speech_speed,
            replay.player_movement_curve,
            replay.ship_motion_speed,
            replay.dialogue_waits_for_ship_motion,
        ),
    }))
    .map_err(|err| err.to_string())
}

fn replay_director_request(
    replay: &ReplayFile,
    custom_prompt: &str,
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
) -> Result<ReplayDirectorRequest, String> {
    if !replay.dialogue.iter().any(replay_dialogue_is_playable) {
        return Err("回放中没有可整理的台词".to_owned());
    }
    let summary_key = replay_director_key(replay);
    let message_count = replay
        .dialogue
        .iter()
        .filter(|line| replay_dialogue_is_playable(line))
        .count();
    let visible_standees = standees
        .iter()
        .map(|(transform, standee)| (standee.user_id, transform.translation))
        .collect::<HashMap<_, _>>();
    let missing_lines = missing_director_dialogue(replay, &visible_standees);
    if !missing_lines.is_empty() {
        return Err(format!(
            "找不到说话者立牌：{}。请使用界面的快速修复，或从当前聊天重建回放",
            missing_lines
                .iter()
                .map(|(_, description)| description.as_str())
                .collect::<Vec<_>>()
                .join("；")
        ));
    }
    let dialogue = replay
        .dialogue
        .iter()
        .enumerate()
        .filter(|(_, line)| replay_dialogue_is_playable(line))
        .enumerate()
        .map(|(index, (dialogue_index, line))| {
            let focus_id = replay_dialogue_focus_id(
                &replay.dialogue,
                dialogue_index,
                &visible_standees,
            );
            serde_json::json!({
                "index": index,
                "speaker_id": focus_id.unwrap_or(line.sender_id).to_string(),
                "name": line.name,
                "role": line.role,
                "text": line.text.trim(),
                "has_character_model": focus_id.is_some(),
            })
        })
        .collect::<Vec<_>>();
    let text = serde_json::to_string(&serde_json::json!({ "dialogue": dialogue }))
        .map_err(|err| err.to_string())?;
    let custom_prompt = custom_prompt
        .chars()
        .take(DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS)
        .collect::<String>();
    let edit_state = replay_edit_fingerprint_state(replay)?;
    let fingerprint = blake3::hash(
        format!(
            "{DIRECTOR_CACHE_FINGERPRINT_VERSION}\n{summary_key}\n{custom_prompt}\n{text}\n{edit_state}"
        )
        .as_bytes(),
    )
    .to_hex()
    .to_string();
    Ok(ReplayDirectorRequest {
        summary_key,
        message_count,
        text,
        custom_prompt,
        fingerprint,
    })
}

fn director_request_fingerprint_matches(
    manager: &DeepseekManager,
    request: &ReplayDirectorRequest,
) -> bool {
    let fingerprint_key = director_fingerprint_key(
        &request.summary_key,
        request.message_count,
    );
    manager
        .director_request_fingerprints
        .get(&fingerprint_key)
        .map(String::as_str)
        == Some(request.fingerprint.as_str())
}

fn saved_director_block<'a>(
    replay: &ReplayFile,
    manager: &'a DeepseekManager,
    request: &ReplayDirectorRequest,
) -> Option<&'a DeepseekSummaryBlock> {
    if !director_request_fingerprint_matches(manager, request) {
        return None;
    }
    replay_summary_block(replay, manager)
        .filter(|block| !block.pending && block.error.is_none() && !block.latest.trim().is_empty())
}

fn queue_replay_director(
    replay: &ReplayFile,
    sender: Option<&DeepseekIOSender>,
    manager: &mut DeepseekManager,
    custom_prompt: &str,
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
) -> Result<DirectorPlanSource, String> {
    let director_request = replay_director_request(replay, custom_prompt, standees)?;
    if saved_director_block(replay, manager, &director_request).is_some() {
        return Ok(DirectorPlanSource::Saved);
    }
    if let Some(block) = replay_summary_block(replay, manager) {
        if block.pending {
            return Err("这版台词正在整理，请等待当前请求完成".to_owned());
        }
    }
    let sender = sender.ok_or_else(|| "DeepSeek 连接尚未就绪，请稍后重试".to_owned())?;
    let request = serde_json::to_string(&DeepseekRequest::Director {
        target_id: director_request.summary_key.clone(),
        message_count: director_request.message_count,
        text: director_request.text,
        custom_prompt: director_request.custom_prompt,
    })
    .map(Message::text)
    .map_err(|err| err.to_string())?;
    sender.0.try_send(request).map_err(|err| err.to_string())?;
    manager.director_request_fingerprints.insert(
        director_fingerprint_key(
            &director_request.summary_key,
            director_request.message_count,
        ),
        director_request.fingerprint,
    );
    manager
        .summaries
        .entry(director_request.summary_key)
        .or_default()
        .upsert_block(DeepseekSummaryBlock {
            latest: String::new(),
            message_count: director_request.message_count,
            pending: true,
            error: None,
        });
    Ok(DirectorPlanSource::Api)
}

fn apply_ready_director_plan(
    studio: &mut ReplayStudio,
    manager: &DeepseekManager,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
) -> Result<bool, String> {
    if !studio.director_request_pending || replay_has_live_take(studio) {
        return Ok(false);
    }
    let Some(replay) = studio.replay.as_ref() else {
        studio.director_request_pending = false;
        studio.auto_export_after_director = false;
        return Ok(false);
    };
    let Some(block) = replay_summary_block(replay, manager) else {
        studio.director_request_pending = false;
        studio.auto_export_after_director = false;
        studio.director_response_hash = None;
        return Err("回放台词已变化，请重新生成导演方案".to_owned());
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
    // The response is addressed by dialogue count, which does not identify the
    // edited content. Validate against the fingerprint saved with the request
    // before allowing an asynchronous response to replace the DM's latest work.
    let request_matches = replay_director_request(
        replay,
        &studio.deepseek_custom_prompt,
        standees,
    )
    .is_ok_and(|request| director_request_fingerprint_matches(manager, &request));
    if !request_matches {
        studio.director_request_pending = false;
        studio.auto_export_after_director = false;
        studio.director_response_hash = None;
        return Err("回放内容、轨迹或导演要求已变化，请重新生成导演方案".to_owned());
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
    let playable_count = replay
        .dialogue
        .iter()
        .filter(|line| replay_dialogue_is_playable(line))
        .count();
    if plan.dialogue.len() != playable_count {
        studio.auto_export_after_director = false;
        return Err(format!(
            "方案包含 {} 句，但回放需要 {} 句",
            plan.dialogue.len(),
            playable_count
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
    for (line, cue) in replay
        .dialogue
        .iter_mut()
        .filter(|line| replay_dialogue_is_playable(line))
        .zip(&cues)
    {
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
        if !line.duration_locked {
            line.duration_ms = scaled_dialogue_duration_ms(text, replay.master_dialogue_duration);
        }
    }
    recompile_edited_replay_dialogue(replay);
    extend_replay_for_speech(replay);

    let base = camera
        .single()
        .ok()
        .cloned()
        .or_else(|| replay.camera.first().map(frame_transform))
        .ok_or_else(|| "找不到可用的导演基础镜头".to_owned())?;
    let speaker_positions = standee_positions(standees);
    let obstacles = ReplayCameraObstacles::from_scene(&replay.scene);
    let playable = replay
        .dialogue
        .iter()
        .filter(|line| line.included && line.time_ms != u64::MAX)
        .cloned()
        .collect::<Vec<_>>();
    replay.camera = director_camera_track(
        &base,
        &playable,
        &cues,
        replay.duration_ms,
        &speaker_positions,
        replay.camera_distance_scale,
        replay.camera_yaw_degrees,
        &obstacles,
    );
    studio.playback_ms = 0;
    studio.status = format!(
        "已应用 DeepSeek 导演方案：{} 句润色台词、{} 个镜头帧",
        playable.len(),
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
    camera_distance_scale: f32,
    camera_yaw_degrees: f32,
    camera_transition_curve: f32,
    player_movement_curve: f32,
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
        camera_distance_scale: normalized_directed_camera_distance_scale(camera_distance_scale),
        camera_yaw_degrees: normalized_directed_camera_yaw_degrees(camera_yaw_degrees),
        camera_transition_curve: normalized_camera_transition_curve(camera_transition_curve),
        player_movement_curve: normalized_player_movement_curve(player_movement_curve),
        player_movements: Vec::new(),
        authored_player_movements: BTreeSet::new(),
        authored_ship_trajectories: BTreeSet::new(),
        player_movement_history_cursor_unix_ms: unix_time_ms(),
        ship_trajectories: Vec::new(),
        standee_positions: Vec::new(),
        ship_motion_speed: default_ship_motion_speed(),
        dialogue_waits_for_ship_motion: default_dialogue_waits_for_ship_motion(),
        terrain_changes: Vec::new(),
        ship_hull_changes: Vec::new(),
        ship_trajectory_history_cursor_unix_ms: unix_time_ms(),
        dialogue: Vec::new(),
        area_blocks: Vec::new(),
        manual_dialogue_order: false,
        area_radius_cells: default_area_radius_cells(),
        master_speech_speed: default_master_speech_speed(),
        master_dialogue_duration: default_master_dialogue_duration(),
        speaker_voice_settings: HashMap::new(),
    }
}

fn prepare_replay_movement_for_playback(
    studio: &mut ReplayStudio,
    player_movement_history: &ReplayPlayerMovementHistory,
    ship_trajectory_history: &ReplayShipTrajectoryHistory,
) {
    if let Some(replay) = studio.replay.as_mut() {
        append_new_player_movements_from_history(replay, player_movement_history);
        append_new_ship_trajectories_from_history(replay, ship_trajectory_history);
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
    studio.playback_ms = 0;
    studio.speech_wait_cue = None;
    studio.speech_wait_elapsed_seconds = 0.0;
    studio.speech_bypass_cues.clear();
    studio.live_editing_enabled = false;
    studio.live_movement_punch_in = None;
    studio.live_ship_punch_in = None;
    studio.mode = ReplayMode::Playing;
    studio.status = REPLAY_PLAYING_STATUS.to_owned();
}

fn replay_has_completed(studio: &ReplayStudio) -> bool {
    studio.mode == ReplayMode::Paused
        && !replay_has_live_take(studio)
        && studio
            .replay
            .as_ref()
            .is_some_and(|replay| studio.playback_ms >= replay.duration_ms)
}

fn append_dm_replay_dialogue(replay: &mut ReplayFile) -> u64 {
    let line_id = replay
        .dialogue
        .iter()
        .map(|line| line.line_id)
        .max()
        .unwrap_or_default()
        .saturating_add(1);
    let source_time = replay
        .dialogue
        .iter()
        .map(|line| line.source_time)
        .max()
        .unwrap_or_else(|| unix_time_ms() / 1_000)
        .saturating_add(1);
    let last_block_line = replay
        .area_blocks
        .last()
        .and_then(|block| block.line_ids.last())
        .and_then(|last_id| replay.dialogue.iter().find(|line| line.line_id == *last_id))
        .or_else(|| replay.dialogue.last());
    let (camera_focus_id, turn_index, position_cells, area) = last_block_line
        .map(|line| {
            (
                (line.side == DialogueSide::Right)
                    .then_some(line.sender_id)
                    .or(line.camera_focus_id),
                line.turn_index,
                line.position_cells,
                line.area.clone(),
            )
        })
        .unwrap_or((None, 0, [0; 3], "区域 1".to_owned()));
    let visibility = match &replay.audience {
        ReplayAudience::Public => Visibility::Public,
        ReplayAudience::Party(id) => Visibility::Party(id.clone()),
        ReplayAudience::Player(id) => Visibility::Player(*id),
        ReplayAudience::All | ReplayAudience::Gm => Visibility::Gm,
    };
    replay.dialogue.push(ReplayDialogue {
        time_ms: u64::MAX,
        duration_ms: scaled_dialogue_duration_ms("", replay.master_dialogue_duration),
        duration_locked: false,
        sender_id: 0,
        camera_focus_id,
        name: "DM".to_owned(),
        role: "GM".to_owned(),
        text: String::new(),
        speech_text: None,
        speech_enabled: true,
        speech_rate: default_line_speech_rate(),
        speech_volume: default_line_speech_volume(),
        avatar: String::new(),
        avatar_data_url: None,
        visibility,
        side: DialogueSide::Left,
        line_id,
        source_time,
        turn_index,
        position_cells,
        area: area.clone(),
        included: true,
        snapshot_recorded: true,
        metadata_estimated: false,
        forwarded: false,
    });

    if let Some(block) = replay.area_blocks.last_mut() {
        block.line_ids.push(line_id);
    } else {
        replay.area_blocks.push(ReplayAreaBlock {
            id: 1,
            area,
            line_ids: vec![line_id],
        });
    }
    line_id
}

fn delete_replay_dialogue(replay: &mut ReplayFile, line_id: u64) -> bool {
    let previous_len = replay.dialogue.len();
    replay.dialogue.retain(|line| line.line_id != line_id);
    if replay.dialogue.len() == previous_len {
        return false;
    }
    for block in &mut replay.area_blocks {
        block.line_ids.retain(|candidate| *candidate != line_id);
    }
    replay
        .area_blocks
        .retain(|block| !block.line_ids.is_empty());
    true
}

fn move_replay_dialogue(
    replay: &mut ReplayFile,
    dragged_line_id: u64,
    target_line_id: u64,
    insert_after: bool,
) -> bool {
    if dragged_line_id == target_line_id {
        return false;
    }
    let Some(dragged_index) = replay
        .dialogue
        .iter()
        .position(|line| line.line_id == dragged_line_id)
    else {
        return false;
    };
    if !replay
        .dialogue
        .iter()
        .any(|line| line.line_id == target_line_id)
    {
        return false;
    }

    let target_block_id = replay
        .area_blocks
        .iter()
        .find(|block| block.line_ids.contains(&target_line_id))
        .map(|block| block.id);
    for block in &mut replay.area_blocks {
        block
            .line_ids
            .retain(|candidate| *candidate != dragged_line_id);
    }
    replay
        .area_blocks
        .retain(|block| !block.line_ids.is_empty());
    if let Some(target_block_id) = target_block_id {
        if let Some(block) = replay
            .area_blocks
            .iter_mut()
            .find(|block| block.id == target_block_id)
        {
            if let Some(target_index) = block
                .line_ids
                .iter()
                .position(|candidate| *candidate == target_line_id)
            {
                let insertion_index = target_index + usize::from(insert_after);
                block.line_ids.insert(insertion_index, dragged_line_id);
                if let Some(line) = replay
                    .dialogue
                    .iter_mut()
                    .find(|line| line.line_id == dragged_line_id)
                {
                    line.area = block.area.clone();
                }
            }
        }
    }

    let dragged = replay.dialogue.remove(dragged_index);
    let target_index = replay
        .dialogue
        .iter()
        .position(|line| line.line_id == target_line_id)
        .expect("target line was checked before removal");
    let insertion_index = target_index + usize::from(insert_after);
    replay.dialogue.insert(insertion_index, dragged);
    replay.manual_dialogue_order = true;
    true
}

fn set_all_replay_dialogue_included(replay: &mut ReplayFile, included: bool) -> bool {
    let mut changed = false;
    for line in &mut replay.dialogue {
        changed |= line.included != included;
        line.included = included;
    }
    changed
}

fn reassign_replay_dialogue_to_block(
    replay: &mut ReplayFile,
    line_id: u64,
    target_block_id: u64,
) -> bool {
    if !replay.dialogue.iter().any(|line| line.line_id == line_id) {
        return false;
    }
    let Some(target_area) = replay
        .area_blocks
        .iter()
        .find(|block| block.id == target_block_id)
        .map(|block| block.area.clone())
    else {
        return false;
    };
    if replay
        .area_blocks
        .iter()
        .find(|block| block.line_ids.contains(&line_id))
        .is_some_and(|block| block.id == target_block_id)
    {
        return false;
    }

    for block in &mut replay.area_blocks {
        block.line_ids.retain(|candidate| *candidate != line_id);
    }
    if let Some(block) = replay
        .area_blocks
        .iter_mut()
        .find(|block| block.id == target_block_id)
    {
        block.line_ids.push(line_id);
    }
    replay
        .area_blocks
        .retain(|block| !block.line_ids.is_empty());
    if let Some(line) = replay
        .dialogue
        .iter_mut()
        .find(|line| line.line_id == line_id)
    {
        line.area = target_area;
    }
    true
}

fn apply_replay_dialogue_area_edit(replay: &mut ReplayFile, line_id: u64, area: String) -> bool {
    let Some(line_index) = replay
        .dialogue
        .iter()
        .position(|line| line.line_id == line_id)
    else {
        return false;
    };
    let source_index = replay
        .area_blocks
        .iter()
        .position(|block| block.line_ids.contains(&line_id));
    if let Some(source_index) = source_index {
        if replay.area_blocks[source_index].area == area {
            replay.dialogue[line_index].area = area;
            return true;
        }
        if replay.area_blocks[source_index].line_ids.len() == 1 {
            replay.area_blocks[source_index].area = area.clone();
            replay.dialogue[line_index].area = area;
            return true;
        }
        replay.area_blocks[source_index]
            .line_ids
            .retain(|candidate| *candidate != line_id);
    }

    if let Some(target) = replay
        .area_blocks
        .iter_mut()
        .find(|block| block.area == area)
    {
        target.line_ids.push(line_id);
    } else {
        let next_id = replay
            .area_blocks
            .iter()
            .map(|block| block.id)
            .max()
            .unwrap_or_default()
            .saturating_add(1);
        let insert_at = source_index
            .map(|index| index.saturating_add(1).min(replay.area_blocks.len()))
            .unwrap_or(replay.area_blocks.len());
        replay.area_blocks.insert(insert_at, ReplayAreaBlock {
            id: next_id,
            area: area.clone(),
            line_ids: vec![line_id],
        });
    }
    replay.dialogue[line_index].area = area;
    true
}

fn replay_live_edit_panel(
    ui: &mut egui::Ui,
    studio: &mut ReplayStudio,
    live_edits: &mut ReplayLiveEditParams,
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
) {
    if studio.replay.is_none()
        || studio.mode == ReplayMode::Recording
        || studio.video_render.is_some()
    {
        return;
    }

    let playback_active = matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    );
    let playhead_ms = studio.playback_ms;
    let active_sender_id = studio.replay.as_ref().and_then(|replay| {
        active_dialogue_index(&replay.dialogue, playhead_ms)
            .map(|index| replay.dialogue[index].sender_id)
    });
    let speaker_names = studio
        .replay
        .as_ref()
        .map(|replay| {
            replay
                .dialogue
                .iter()
                .map(|line| (line.sender_id, line.name.clone()))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let mut standee_choices = standees
        .iter()
        .map(|(transform, standee)| {
            (
                standee.user_id,
                speaker_names
                    .get(&standee.user_id)
                    .cloned()
                    .unwrap_or_else(|| format!("角色 {}", standee.user_id)),
                transform.translation,
            )
        })
        .collect::<Vec<_>>();
    standee_choices.sort_by_key(|(user_id, ..)| *user_id);
    if studio.live_movement_user_id.is_none_or(|selected| {
        !standee_choices
            .iter()
            .any(|(user_id, ..)| *user_id == selected)
    }) {
        studio.live_movement_user_id = live_edits
            .possession
            .active_user_id
            .filter(|selected| {
                standee_choices
                    .iter()
                    .any(|(user_id, ..)| user_id == selected)
            })
            .or(active_sender_id.filter(|selected| {
                standee_choices
                    .iter()
                    .any(|(user_id, ..)| user_id == selected)
            }))
            .or_else(|| standee_choices.first().map(|(user_id, ..)| *user_id));
    }

    let mut ship_choices = live_edits
        .ships
        .iter()
        .map(|(ship, transform)| {
            (
                ship.id.clone(),
                ship.name.clone(),
                *transform,
            )
        })
        .collect::<Vec<_>>();
    ship_choices.sort_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));
    if studio
        .live_ship_id
        .as_ref()
        .is_none_or(|selected| !ship_choices.iter().any(|(ship_id, ..)| ship_id == selected))
    {
        studio.live_ship_id = live_edits
            .ship_control
            .driving_ship_id
            .as_ref()
            .filter(|selected| {
                ship_choices
                    .iter()
                    .any(|(ship_id, ..)| ship_id == *selected)
            })
            .cloned()
            .or_else(|| ship_choices.first().map(|(ship_id, ..)| ship_id.clone()));
    }

    ui.separator();
    ui.group(|ui| {
        ui.heading("播放头现场编辑");
        ui.small("播放或暂停时直接改当前一句、覆录移动，并在当前时间插入或阻止爆炸；不会跳回时间轴开头。");
        let take_active =
            studio.live_movement_punch_in.is_some() || studio.live_ship_punch_in.is_some();
        let live_toggle = ui.add_enabled(
            playback_active && !take_active,
            egui::Checkbox::new(&mut studio.live_editing_enabled, "解锁场景与接管工具"),
        );
        live_toggle.on_hover_text("解锁后可在回放画面里使用现有体素、爆炸、角色接管和飞船接管工具。项目编辑不会保存成战役现场。");
        if !playback_active {
            ui.small("先点击“播放”，再在这里现场修改。");
        } else if studio.live_editing_enabled {
            ui.colored_label(
                egui::Color32::from_rgb(80, 190, 120),
                format!("● 现场编辑已开启 · 播放头 {}", format_time(playhead_ms)),
            );
        }

        if take_active {
            ui.colored_label(
                egui::Color32::from_rgb(225, 90, 70),
                "● 正在覆录；超过片尾会自动延长回放。保存或取消后可继续编辑时间轴。",
            );
            ui.horizontal(|ui| {
                if ui.button("保存本次录制").clicked() {
                    if let Some(take) = studio.live_movement_punch_in.as_ref() {
                        let position = standee_choices.iter()
                            .find(|(user_id, ..)| *user_id == take.user_id)
                            .map(|(_, _, position)| *position);
                        finish_live_movement_take(studio, position);
                    } else if let Some(take) = studio.live_ship_punch_in.as_ref() {
                        let transform = ship_choices.iter()
                            .find(|(ship_id, ..)| *ship_id == take.ship_id)
                            .map(|(_, _, transform)| *transform);
                        finish_live_ship_take(studio, transform);
                    }
                }
                if ui.button("取消本次录制").clicked() {
                    cancel_live_replay_take(studio);
                }
            });
            return;
        }

        ui.collapsing("当前台词：文字、精确时长与声音", |ui| {
            let dialogue_index = studio.replay.as_ref().and_then(|replay| {
                active_dialogue_index(&replay.dialogue, playhead_ms)
                    .or_else(|| {
                        replay.dialogue.iter().rposition(|line| {
                            replay_dialogue_is_playable(line) && line.time_ms <= playhead_ms
                        })
                    })
                    .or_else(|| replay.dialogue.iter().position(replay_dialogue_is_playable))
            });
            let Some(dialogue_index) = dialogue_index else {
                ui.small("当前回放没有可编辑台词。");
                return;
            };
            let line = studio.replay.as_ref().expect("checked above").dialogue[dialogue_index]
                .clone();
            let mut text = line.text.clone();
            let mut speech_text = line
                .speech_text
                .clone()
                .unwrap_or_else(|| line.text.clone());
            let original_speech_text = line.speech_text.clone();
            let mut duration_seconds = line.duration_ms as f64 / 1_000.0;
            let mut duration_locked = line.duration_locked;
            let mut speech_enabled = line.speech_enabled;
            let mut speech_rate = normalized_line_speech_rate(line.speech_rate);
            let mut speech_volume = normalized_line_speech_volume(line.speech_volume);
            let line_start_ms = line.time_ms;
            let line_name = line.name.clone();
            ui.horizontal(|ui| {
                ui.strong(format!("{} · {}", line_name, format_time(line_start_ms)));
                if playhead_ms < line_start_ms && ui.button("跳到此句").clicked() {
                    studio.playback_ms = line_start_ms;
                }
            });
            let text_changed = ui
                .add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_rows(2)
                        .desired_width(ui.available_width()),
                )
                .changed();

            let mut requested_duration_ms = None;
            ui.horizontal(|ui| {
                ui.label("精确显示");
                if ui
                    .add(
                        egui::DragValue::new(&mut duration_seconds)
                            .speed(0.05)
                            .range(0.1..=120.0)
                            .fixed_decimals(2)
                            .suffix(" 秒"),
                    )
                    .changed()
                {
                    requested_duration_ms = Some((duration_seconds * 1_000.0).round() as u64);
                }
                ui.checkbox(&mut duration_locked, "锁定").on_hover_text(
                    "锁定后，自动语音适配不会再次拉长这一句。声音超出时会在句尾截断。",
                );
                if ui.button("按文字估时").clicked() {
                    let master_duration = studio
                        .replay
                        .as_ref()
                        .map(|replay| replay.master_dialogue_duration)
                        .unwrap_or(1.0);
                    requested_duration_ms = Some(scaled_dialogue_duration_ms(&text, master_duration));
                    duration_locked = true;
                }
            });
            ui.checkbox(&mut speech_enabled, "本句播放角色语音");
            ui.horizontal(|ui| {
                ui.add(
                    egui::Slider::new(&mut speech_rate, 0.5..=2.0)
                        .text("本句语速×")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut speech_volume, 0.0..=2.0)
                        .text("音量×")
                        .fixed_decimals(2),
                );
            });
            ui.label("语音读法（可与字幕不同）");
            let speech_text_changed = ui
                .add(
                    egui::TextEdit::multiline(&mut speech_text)
                        .desired_rows(2)
                        .desired_width(ui.available_width()),
                )
                .changed();

            let voice_or_text_changed = text_changed
                || speech_text_changed
                || duration_locked != line.duration_locked
                || speech_enabled != line.speech_enabled
                || (speech_rate - line.speech_rate).abs() > f32::EPSILON
                || (speech_volume - line.speech_volume).abs() > f32::EPSILON;
            if voice_or_text_changed || requested_duration_ms.is_some() {
                let playback_ms = studio.playback_ms;
                let mut updated_playback_ms = None;
                if let Some(replay) = studio.replay.as_mut() {
                    let line = &mut replay.dialogue[dialogue_index];
                    line.text = text;
                    line.duration_locked = duration_locked;
                    line.speech_enabled = speech_enabled;
                    line.speech_rate = normalized_line_speech_rate(speech_rate);
                    line.speech_volume = normalized_line_speech_volume(speech_volume);
                    if speech_text_changed {
                        line.speech_text = (!speech_text.trim().is_empty())
                            .then(|| speech_text.trim().to_owned());
                    } else if text_changed {
                        // Do not leave an old pronunciation script speaking the
                        // pre-edit words. Falling back to the new subtitle is safe.
                        line.speech_text = None;
                    } else {
                        line.speech_text = original_speech_text;
                    }
                    if let Some(duration_ms) = requested_duration_ms {
                        updated_playback_ms = set_exact_dialogue_duration(
                            replay,
                            dialogue_index,
                            duration_ms,
                            playback_ms,
                        );
                    }
                }
                if let Some(playback_ms) = updated_playback_ms {
                    studio.playback_ms = playback_ms;
                    studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
                }
                studio.speech_wait_cue = None;
                studio.speech_wait_elapsed_seconds = 0.0;
                studio.speech_bypass_cues.clear();
                studio.status = format!("已在播放头更新 {} 的台词、时长或声音", line_name);
            }
        });

        ui.collapsing("角色移动：停止或覆录", |ui| {
            if standee_choices.is_empty() {
                ui.small("场景中没有可编辑的角色立牌。");
                return;
            }
            let selected_label = studio
                .live_movement_user_id
                .and_then(|selected| {
                    standee_choices
                        .iter()
                        .find(|(user_id, _, _)| *user_id == selected)
                        .map(|(user_id, name, _)| format!("{name} ({user_id})"))
                })
                .unwrap_or_else(|| "选择角色".to_owned());
            egui::ComboBox::from_id_salt("replay-live-movement-user")
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    for (user_id, name, _) in &standee_choices {
                        ui.selectable_value(
                            &mut studio.live_movement_user_id,
                            Some(*user_id),
                            format!("{name} ({user_id})"),
                        );
                    }
                });
            let Some(user_id) = studio.live_movement_user_id else { return };
            let current_position = standee_choices
                .iter()
                .find(|(candidate, _, _)| *candidate == user_id)
                .map(|(_, _, position)| *position);
            if let Some(position) = current_position {
                if let Some(position) = replay_position_frame_editor(
                    ui,
                    position,
                    playback_active && !take_active,
                ) {
                    if let Some(replay) = studio.replay.as_mut() {
                        replace_replay_movement_segment(
                            replay,
                            user_id,
                            playhead_ms,
                            playhead_ms,
                            &[ReplayStandeePosition {
                                time_ms: playhead_ms,
                                user_id,
                                position: position.to_array(),
                            }],
                        );
                        studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
                        studio.status = format!("已保存角色 {user_id} 在 {} 的位置帧", format_time(playhead_ms));
                    }
                }
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        playback_active && current_position.is_some(),
                        egui::Button::new("从此停止移动"),
                    )
                    .clicked()
                {
                    if let (Some(replay), Some(position)) =
                        (studio.replay.as_mut(), current_position)
                    {
                        stop_replay_movement_at(replay, user_id, playhead_ms, position);
                        studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
                        studio.status = format!(
                            "已让角色 {user_id} 从 {} 起停在当前位置",
                            format_time(playhead_ms),
                        );
                    }
                }
                let possessed = live_edits.possession.active_user_id == Some(user_id);
                if ui
                    .add_enabled(
                        playback_active
                            && studio.live_editing_enabled
                            && possessed
                            && studio.live_ship_punch_in.is_none()
                            && current_position.is_some(),
                        egui::Button::new("从此录制新移动"),
                    )
                    .clicked()
                {
                    begin_live_movement_take(
                        studio,
                        user_id,
                        current_position.expect("button required a position"),
                    );
                }
            });
            if live_edits.possession.active_user_id != Some(user_id) {
                ui.small("要覆录：先开启现场编辑，再用画面中的接管工具控制这个角色。无需接管也可直接“从此停止移动”。");
            }
        });

        ui.collapsing("飞船移动：停止或覆录", |ui| {
            if ship_choices.is_empty() {
                ui.small("场景中没有可编辑飞船。");
                return;
            }
            let selected_label = studio
                .live_ship_id
                .as_ref()
                .and_then(|selected| {
                    ship_choices
                        .iter()
                        .find(|(ship_id, _, _)| ship_id == selected)
                        .map(|(_, name, _)| name.clone())
                })
                .unwrap_or_else(|| "选择飞船".to_owned());
            egui::ComboBox::from_id_salt("replay-live-ship")
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    for (ship_id, name, _) in &ship_choices {
                        ui.selectable_value(
                            &mut studio.live_ship_id,
                            Some(ship_id.clone()),
                            name,
                        );
                    }
                });
            let Some(ship_id) = studio.live_ship_id.clone() else { return };
            let selected_ship = ship_choices
                .iter()
                .find(|(candidate, _, _)| candidate == &ship_id)
                .cloned();
            if let Some((_, ship_name, transform)) = selected_ship {
                if let Some(position) = replay_position_frame_editor(
                    ui,
                    transform.translation,
                    playback_active && !take_active,
                ) {
                    if let Some(replay) = studio.replay.as_mut() {
                        replace_replay_ship_segment(
                            replay,
                            &ship_id,
                            &ship_name,
                            playhead_ms,
                            playhead_ms,
                            &[ReplayShipKeyframe {
                                time_ms: playhead_ms,
                                translation: position.to_array(),
                                rotation: transform.rotation.to_array(),
                            }],
                        );
                        studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
                        studio.status = format!("已保存飞船 {ship_name} 在 {} 的位置帧", format_time(playhead_ms));
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(playback_active, egui::Button::new("从此停止飞船"))
                        .clicked()
                    {
                        if let Some(replay) = studio.replay.as_mut() {
                            stop_replay_ship_at(
                                replay,
                                &ship_id,
                                &ship_name,
                                playhead_ms,
                                transform,
                            );
                            studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
                            studio.status = format!(
                                "已让飞船 {ship_name} 从 {} 起停止",
                                format_time(playhead_ms),
                            );
                        }
                    }
                    let driving = live_edits.ship_control.driving_ship_id.as_deref()
                        == Some(ship_id.as_str());
                    if ui
                        .add_enabled(
                            playback_active
                                && studio.live_editing_enabled
                                && driving
                                && studio.live_movement_punch_in.is_none(),
                            egui::Button::new("从此录制新飞行"),
                        )
                        .clicked()
                    {
                        begin_live_ship_take(
                            studio,
                            ship_id.clone(),
                            ship_name.clone(),
                            transform,
                        );
                    }
                    if !driving {
                        ui.small("覆录前先用飞船接管工具进入驾驶状态。");
                    }
                });
            }
        });

        ui.collapsing("爆炸与场景事件", |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        playback_active,
                        egui::Button::new("💥 选择爆炸工具并在此时插入"),
                    )
                    .clicked()
                {
                    studio.live_editing_enabled = true;
                    live_edits.editor.mode = VoxelEditMode::Explode;
                    studio.status = format!(
                        "爆炸工具已选中；在回放画面点击目标，事件会写入 {}",
                        format_time(playhead_ms),
                    );
                }
                ui.label(format!("当前工具：{}", live_edits.editor.mode.label()));
            });
            ui.add(
                egui::Slider::new(&mut live_edits.editor.physics_explosion_radius, 0.5..=16.0)
                    .text("爆炸半径（世界单位）"),
            );
            ui.add(
                egui::Slider::new(
                    &mut live_edits.editor.physics_explosion_impulse,
                    0.0..=200.0,
                )
                .text("爆炸冲量"),
            );
            ui.small(format!(
                "爆炸仍使用统一体素尺寸 {:.2}；点击产生的所有地形/船体变化会绑定到当前播放头。",
                VOXEL_SIZE,
            ));

            let summaries = studio
                .replay
                .as_ref()
                .map(|replay| replay_scene_event_summaries(replay, playhead_ms))
                .unwrap_or_default();
            if summaries.is_empty() {
                ui.small("时间轴还没有爆炸或场景修改。");
            }
            let mut requested_toggle = None;
            let mut requested_seek = None;
            for summary in summaries {
                ui.horizontal(|ui| {
                    let mut enabled = summary.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        requested_toggle = Some((summary.time_ms, enabled));
                    }
                    ui.label(format!(
                        "{} · 地形 {} · 船体 {}",
                        format_time(summary.time_ms),
                        summary.terrain_cells,
                        summary.hull_cells,
                    ));
                    if ui.small_button("定位").clicked() {
                        requested_seek = Some(summary.time_ms);
                    }
                    if !summary.enabled {
                        ui.colored_label(egui::Color32::from_rgb(190, 145, 70), "已阻止");
                    }
                });
            }
            if let Some((time_ms, enabled)) = requested_toggle {
                if let Some(replay) = studio.replay.as_mut() {
                    for change in &mut replay.terrain_changes {
                        if change.time_ms == time_ms {
                            change.enabled = enabled;
                        }
                    }
                    for change in &mut replay.ship_hull_changes {
                        if change.time_ms == time_ms {
                            change.enabled = enabled;
                        }
                    }
                }
                studio.scene_revision = studio.scene_revision.wrapping_add(1);
                studio.status = if enabled {
                    format!("已恢复 {} 的场景事件", format_time(time_ms))
                } else {
                    format!("已阻止 {} 的爆炸/场景事件；可随时重新勾选", format_time(time_ms))
                };
            }
            if let Some(time_ms) = requested_seek {
                studio.playback_ms = time_ms.min(
                    studio
                        .replay
                        .as_ref()
                        .map(|replay| replay.duration_ms)
                        .unwrap_or_default(),
                );
            }
        });
    });
}

/// Writes through on each coordinate change, so playback applies the authored
/// pose on its next update. Pausing makes precise frame placement easier.
fn replay_position_frame_editor(ui: &mut Ui, position: Vec3, enabled: bool) -> Option<Vec3> {
    let mut cells = (position / VOXEL_SIZE).to_array();
    let mut changed = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.label("当前位置帧（格）；暂停后可精确调整，修改会立即保存");
        ui.horizontal_wrapped(|ui| {
            for (axis, value) in ["X", "Y", "Z"].into_iter().zip(&mut cells) {
                ui.label(axis);
                changed |= ui.add(DragValue::new(value).speed(1.0)).changed();
            }
            changed |= ui.button("保存当前位置帧").clicked();
        });
    });
    (changed && cells.iter().all(|value| value.is_finite()))
        .then(|| Vec3::from_array(cells) * VOXEL_SIZE)
}

fn replay_dialogue_editor(
    ui: &mut egui::Ui,
    studio: &mut ReplayStudio,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
) {
    if studio.replay.is_none() || matches!(studio.mode, ReplayMode::Recording) {
        return;
    }
    let playback_anchor = studio.replay.as_ref().and_then(|replay| {
        active_dialogue_index(&replay.dialogue, studio.playback_ms).map(|index| {
            (
                replay.dialogue[index].line_id,
                studio
                    .playback_ms
                    .saturating_sub(replay.dialogue[index].time_ms),
            )
        })
    });

    ui.separator();
    ui.collapsing("区域与台词编辑", |ui| {
        let mut changed = false;
        let mut rebuild_blocks_requested = false;
        let mut recluster_requested = false;
        let mut block_move = None;
        let mut block_split = None;
        let mut block_merge = None;
        let mut block_renames = Vec::new();
        let mut dialogue_drop = None;
        let mut dialogue_delete = None;
        let mut dialogue_add_requested = false;
        {
            let replay = studio.replay.as_mut().expect("checked above");
            let legacy_layout = replay_uses_legacy_chronological_layout(replay);
            let block_options = replay
                .area_blocks
                .iter()
                .enumerate()
                .map(|(index, block)| {
                    (
                        block.id,
                        format!("{}. {}", index + 1, block.area),
                    )
                })
                .collect::<Vec<_>>();
            let line_blocks = replay
                .area_blocks
                .iter()
                .flat_map(|block| {
                    block
                        .line_ids
                        .iter()
                        .map(move |line_id| (*line_id, block.id))
                })
                .collect::<HashMap<_, _>>();
            let mut line_reassignments = Vec::new();
            let mut line_area_changes = Vec::new();
            ui.add_enabled_ui(!legacy_layout, |ui| {
                ui.horizontal(|ui| {
                    ui.label("自动同区距离");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut replay.area_radius_cells)
                                .range(1..=10_000)
                                .suffix(" 格"),
                        )
                        .changed();
                    if ui.button("重新自动分区").clicked() {
                        recluster_requested = true;
                    }
                    if ui.button("重建三回合区块").clicked() {
                        rebuild_blocks_requested = true;
                    }
                });
            });
            if legacy_layout {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 150, 55),
                    "旧版项目保持原始时间顺序；自动分区和三回合重建不可用。",
                );
            } else {
                ui.small("按 XZ 水平距离自动分区；重新分区或重建三回合区块会覆盖区域名和手动区块顺序。");
            }
            ui.horizontal(|ui| {
                if ui.button("全选").clicked() {
                    changed |= set_all_replay_dialogue_included(replay, true);
                }
                if ui.button("全部取消").clicked() {
                    changed |= set_all_replay_dialogue_included(replay, false);
                }
                if ui.button("新增 DM 台词").clicked() {
                    dialogue_add_requested = true;
                }
            });
            ui.small("拖动台词左上角的手柄可排序（含 DM 台词）；拖到卡片上半部/下半部会插到其前/后。删除只影响当前回放草稿。");

            egui::ScrollArea::vertical()
                .id_salt("replay-dialogue-editor")
                .max_height(320.0)
                .show(ui, |ui| {
                    for line in &mut replay.dialogue {
                        let target_line_id = line.line_id;
                        let (drop_zone, dropped) = ui.dnd_drop_zone::<ReplayDialogueDrag, _>(
                            egui::Frame::group(ui.style()),
                            |ui| {
                            ui.horizontal(|ui| {
                                ui.dnd_drag_source(
                                    egui::Id::new(("replay-dialogue-drag", line.line_id)),
                                    ReplayDialogueDrag {
                                        line_id: line.line_id,
                                    },
                                    |ui| ui.label("⠿ 拖动"),
                                );
                                changed |= ui.checkbox(&mut line.included, "使用").changed();
                                ui.label(format!(
                                    "{} · 原始时间 {} · ID {}",
                                    line.name, line.source_time, line.line_id
                                ));
                                if line.metadata_estimated {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(220, 150, 55),
                                        "旧消息：回合/位置为估算",
                                    );
                                }
                                if ui.button("删除").clicked() {
                                    dialogue_delete = Some(line.line_id);
                                }
                            });
                            let text_changed = ui
                                .add(
                                    egui::TextEdit::multiline(&mut line.text)
                                        .desired_rows(2)
                                        .desired_width(ui.available_width()),
                                )
                                .changed();
                            if text_changed {
                                line.speech_text = None;
                                changed = true;
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.label("回合");
                                let turn_changed = ui
                                    .add(egui::DragValue::new(
                                        &mut line.turn_index,
                                    ))
                                    .changed();
                                changed |= turn_changed;
                                ui.label("区域");
                                let area_changed = ui
                                    .add(
                                        egui::TextEdit::singleline(&mut line.area)
                                            .desired_width(90.0),
                                    )
                                    .changed();
                                if area_changed {
                                    line_area_changes.push((line.line_id, line.area.clone()));
                                    changed = true;
                                }
                                let mut position_changed = false;
                                for (axis, value) in
                                    ["X", "Y", "Z"].into_iter().zip(&mut line.position_cells)
                                {
                                    ui.label(axis);
                                    position_changed |=
                                        ui.add(egui::DragValue::new(value)).changed();
                                }
                                if turn_changed || position_changed {
                                    line.metadata_estimated = false;
                                    changed = true;
                                }
                                if !block_options.is_empty() {
                                    let mut selected_block =
                                        line_blocks.get(&line.line_id).copied().unwrap_or_default();
                                    egui::ComboBox::from_id_salt((
                                        "replay-line-block",
                                        line.line_id,
                                    ))
                                    .selected_text(
                                        block_options
                                            .iter()
                                            .find(|(id, _)| *id == selected_block)
                                            .map(|(_, area)| area.as_str())
                                            .unwrap_or("未分配"),
                                    )
                                    .show_ui(ui, |ui| {
                                        for (block_id, area) in &block_options {
                                            ui.selectable_value(
                                                &mut selected_block,
                                                *block_id,
                                                area,
                                            );
                                        }
                                    });
                                    if line_blocks.get(&line.line_id).copied()
                                        != Some(selected_block)
                                    {
                                        line_reassignments.push((line.line_id, selected_block));
                                    }
                                }
                            });
                        },
                        );
                        if let Some(dragged) = dropped {
                            if dragged.line_id != target_line_id {
                                let insert_after = ui
                                    .ctx()
                                    .pointer_latest_pos()
                                    .is_some_and(|pointer| {
                                        pointer.y >= drop_zone.response.rect.center().y
                                    });
                                dialogue_drop =
                                    Some((dragged.line_id, target_line_id, insert_after));
                            }
                        }
                    }
                });
            if dialogue_add_requested {
                append_dm_replay_dialogue(replay);
                changed = true;
            }
            if let Some(line_id) = dialogue_delete {
                changed |= delete_replay_dialogue(replay, line_id);
            }
            if let Some((dragged_line_id, target_line_id, insert_after)) = dialogue_drop {
                changed |= move_replay_dialogue(
                    replay,
                    dragged_line_id,
                    target_line_id,
                    insert_after,
                );
            }
            for (line_id, area) in line_area_changes {
                changed |= apply_replay_dialogue_area_edit(replay, line_id, area);
            }
            for (line_id, target_block) in line_reassignments {
                changed |= reassign_replay_dialogue_to_block(replay, line_id, target_block);
            }

            ui.label("播放区块（每块最多三个不同回合）");
            for index in 0..replay.area_blocks.len() {
                let block_count = replay.area_blocks.len();
                let block = &mut replay.area_blocks[index];
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", index + 1));
                    let area_changed = ui
                        .add(egui::TextEdit::singleline(&mut block.area).desired_width(100.0))
                        .changed();
                    if area_changed {
                        block_renames.push((block.id, block.area.clone()));
                        changed = true;
                    }
                    ui.label(format!("{} 句", block.line_ids.len()));
                    if ui.add_enabled(index > 0, egui::Button::new("↑")).clicked() {
                        block_move = Some((index, index - 1));
                    }
                    if ui
                        .add_enabled(
                            index + 1 < block_count,
                            egui::Button::new("↓"),
                        )
                        .clicked()
                    {
                        block_move = Some((index, index + 1));
                    }
                    if ui
                        .add_enabled(
                            block.line_ids.len() > 1,
                            egui::Button::new("拆分"),
                        )
                        .clicked()
                    {
                        block_split = Some(index);
                    }
                    if ui
                        .add_enabled(
                            index + 1 < block_count,
                            egui::Button::new("与下块合并"),
                        )
                        .clicked()
                    {
                        block_merge = Some(index);
                    }
                });
            }
            for (block_id, area) in block_renames {
                if let Some(block) = replay.area_blocks.iter().find(|block| block.id == block_id) {
                    let line_ids = block.line_ids.iter().copied().collect::<HashSet<_>>();
                    for line in &mut replay.dialogue {
                        if line_ids.contains(&line.line_id) {
                            line.area = area.clone();
                        }
                    }
                }
            }
            if let Some((from, to)) = block_move {
                replay.area_blocks.swap(from, to);
                changed = true;
            }
            if let Some(index) = block_split {
                let split_at = replay.area_blocks[index].line_ids.len() / 2;
                let line_ids = replay.area_blocks[index].line_ids.split_off(split_at);
                let next_id = replay
                    .area_blocks
                    .iter()
                    .map(|block| block.id)
                    .max()
                    .unwrap_or_default()
                    .saturating_add(1);
                let area = replay.area_blocks[index].area.clone();
                replay.area_blocks.insert(index + 1, ReplayAreaBlock {
                    id: next_id,
                    area,
                    line_ids,
                });
                changed = true;
            }
            if let Some(index) = block_merge {
                let next = replay.area_blocks.remove(index + 1);
                let moved_line_ids = next.line_ids.iter().copied().collect::<HashSet<_>>();
                let destination_area = replay.area_blocks[index].area.clone();
                replay.area_blocks[index].line_ids.extend(next.line_ids);
                for line in &mut replay.dialogue {
                    if moved_line_ids.contains(&line.line_id) {
                        line.area = destination_area.clone();
                    }
                }
                changed = true;
            }
            if recluster_requested {
                auto_group_replay_areas(replay);
                rebuild_blocks_requested = true;
            }
            if rebuild_blocks_requested {
                replay.manual_dialogue_order = false;
                rebuild_area_blocks(replay);
            }
            if changed || rebuild_blocks_requested {
                recompile_edited_replay_dialogue(replay);
                extend_replay_for_speech(replay);
                let playable = replay
                    .dialogue
                    .iter()
                    .filter(|line| line.included && line.time_ms != u64::MAX)
                    .cloned()
                    .collect::<Vec<_>>();
                if let Ok(base) = camera.single() {
                    let positions = replay_speaker_positions(&playable);
                    replay.camera = turn_based_camera_track(
                        base,
                        &playable,
                        replay.duration_ms,
                        &positions,
                        replay.camera_distance_scale,
                        replay.camera_yaw_degrees,
                        &ReplayCameraObstacles::from_scene(&replay.scene),
                    );
                }
            }
        }
        if changed || rebuild_blocks_requested {
            studio.playback_ms = playback_anchor
                .and_then(|(line_id, elapsed_ms)| {
                    studio.replay.as_ref().and_then(|replay| {
                        replay
                            .dialogue
                            .iter()
                            .find(|line| line.line_id == line_id && replay_dialogue_is_playable(line))
                            .map(|line| {
                                line.time_ms.saturating_add(
                                    elapsed_ms.min(line.duration_ms.saturating_sub(1)),
                                )
                            })
                    })
                })
                .unwrap_or_else(|| {
                    studio.playback_ms.min(
                        studio
                            .replay
                            .as_ref()
                            .map(|replay| replay.duration_ms)
                            .unwrap_or_default(),
                    )
                });
            studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
            studio.speech_wait_cue = None;
            studio.speech_wait_elapsed_seconds = 0.0;
            studio.speech_bypass_cues.clear();
            studio.director_response_hash = None;
            studio.director_request_pending = false;
            studio.auto_export_after_director = false;
            studio.status = "已应用 DM 的台词、回合、位置、区域或区块编辑".to_owned();
        }
    });
}

fn rebuild_player_movements_from_history(
    replay: &mut ReplayFile,
    history: &ReplayPlayerMovementHistory,
) {
    let imported = replay_player_movements_from_history(
        history,
        &replay.campaign_id,
        &replay.dialogue,
    );
    let replaced_ids = replay
        .player_movements
        .iter()
        .chain(imported.iter())
        .map(|movement| movement.user_id)
        .filter(|id| !replay.authored_player_movements.contains(id))
        .collect::<HashSet<_>>();
    replay
        .standee_positions
        .retain(|sample| !replaced_ids.contains(&sample.user_id));
    let imported_through = history
        .sessions
        .iter()
        .filter(|session| {
            session.campaign_id == replay.campaign_id && replaced_ids.contains(&session.user_id)
        })
        .flat_map(|session| session.keyframes.iter().map(|frame| frame.source_unix_ms))
        .max()
        .unwrap_or_default();
    replay.player_movement_history_cursor_unix_ms = replay
        .player_movement_history_cursor_unix_ms
        .max(imported_through);
    replay
        .player_movements
        .retain(|movement| replay.authored_player_movements.contains(&movement.user_id));
    replay.player_movements.extend(
        imported
            .into_iter()
            .filter(|movement| !replay.authored_player_movements.contains(&movement.user_id)),
    );
}

fn restore_player_movement_history(replay: &mut ReplayFile, user_id: u64) {
    replay.authored_player_movements.remove(&user_id);
    replay
        .standee_positions
        .retain(|sample| sample.user_id != user_id);
}

fn restore_ship_trajectory_history(replay: &mut ReplayFile, ship_id: &str) {
    replay.authored_ship_trajectories.remove(ship_id);
    replay
        .ship_trajectories
        .retain(|trajectory| trajectory.ship_id != ship_id);
}

fn replay_movement_timing_editor(
    ui: &mut egui::Ui,
    studio: &mut ReplayStudio,
    history: &mut Persistent<ReplayPlayerMovementHistory>,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
) {
    let Some(replay) = studio.replay.as_ref() else { return };
    let campaign_id = replay.campaign_id.clone();
    let visible_user_ids = replay
        .dialogue
        .iter()
        .filter(|line| replay_dialogue_is_playable(line))
        .flat_map(|line| [Some(line.sender_id), line.camera_focus_id])
        .flatten()
        .collect::<HashSet<_>>();
    let line_options = replay
        .dialogue
        .iter()
        .filter(|line| replay_dialogue_is_playable(line) && line.source_time > 0)
        .map(|line| {
            (
                line.source_time,
                line.sender_id,
                format!(
                    "#{} {}：{}",
                    line.line_id,
                    line.name,
                    line.text.chars().take(28).collect::<String>()
                ),
            )
        })
        .collect::<Vec<_>>();
    let session_indices = history
        .sessions
        .iter()
        .enumerate()
        .filter(|(_, session)| {
            session.campaign_id == campaign_id
                && visible_user_ids.contains(&session.user_id)
                && !session.keyframes.is_empty()
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let authored_ids = replay
        .authored_player_movements
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let mut requested_restores = Vec::<u64>::new();
    let mut changed = false;
    let mut deleted_sessions = Vec::new();
    ui.collapsing(format!("玩家移动数据与时序（{} 条）", session_indices.len()), |ui| {
        ui.small("DM 可编辑轨迹帧、开始台词和延迟；重新“从现有聊天生成”仍会应用这些持久数据。坐标单位为体素格。");
        if !authored_ids.is_empty() {
            ui.small("现场编辑的轨迹优先保留；历史调整只更新其他轨迹。下列按钮会用历史覆盖对应的现场编辑。");
            for id in &authored_ids {
                if ui.button(format!("用历史恢复角色 {id}")).clicked() {
                    requested_restores.push(id.clone());
                }
            }
        }
        if session_indices.is_empty() {
            ui.label("当前回放没有已保存的玩家移动数据。");
        }
        for (display_index, session_index) in session_indices.iter().copied().enumerate() {
            let session = &mut history.sessions[session_index];
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{}. 玩家 {} · 回合 {} · {} 帧",
                        display_index + 1,
                        session.user_id,
                        session.turn_index,
                        session.keyframes.len()
                    ));
                    if ui.button("删除轨迹").clicked() {
                        deleted_sessions.push(session_index);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("开始于台词");
                    let mut selected = (
                        session.start_after_source_time,
                        session.start_after_sender_id,
                    );
                    let selected_text = line_options
                        .iter()
                        .find(|(source_time, sender_id, _)| {
                            selected == (Some(*source_time), Some(*sender_id))
                        })
                        .map(|(_, _, label)| label.as_str())
                        .unwrap_or("自动：该玩家本回合最近台词");
                    egui::ComboBox::from_id_salt((
                        "replay-movement-anchor",
                        session_index,
                    ))
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected, (None, None), "自动：该玩家本回合最近台词");
                        for (source_time, sender_id, label) in &line_options {
                            ui.selectable_value(
                                &mut selected,
                                (Some(*source_time), Some(*sender_id)),
                                label,
                            );
                        }
                    });
                    if selected
                        != (
                            session.start_after_source_time,
                            session.start_after_sender_id,
                        )
                    {
                        session.start_after_source_time = selected.0;
                        session.start_after_sender_id = selected.1;
                        changed = true;
                    }
                    ui.label("延迟");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut session.start_delay_ms)
                                .range(0..=600_000)
                                .speed(100)
                                .suffix(" ms"),
                        )
                        .changed();
                });
                let first_source_unix_ms = session
                    .keyframes
                    .first()
                    .map(|frame| frame.source_unix_ms)
                    .unwrap_or_default();
                egui::CollapsingHeader::new("轨迹帧")
                    .id_salt(("replay-movement-keyframes", session_index))
                    .show(ui, |ui| {
                    let mut minimum_offset_ms = 0;
                    for (frame_index, frame) in session.keyframes.iter_mut().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(format!("#{}", frame_index + 1));
                            let mut offset_ms =
                                frame.source_unix_ms.saturating_sub(first_source_unix_ms);
                            if frame_index == 0 {
                                ui.label("+0 ms");
                            } else {
                                let response = ui.add(
                                    egui::DragValue::new(&mut offset_ms)
                                        .range(minimum_offset_ms..=600_000)
                                        .speed(50)
                                        .prefix("+")
                                        .suffix(" ms"),
                                );
                                if response.changed() {
                                    frame.source_unix_ms =
                                        first_source_unix_ms.saturating_add(offset_ms);
                                    changed = true;
                                }
                            }
                            for (axis, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                                ui.label(label);
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(
                                            &mut frame.position_cells[axis],
                                        )
                                        .range(-1_000_000.0..=1_000_000.0)
                                        .speed(0.25)
                                        .fixed_decimals(2),
                                    )
                                    .changed();
                            }
                            minimum_offset_ms = offset_ms;
                        });
                    }
                });
            });
        }
    });
    deleted_sessions.sort_unstable();
    deleted_sessions.dedup();
    for session_index in deleted_sessions.into_iter().rev() {
        history.sessions.remove(session_index);
        changed = true;
    }

    if !changed && requested_restores.is_empty() {
        return;
    }
    let persist_result = if changed { history.persist() } else { Ok(()) };
    if let Err(err) = persist_result {
        studio.status = format!("无法保存玩家移动时序：{err}");
        return;
    }
    let replay = studio.replay.as_mut().expect("checked above");
    for id in requested_restores {
        restore_player_movement_history(replay, id);
    }
    rebuild_player_movements_from_history(replay, history);
    refresh_replay_duration(replay, 5_000);
    if let Ok(base) = camera.single() {
        let positions = replay_speaker_positions(&replay.dialogue);
        replay.camera = turn_based_camera_track(
            base,
            &replay.dialogue,
            replay.duration_ms,
            &positions,
            replay.camera_distance_scale,
            replay.camera_yaw_degrees,
            &ReplayCameraObstacles::from_scene(&replay.scene),
        );
    }
    let duration_ms = replay.duration_ms;
    studio.playback_ms = studio.playback_ms.min(duration_ms);
    studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
    studio.status = "已保存玩家移动轨迹、开始台词与延迟".to_owned();
}

fn replay_ship_trajectory_editor(
    ui: &mut egui::Ui,
    studio: &mut ReplayStudio,
    ship_history: &mut Persistent<ReplayShipTrajectoryHistory>,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
) {
    let Some(replay) = studio.replay.as_ref() else { return };
    let campaign_id = replay.campaign_id.clone();
    let line_options = replay
        .dialogue
        .iter()
        .filter(|line| replay_dialogue_is_playable(line) && line.source_time > 0)
        .map(|line| {
            (
                line.source_time,
                format!(
                    "#{} {}：{}",
                    line.line_id,
                    line.name,
                    line.text.chars().take(28).collect::<String>()
                ),
            )
        })
        .collect::<Vec<_>>();
    let session_indices = ship_history
        .sessions
        .iter()
        .enumerate()
        .filter(|(_, session)| session.campaign_id == campaign_id && !session.keyframes.is_empty())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let authored_ids = replay
        .authored_ship_trajectories
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let mut requested_restores = Vec::<String>::new();
    let mut changed = false;
    let mut deleted_sessions = Vec::new();
    ui.collapsing(
        format!("飞船轨迹数据与时序（{} 条）", session_indices.len()),
        |ui| {
            ui.small("DM 可编辑轨迹帧、开始台词和延迟；重新“从现有聊天生成”仍会应用这些持久数据。坐标单位为世界单位。");
            if !authored_ids.is_empty() {
                ui.small("现场编辑的轨迹优先保留；历史调整只更新其他轨迹。下列按钮会用历史覆盖对应的现场编辑。");
                for id in &authored_ids {
                    if ui.button(format!("用历史恢复飞船 {id}")).clicked() {
                        requested_restores.push(id.clone());
                    }
                }
            }
            if session_indices.is_empty() {
                ui.label("当前回放没有已保存的飞船轨迹数据。");
            }
            for (display_index, session_index) in session_indices.iter().copied().enumerate() {
                let session = &mut ship_history.sessions[session_index];
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "{}. {}（{}）· 回合 {} · {} 帧",
                            display_index + 1,
                            session.ship_name,
                            session.ship_id,
                            session.turn_index,
                            session.keyframes.len()
                        ));
                        if ui.button("删除轨迹").clicked() {
                            deleted_sessions.push(session_index);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("开始于台词");
                        let mut selected_source_time = session.start_after_source_time;
                        let selected_text = line_options
                            .iter()
                            .find(|(source_time, _)| {
                                selected_source_time == Some(*source_time)
                            })
                            .map(|(_, label)| label.as_str())
                            .unwrap_or("自动：按移动发生时间");
                        egui::ComboBox::from_id_salt((
                            "replay-ship-anchor",
                            session_index,
                        ))
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut selected_source_time,
                                None,
                                "自动：按移动发生时间",
                            );
                            for (source_time, label) in &line_options {
                                ui.selectable_value(
                                    &mut selected_source_time,
                                    Some(*source_time),
                                    label,
                                );
                            }
                        });
                        if selected_source_time != session.start_after_source_time {
                            session.start_after_source_time = selected_source_time;
                            changed = true;
                        }
                        ui.label("延迟");
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut session.start_delay_ms)
                                    .range(0..=600_000)
                                    .speed(100)
                                    .suffix(" ms"),
                            )
                            .changed();
                    });
                    let first_source_unix_ms = session
                        .keyframes
                        .first()
                        .map(|frame| frame.source_unix_ms)
                        .unwrap_or_default();
                    egui::CollapsingHeader::new("轨迹帧")
                        .id_salt(("replay-ship-keyframes", session_index))
                        .show(ui, |ui| {
                            let mut minimum_offset_ms = 0;
                            for (frame_index, frame) in
                                session.keyframes.iter_mut().enumerate()
                            {
                                ui.horizontal_wrapped(|ui| {
                                    ui.monospace(format!("#{}", frame_index + 1));
                                    let mut offset_ms = frame
                                        .source_unix_ms
                                        .saturating_sub(first_source_unix_ms);
                                    if frame_index == 0 {
                                        ui.label("+0 ms");
                                    } else {
                                        let response = ui.add(
                                            egui::DragValue::new(&mut offset_ms)
                                                .range(minimum_offset_ms..=600_000)
                                                .speed(50)
                                                .prefix("+")
                                                .suffix(" ms"),
                                        );
                                        if response.changed() {
                                            frame.source_unix_ms =
                                                first_source_unix_ms.saturating_add(offset_ms);
                                            changed = true;
                                        }
                                    }
                                    for (axis, label) in
                                        ["X", "Y", "Z"].into_iter().enumerate()
                                    {
                                        ui.label(label);
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(
                                                    &mut frame.translation[axis],
                                                )
                                                .range(-1_000_000.0..=1_000_000.0)
                                                .speed(0.25)
                                                .fixed_decimals(2),
                                            )
                                            .changed();
                                    }
                                    minimum_offset_ms = offset_ms;
                                });
                            }
                        });
                });
            }
        },
    );
    deleted_sessions.sort_unstable();
    deleted_sessions.dedup();
    for session_index in deleted_sessions.into_iter().rev() {
        ship_history.sessions.remove(session_index);
        changed = true;
    }

    if !changed && requested_restores.is_empty() {
        return;
    }
    let persist_result = if changed { ship_history.persist() } else { Ok(()) };
    if let Err(err) = persist_result {
        studio.status = format!("无法保存飞船轨迹时序：{err}");
        return;
    }
    let replay = studio.replay.as_mut().expect("checked above");
    for id in requested_restores {
        restore_ship_trajectory_history(replay, &id);
    }
    rebuild_replay_timeline_with_ships(replay, ship_history, camera);
    let duration_ms = replay.duration_ms;
    studio.playback_ms = studio.playback_ms.min(duration_ms);
    studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
    studio.status = "已保存飞船轨迹、开始台词与延迟".to_owned();
}

fn rebuild_ship_trajectories_from_history(
    replay: &mut ReplayFile,
    ship_history: &ReplayShipTrajectoryHistory,
) {
    replay.ship_trajectories.retain(|trajectory| {
        replay
            .authored_ship_trajectories
            .contains(&trajectory.ship_id)
    });
    compile_scene_dynamics_timeline(replay, ship_history, 0, &[], &[]);
    replay.ship_trajectory_history_cursor_unix_ms = unix_time_ms();
    extend_replay_for_speech(replay);
    refresh_replay_duration(replay, 5_000);
}

fn rebuild_replay_timeline_with_ships(
    replay: &mut ReplayFile,
    ship_history: &ReplayShipTrajectoryHistory,
    camera: &Query<&Transform, With<VoxelViewportCamera>>,
) {
    rebuild_ship_trajectories_from_history(replay, ship_history);
    if let Ok(base) = camera.single() {
        let positions = replay_speaker_positions(&replay.dialogue);
        replay.camera = turn_based_camera_track(
            base,
            &replay.dialogue,
            replay.duration_ms,
            &positions,
            replay.camera_distance_scale,
            replay.camera_yaw_degrees,
            &ReplayCameraObstacles::from_scene(&replay.scene),
        );
    }
}

fn stop_playback(studio: &mut ReplayStudio, grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>) {
    cancel_live_replay_take(studio);
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
    studio.speech_wait_cue = None;
    studio.speech_wait_elapsed_seconds = 0.0;
    studio.speech_bypass_cues.clear();
    studio.live_editing_enabled = false;
    studio.live_movement_punch_in = None;
    studio.live_ship_punch_in = None;
}

fn start_video_export(
    studio: &mut ReplayStudio,
    capture_active: &mut ReplayVideoCaptureActive,
    grids: &mut Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
    player_movement_history: &ReplayPlayerMovementHistory,
    ship_trajectory_history: &ReplayShipTrajectoryHistory,
) {
    if replay_has_live_take(studio) {
        studio.status = "请先保存或取消本次录制，再导出视频".to_owned();
        return;
    }
    prepare_replay_movement_for_playback(
        studio, player_movement_history, ship_trajectory_history,
    );
    let Some((duration_ms, dialogue, master_speech_speed, speaker_voice_settings)) =
        studio.replay.as_ref().map(|replay| {
            (
                replay.duration_ms,
                replay
                    .dialogue
                    .iter()
                    .filter(|line| replay_dialogue_is_playable(line))
                    .cloned()
                    .collect::<Vec<_>>(),
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
    let music_file = if studio.music_enabled {
        let Some(path) = studio.music_file.as_ref().filter(|path| path.is_file()) else {
            studio.status = "请选择 assets/audio 中的本地 BGM，或关闭 BGM".to_owned();
            return;
        };
        Some(path.clone())
    } else {
        None
    };
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
        music_file,
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

fn background_music_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(BACKGROUND_MUSIC_DIRECTORY)
}

fn discover_background_music() -> Vec<PathBuf> {
    discover_background_music_in(&background_music_directory())
}

fn discover_background_music_in(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    BACKGROUND_MUSIC_EXTENSIONS
                        .iter()
                        .any(|supported| extension.eq_ignore_ascii_case(supported))
                })
        })
        .collect::<Vec<_>>();
    files.sort_by_cached_key(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    });
    files
}

fn refresh_background_music(studio: &mut ReplayStudio) {
    let had_selection = studio.music_file.is_some();
    studio.music_files = discover_background_music();
    if studio
        .music_file
        .as_ref()
        .is_none_or(|selected| !studio.music_files.contains(selected))
    {
        studio.music_file = studio.music_files.first().cloned();
    }
    if studio.music_file.is_none() {
        studio.music_enabled = false;
    } else if !had_selection {
        studio.music_enabled = true;
    }
    studio.status = if studio.music_files.is_empty() {
        format!(
            "BGM 文件夹为空：{}",
            background_music_directory().display()
        )
    } else {
        format!(
            "已找到 {} 首本地 BGM",
            studio.music_files.len()
        )
    };
}

fn open_background_music_directory() -> Result<(), String> {
    let directory = background_music_directory();
    fs::create_dir_all(&directory).map_err(|err| format!("无法创建 BGM 文件夹：{err}"))?;
    open_directory(&directory).map_err(|err| format!("无法打开 BGM 文件夹：{err}"))
}

#[cfg(windows)]
fn open_directory(path: &Path) -> std::io::Result<()> {
    Command::new("explorer").arg(path).spawn().map(|_| ())
}

#[cfg(target_os = "macos")]
fn open_directory(path: &Path) -> std::io::Result<()> {
    Command::new("open").arg(path).spawn().map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_directory(path: &Path) -> std::io::Result<()> {
    Command::new("xdg-open").arg(path).spawn().map(|_| ())
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
        "未安装 EmotiVoice 中文运行环境；请运行 scripts/setup_emotivoice.ps1".to_owned()
    })
}

fn encode_video_frames(
    frames_path: &Path,
    output_path: &Path,
    fps: u32,
    duration_ms: u64,
    music_file: Option<&Path>,
    music_volume: f32,
    speech_enabled: bool,
    speech_volume: f32,
    master_speech_speed: f32,
    dialogue: &[ReplayDialogue],
    speaker_voice_settings: &HashMap<u64, SpeakerVoiceSettings>,
) -> Result<(), String> {
    let frame_pattern = frames_path.join("frame_%06d.png");
    let soundtrack_path = frames_path.join("background-music.wav");
    let narration_path = frames_path.join("character-narration.wav");
    if let Some(music_file) = music_file {
        write_background_music_track(
            music_file,
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
    let music_input = if music_file.is_some() {
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
    if music_file.is_some() || speech_enabled {
        command.args(["-c:a", "aac", "-b:a", "160k", "-shortest"]);
    }
    command.arg(&temporary_output);
    hide_command_window(&mut command);
    let output = command
        .output()
        .map_err(|err| format!("无法启动 FFmpeg：{err}"))?;
    if output.status.success() {
        if music_file.is_some() || speech_enabled {
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
    allow_truncate: bool,
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

    let spoken_dialogue = dialogue
        .iter()
        .filter(|line| line.speech_enabled)
        .collect::<Vec<_>>();
    let speech_jobs = spoken_dialogue
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
                onnx_speed: line_onnx_speed(
                    line,
                    settings.speech_rate,
                    master_speech_speed,
                ),
                duration_ms: line.duration_ms,
                allow_truncate: line.duration_locked,
            }
        })
        .collect::<Vec<_>>();
    synthesize_speech_batch(working_directory, &speech_jobs)?;

    let mut cues = Vec::with_capacity(spoken_dialogue.len());
    for (index, line) in spoken_dialogue.iter().enumerate() {
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
                .unwrap_or(1.0)
                * normalized_line_speech_volume(line.speech_volume),
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

const DEFAULT_EMOTIVOICE_SPEAKERS: [&str; 11] = [
    "9000", "984", "985", "6671", "6670", "65", "92", "102", "225", "1088", "1093",
];
const RECOMMENDED_EMOTIVOICE_SPEAKERS: [&str; 9] = [
    "9000", "984", "985", "65", "92", "102", "225", "1088", "1093",
];

const EMOTIVOICE_VOICE_CATALOG: &str = concat!(
    include_str!("../assets/emotivoice/catalog/voices_00.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_01.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_02.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_03.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_04.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_05.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_06.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_07.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_08.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_09.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_10.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_11.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_12.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_13.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_14.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_15.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_16.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_17.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_18.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_19.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_20.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_21.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_22.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_23.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_24.tsv"),
    "\n",
    include_str!("../assets/emotivoice/catalog/voices_25.tsv"),
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EmotivoiceVoiceProfile {
    id: &'static str,
    gender: &'static str,
    name: &'static str,
    description: &'static str,
}

const EMOTIVOICE_EMOTIONS: [&str; 7] = ["普通", "开心", "悲伤", "生气", "惊讶", "厌恶", "恐惧"];

fn emotivoice_voice_profiles() -> &'static [EmotivoiceVoiceProfile] {
    static PROFILES: OnceLock<Vec<EmotivoiceVoiceProfile>> = OnceLock::new();
    PROFILES
        .get_or_init(|| {
            EMOTIVOICE_VOICE_CATALOG
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        return None;
                    }
                    let mut fields = line.splitn(4, '\t');
                    Some(EmotivoiceVoiceProfile {
                        id: fields.next()?,
                        gender: fields.next()?,
                        name: fields.next()?,
                        description: fields.next().unwrap_or_default(),
                    })
                })
                .collect()
        })
        .as_slice()
}

fn installed_emotivoice_speakers() -> &'static [String] {
    static INSTALLED_SPEAKERS: OnceLock<Vec<String>> = OnceLock::new();
    INSTALLED_SPEAKERS
        .get_or_init(|| {
            if onnx_tts_is_available() {
                emotivoice_voice_profiles()
                    .iter()
                    .map(|profile| profile.id.to_owned())
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
    emotivoice_voice_profiles()
        .iter()
        .find(|profile| profile.id == speaker)
        .map(emotivoice_profile_label)
        .unwrap_or_else(|| format!("音色 {speaker}"))
}

fn emotivoice_profile_label(profile: &EmotivoiceVoiceProfile) -> String {
    let gender = if profile.gender == "F" { "女声" } else { "男声" };
    let recommended = if RECOMMENDED_EMOTIVOICE_SPEAKERS.contains(&profile.id) {
        "（推荐）"
    } else {
        ""
    };
    format!(
        "{gender} {} · {}{recommended}",
        profile.id, profile.name
    )
}

fn emotivoice_voice_matches(profile: &EmotivoiceVoiceProfile, query: &str) -> bool {
    query.is_empty()
        || profile.id.to_lowercase().contains(query)
        || profile.name.to_lowercase().contains(query)
        || profile.gender.to_lowercase().contains(query)
        || profile.description.to_lowercase().contains(query)
        || (profile.gender == "M" && ("男声".contains(query) || "男性".contains(query)))
        || (profile.gender == "F" && ("女声".contains(query) || "女性".contains(query)))
}

fn default_emotivoice_speaker(sender_id: u64) -> &'static str {
    let index = (sender_id as usize) % DEFAULT_EMOTIVOICE_SPEAKERS.len();
    DEFAULT_EMOTIVOICE_SPEAKERS[index]
}

fn resolved_emotivoice_speaker(configured: Option<&str>, sender_id: u64) -> String {
    configured
        .map(str::trim)
        .filter(|speaker| {
            emotivoice_voice_profiles()
                .iter()
                .any(|profile| profile.id == *speaker)
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

    for (index, line) in replay.dialogue.iter().enumerate() {
        if !line.included || line.time_ms == u64::MAX {
            continue;
        }
        let settings = replay
            .speaker_voice_settings
            .get(&line.sender_id)
            .cloned()
            .unwrap_or_else(|| default_speaker_voice_settings(line.sender_id));
        let required_duration = minimum_speech_window_ms_for_line(
            line,
            settings.speech_rate,
            replay.master_speech_speed,
        );
        let new_duration = if line.duration_locked
            || !line.speech_enabled
            || line.duration_ms <= SHORT_DIALOGUE_MAX_MS
        {
            // Quick exchanges stay quick: a short line keeps its reading
            // duration instead of being stretched to fit synthesized audio.
            // Exact GM timings and muted cues are also never auto-stretched.
            line.duration_ms
        } else {
            line.duration_ms.max(required_duration)
        };
        let new_start = line.time_ms.saturating_add(accumulated_extension);
        let old_end = line.time_ms.saturating_add(line.duration_ms);
        let new_end = new_start.saturating_add(new_duration);
        segments.push((
            line.time_ms,
            old_end,
            new_start,
            new_end,
        ));
        updated_lines.push((index, new_start, new_duration));
        accumulated_extension =
            accumulated_extension.saturating_add(new_duration.saturating_sub(line.duration_ms));
    }

    if accumulated_extension == 0 {
        return false;
    }

    for (index, new_start, new_duration) in updated_lines {
        replay.dialogue[index].time_ms = new_start;
        replay.dialogue[index].duration_ms = new_duration;
    }
    for frame in &mut replay.camera {
        frame.time_ms = stretched_replay_time(frame.time_ms, &segments);
    }
    for movement in &mut replay.player_movements {
        for frame in &mut movement.keyframes {
            frame.time_ms = stretched_replay_time(frame.time_ms, &segments);
        }
    }
    for trajectory in &mut replay.ship_trajectories {
        for frame in &mut trajectory.keyframes {
            frame.time_ms = stretched_replay_time(frame.time_ms, &segments);
        }
    }
    for sample in &mut replay.standee_positions {
        sample.time_ms = stretched_replay_time(sample.time_ms, &segments);
    }
    for change in &mut replay.terrain_changes {
        change.time_ms = stretched_replay_time(change.time_ms, &segments);
    }
    for change in &mut replay.ship_hull_changes {
        change.time_ms = stretched_replay_time(change.time_ms, &segments);
    }
    replay.duration_ms = stretched_replay_time(replay.duration_ms, &segments);
    true
}

fn default_master_speech_speed() -> f32 { 1.10 }
fn default_ship_motion_speed() -> f32 { 3.0 }
fn default_dialogue_waits_for_ship_motion() -> bool { true }

fn default_directed_camera_distance_scale() -> f32 { DEFAULT_DIRECTED_CAMERA_DISTANCE_SCALE }

fn default_directed_camera_yaw_degrees() -> f32 { DEFAULT_DIRECTED_CAMERA_YAW_DEGREES }

fn default_camera_transition_curve() -> f32 { DEFAULT_CAMERA_TRANSITION_CURVE }

fn default_player_movement_curve() -> f32 { DEFAULT_PLAYER_MOVEMENT_CURVE }

fn normalized_directed_camera_distance_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(
            MIN_DIRECTED_CAMERA_DISTANCE_SCALE,
            MAX_DIRECTED_CAMERA_DISTANCE_SCALE,
        )
    } else {
        default_directed_camera_distance_scale()
    }
}

fn normalized_directed_camera_yaw_degrees(yaw_degrees: f32) -> f32 {
    if yaw_degrees.is_finite() {
        yaw_degrees.clamp(
            MIN_DIRECTED_CAMERA_YAW_DEGREES,
            MAX_DIRECTED_CAMERA_YAW_DEGREES,
        )
    } else {
        default_directed_camera_yaw_degrees()
    }
}

fn normalized_camera_transition_curve(curve: f32) -> f32 {
    if curve.is_finite() {
        curve.clamp(
            MIN_CAMERA_TRANSITION_CURVE,
            MAX_CAMERA_TRANSITION_CURVE,
        )
    } else {
        default_camera_transition_curve()
    }
}

fn normalized_player_movement_curve(curve: f32) -> f32 {
    if curve.is_finite() {
        curve.clamp(
            MIN_PLAYER_MOVEMENT_CURVE,
            MAX_PLAYER_MOVEMENT_CURVE,
        )
    } else {
        default_player_movement_curve()
    }
}

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
    for movement in &mut replay.player_movements {
        for frame in &mut movement.keyframes {
            frame.time_ms = scaled_millis(frame.time_ms, ratio);
        }
    }
    for trajectory in &mut replay.ship_trajectories {
        for frame in &mut trajectory.keyframes {
            frame.time_ms = scaled_millis(frame.time_ms, ratio);
        }
    }
    for sample in &mut replay.standee_positions {
        sample.time_ms = scaled_millis(sample.time_ms, ratio);
    }
    for change in &mut replay.terrain_changes {
        change.time_ms = scaled_millis(change.time_ms, ratio);
    }
    for change in &mut replay.ship_hull_changes {
        change.time_ms = scaled_millis(change.time_ms, ratio);
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
        line.line_id.hash(&mut hasher);
        line.included.hash(&mut hasher);
        line.time_ms.hash(&mut hasher);
        line.sender_id.hash(&mut hasher);
        line.duration_ms.hash(&mut hasher);
        line.duration_locked.hash(&mut hasher);
        line.text.hash(&mut hasher);
        line.speech_text.hash(&mut hasher);
        line.speech_enabled.hash(&mut hasher);
        line.speech_rate.to_bits().hash(&mut hasher);
        line.speech_volume.to_bits().hash(&mut hasher);
        line.turn_index.hash(&mut hasher);
        line.position_cells.hash(&mut hasher);
        line.area.hash(&mut hasher);
    }
    for block in &replay.area_blocks {
        block.id.hash(&mut hasher);
        block.area.hash(&mut hasher);
        block.line_ids.hash(&mut hasher);
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
    let mut tts = None;
    for job in jobs {
        let max_samples = job.duration_ms.saturating_mul(32_000) / 1_000;
        let wav = if let Some(wav) = read_speech_cache(
            &job.text,
            &job.speaker,
            &job.emotion,
            job.onnx_speed,
        ) {
            wav
        } else {
            let tts = match tts.as_mut() {
                Some(tts) => tts,
                None => tts.insert(create_onnx_tts()?),
            };
            let wav = tts.synthesize(
                &job.text,
                &job.speaker,
                &job.emotion,
                job.onnx_speed,
            )?;
            cache_speech(
                &job.text,
                &job.speaker,
                &job.emotion,
                job.onnx_speed,
                &wav,
            );
            wav
        };
        fs::write(&job.output_path, wav)
            .map_err(|err| format!("无法保存 EmotiVoice 角色语音：{err}"))?;
        let samples = read_pcm16_mono_wav(Path::new(&job.output_path))?;
        if !job.allow_truncate && samples.len() as u64 > max_samples {
            return Err(format!(
                "EmotiVoice 生成的语音超过 {} 毫秒；请提高整体语速或延长整体台词停留",
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

fn write_background_music_track(
    source: &Path,
    path: &Path,
    duration_ms: u64,
    volume: f32,
) -> Result<(), String> {
    if !source.is_file() {
        return Err(format!(
            "找不到本地 BGM：{}",
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
        .map_err(|err| format!("无法启动 FFmpeg 处理本地 BGM：{err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "无法处理本地 BGM：{}",
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

fn apply_scene(grid: &mut Mut<Grid<u8>>, scene: &ReplayScene) -> usize {
    let mut target = scene
        .voxels
        .iter()
        .map(|voxel| {
            (
                IVec3::from_array(voxel.position),
                voxel.material,
            )
        })
        .collect::<HashMap<_, _>>();
    let occupied = grid
        .iter()
        .flat_map(|(chunk_position, chunk)| {
            prism(IVec3::ZERO, DIMS)
                .filter(|local| chunk[*local] != 0)
                .map(|local| *chunk_position * DIMS + local)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut updates = Vec::new();
    for cell in occupied {
        let current = grid.get(cell).copied().unwrap_or_default();
        let requested = target.remove(&cell).unwrap_or_default();
        if current != requested {
            updates.push((cell, requested));
        }
    }
    updates.extend(target.into_iter().filter(|(_, material)| *material != 0));
    let changed = updates.len();
    for (cell, material) in updates {
        grid.set(cell, material);
    }
    changed
}

fn normalized_line_speech_rate(rate: f32) -> f32 {
    if rate.is_finite() {
        rate.clamp(0.5, 2.0)
    } else {
        default_line_speech_rate()
    }
}

fn normalized_line_speech_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 2.0)
    } else {
        default_line_speech_volume()
    }
}

fn line_onnx_speed(line: &ReplayDialogue, relative_rate: i32, master_speed: f32) -> f32 {
    combined_onnx_speed(relative_rate, master_speed) * normalized_line_speech_rate(line.speech_rate)
}

fn minimum_speech_window_ms_for_line(
    line: &ReplayDialogue,
    relative_rate: i32,
    master_speed: f32,
) -> u64 {
    let text = speech_text_for_line(line);
    let speed = effective_emotivoice_speed(
        &text,
        line_onnx_speed(line, relative_rate, master_speed),
    ) as f64;
    let padding_ms = if is_short_utterance(&text) {
        SHORT_UTTERANCE_HEAD_PAD_MS.saturating_add(SHORT_UTTERANCE_TAIL_PAD_MS)
    } else {
        0
    };
    ((estimated_speech_duration_ms(&text) as f64 * 1.20 / speed).ceil() as u64)
        .saturating_add(250)
        .saturating_add(padding_ms)
}

fn shifted_replay_time(time_ms: u64, boundary_ms: u64, delta_ms: i64) -> u64 {
    if time_ms == u64::MAX || time_ms < boundary_ms || delta_ms == 0 {
        return time_ms;
    }
    if delta_ms > 0 {
        time_ms.saturating_add(delta_ms as u64)
    } else {
        time_ms.saturating_sub(delta_ms.unsigned_abs())
    }
}

/// Ripple-trims one dialogue cue without rebuilding the replay or returning the
/// playhead to zero. Events at/after the old cue end retain their relative
/// order; events already occurring inside the cue keep their authored timing.
fn set_exact_dialogue_duration(
    replay: &mut ReplayFile,
    dialogue_index: usize,
    requested_duration_ms: u64,
    playback_ms: u64,
) -> Option<u64> {
    let line = replay.dialogue.get(dialogue_index)?;
    if !replay_dialogue_is_playable(line) {
        return None;
    }
    let start_ms = line.time_ms;
    let old_duration_ms = line.duration_ms.max(1);
    let old_end_ms = start_ms.saturating_add(old_duration_ms);
    let duration_ms = requested_duration_ms.clamp(
        MIN_LIVE_DIALOGUE_MS,
        MAX_LIVE_DIALOGUE_MS,
    );
    let delta_ms = duration_ms as i64 - old_duration_ms as i64;

    replay.dialogue[dialogue_index].duration_ms = duration_ms;
    replay.dialogue[dialogue_index].duration_locked = true;
    for (index, line) in replay.dialogue.iter_mut().enumerate() {
        if index != dialogue_index {
            line.time_ms = shifted_replay_time(line.time_ms, old_end_ms, delta_ms);
        }
    }
    for frame in &mut replay.camera {
        frame.time_ms = shifted_replay_time(frame.time_ms, old_end_ms, delta_ms);
    }
    for movement in &mut replay.player_movements {
        for frame in &mut movement.keyframes {
            frame.time_ms = shifted_replay_time(frame.time_ms, old_end_ms, delta_ms);
        }
    }
    for trajectory in &mut replay.ship_trajectories {
        for frame in &mut trajectory.keyframes {
            frame.time_ms = shifted_replay_time(frame.time_ms, old_end_ms, delta_ms);
        }
    }
    for sample in &mut replay.standee_positions {
        sample.time_ms = shifted_replay_time(sample.time_ms, old_end_ms, delta_ms);
    }
    for change in &mut replay.terrain_changes {
        change.time_ms = shifted_replay_time(change.time_ms, old_end_ms, delta_ms);
    }
    for change in &mut replay.ship_hull_changes {
        change.time_ms = shifted_replay_time(change.time_ms, old_end_ms, delta_ms);
    }
    replay.camera.sort_by_key(|frame| frame.time_ms);
    for movement in &mut replay.player_movements {
        movement.keyframes.sort_by_key(|frame| frame.time_ms);
    }
    for trajectory in &mut replay.ship_trajectories {
        trajectory.keyframes.sort_by_key(|frame| frame.time_ms);
    }
    replay
        .standee_positions
        .sort_by_key(|sample| (sample.user_id, sample.time_ms));
    replay.terrain_changes.sort_by_key(|change| change.time_ms);
    replay.ship_hull_changes.sort_by_key(|change| change.time_ms);
    let shifted_duration = shifted_replay_time(replay.duration_ms, old_end_ms, delta_ms)
        .max(start_ms.saturating_add(duration_ms));
    replay.duration_ms = shifted_duration.max(replay_timeline_content_end(replay));

    let updated_playback_ms = if playback_ms >= old_end_ms {
        shifted_replay_time(playback_ms, old_end_ms, delta_ms)
    } else if playback_ms >= start_ms {
        start_ms.saturating_add(
            playback_ms
                .saturating_sub(start_ms)
                .min(duration_ms.saturating_sub(1)),
        )
    } else {
        playback_ms
    };
    Some(updated_playback_ms.min(replay.duration_ms))
}

/// Rebuilding one editor track must never make another authored track
/// unreachable. Keep a minimum viewing window even for an empty project.
fn refresh_replay_duration(replay: &mut ReplayFile, minimum_ms: u64) {
    replay.duration_ms = replay_timeline_content_end(replay).max(minimum_ms);
}

fn recompile_edited_replay_dialogue(replay: &mut ReplayFile) {
    let dialogue_end = compile_area_block_timeline(replay);
    refresh_replay_duration(replay, dialogue_end);
}

fn replay_timeline_content_end(replay: &ReplayFile) -> u64 {
    replay
        .dialogue
        .iter()
        .filter(|line| replay_dialogue_is_playable(line))
        .map(|line| line.time_ms.saturating_add(line.duration_ms))
        .chain(replay.camera.iter().map(|frame| frame.time_ms))
        .chain(
            replay
                .player_movements
                .iter()
                .flat_map(|movement| movement.keyframes.iter().map(|frame| frame.time_ms)),
        )
        .chain(
            replay
                .ship_trajectories
                .iter()
                .flat_map(|trajectory| trajectory.keyframes.iter().map(|frame| frame.time_ms)),
        )
        .chain(replay.standee_positions.iter().map(|sample| sample.time_ms))
        .chain(replay.terrain_changes.iter().map(|change| change.time_ms))
        .chain(replay.ship_hull_changes.iter().map(|change| change.time_ms))
        .max()
        .unwrap_or_default()
}

fn seed_replay_standee_track(replay: &mut ReplayFile, user_id: u64) {
    // World-position samples take precedence during playback. When editing a
    // history-only track, seed its existing frames before adding that override;
    // otherwise a single new frame would hide all earlier and later movement.
    if !replay
        .standee_positions
        .iter()
        .any(|sample| sample.user_id == user_id)
    {
        replay.standee_positions.extend(
            replay
                .player_movements
                .iter()
                .filter(|movement| movement.user_id == user_id)
                .flat_map(|movement| movement.keyframes.iter())
                .map(|frame| ReplayStandeePosition {
                    time_ms: frame.time_ms,
                    user_id,
                    position: (Vec3::from_array(frame.position_cells) * VOXEL_SIZE).to_array(),
                }),
        );
    }
}

fn replace_replay_movement_segment(
    replay: &mut ReplayFile,
    user_id: u64,
    start_ms: u64,
    end_ms: u64,
    samples: &[ReplayStandeePosition],
) -> usize {
    if samples.is_empty() {
        return 0;
    }
    replay.authored_player_movements.insert(user_id);
    seed_replay_standee_track(replay, user_id);
    replay.standee_positions.retain(|sample| {
        sample.user_id != user_id || sample.time_ms < start_ms || sample.time_ms > end_ms
    });
    replay.standee_positions.extend(samples.iter().copied());
    replay
        .standee_positions
        .sort_by_key(|sample| (sample.user_id, sample.time_ms));
    replay
        .standee_positions
        .dedup_by_key(|sample| (sample.user_id, sample.time_ms));

    let mut keyframes = replay
        .player_movements
        .iter()
        .filter(|movement| movement.user_id == user_id)
        .flat_map(|movement| movement.keyframes.iter().copied())
        .filter(|frame| frame.time_ms < start_ms || frame.time_ms > end_ms)
        .collect::<Vec<_>>();
    keyframes.extend(
        samples.iter().map(|sample| ReplayPlayerMovementKeyframe {
            time_ms: sample.time_ms,
            position_cells: (Vec3::from_array(sample.position) / VOXEL_SIZE).to_array(),
        }),
    );
    keyframes.sort_by_key(|frame| frame.time_ms);
    keyframes.dedup_by_key(|frame| frame.time_ms);
    replay
        .player_movements
        .retain(|movement| movement.user_id != user_id);
    replay
        .player_movements
        .push(ReplayPlayerMovement { user_id, keyframes });
    replay.duration_ms = replay.duration_ms.max(replay_timeline_content_end(replay));
    samples.len()
}

fn stop_replay_movement_at(replay: &mut ReplayFile, user_id: u64, time_ms: u64, position: Vec3) {
    replay.authored_player_movements.insert(user_id);
    seed_replay_standee_track(replay, user_id);
    replay
        .standee_positions
        .retain(|sample| sample.user_id != user_id || sample.time_ms < time_ms);
    replay.standee_positions.push(ReplayStandeePosition {
        time_ms,
        user_id,
        position: position.to_array(),
    });
    replay
        .standee_positions
        .sort_by_key(|sample| (sample.user_id, sample.time_ms));

    let mut keyframes = replay
        .player_movements
        .iter()
        .filter(|movement| movement.user_id == user_id)
        .flat_map(|movement| movement.keyframes.iter().copied())
        .filter(|frame| frame.time_ms < time_ms)
        .collect::<Vec<_>>();
    keyframes.push(ReplayPlayerMovementKeyframe {
        time_ms,
        position_cells: (position / VOXEL_SIZE).to_array(),
    });
    keyframes.sort_by_key(|frame| frame.time_ms);
    keyframes.dedup_by_key(|frame| frame.time_ms);
    replay
        .player_movements
        .retain(|movement| movement.user_id != user_id);
    replay
        .player_movements
        .push(ReplayPlayerMovement { user_id, keyframes });
}

fn replace_replay_ship_segment(
    replay: &mut ReplayFile,
    ship_id: &str,
    ship_name: &str,
    start_ms: u64,
    end_ms: u64,
    samples: &[ReplayShipKeyframe],
) -> usize {
    if samples.is_empty() {
        return 0;
    }
    replay.authored_ship_trajectories.insert(ship_id.to_owned());
    let existing_name = replay
        .ship_trajectories
        .iter()
        .find(|trajectory| trajectory.ship_id == ship_id)
        .map(|trajectory| trajectory.ship_name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| ship_name.to_owned());
    let mut keyframes = replay
        .ship_trajectories
        .iter()
        .filter(|trajectory| trajectory.ship_id == ship_id)
        .flat_map(|trajectory| trajectory.keyframes.iter().copied())
        .filter(|frame| frame.time_ms < start_ms || frame.time_ms > end_ms)
        .collect::<Vec<_>>();
    keyframes.extend_from_slice(samples);
    keyframes.sort_by_key(|frame| frame.time_ms);
    keyframes.dedup_by_key(|frame| frame.time_ms);
    replay
        .ship_trajectories
        .retain(|trajectory| trajectory.ship_id != ship_id);
    replay.ship_trajectories.push(ReplayShipTrajectory {
        ship_id: ship_id.to_owned(),
        ship_name: existing_name,
        keyframes,
    });
    replay.duration_ms = replay.duration_ms.max(replay_timeline_content_end(replay));
    samples.len()
}

fn stop_replay_ship_at(
    replay: &mut ReplayFile,
    ship_id: &str,
    ship_name: &str,
    time_ms: u64,
    transform: Transform,
) {
    replay.authored_ship_trajectories.insert(ship_id.to_owned());
    let existing_name = replay
        .ship_trajectories
        .iter()
        .find(|trajectory| trajectory.ship_id == ship_id)
        .map(|trajectory| trajectory.ship_name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| ship_name.to_owned());
    let mut keyframes = replay
        .ship_trajectories
        .iter()
        .filter(|trajectory| trajectory.ship_id == ship_id)
        .flat_map(|trajectory| trajectory.keyframes.iter().copied())
        .filter(|frame| frame.time_ms < time_ms)
        .collect::<Vec<_>>();
    push_live_ship_sample(&mut keyframes, time_ms, transform);
    keyframes.sort_by_key(|frame| frame.time_ms);
    keyframes.dedup_by_key(|frame| frame.time_ms);
    replay
        .ship_trajectories
        .retain(|trajectory| trajectory.ship_id != ship_id);
    replay.ship_trajectories.push(ReplayShipTrajectory {
        ship_id: ship_id.to_owned(),
        ship_name: existing_name,
        keyframes,
    });
}

fn replay_has_live_take(studio: &ReplayStudio) -> bool {
    studio.live_movement_punch_in.is_some() || studio.live_ship_punch_in.is_some()
}

fn cancel_live_replay_take(studio: &mut ReplayStudio) {
    let original_duration_ms = studio
        .live_movement_punch_in
        .take()
        .map(|take| take.original_duration_ms)
        .or_else(|| {
            studio
                .live_ship_punch_in
                .take()
                .map(|take| take.original_duration_ms)
        });
    let Some(original_duration_ms) = original_duration_ms else { return };
    if let Some(replay) = studio.replay.as_mut() {
        // Terrain edits are recorded separately and remain authored content even
        // when the movement take is discarded.
        refresh_replay_duration(replay, original_duration_ms);
        studio.playback_ms = studio.playback_ms.min(replay.duration_ms);
    }
    studio.mode = ReplayMode::Paused;
    studio.speech_wait_cue = None;
    studio.speech_wait_elapsed_seconds = 0.0;
    studio.status = "已取消本次移动录制；原轨迹保持不变，独立场景编辑仍保留".to_owned();
}

fn begin_live_movement_take(studio: &mut ReplayStudio, user_id: u64, position: Vec3) {
    if replay_has_live_take(studio) || studio.replay.is_none() {
        return;
    }
    let original_duration_ms = studio.replay.as_ref().expect("checked above").duration_ms;
    let playback_ms = studio.playback_ms;
    let mut samples = Vec::new();
    push_live_standee_sample(
        &mut samples,
        playback_ms,
        user_id,
        position,
    );
    studio.live_ship_punch_in = None;
    studio.live_movement_punch_in = Some(ReplayMovementPunchIn {
        user_id,
        start_ms: playback_ms,
        original_duration_ms,
        last_sample_ms: Some(playback_ms),
        samples,
    });
    studio.live_editing_enabled = true;
    studio.mode = ReplayMode::Playing;
    studio.status = format!(
        "正在从 {} 覆录角色 {user_id} 的移动；移动角色后点击“保存本次录制”",
        format_time(playback_ms),
    );
}

fn finish_live_movement_take(studio: &mut ReplayStudio, final_position: Option<Vec3>) -> usize {
    let Some(mut punch_in) = studio.live_movement_punch_in.take() else {
        return 0;
    };
    let end_ms = studio.playback_ms.max(punch_in.start_ms);
    if let Some(position) = final_position {
        push_live_standee_sample(
            &mut punch_in.samples,
            end_ms,
            punch_in.user_id,
            position,
        );
    }
    punch_in
        .samples
        .retain(|sample| (punch_in.start_ms..=end_ms).contains(&sample.time_ms));
    let count = studio
        .replay
        .as_mut()
        .map(|replay| {
            replace_replay_movement_segment(
                replay,
                punch_in.user_id,
                punch_in.start_ms,
                end_ms,
                &punch_in.samples,
            )
        })
        .unwrap_or_default();
    if count > 0 {
        studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
        studio.status = format!(
            "已保存角色 {} 在 {}–{} 的 {count} 帧移动覆录",
            punch_in.user_id,
            format_time(punch_in.start_ms),
            format_time(end_ms),
        );
    }
    count
}

fn begin_live_ship_take(
    studio: &mut ReplayStudio,
    ship_id: String,
    ship_name: String,
    transform: Transform,
) {
    if replay_has_live_take(studio) || studio.replay.is_none() {
        return;
    }
    let original_duration_ms = studio.replay.as_ref().expect("checked above").duration_ms;
    let playback_ms = studio.playback_ms;
    let mut samples = Vec::new();
    push_live_ship_sample(&mut samples, playback_ms, transform);
    studio.live_movement_punch_in = None;
    studio.live_ship_punch_in = Some(ReplayShipPunchIn {
        ship_id: ship_id.clone(),
        ship_name,
        start_ms: playback_ms,
        original_duration_ms,
        last_sample_ms: Some(playback_ms),
        samples,
    });
    studio.live_editing_enabled = true;
    studio.mode = ReplayMode::Playing;
    studio.status = format!(
        "正在从 {} 覆录飞船 {ship_id}；驾驶后点击“保存本次录制”",
        format_time(playback_ms),
    );
}

fn finish_live_ship_take(studio: &mut ReplayStudio, final_transform: Option<Transform>) -> usize {
    let Some(mut punch_in) = studio.live_ship_punch_in.take() else {
        return 0;
    };
    let end_ms = studio.playback_ms.max(punch_in.start_ms);
    if let Some(transform) = final_transform {
        push_live_ship_sample(&mut punch_in.samples, end_ms, transform);
    }
    punch_in
        .samples
        .retain(|sample| (punch_in.start_ms..=end_ms).contains(&sample.time_ms));
    let count = studio
        .replay
        .as_mut()
        .map(|replay| {
            replace_replay_ship_segment(
                replay,
                &punch_in.ship_id,
                &punch_in.ship_name,
                punch_in.start_ms,
                end_ms,
                &punch_in.samples,
            )
        })
        .unwrap_or_default();
    if count > 0 {
        studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
        studio.status = format!(
            "已保存飞船 {} 在 {}–{} 的 {count} 帧移动覆录",
            punch_in.ship_name,
            format_time(punch_in.start_ms),
            format_time(end_ms),
        );
    }
    count
}

#[derive(Debug, Clone, Copy)]
struct ReplaySceneEventSummary {
    time_ms: u64,
    terrain_cells: usize,
    hull_cells: usize,
    enabled: bool,
}

fn replay_scene_event_summaries(
    replay: &ReplayFile,
    playback_ms: u64,
) -> Vec<ReplaySceneEventSummary> {
    let mut times = replay
        .terrain_changes
        .iter()
        .map(|change| change.time_ms)
        .chain(replay.ship_hull_changes.iter().map(|change| change.time_ms))
        .collect::<Vec<_>>();
    times.sort_unstable();
    times.dedup();
    times.sort_by_key(|time_ms| time_ms.abs_diff(playback_ms));
    times.truncate(12);
    times.sort_unstable();
    times
        .into_iter()
        .map(|time_ms| {
            let terrain = replay
                .terrain_changes
                .iter()
                .filter(|change| change.time_ms == time_ms)
                .collect::<Vec<_>>();
            let hull = replay
                .ship_hull_changes
                .iter()
                .filter(|change| change.time_ms == time_ms)
                .collect::<Vec<_>>();
            ReplaySceneEventSummary {
                time_ms,
                terrain_cells: terrain.len(),
                hull_cells: hull.len(),
                enabled: terrain.iter().all(|change| change.enabled)
                    && hull.iter().all(|change| change.enabled),
            }
        })
        .collect()
}

fn camera_keyframe(time_ms: u64, transform: &Transform) -> ReplayCameraKeyframe {
    ReplayCameraKeyframe {
        time_ms,
        translation: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
    }
}

fn assign_replay_line_ids(dialogue: &mut [ReplayDialogue]) {
    let mut next_id = dialogue
        .iter()
        .map(|line| line.line_id)
        .max()
        .unwrap_or_default()
        .saturating_add(1)
        .max(1);
    let mut seen = HashSet::new();
    for line in dialogue {
        if line.line_id == 0 || !seen.insert(line.line_id) {
            line.line_id = next_id;
            seen.insert(next_id);
            next_id = next_id.saturating_add(1);
        }
    }
}

#[derive(Default)]
struct ReplayStandeePlaybackState {
    active: bool,
    replay_key: Option<u64>,
    timeline_revision: u64,
    track: HashMap<u64, Vec<ReplayStandeePosition>>,
    original_positions: HashMap<u64, Vec3>,
    original_rotations: HashMap<u64, Quat>,
}

fn apply_replay_standee_positions(
    studio: Res<ReplayStudio>,
    mut standees: Query<
        (&mut Transform, &VoxelPlayerStandee),
        (
            With<VoxelPlayerStandee>,
            Without<VoxelViewportCamera>,
        ),
    >,
    spaceships: Query<(&VoxelSpaceship, &VoxelPhysicsBody), Without<VoxelViewportCamera>>,
    mut state: Local<ReplayStandeePlaybackState>,
) {
    let active = matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    ) || studio.video_render.is_some();
    if active && !state.active {
        state.original_positions = standees
            .iter()
            .map(|(transform, standee)| (standee.user_id, transform.translation))
            .collect();
        state.original_rotations = standees
            .iter()
            .map(|(transform, standee)| (standee.user_id, transform.rotation))
            .collect();
    }

    if active {
        if let Some(replay) = studio.replay.as_ref() {
            if state.replay_key != Some(replay.created_at_unix_ms)
                || state.timeline_revision != studio.timeline_revision
            {
                state.replay_key = Some(replay.created_at_unix_ms);
                state.timeline_revision = studio.timeline_revision;
                let mut track = HashMap::<u64, Vec<ReplayStandeePosition>>::new();
                for sample in &replay.standee_positions {
                    track.entry(sample.user_id).or_default().push(*sample);
                }
                state.track = track;
            }
            let camera_position = interpolated_camera(
                &replay.camera,
                studio.playback_ms,
                replay.camera_transition_curve,
            )
            .map(|transform| transform.translation);
            let ship_bounds = spaceships
                .iter()
                .filter_map(|(ship, body)| {
                    let min = body
                        .cells
                        .iter()
                        .map(|(cell, _)| *cell)
                        .reduce(IVec3::min)?;
                    let max = body
                        .cells
                        .iter()
                        .map(|(cell, _)| *cell)
                        .reduce(IVec3::max)?;
                    Some((
                        ship.id.clone(),
                        (
                            min.as_vec3() * VOXEL_SIZE,
                            (max + IVec3::ONE).as_vec3() * VOXEL_SIZE,
                        ),
                    ))
                })
                .collect::<HashMap<_, _>>();
            let mut first_positions = HashMap::<u64, Vec3>::new();
            let positions = replay
                .dialogue
                .iter()
                .filter(|line| {
                    replay_dialogue_is_playable(line) && replay_dialogue_has_saved_position(line)
                })
                .fold(
                    HashMap::new(),
                    |mut positions: HashMap<u64, (Vec3, u64)>, line| {
                        let position =
                            IVec3::from_array(line.position_cells).as_vec3() * VOXEL_SIZE;
                        first_positions.entry(line.sender_id).or_insert(position);
                        if line.time_ms <= studio.playback_ms {
                            positions.insert(line.sender_id, (position, line.time_ms));
                        }
                        positions
                    },
                );
            for (mut transform, standee) in &mut standees {
                if studio
                    .live_movement_punch_in
                    .as_ref()
                    .is_some_and(|punch_in| punch_in.user_id == standee.user_id)
                {
                    continue;
                }
                let tracked_position = state
                    .track
                    .get(&standee.user_id)
                    .and_then(|samples| interpolated_standee_position(samples, studio.playback_ms));
                let dialogue_position = positions.get(&standee.user_id).copied().or_else(|| {
                    first_positions
                        .get(&standee.user_id)
                        .copied()
                        .map(|position| (position, 0))
                });
                let resolved = if let Some(position) = tracked_position {
                    Some((position, studio.playback_ms))
                } else if let Some(position) = interpolated_player_position(
                    &replay.player_movements,
                    standee.user_id,
                    studio.playback_ms,
                    replay.player_movement_curve,
                ) {
                    // Movement keyframes recorded live already follow a moving
                    // ship; once they go stale (the player stopped being
                    // moved), the ship carry takes over from the last frame.
                    let last_frame_time = replay
                        .player_movements
                        .iter()
                        .find(|movement| movement.user_id == standee.user_id)
                        .and_then(|movement| movement.keyframes.last())
                        .map(|frame| frame.time_ms)
                        .unwrap_or(studio.playback_ms);
                    Some((
                        position,
                        last_frame_time.min(studio.playback_ms),
                    ))
                } else if let Some((position, source_time)) = dialogue_position {
                    Some((position, source_time))
                } else {
                    None
                };
                if let Some((position, source_time)) = resolved {
                    if let Some(carried) = replay_carried_standee_position(
                        replay,
                        &ship_bounds,
                        position,
                        source_time,
                        studio.playback_ms,
                    ) {
                        transform.translation = carried;
                    } else {
                        transform.translation = position;
                    }
                }
                if let Some(rotation) = camera_position.and_then(|camera_position| {
                    replay_standee_facing_rotation(transform.translation, camera_position)
                }) {
                    transform.rotation = rotation;
                }
            }
        }
    } else if state.active {
        for (mut transform, standee) in &mut standees {
            if let Some(position) = state.original_positions.get(&standee.user_id) {
                transform.translation = *position;
            }
            if let Some(rotation) = state.original_rotations.get(&standee.user_id) {
                transform.rotation = *rotation;
            }
        }
        state.original_positions.clear();
        state.original_rotations.clear();
    }
    state.active = active;
}

fn replay_standee_facing_rotation(standee_position: Vec3, camera_position: Vec3) -> Option<Quat> {
    let direction = (camera_position - standee_position) * Vec3::new(1.0, 0.0, 1.0);
    let direction = direction.try_normalize()?;
    Some(Quat::from_rotation_y(
        (-direction.x).atan2(-direction.z),
    ))
}

/// Carries a standee whose recorded dialogue position was inside a moving
/// ship: the ship-local offset at the recorded time is applied to the ship's
/// current replay pose, so the person travels with the vessel exactly like the
/// live "carry players with spaceships" behavior.
fn replay_carried_standee_position(
    replay: &ReplayFile,
    ship_bounds: &HashMap<String, (Vec3, Vec3)>,
    recorded_position: Vec3,
    recorded_time_ms: u64,
    playback_ms: u64,
) -> Option<Vec3> {
    for trajectory in &replay.ship_trajectories {
        let (local_min, local_max) = ship_bounds.get(&trajectory.ship_id)?;
        let (source_translation, source_rotation) =
            interpolated_ship_pose(trajectory, recorded_time_ms)?;
        let source_affine = Transform {
            translation: source_translation,
            rotation: source_rotation,
            scale: Vec3::ONE,
        }
        .compute_affine();
        let local = source_affine.inverse().transform_point3(recorded_position);
        let inside = local.cmpge(*local_min).all() && local.cmple(*local_max).all();
        if !inside {
            continue;
        }
        let (current_translation, current_rotation) =
            interpolated_ship_pose(trajectory, playback_ms)?;
        let current_affine = Transform {
            translation: current_translation,
            rotation: current_rotation,
            scale: Vec3::ONE,
        }
        .compute_affine();
        return Some(current_affine.transform_point3(local));
    }
    None
}

pub(crate) fn replay_scene_dynamics_active(studio: &ReplayStudio) -> bool {
    matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    ) || studio.video_render.is_some()
}

#[derive(Default)]
struct ReplayTerrainPlaybackState {
    replay_key: Option<u64>,
    scene_revision: u64,
    baseline: HashMap<IVec3, u8>,
    applied: HashMap<IVec3, u8>,
    next_change: usize,
    covered_time_ms: u64,
}

/// Applies recorded grid terrain changes to the live voxel grid as playback
/// time advances. Rewinding (timeline drag or turn jump) rebuilds the target
/// state from the scene baseline so cells can be restored deterministically.
fn apply_replay_terrain_changes(
    mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>,
    mut dirty_chunks: ResMut<VoxelGeometryDirtyChunks>,
    studio: Res<ReplayStudio>,
    mut state: Local<ReplayTerrainPlaybackState>,
) {
    if !replay_scene_dynamics_active(&studio) {
        if state.replay_key.is_some() {
            *state = ReplayTerrainPlaybackState::default();
        }
        return;
    }
    let Some(replay) = studio.replay.as_ref() else {
        return;
    };
    let Ok(mut grid) = grids.single_mut() else {
        return;
    };
    let replay_key = replay.created_at_unix_ms;
    if state.replay_key != Some(replay_key) {
        state.replay_key = Some(replay_key);
        state.scene_revision = studio.scene_revision;
        state.baseline = replay
            .scene
            .voxels
            .iter()
            .map(|voxel| {
                (
                    IVec3::from_array(voxel.position),
                    voxel.material,
                )
            })
            .collect();
        state.applied = state.baseline.clone();
        state.next_change = 0;
        state.covered_time_ms = 0;
    } else if state.scene_revision != studio.scene_revision {
        state.scene_revision = studio.scene_revision;
        // Force the deterministic rewind path so disabling or inserting an
        // event at/before the current playhead is visible on the next frame.
        state.covered_time_ms = u64::MAX;
    }
    apply_terrain_changes_at(
        &mut grid,
        &mut dirty_chunks,
        &mut state,
        &replay.terrain_changes,
        studio.playback_ms,
    );
}

fn apply_terrain_changes_at(
    grid: &mut Mut<Grid<u8>>,
    dirty_chunks: &mut VoxelGeometryDirtyChunks,
    state: &mut ReplayTerrainPlaybackState,
    changes: &[ReplayTerrainChange],
    time_ms: u64,
) {
    if time_ms < state.covered_time_ms {
        let mut target = state.baseline.clone();
        let mut count = 0;
        for change in changes {
            if change.time_ms > time_ms {
                break;
            }
            if change.enabled {
                target.insert(
                    IVec3::from_array(change.position),
                    change.material,
                );
            }
            count += 1;
        }
        state.next_change = count;
        let affected_cells = state
            .applied
            .keys()
            .copied()
            .chain(target.keys().copied())
            .chain(
                changes[..count]
                    .iter()
                    .map(|change| IVec3::from_array(change.position)),
            )
            .collect::<HashSet<_>>();
        apply_terrain_target_diff(
            grid,
            dirty_chunks,
            &target,
            &affected_cells,
        );
        state.applied = target;
        state.covered_time_ms = time_ms;
        return;
    }
    while let Some(change) = changes.get(state.next_change) {
        if change.time_ms > time_ms {
            break;
        }
        if change.enabled {
            let cell = IVec3::from_array(change.position);
            grid.set(cell, change.material);
            dirty_chunks.mark_cell_and_neighbors(cell);
            state.applied.insert(cell, change.material);
        }
        state.next_change += 1;
    }
    state.covered_time_ms = state.covered_time_ms.max(time_ms);
}

fn apply_terrain_target_diff(
    grid: &mut Mut<Grid<u8>>,
    dirty_chunks: &mut VoxelGeometryDirtyChunks,
    target: &HashMap<IVec3, u8>,
    affected_cells: &HashSet<IVec3>,
) {
    // A live viewport tool mutates the real grid before its replay event is
    // captured. Compare against that real grid, not only our cached replay
    // state, so immediately disabling a just-created event restores the cell.
    for &cell in affected_cells {
        let material = target.get(&cell).copied().unwrap_or_default();
        if grid.get(cell).copied().unwrap_or_default() != material {
            grid.set(cell, material);
            dirty_chunks.mark_cell_and_neighbors(cell);
        }
    }
}

#[derive(Default)]
struct ReplayShipHullPlaybackState {
    replay_key: Option<u64>,
    scene_revision: u64,
    original_cells: HashMap<Entity, Vec<(IVec3, u8)>>,
    applied: HashMap<Entity, HashMap<IVec3, u8>>,
    next_change: usize,
    covered_time_ms: u64,
    active: bool,
}

/// Applies recorded ship hull cell changes (e.g. explosions blasting parts of
/// a dynamic voxel ship away) to the live occupancy cache, so the chunked hull
/// meshes are rebuilt exactly like the original session. On stop, the original
/// hull cells are restored.
fn apply_replay_ship_hull_changes(
    mut commands: Commands,
    mut occupancy: ResMut<VoxelSpaceshipOccupancyCache>,
    ships: Query<(
        Entity,
        &VoxelSpaceship,
        &VoxelPhysicsBody,
    )>,
    studio: Res<ReplayStudio>,
    mut state: Local<ReplayShipHullPlaybackState>,
) {
    let active = replay_scene_dynamics_active(&studio);
    if !active {
        if state.active {
            for (entity, cells) in &state.original_cells {
                occupancy.replace_ship_cells(*entity, cells);
                if let Ok(mut entity_commands) = commands.get_entity(*entity) {
                    entity_commands.insert(VoxelSpaceshipNeedsRebuild);
                }
            }
            *state = ReplayShipHullPlaybackState::default();
        }
        return;
    }
    let Some(replay) = studio.replay.as_ref() else {
        return;
    };
    let replay_key = replay.created_at_unix_ms;
    if state.replay_key != Some(replay_key) {
        state.replay_key = Some(replay_key);
        state.scene_revision = studio.scene_revision;
        state.original_cells = ships
            .iter()
            .map(|(entity, _, body)| (entity, body.cells.clone()))
            .collect();
        state.applied = state
            .original_cells
            .iter()
            .map(|(entity, cells)| (*entity, cells.iter().copied().collect()))
            .collect();
        state.next_change = 0;
        state.covered_time_ms = 0;
    } else if state.scene_revision != studio.scene_revision {
        state.scene_revision = studio.scene_revision;
        state.covered_time_ms = u64::MAX;
    }
    state.active = true;
    let mut id_to_entity = HashMap::new();
    for (entity, ship, _) in &ships {
        id_to_entity.insert(ship.id.clone(), entity);
    }
    apply_ship_hull_changes_at(
        &mut commands,
        &mut occupancy,
        &mut state,
        &id_to_entity,
        &replay.ship_hull_changes,
        studio.playback_ms,
    );
}

fn apply_ship_hull_changes_at(
    commands: &mut Commands,
    occupancy: &mut VoxelSpaceshipOccupancyCache,
    state: &mut ReplayShipHullPlaybackState,
    id_to_entity: &HashMap<String, Entity>,
    changes: &[ReplayShipHullChange],
    time_ms: u64,
) {
    if time_ms < state.covered_time_ms {
        let mut count = 0;
        for change in changes {
            if change.time_ms > time_ms {
                break;
            }
            count += 1;
        }
        state.next_change = count;
        for (entity, original) in &state.original_cells {
            let mut target = original.iter().copied().collect::<HashMap<_, _>>();
            for change in &changes[..count] {
                if change.enabled && id_to_entity.get(&change.ship_id) == Some(entity) {
                    set_replay_ship_hull_cell(
                        &mut target,
                        IVec3::from_array(change.position),
                        change.material,
                    );
                }
            }
            let actual = occupancy
                .ships
                .get(entity)
                .map(|entry| {
                    entry
                        .chunks
                        .values()
                        .flat_map(|chunk| chunk.iter().map(|(cell, material)| (*cell, *material)))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            if actual != target {
                let cells = target
                    .iter()
                    .map(|(cell, material)| (*cell, *material))
                    .collect::<Vec<_>>();
                occupancy.replace_ship_cells(*entity, &cells);
                if let Ok(mut entity_commands) = commands.get_entity(*entity) {
                    entity_commands.insert(VoxelSpaceshipNeedsRebuild);
                }
            }
            state.applied.insert(*entity, target);
        }
        state.covered_time_ms = time_ms;
        return;
    }

    let mut changed_ships = HashSet::new();
    while let Some(change) = changes.get(state.next_change) {
        if change.time_ms > time_ms {
            break;
        }
        if change.enabled {
            if let Some(&entity) = id_to_entity.get(&change.ship_id) {
                set_replay_ship_hull_cell(
                    state.applied.entry(entity).or_default(),
                    IVec3::from_array(change.position),
                    change.material,
                );
                changed_ships.insert(entity);
            }
        }
        state.next_change += 1;
    }
    for entity in changed_ships {
        let target = state.applied.get(&entity).cloned().unwrap_or_default();
        let cells = target
            .iter()
            .map(|(cell, material)| (*cell, *material))
            .collect::<Vec<_>>();
        occupancy.replace_ship_cells(entity, &cells);
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.insert(VoxelSpaceshipNeedsRebuild);
        }
    }
    state.covered_time_ms = state.covered_time_ms.max(time_ms);
}

fn set_replay_ship_hull_cell(
    cells: &mut HashMap<IVec3, u8>,
    cell: IVec3,
    material: u8,
) {
    if material == 0 {
        cells.remove(&cell);
    } else {
        cells.insert(cell, material);
    }
}

#[derive(Default)]
struct ReplayShipPlaybackState {
    active: bool,
    original: HashMap<
        String,
        (
            Transform,
            LinearVelocity,
            AngularVelocity,
        ),
    >,
}

/// Drives the recorded ship trajectories during playback: each ship's world
/// transform is interpolated from its keyframes and physics velocities are
/// zeroed so the deterministic track wins over live simulation. Original poses
/// are restored when playback stops.
fn apply_replay_ship_positions(
    studio: Res<ReplayStudio>,
    mut ships: Query<
        (
            &VoxelSpaceship,
            &mut Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        Without<VoxelSpaceshipDocked>,
    >,
    mut state: Local<ReplayShipPlaybackState>,
) {
    let active = replay_scene_dynamics_active(&studio);
    if active && !state.active {
        state.original = ships
            .iter()
            .map(|(ship, transform, linear, angular)| {
                (
                    ship.id.clone(),
                    (*transform, *linear, *angular),
                )
            })
            .collect();
    }
    if active {
        if let Some(replay) = studio.replay.as_ref() {
            for (ship, mut transform, mut linear, mut angular) in &mut ships {
                if studio
                    .live_ship_punch_in
                    .as_ref()
                    .is_some_and(|punch_in| punch_in.ship_id == ship.id)
                {
                    continue;
                }
                if let Some(trajectory) = replay
                    .ship_trajectories
                    .iter()
                    .find(|trajectory| trajectory.ship_id == ship.id)
                {
                    if let Some((translation, rotation)) =
                        interpolated_ship_pose(trajectory, studio.playback_ms)
                    {
                        transform.translation = translation;
                        transform.rotation = rotation;
                        linear.0 = Vec3::ZERO;
                        angular.0 = Vec3::ZERO;
                    }
                }
            }
        }
    } else if state.active {
        for (ship, mut transform, mut linear, mut angular) in &mut ships {
            if let Some((original_transform, original_linear, original_angular)) =
                state.original.get(&ship.id)
            {
                *transform = *original_transform;
                *linear = *original_linear;
                *angular = *original_angular;
            }
        }
        state.original.clear();
    }
    state.active = active;
}

fn replay_uses_legacy_chronological_layout(replay: &ReplayFile) -> bool {
    !replay.dialogue.is_empty()
        && replay
            .dialogue
            .iter()
            .all(|line| !line.snapshot_recorded && line.area == "旧时间线")
        && replay.area_blocks.len() == 1
        && replay.area_blocks[0].area == "旧时间线"
}

fn auto_group_replay_areas(replay: &mut ReplayFile) {
    if replay_uses_legacy_chronological_layout(replay) {
        return;
    }
    let radius_squared = replay.area_radius_cells.max(1).pow(2) as i64;
    let mut remaining = replay
        .dialogue
        .iter()
        .enumerate()
        .filter(|(_, line)| line.snapshot_recorded)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    remaining.sort_by_key(|index| {
        let line = &replay.dialogue[*index];
        (line.source_time, line.line_id)
    });

    let mut area_number = 1_u32;
    while let Some(seed) = remaining.first().copied() {
        remaining.remove(0);
        let mut component = vec![seed];
        let mut cursor = 0;
        while cursor < component.len() {
            let origin = replay.dialogue[component[cursor]].position_cells;
            let mut index = 0;
            while index < remaining.len() {
                let candidate = replay.dialogue[remaining[index]].position_cells;
                let dx = i64::from(origin[0]) - i64::from(candidate[0]);
                let dz = i64::from(origin[2]) - i64::from(candidate[2]);
                if dx * dx + dz * dz <= radius_squared {
                    component.push(remaining.remove(index));
                } else {
                    index += 1;
                }
            }
            cursor += 1;
        }
        let area = format!("区域 {area_number}");
        for index in component {
            replay.dialogue[index].area = area.clone();
        }
        area_number = area_number.saturating_add(1);
    }
}

fn rebuild_area_blocks(replay: &mut ReplayFile) {
    if replay_uses_legacy_chronological_layout(replay) {
        return;
    }
    let mut areas = HashMap::<String, Vec<&ReplayDialogue>>::new();
    for line in replay.dialogue.iter().filter(|line| line.snapshot_recorded) {
        areas.entry(line.area.clone()).or_default().push(line);
    }
    let mut areas = areas.into_iter().collect::<Vec<_>>();
    areas.sort_by_key(|(area, lines)| {
        (
            lines
                .iter()
                .map(|line| line.source_time)
                .min()
                .unwrap_or(u64::MAX),
            area.clone(),
        )
    });

    let mut blocks = Vec::new();
    let mut next_block_id = 1_u64;
    for (area, mut lines) in areas {
        lines.sort_by_key(|line| {
            (
                line.turn_index,
                line.source_time,
                line.line_id,
            )
        });
        let mut current_turns = HashSet::new();
        let mut current_ids = Vec::new();
        for line in lines {
            if !current_turns.contains(&line.turn_index)
                && current_turns.len() >= AREA_BLOCK_TURN_LIMIT
            {
                blocks.push(ReplayAreaBlock {
                    id: next_block_id,
                    area: area.clone(),
                    line_ids: std::mem::take(&mut current_ids),
                });
                next_block_id = next_block_id.saturating_add(1);
                current_turns.clear();
            }
            current_turns.insert(line.turn_index);
            current_ids.push(line.line_id);
        }
        if !current_ids.is_empty() {
            blocks.push(ReplayAreaBlock {
                id: next_block_id,
                area,
                line_ids: current_ids,
            });
            next_block_id = next_block_id.saturating_add(1);
        }
    }
    let line_order = replay
        .dialogue
        .iter()
        .map(|line| {
            (
                line.line_id,
                (line.source_time, line.line_id),
            )
        })
        .collect::<HashMap<_, _>>();
    blocks.sort_by_key(|block| {
        block
            .line_ids
            .iter()
            .filter_map(|line_id| line_order.get(line_id).copied())
            .min()
            .unwrap_or((u64::MAX, u64::MAX))
    });
    for (index, block) in blocks.iter_mut().enumerate() {
        block.id = index as u64 + 1;
    }
    replay.area_blocks = blocks;
}

fn compile_area_block_timeline(replay: &mut ReplayFile) -> u64 {
    let order = replay
        .area_blocks
        .iter()
        .flat_map(|block| block.line_ids.iter().copied())
        .enumerate()
        .map(|(index, line_id)| (line_id, index))
        .collect::<HashMap<_, _>>();
    let mut gm_boundaries = replay
        .dialogue
        .iter()
        .filter(|line| {
            !replay.manual_dialogue_order
                && line.included
                && line.side == DialogueSide::Left
                && order.contains_key(&line.line_id)
        })
        .map(|line| (line.source_time, line.line_id))
        .collect::<Vec<_>>();
    gm_boundaries.sort_unstable();
    replay.dialogue.sort_by_key(|line| {
        // Automatic area order rearranges players only between GM lines. An
        // explicit DM drag disables those boundaries and follows the edited
        // block order, while retaining the original source timestamps and IDs.
        let source_order = (line.source_time, line.line_id);
        let gm_epoch = gm_boundaries.partition_point(|boundary| *boundary < source_order);
        let is_gm_boundary = line.side == DialogueSide::Left
            && gm_boundaries
                .get(gm_epoch)
                .is_some_and(|boundary| *boundary == source_order);
        (
            !line.included,
            !order.contains_key(&line.line_id),
            gm_epoch,
            is_gm_boundary,
            order.get(&line.line_id).copied().unwrap_or(usize::MAX),
            line.source_time,
            line.line_id,
        )
    });

    let mut timeline_ms = 350_u64;
    for line in &mut replay.dialogue {
        if !line.included || !order.contains_key(&line.line_id) {
            line.time_ms = u64::MAX;
            continue;
        }
        if !line.duration_locked {
            line.duration_ms = scaled_dialogue_duration_ms(
                &line.text,
                replay.master_dialogue_duration,
            );
        }
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

#[derive(Debug, Clone)]
struct ReplayTurn {
    ordinal: usize,
    turn_index: u32,
    start_ms: u64,
    end_ms: u64,
    speaker_names: Vec<String>,
}

impl ReplayTurn {
    fn label(&self) -> String { format!("回合 {}", self.turn_index) }
}

/// Splits the compiled dialogue timeline into contiguous playback turns by
/// grouping consecutive lines that share the same `turn_index`, so the GM can
/// review the session one round at a time like at the table.
fn replay_turns(replay: &ReplayFile) -> Vec<ReplayTurn> {
    let mut turns: Vec<ReplayTurn> = Vec::new();
    for line in replay
        .dialogue
        .iter()
        .filter(|line| line.included && line.time_ms != u64::MAX)
    {
        let end_ms = line.time_ms.saturating_add(line.duration_ms);
        if let Some(turn) = turns.last_mut() {
            if turn.turn_index == line.turn_index {
                turn.end_ms = turn.end_ms.max(end_ms);
                if !line.name.is_empty()
                    && !turn.speaker_names.iter().any(|name| name == &line.name)
                {
                    turn.speaker_names.push(line.name.clone());
                }
                continue;
            }
        }
        turns.push(ReplayTurn {
            ordinal: turns.len().saturating_add(1),
            turn_index: line.turn_index,
            start_ms: line.time_ms,
            end_ms,
            speaker_names: if line.name.is_empty() { Vec::new() } else { vec![line.name.clone()] },
        });
    }
    // A ship movement anchored right after a turn's last line belongs to that
    // round: extend the turn end past the trailing motion keyframes so the
    // round-boundary pause happens after the ship visibly moves.
    for index in 0..turns.len() {
        let window_end = turns
            .get(index + 1)
            .map(|next| next.start_ms)
            .unwrap_or(u64::MAX);
        if let Some(trailing) = replay
            .ship_trajectories
            .iter()
            .flat_map(|trajectory| trajectory.keyframes.iter().map(|frame| frame.time_ms))
            .filter(|time_ms| *time_ms > turns[index].end_ms && *time_ms < window_end)
            .max()
        {
            turns[index].end_ms = trailing;
        }
    }
    turns
}

fn current_replay_turn(turns: &[ReplayTurn], playback_ms: u64) -> Option<usize> {
    turns
        .iter()
        .position(|turn| playback_ms >= turn.start_ms && playback_ms < turn.end_ms)
        .or_else(|| turns.iter().position(|turn| playback_ms < turn.start_ms))
        .or_else(|| turns.len().checked_sub(1))
}

fn previous_replay_turn(turns: &[ReplayTurn], playback_ms: u64) -> Option<usize> {
    turns.iter().rposition(|turn| turn.start_ms < playback_ms)
}

fn next_replay_turn(turns: &[ReplayTurn], playback_ms: u64) -> Option<usize> {
    turns.iter().position(|turn| turn.start_ms > playback_ms)
}

fn jump_replay_to_turn(studio: &mut ReplayStudio, turns: &[ReplayTurn], index: usize) {
    if replay_has_live_take(studio) {
        return;
    }
    let Some(turn) = turns.get(index) else {
        return;
    };
    studio.playback_ms = turn.start_ms;
    if matches!(
        studio.mode,
        ReplayMode::Playing | ReplayMode::Paused
    ) {
        studio.mode = ReplayMode::Paused;
    }
    let speakers = if turn.speaker_names.is_empty() {
        String::new()
    } else {
        format!("（{}）", turn.speaker_names.join("、"))
    };
    studio.status = format!(
        "已跳转到第 {} 段 {}{speakers}",
        turn.ordinal,
        turn.label()
    );
}

fn standee_positions(
    standees: &Query<(&Transform, &VoxelPlayerStandee), Without<VoxelViewportCamera>>,
) -> HashMap<u64, Vec3> {
    standees
        .iter()
        .map(|(transform, standee)| (standee.user_id, transform.translation))
        .collect()
}

fn replay_speaker_positions(dialogue: &[ReplayDialogue]) -> HashMap<u64, Vec3> {
    dialogue
        .iter()
        .filter(|line| replay_dialogue_has_saved_position(line) && line.side == DialogueSide::Right)
        .map(|line| {
            (
                line.sender_id,
                IVec3::from_array(line.position_cells).as_vec3() * VOXEL_SIZE,
            )
        })
        .collect()
}

fn replay_player_movements_from_history(
    history: &ReplayPlayerMovementHistory,
    campaign_id: &str,
    dialogue: &[ReplayDialogue],
) -> Vec<ReplayPlayerMovement> {
    let visible_user_ids = dialogue
        .iter()
        .filter(|line| replay_dialogue_is_playable(line))
        .flat_map(|line| [Some(line.sender_id), line.camera_focus_id])
        .flatten()
        .collect::<HashSet<_>>();

    history
        .sessions
        .iter()
        .filter(|session| session.campaign_id == campaign_id)
        .filter(|session| visible_user_ids.contains(&session.user_id))
        .filter_map(|session| {
            let first_source_unix_ms = session.keyframes.first()?.source_unix_ms;
            let anchor = movement_session_anchor_line(session, dialogue, first_source_unix_ms)?;
            let movement_start = anchor.time_ms.saturating_add(session.start_delay_ms);
            let keyframes = session
                .keyframes
                .iter()
                .map(|frame| ReplayPlayerMovementKeyframe {
                    time_ms: movement_start
                        .saturating_add(frame.source_unix_ms - first_source_unix_ms),
                    position_cells: frame.position_cells,
                })
                .collect::<Vec<_>>();
            (!keyframes.is_empty()).then_some(ReplayPlayerMovement {
                user_id: session.user_id,
                keyframes,
            })
        })
        .collect()
}

fn movement_session_anchor_line<'a>(
    session: &PersistedPlayerMovementSession,
    dialogue: &'a [ReplayDialogue],
    first_source_unix_ms: u64,
) -> Option<&'a ReplayDialogue> {
    let playable =
        |line: &&ReplayDialogue| line.included && line.source_time > 0 && line.time_ms != u64::MAX;
    if let Some(source_time) = session.start_after_source_time {
        return dialogue.iter().filter(playable).find(|line| {
            line.source_time == source_time
                && session
                    .start_after_sender_id
                    .is_none_or(|sender_id| line.sender_id == sender_id)
        });
    }

    let first_source_time = first_source_unix_ms / 1_000;
    let mut same_turn = dialogue
        .iter()
        .filter(playable)
        .filter(|line| line.sender_id == session.user_id)
        .filter(|line| line.turn_index == session.turn_index)
        .collect::<Vec<_>>();
    if same_turn.is_empty() {
        same_turn = dialogue
            .iter()
            .filter(playable)
            .filter(|line| line.sender_id == session.user_id)
            .collect();
    }
    same_turn
        .iter()
        .copied()
        .filter(|line| line.source_time <= first_source_time)
        .max_by_key(|line| (line.source_time, line.line_id))
        .or_else(|| {
            same_turn
                .into_iter()
                .min_by_key(|line| line.source_time.abs_diff(first_source_time))
        })
}

fn append_new_player_movements_from_history(
    replay: &mut ReplayFile,
    history: &ReplayPlayerMovementHistory,
) -> usize {
    let cutoff_unix_ms = replay
        .player_movement_history_cursor_unix_ms
        .max(replay.created_at_unix_ms);
    let visible_user_ids = replay
        .dialogue
        .iter()
        .filter(|line| replay_dialogue_is_playable(line))
        .flat_map(|line| [Some(line.sender_id), line.camera_focus_id])
        .flatten()
        .collect::<HashSet<_>>();
    let mut pending = history
        .sessions
        .iter()
        .filter(|session| session.campaign_id == replay.campaign_id)
        .filter(|session| visible_user_ids.contains(&session.user_id))
        .filter(|session| !replay.authored_player_movements.contains(&session.user_id))
        .filter_map(|session| {
            let frames = session
                .keyframes
                .iter()
                .filter(|frame| frame.source_unix_ms > cutoff_unix_ms)
                .copied()
                .collect::<Vec<_>>();
            let first_source_unix_ms = frames.first()?.source_unix_ms;
            Some((
                first_source_unix_ms,
                session.user_id,
                frames,
            ))
        })
        .collect::<Vec<_>>();
    pending.sort_unstable_by_key(|(source_unix_ms, ..)| *source_unix_ms);
    if pending.is_empty() {
        return 0;
    }

    let mut timeline_cursor = replay.duration_ms;
    let mut imported_through = cutoff_unix_ms;
    let mut speaker_positions = replay_speaker_positions(&replay.dialogue);
    let obstacles = ReplayCameraObstacles::from_scene(&replay.scene);
    let imported_count = pending.len();
    for (_, user_id, frames) in pending {
        let first_source_unix_ms = frames[0].source_unix_ms;
        let first_position = Vec3::from_array(frames[0].position_cells) * VOXEL_SIZE;
        speaker_positions.insert(user_id, first_position);

        let movement_start = if let Some(last_camera) = replay.camera.last() {
            let current = frame_transform(last_camera);
            if last_camera.time_ms < timeline_cursor {
                replay.camera.push(camera_keyframe(
                    timeline_cursor,
                    &current,
                ));
            }
            let rig = DirectedCameraRig::for_dialogue(
                &current,
                &replay.dialogue,
                &speaker_positions,
                replay.camera_distance_scale,
                replay.camera_yaw_degrees,
            );
            let focused = rig.speaker_shot(
                first_position,
                DirectorShot::SpeakerMedium,
                0.0,
                &obstacles,
            );
            let movement_start = timeline_cursor.saturating_add(FOCUS_TRANSITION_MS);
            replay.camera.push(camera_keyframe(
                movement_start,
                &focused,
            ));
            movement_start
        } else {
            let base = Transform::from_translation(first_position + Vec3::new(0.0, 1.5, 5.0))
                .looking_at(first_position, Vec3::Y);
            let rig = DirectedCameraRig::for_dialogue(
                &base,
                &replay.dialogue,
                &speaker_positions,
                replay.camera_distance_scale,
                replay.camera_yaw_degrees,
            );
            let focused = rig.speaker_shot(
                first_position,
                DirectorShot::SpeakerMedium,
                0.0,
                &obstacles,
            );
            replay.camera.push(camera_keyframe(
                timeline_cursor,
                &focused,
            ));
            timeline_cursor
        };

        let keyframes = frames
            .iter()
            .map(|frame| ReplayPlayerMovementKeyframe {
                time_ms: movement_start.saturating_add(frame.source_unix_ms - first_source_unix_ms),
                position_cells: frame.position_cells,
            })
            .collect::<Vec<_>>();
        imported_through = imported_through.max(
            frames
                .last()
                .map(|frame| frame.source_unix_ms)
                .unwrap_or(first_source_unix_ms),
        );
        timeline_cursor = keyframes
            .last()
            .map(|frame| frame.time_ms)
            .unwrap_or(movement_start);
        replay
            .player_movements
            .push(ReplayPlayerMovement { user_id, keyframes });
    }
    replay.duration_ms = replay.duration_ms.max(timeline_cursor);
    replay.player_movement_history_cursor_unix_ms = imported_through;
    imported_count
}

const MOTION_EVENT_GAP_MS: u64 = 3_000;

/// One contiguous ship motion from the persistent trajectory history. A
/// session is split into events whenever the ship stopped moving for longer
/// than `MOTION_EVENT_GAP_MS`.
struct ShipMotionEvent {
    ship_id: String,
    ship_name: String,
    start_source_ms: u64,
    end_source_ms: u64,
    anchor_source_time: Option<u64>,
    start_delay_ms: u64,
    keyframes: Vec<PersistedShipKeyframe>,
}

/// A replay-timeline slice with its real-world source range, used to map
/// terrain/hull edits onto the compiled dialogue + ship-motion timeline.
struct TimelineSegment {
    source_start_ms: u64,
    source_end_ms: u64,
    replay_start_ms: u64,
    replay_end_ms: u64,
}

fn ship_motion_events(
    history: &ReplayShipTrajectoryHistory,
    campaign_id: &str,
    cutoff_unix_ms: u64,
) -> Vec<ShipMotionEvent> {
    let mut events = Vec::new();
    for session in history
        .sessions
        .iter()
        .filter(|session| session.campaign_id == campaign_id)
    {
        let mut frames = session
            .keyframes
            .iter()
            .filter(|frame| frame.source_unix_ms > cutoff_unix_ms)
            .copied()
            .collect::<Vec<_>>();
        if frames.is_empty() {
            continue;
        }
        if let Some(previous) = session
            .keyframes
            .iter()
            .rev()
            .find(|frame| {
                frame.source_unix_ms <= cutoff_unix_ms
                    && frames.first().is_some_and(|first| {
                        first.source_unix_ms.saturating_sub(frame.source_unix_ms)
                            <= MOTION_EVENT_GAP_MS
                    })
            })
            .copied()
        {
            frames.insert(0, previous);
        }
        let mut runs = Vec::<Vec<PersistedShipKeyframe>>::new();
        let mut run = Vec::new();
        let mut previous_source = frames
            .first()
            .map(|frame| frame.source_unix_ms)
            .unwrap_or_default();
        for frame in frames {
            if !run.is_empty()
                && frame.source_unix_ms.saturating_sub(previous_source) > MOTION_EVENT_GAP_MS
            {
                runs.push(std::mem::take(&mut run));
            }
            run.push(frame);
            previous_source = frame.source_unix_ms;
        }
        if !run.is_empty() {
            runs.push(run);
        }
        // A run counts as motion when any of its poses differs from the pose
        // the ship had before the run. A discrete reposition that arrives as a
        // single new pose gets the previous pose prepended as its start frame.
        let mut previous_pose: Option<([f32; 3], [f32; 4])> = None;
        for run in &runs {
            let moved = match previous_pose {
                Some((translation, rotation)) => run
                    .iter()
                    .any(|frame| frame.translation != translation || frame.rotation != rotation),
                None => run.first().is_some_and(|first| {
                    run.iter().any(|frame| {
                        frame.translation != first.translation || frame.rotation != first.rotation
                    })
                }),
            };
            if moved {
                let mut frames = run.clone();
                if let Some((translation, rotation)) = previous_pose {
                    if let Some(first) = frames.first() {
                        if first.translation != translation || first.rotation != rotation {
                            frames.insert(0, PersistedShipKeyframe {
                                source_unix_ms: first.source_unix_ms.saturating_sub(1),
                                translation,
                                rotation,
                            });
                        }
                    }
                }
                events.push(make_ship_motion_event(session, frames));
            }
            if let Some(last) = run.last() {
                previous_pose = Some((last.translation, last.rotation));
            }
        }
    }
    events.sort_by_key(|event| event.start_source_ms);
    events
}

fn make_ship_motion_event(
    session: &PersistedShipTrajectorySession,
    keyframes: Vec<PersistedShipKeyframe>,
) -> ShipMotionEvent {
    let start_source_ms = keyframes
        .first()
        .map(|frame| frame.source_unix_ms)
        .unwrap_or_default();
    let end_source_ms = keyframes
        .last()
        .map(|frame| frame.source_unix_ms)
        .unwrap_or(start_source_ms);
    ShipMotionEvent {
        ship_id: session.ship_id.clone(),
        ship_name: session.ship_name.clone(),
        start_source_ms,
        end_source_ms,
        anchor_source_time: session.start_after_source_time,
        start_delay_ms: session.start_delay_ms,
        keyframes,
    }
}

/// Rebuilds the replay timeline by interleaving dialogue lines with ship
/// motion events in real-world order: each motion is inserted right after the
/// dialogue line that was current when the ship started moving, so "the ship
/// moved, then the player spoke" plays back in the same order. Pending
/// terrain/hull edits are mapped onto the resulting timeline by real time, and
/// ship trajectories are merged into `replay.ship_trajectories`.
fn compile_scene_dynamics_timeline(
    replay: &mut ReplayFile,
    ship_history: &ReplayShipTrajectoryHistory,
    cutoff_unix_ms: u64,
    pending_terrain: &[(u64, IVec3, u8)],
    pending_hull: &[(u64, String, IVec3, u8)],
) -> usize {
    compile_area_block_timeline(replay);
    let playable = replay
        .dialogue
        .iter()
        .enumerate()
        .filter(|(_, line)| line.included && line.time_ms != u64::MAX)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let events = ship_motion_events(
        ship_history,
        &replay.campaign_id,
        cutoff_unix_ms,
    );
    let events = events
        .into_iter()
        .filter(|event| !replay.authored_ship_trajectories.contains(&event.ship_id))
        .collect::<Vec<_>>();

    let mut before_first = Vec::new();
    let mut events_by_anchor = HashMap::<usize, Vec<ShipMotionEvent>>::new();
    for event in events {
        let anchor = event
            .anchor_source_time
            .and_then(|source_time| {
                playable
                    .iter()
                    .position(|&line_index| replay.dialogue[line_index].source_time == source_time)
            })
            .or_else(|| {
                playable.iter().rposition(|&line_index| {
                    let line = &replay.dialogue[line_index];
                    line.source_time > 0
                        && line.source_time.saturating_mul(1_000) <= event.start_source_ms
                })
            });
        if let Some(position) = anchor {
            events_by_anchor.entry(position).or_default().push(event);
        } else {
            before_first.push(event);
        }
    }

    let mut cursor = 350_u64;
    let mut segments = Vec::<TimelineSegment>::new();
    let mut imported_keyframes = HashMap::<String, (String, Vec<ReplayShipKeyframe>)>::new();
    let ship_motion_speed = replay.ship_motion_speed.max(0.1);
    let motion_at_end = !replay.dialogue_waits_for_ship_motion;
    let mut end_motion = Vec::new();
    if motion_at_end {
        end_motion.extend(before_first);
        for (_, mut anchored) in events_by_anchor.drain() {
            anchored.sort_by_key(|event| event.start_source_ms);
            end_motion.extend(anchored);
        }
    } else {
        for event in before_first {
            place_motion_cluster(
                vec![event],
                &mut cursor,
                &mut segments,
                &mut imported_keyframes,
                ship_motion_speed,
            );
        }
    }
    for (position, &line_index) in playable.iter().enumerate() {
        let line = &mut replay.dialogue[line_index];
        line.time_ms = cursor;
        let line_end = line.time_ms.saturating_add(line.duration_ms);
        segments.push(TimelineSegment {
            source_start_ms: line.source_time.saturating_mul(1_000),
            source_end_ms: 0,
            replay_start_ms: line.time_ms,
            replay_end_ms: line_end,
        });
        cursor = line_end.saturating_add(HISTORY_DIALOGUE_GAP_MS);
        if !motion_at_end {
            if let Some(mut anchored) = events_by_anchor.remove(&position) {
                // Order by real time so motions after the same line play in
                // order.
                anchored.sort_by_key(|event| event.start_source_ms);
                for event in anchored {
                    place_motion_cluster(
                        vec![event],
                        &mut cursor,
                        &mut segments,
                        &mut imported_keyframes,
                        ship_motion_speed,
                    );
                }
            }
        }
    }
    for cluster in end_motion {
        place_motion_cluster(
            vec![cluster],
            &mut cursor,
            &mut segments,
            &mut imported_keyframes,
            ship_motion_speed,
        );
    }
    fill_segment_sources(&mut segments);
    let max_pending_source = pending_terrain
        .iter()
        .map(|(source, ..)| *source)
        .chain(pending_hull.iter().map(|(source, ..)| *source))
        .max()
        .unwrap_or_default();
    if let Some(last) = segments.last_mut() {
        last.source_end_ms = last.source_end_ms.max(max_pending_source.saturating_add(1));
    }

    let imported_ships = merge_ship_trajectories(replay, imported_keyframes);
    let scene_end = map_pending_scene_changes(
        replay,
        &segments,
        pending_terrain,
        pending_hull,
    );
    let timeline_end = segments
        .iter()
        .map(|segment| segment.replay_end_ms)
        .max()
        .unwrap_or_default()
        .max(cursor)
        .max(scene_end);
    replay.duration_ms = replay.duration_ms.max(timeline_end);
    imported_ships
}

/// Appends ship trajectories recorded since the replay's last import cursor to
/// the end of the existing timeline, mirroring the player-movement append so a
/// replay keeps its already-compiled dialogue and camera track untouched.
fn append_new_ship_trajectories_from_history(
    replay: &mut ReplayFile,
    history: &ReplayShipTrajectoryHistory,
) -> usize {
    let cutoff_unix_ms = replay
        .ship_trajectory_history_cursor_unix_ms
        .max(replay.created_at_unix_ms);
    let events = ship_motion_events(
        history,
        &replay.campaign_id,
        cutoff_unix_ms,
    );
    let events = events
        .into_iter()
        .filter(|event| !replay.authored_ship_trajectories.contains(&event.ship_id))
        .collect::<Vec<_>>();
    if events.is_empty() {
        return 0;
    }
    let mut next_start = replay.duration_ms;
    let mut segments = Vec::<TimelineSegment>::new();
    let mut imported_keyframes = HashMap::<String, (String, Vec<ReplayShipKeyframe>)>::new();
    let ship_motion_speed = replay.ship_motion_speed.max(0.1);
    let mut events = events;
    events.sort_by_key(|event| event.start_source_ms);
    for event in events {
        place_motion_cluster(
            vec![event],
            &mut next_start,
            &mut segments,
            &mut imported_keyframes,
            ship_motion_speed,
        );
    }
    let imported = merge_ship_trajectories(replay, imported_keyframes);
    replay.duration_ms = replay.duration_ms.max(next_start);
    replay.ship_trajectory_history_cursor_unix_ms = unix_time_ms();
    imported
}

fn place_motion_cluster(
    events: Vec<ShipMotionEvent>,
    cursor: &mut u64,
    segments: &mut Vec<TimelineSegment>,
    imported_keyframes: &mut HashMap<String, (String, Vec<ReplayShipKeyframe>)>,
    ship_motion_speed: f32,
) {
    let window_start = events
        .iter()
        .map(|event| event.start_source_ms)
        .min()
        .unwrap_or_default();
    let window_end = events
        .iter()
        .map(|event| event.end_source_ms)
        .max()
        .unwrap_or_default();
    let real_duration = window_end.saturating_sub(window_start);
    let scaled_duration = (real_duration as f64 / ship_motion_speed.max(0.1) as f64).round() as u64;
    let segment_ms = scaled_duration.clamp(
        MIN_MOVEMENT_SEGMENT_MS,
        MAX_MOVEMENT_SEGMENT_MS,
    );
    let cluster_delay = events
        .iter()
        .map(|event| event.start_delay_ms)
        .max()
        .unwrap_or_default();
    let replay_start_ms = cursor.saturating_add(cluster_delay);
    let replay_end_ms = replay_start_ms.saturating_add(segment_ms);
    segments.push(TimelineSegment {
        source_start_ms: window_start,
        source_end_ms: 0,
        replay_start_ms,
        replay_end_ms,
    });
    *cursor = replay_end_ms.saturating_add(HISTORY_DIALOGUE_GAP_MS);
    for event in events {
        let entry = imported_keyframes
            .entry(event.ship_id.clone())
            .or_insert_with(|| (event.ship_name.clone(), Vec::new()));
        entry.0 = event.ship_name.clone();
        for keyframe in event.keyframes {
            let fraction = if real_duration > 0 {
                (keyframe.source_unix_ms.saturating_sub(window_start) as f64 / real_duration as f64)
                    .clamp(0.0, 1.0)
            } else {
                0.0
            };
            let frame_ms = replay_start_ms
                .saturating_add(((replay_end_ms - replay_start_ms) as f64 * fraction) as u64);
            entry.1.push(ReplayShipKeyframe {
                time_ms: frame_ms,
                translation: keyframe.translation,
                rotation: keyframe.rotation,
            });
        }
    }
}

/// Closes the source range of every segment from the next segment's start, so
/// terrain edits can be mapped onto the whole timeline by real time.
fn fill_segment_sources(segments: &mut [TimelineSegment]) {
    for index in 0..segments.len() {
        let next_start = segments
            .get(index + 1)
            .map(|next| next.source_start_ms)
            .unwrap_or_else(|| segments[index].source_start_ms.saturating_add(10_000));
        segments[index].source_end_ms = next_start.max(segments[index].source_start_ms);
    }
}

/// Merges the freshly placed ship keyframes into `replay.ship_trajectories`,
/// keeping previously imported frames whose segment times are unchanged.
fn merge_ship_trajectories(
    replay: &mut ReplayFile,
    imported_keyframes: HashMap<String, (String, Vec<ReplayShipKeyframe>)>,
) -> usize {
    let imported_keyframes = imported_keyframes
        .into_iter()
        .filter(|(ship_id, _)| !replay.authored_ship_trajectories.contains(ship_id))
        .collect::<HashMap<_, _>>();
    let imported_ships = imported_keyframes.len();
    for (ship_id, (ship_name, mut keyframes)) in imported_keyframes {
        keyframes.sort_by_key(|frame| frame.time_ms);
        keyframes.dedup_by(|right, left| {
            if right.time_ms == left.time_ms {
                *left = *right;
                true
            } else {
                false
            }
        });
        if let Some(existing) = replay
            .ship_trajectories
            .iter_mut()
            .find(|trajectory| trajectory.ship_id == ship_id)
        {
            existing.ship_name = ship_name;
            for frame in keyframes {
                if let Some(last) = existing
                    .keyframes
                    .iter_mut()
                    .find(|last| last.time_ms == frame.time_ms)
                {
                    *last = frame;
                } else {
                    existing.keyframes.push(frame);
                }
            }
            existing.keyframes.sort_by_key(|frame| frame.time_ms);
            existing.keyframes.dedup_by(|right, left| {
                if right.time_ms == left.time_ms {
                    *left = *right;
                    true
                } else {
                    false
                }
            });
        } else {
            replay.ship_trajectories.push(ReplayShipTrajectory {
                ship_id,
                ship_name,
                keyframes,
            });
        }
    }
    imported_ships
}

/// Maps pending terrain/hull edits onto the compiled dialogue + motion
/// timeline proportionally to their real message time.
fn map_pending_scene_changes(
    replay: &mut ReplayFile,
    segments: &[TimelineSegment],
    pending_terrain: &[(u64, IVec3, u8)],
    pending_hull: &[(u64, String, IVec3, u8)],
) -> u64 {
    let mut scene_end = 0_u64;
    for (source_unix_ms, cell, material) in pending_terrain {
        let time_ms = map_source_via_segments(segments, *source_unix_ms);
        replay.terrain_changes.push(ReplayTerrainChange {
            time_ms,
            position: cell.to_array(),
            material: *material,
            enabled: true,
        });
        scene_end = scene_end.max(time_ms);
    }
    for (source_unix_ms, ship_id, cell, material) in pending_hull {
        let time_ms = map_source_via_segments(segments, *source_unix_ms);
        replay.ship_hull_changes.push(ReplayShipHullChange {
            time_ms,
            ship_id: ship_id.clone(),
            position: cell.to_array(),
            material: *material,
            enabled: true,
        });
        scene_end = scene_end.max(time_ms);
    }
    scene_end
}

fn map_source_via_segments(segments: &[TimelineSegment], source_unix_ms: u64) -> u64 {
    for segment in segments {
        if source_unix_ms >= segment.source_start_ms && source_unix_ms < segment.source_end_ms {
            let span = segment
                .source_end_ms
                .saturating_sub(segment.source_start_ms);
            let fraction = if span > 0 {
                (source_unix_ms.saturating_sub(segment.source_start_ms) as f64 / span as f64)
                    .clamp(0.0, 1.0)
            } else {
                0.0
            };
            return segment.replay_start_ms.saturating_add(
                ((segment
                    .replay_end_ms
                    .saturating_sub(segment.replay_start_ms)) as f64
                    * fraction) as u64,
            );
        }
    }
    segments
        .first()
        .map(|segment| segment.replay_start_ms)
        .unwrap_or(350)
}

fn replay_camera_focus_at(
    dialogue: &[ReplayDialogue],
    time_ms: u64,
    speaker_positions: &HashMap<u64, Vec3>,
) -> Option<Vec3> {
    let index = dialogue
        .iter()
        .rposition(|line| replay_dialogue_is_playable(line) && line.time_ms <= time_ms)
        .or_else(|| dialogue.iter().position(replay_dialogue_is_playable))?;
    replay_dialogue_focus_position(dialogue, index, speaker_positions)
}

fn rescale_replay_camera_distance(
    replay: &mut ReplayFile,
    requested_scale: f32,
    speaker_positions: &HashMap<u64, Vec3>,
) {
    let previous_scale = normalized_directed_camera_distance_scale(replay.camera_distance_scale);
    let requested_scale = normalized_directed_camera_distance_scale(requested_scale);
    let ratio = requested_scale / previous_scale;
    if (ratio - 1.0).abs() > f32::EPSILON {
        for frame in &mut replay.camera {
            let Some(focus) = replay_camera_focus_at(
                &replay.dialogue,
                frame.time_ms,
                speaker_positions,
            ) else {
                continue;
            };
            let mut transform = frame_transform(frame);
            transform.translation = focus + (transform.translation - focus) * ratio;
            transform = transform.looking_at(focus, Vec3::Y);
            *frame = camera_keyframe(frame.time_ms, &transform);
        }
    }
    replay.camera_distance_scale = requested_scale;
}

fn rotate_replay_camera_yaw(
    replay: &mut ReplayFile,
    requested_yaw_degrees: f32,
    speaker_positions: &HashMap<u64, Vec3>,
) -> f32 {
    const ROTATION_FRACTIONS: [f32; 9] = [1.0, 0.875, 0.75, 0.625, 0.5, 0.375, 0.25, 0.125, 0.0];

    let previous_yaw_degrees = normalized_directed_camera_yaw_degrees(replay.camera_yaw_degrees);
    let requested_yaw_degrees = normalized_directed_camera_yaw_degrees(requested_yaw_degrees);
    let yaw_delta_radians = (requested_yaw_degrees - previous_yaw_degrees).to_radians();
    if yaw_delta_radians.abs() <= f32::EPSILON || replay.camera.is_empty() {
        replay.camera_yaw_degrees = requested_yaw_degrees;
        return requested_yaw_degrees;
    }
    let base = replay.camera.first().map(frame_transform);
    let rig = base.as_ref().map(|base| {
        DirectedCameraRig::for_dialogue(
            base,
            &replay.dialogue,
            speaker_positions,
            replay.camera_distance_scale,
            previous_yaw_degrees,
        )
    });
    let obstacles = ReplayCameraObstacles::from_scene(&replay.scene);
    for fraction in ROTATION_FRACTIONS {
        let rotation = Quat::from_rotation_y(yaw_delta_radians * fraction);
        let mut adjusted = Vec::with_capacity(replay.camera.len());
        let mut valid = true;
        for frame in &replay.camera {
            let Some(focus) = replay_camera_focus_at(
                &replay.dialogue,
                frame.time_ms,
                speaker_positions,
            ) else {
                adjusted.push(frame.clone());
                continue;
            };
            let transform = frame_transform(frame);
            let offset = transform.translation - focus;
            if offset.length_squared() <= f32::EPSILON {
                adjusted.push(frame.clone());
                continue;
            }
            let translation = focus + rotation * offset;
            let stays_on_camera_side = rig.as_ref().is_none_or(|rig| {
                rig.subject_count < 2
                    || rig.signed_side(translation) >= DirectedCameraRig::LINE_MARGIN
            });
            if !stays_on_camera_side || !obstacles.camera_is_clear(translation) {
                valid = false;
                break;
            }
            adjusted.push(camera_keyframe(
                frame.time_ms,
                &Transform::from_translation(translation).looking_at(focus, Vec3::Y),
            ));
        }
        if valid {
            let applied_yaw_degrees =
                previous_yaw_degrees + (requested_yaw_degrees - previous_yaw_degrees) * fraction;
            replay.camera = adjusted;
            replay.camera_yaw_degrees = applied_yaw_degrees;
            return applied_yaw_degrees;
        }
    }
    replay.camera_yaw_degrees = previous_yaw_degrees;
    previous_yaw_degrees
}

fn replay_dialogue_focus_id(
    dialogue: &[ReplayDialogue],
    index: usize,
    speaker_positions: &HashMap<u64, Vec3>,
) -> Option<u64> {
    let line = dialogue.get(index)?;
    let direct_id = line.camera_focus_id.unwrap_or(line.sender_id);
    if speaker_positions.contains_key(&direct_id) {
        return Some(direct_id);
    }
    if line.side != DialogueSide::Left {
        return None;
    }
    dialogue[..index]
        .iter()
        .rfind(|candidate| {
            replay_dialogue_is_playable(candidate) && candidate.side == DialogueSide::Right
        })
        .and_then(|candidate| {
            let id = candidate.camera_focus_id.unwrap_or(candidate.sender_id);
            speaker_positions.contains_key(&id).then_some(id)
        })
        .or_else(|| {
            dialogue[index.saturating_add(1)..]
                .iter()
                .find(|candidate| {
                    replay_dialogue_is_playable(candidate) && candidate.side == DialogueSide::Right
                })
                .and_then(|candidate| {
                    let id = candidate.camera_focus_id.unwrap_or(candidate.sender_id);
                    speaker_positions.contains_key(&id).then_some(id)
                })
        })
}

fn missing_director_dialogue(
    replay: &ReplayFile,
    speaker_positions: &HashMap<u64, Vec3>,
) -> Vec<(usize, String)> {
    replay
        .dialogue
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            replay_dialogue_is_playable(line)
                && replay_dialogue_focus_id(
                    &replay.dialogue,
                    *index,
                    speaker_positions,
                )
                .is_none()
        })
        .map(|(index, line)| {
            (
                index,
                director_dialogue_issue_description(index, line),
            )
        })
        .collect()
}

fn director_dialogue_issue_description(index: usize, line: &ReplayDialogue) -> String {
    const PREVIEW_CHAR_LIMIT: usize = 48;

    let normalized_text = line.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = normalized_text
        .chars()
        .take(PREVIEW_CHAR_LIMIT)
        .collect::<String>();
    if normalized_text.chars().count() > PREVIEW_CHAR_LIMIT {
        preview.push('…');
    }
    if preview.is_empty() {
        preview = "（空台词）".to_owned();
    }
    format!(
        "第 {} 句 · {}（QQ {}）：{}",
        index.saturating_add(1),
        line.name,
        line.sender_id,
        preview
    )
}

fn replay_dialogue_focus_position(
    dialogue: &[ReplayDialogue],
    index: usize,
    speaker_positions: &HashMap<u64, Vec3>,
) -> Option<Vec3> {
    let line = dialogue.get(index)?;
    let subject_id = line.camera_focus_id.unwrap_or(line.sender_id);
    replay_standee_position_at(
        dialogue,
        subject_id,
        line.time_ms,
        speaker_positions,
    )
    .or_else(|| {
        // A GM line without an addressed standee still frames the nearest
        // speaker standee so the camera never drifts to the base view.
        if line.side != DialogueSide::Left {
            return None;
        }
        replay_dialogue_focus_id(dialogue, index, speaker_positions).and_then(|focus_id| {
            replay_standee_position_at(
                dialogue,
                focus_id,
                line.time_ms,
                speaker_positions,
            )
        })
    })
}

/// Where a standee will actually be during playback: the recorded position of
/// the speaker's most recent snapshot line at or before `time_ms`, falling
/// back to the standee's current scene position before any line has moved it.
fn replay_standee_position_at(
    dialogue: &[ReplayDialogue],
    subject_id: u64,
    time_ms: u64,
    speaker_positions: &HashMap<u64, Vec3>,
) -> Option<Vec3> {
    dialogue
        .iter()
        .filter(|line| {
            replay_dialogue_is_playable(line)
                && replay_dialogue_has_saved_position(line)
                && line.side == DialogueSide::Right
                && line.time_ms <= time_ms
                && line.sender_id == subject_id
        })
        .last()
        .map(|line| IVec3::from_array(line.position_cells).as_vec3() * VOXEL_SIZE)
        .or_else(|| {
            // Before the player's first recorded line, the standee starts at
            // its earliest recorded position during playback, so early GM
            // lines frame the conversation location instead of the standee's
            // current scene position.
            dialogue
                .iter()
                .filter(|line| {
                    replay_dialogue_is_playable(line)
                        && replay_dialogue_has_saved_position(line)
                        && line.side == DialogueSide::Right
                        && line.sender_id == subject_id
                })
                .next()
                .map(|line| IVec3::from_array(line.position_cells).as_vec3() * VOXEL_SIZE)
        })
        .or_else(|| speaker_positions.get(&subject_id).copied())
}

fn turn_based_camera_track(
    base: &Transform,
    dialogue: &[ReplayDialogue],
    duration_ms: u64,
    speaker_positions: &HashMap<u64, Vec3>,
    camera_distance_scale: f32,
    camera_yaw_degrees: f32,
    obstacles: &ReplayCameraObstacles,
) -> Vec<ReplayCameraKeyframe> {
    let rig = DirectedCameraRig::for_dialogue(
        base,
        dialogue,
        speaker_positions,
        camera_distance_scale,
        camera_yaw_degrees,
    );
    let mut frames = Vec::with_capacity(dialogue.len().saturating_mul(3).saturating_add(2));
    let mut current = (!dialogue.is_empty())
        .then(|| replay_dialogue_focus_position(dialogue, 0, speaker_positions))
        .flatten()
        .map(|target| {
            rig.speaker_shot(
                target,
                DirectorShot::SpeakerMedium,
                0.0,
                obstacles,
            )
        })
        .unwrap_or_else(|| base.clone());
    let mut current_focus = replay_dialogue_focus_id(dialogue, 0, speaker_positions);
    frames.push(camera_keyframe(0, &current));
    for (index, line) in dialogue.iter().enumerate() {
        let line_end = line.time_ms.saturating_add(line.duration_ms);
        if let Some(target) = replay_dialogue_focus_position(dialogue, index, speaker_positions) {
            let focus = replay_dialogue_focus_id(dialogue, index, speaker_positions);
            let focused = rig.speaker_shot(
                target,
                DirectorShot::SpeakerMedium,
                0.0,
                obstacles,
            );
            let settled = focused;
            let transition_end = line
                .time_ms
                .saturating_add(line.duration_ms.min(FOCUS_TRANSITION_MS));
            if focus == current_focus
                && camera_facing_is_unchanged(&current, &focused)
                && current.translation.distance(focused.translation) <= CAMERA_DOLLY_MAX_DISTANCE
            {
                frames.push(camera_keyframe(line.time_ms, &current));
                frames.push(camera_keyframe(
                    transition_end,
                    &focused,
                ));
            } else {
                frames.push(camera_keyframe(
                    line.time_ms.saturating_sub(1),
                    &current,
                ));
                frames.push(camera_keyframe(line.time_ms, &focused));
            }
            frames.push(camera_keyframe(line_end, &settled));
            current = settled;
            current_focus = focus;
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
    camera_distance_scale: f32,
    camera_yaw_degrees: f32,
    obstacles: &ReplayCameraObstacles,
) -> Vec<ReplayCameraKeyframe> {
    let rig = DirectedCameraRig::for_dialogue(
        base,
        dialogue,
        speaker_positions,
        camera_distance_scale,
        camera_yaw_degrees,
    );
    let mut current = base.clone();
    if let (Some(_line), Some(cue)) = (dialogue.first(), cues.first()) {
        if let Some(target) = replay_dialogue_focus_position(dialogue, 0, speaker_positions) {
            current = rig.director_shot(
                target,
                resolved_speaker_shot(cue.shot),
                cue.motion,
                0.0,
                obstacles,
            );
        }
    }
    let mut current_focus = replay_dialogue_focus_id(dialogue, 0, speaker_positions);
    let mut frames = vec![camera_keyframe(0, &current)];
    for (index, (line, cue)) in dialogue.iter().zip(cues).enumerate() {
        let line_end = line.time_ms.saturating_add(line.duration_ms);
        let focus = replay_dialogue_focus_id(dialogue, index, speaker_positions);
        let (arrival, settled) = if let Some(target) =
            replay_dialogue_focus_position(dialogue, index, speaker_positions)
        {
            let speaker_shot = resolved_speaker_shot(cue.shot);
            let desired_arrival = rig.director_shot(
                target,
                speaker_shot,
                cue.motion,
                0.0,
                obstacles,
            );
            let arrival = desired_arrival;
            let desired_settled = rig.director_shot(
                target,
                speaker_shot,
                cue.motion,
                1.0,
                obstacles,
            );
            let settle_limit = (line.duration_ms as f32 / 1_000.0 * 0.12).clamp(0.2, 0.65);
            let settled = limit_camera_travel_toward(
                &arrival,
                &desired_settled,
                settle_limit,
                target,
            );
            (arrival, settled)
        } else {
            continue;
        };
        let transition_end = line
            .time_ms
            .saturating_add(line.duration_ms.min(FOCUS_TRANSITION_MS));
        if focus == current_focus
            && camera_facing_is_unchanged(&current, &arrival)
            && current.translation.distance(arrival.translation) <= CAMERA_DOLLY_MAX_DISTANCE
        {
            frames.push(camera_keyframe(line.time_ms, &current));
            frames.push(camera_keyframe(
                transition_end,
                &arrival,
            ));
        } else {
            frames.push(camera_keyframe(
                line.time_ms.saturating_sub(1),
                &current,
            ));
            frames.push(camera_keyframe(line.time_ms, &arrival));
        }
        frames.push(camera_keyframe(line_end, &settled));
        current = settled;
        current_focus = focus;
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

fn camera_facing_is_unchanged(left: &Transform, right: &Transform) -> bool {
    left.rotation.dot(right.rotation).abs() >= 0.99999
}

fn limit_camera_travel_toward(
    current: &Transform,
    desired: &Transform,
    max_distance: f32,
    focus: Vec3,
) -> Transform {
    let offset = desired.translation - current.translation;
    let translation = current.translation + offset.clamp_length_max(max_distance.max(0.0));
    Transform::from_translation(translation).looking_at(focus, Vec3::Y)
}

#[derive(Debug, Clone, Default)]
struct ReplayCameraObstacles {
    occupied: HashSet<IVec3>,
}

impl ReplayCameraObstacles {
    fn from_scene(scene: &ReplayScene) -> Self {
        Self {
            occupied: scene
                .voxels
                .iter()
                .filter(|voxel| voxel.material != 0)
                .map(|voxel| IVec3::from_array(voxel.position))
                .collect(),
        }
    }

    fn camera_is_clear(&self, camera: Vec3) -> bool {
        let clearance = VOXEL_SIZE * 0.45;
        [
            Vec3::ZERO,
            Vec3::X * clearance,
            Vec3::NEG_X * clearance,
            Vec3::Y * clearance,
            Vec3::NEG_Y * clearance,
            Vec3::Z * clearance,
            Vec3::NEG_Z * clearance,
        ]
        .into_iter()
        .all(|offset| !self.contains_world_point(camera + offset))
    }

    #[cfg(test)]
    fn segment_is_clear(&self, from: Vec3, to: Vec3) -> bool {
        let offset = to - from;
        let distance = offset.length();
        if distance <= f32::EPSILON {
            return !self.contains_world_point(from);
        }
        let direction = offset / distance;
        let step = VOXEL_SIZE * 0.2;
        let end = (distance - VOXEL_SIZE * 0.1).max(0.0);
        let mut travelled = 0.0;
        while travelled <= end {
            if self.contains_world_point(from + direction * travelled) {
                return false;
            }
            travelled += step;
        }
        true
    }

    fn contains_world_point(&self, point: Vec3) -> bool {
        self.occupied
            .contains(&(point / VOXEL_SIZE).floor().as_ivec3())
    }
}

#[derive(Debug, Clone, Copy)]
struct DirectedCameraRig {
    axis_origin: Vec3,
    camera_side: Vec3,
    distance_scale: f32,
    yaw_degrees: f32,
    subject_count: usize,
    group_spread: f32,
}

impl DirectedCameraRig {
    /// Maximum horizontal distance between composition subjects that still
    /// uses the group-center framing for 3+ players. Beyond this, the group
    /// cannot fit in one shot, so each line frames the actual speaker instead
    /// of a midpoint that may be far away from everyone.
    const GROUP_SPREAD_LIMIT: f32 = 16.0;
    const LINE_MARGIN: f32 = 0.25;
    /// Maximum push applied to pull a candidate back across the scene axis.
    /// The correction only nudges cameras that are slightly over the line;
    /// when subjects are spread far apart the candidate near the speaker is
    /// legitimately deep on the "wrong" side of the centroid axis, and
    /// teleporting it across the whole scene would lose the standee entirely.
    const MAX_SIDE_PUSH: f32 = 2.5;

    fn for_dialogue(
        base: &Transform,
        dialogue: &[ReplayDialogue],
        speaker_positions: &HashMap<u64, Vec3>,
        distance_scale: f32,
        yaw_degrees: f32,
    ) -> Self {
        let mut subjects = Vec::<Vec3>::new();
        for (index, line) in dialogue.iter().enumerate() {
            if !replay_dialogue_is_playable(line) {
                continue;
            }
            let Some(position) = replay_dialogue_focus_position(dialogue, index, speaker_positions)
            else {
                continue;
            };
            if subjects
                .iter()
                .all(|subject| horizontal(*subject - position).length_squared() > 0.01)
            {
                subjects.push(position);
            }
        }
        let mut composition_subjects = subjects.clone();
        let mut scene_subjects = speaker_positions.iter().collect::<Vec<_>>();
        scene_subjects.sort_by_key(|(speaker_id, _)| **speaker_id);
        for (_, position) in scene_subjects {
            if composition_subjects
                .iter()
                .all(|subject| horizontal(*subject - *position).length_squared() > 0.01)
            {
                composition_subjects.push(*position);
            }
        }
        let first = composition_subjects.first().copied();
        let second = composition_subjects.get(1).copied();
        let composition_origin = (!composition_subjects.is_empty()).then(|| {
            composition_subjects.iter().copied().sum::<Vec3>() / composition_subjects.len() as f32
        });
        let group_spread = composition_subjects
            .iter()
            .flat_map(|first| {
                composition_subjects
                    .iter()
                    .map(move |second| horizontal(*first - *second).length())
            })
            .fold(0.0_f32, f32::max);
        let (axis_origin, mut camera_side) = match (first, second) {
            (Some(first), Some(second)) => {
                let axis = horizontal(second - first)
                    .try_normalize()
                    .unwrap_or(Vec3::X);
                (
                    composition_origin.unwrap_or((first + second) * 0.5),
                    Vec3::new(-axis.z, 0.0, axis.x),
                )
            },
            (Some(target), None) => {
                let side = horizontal(base.translation - target)
                    .try_normalize()
                    .unwrap_or_else(|| horizontal(base.rotation * Vec3::Z).normalize_or_zero());
                (target, side)
            },
            (None, None) => (
                Vec3::ZERO,
                horizontal(base.rotation * Vec3::Z)
                    .try_normalize()
                    .unwrap_or(Vec3::Z),
            ),
            (None, Some(_)) => unreachable!("a second speaker requires a first speaker"),
        };
        if camera_side.length_squared() <= f32::EPSILON {
            camera_side = Vec3::Z;
        }
        if horizontal(base.translation - axis_origin).dot(camera_side) < 0.0 {
            camera_side = -camera_side;
        }
        Self {
            axis_origin,
            camera_side,
            distance_scale: normalized_directed_camera_distance_scale(distance_scale),
            yaw_degrees: normalized_directed_camera_yaw_degrees(yaw_degrees),
            subject_count: composition_subjects.len(),
            group_spread,
        }
    }

    fn speaker_shot(
        &self,
        target: Vec3,
        shot: DirectorShot,
        distance_delta: f32,
        obstacles: &ReplayCameraObstacles,
    ) -> Transform {
        let base_distance = match shot {
            DirectorShot::SpeakerClose => 3.0,
            DirectorShot::SpeakerMedium => 5.0,
            DirectorShot::SpeakerWide => 8.0,
            DirectorShot::Establishing => 12.0,
            DirectorShot::Environment => 10.0,
        } * self.distance_scale;
        let height = match shot {
            DirectorShot::SpeakerClose => 1.1,
            DirectorShot::SpeakerMedium => 1.5,
            DirectorShot::SpeakerWide => 2.1,
            DirectorShot::Establishing | DirectorShot::Environment => 3.2,
        } * self.distance_scale;
        let camera_anchor =
            if self.subject_count >= 3 && self.group_spread <= Self::GROUP_SPREAD_LIMIT {
                self.axis_origin
            } else {
                target
            };
        let static_position = self.visible_position(
            camera_anchor,
            base_distance,
            height,
            obstacles,
        );
        let dolly_axis = (static_position - target).normalize();
        let desired_position =
            static_position + dolly_axis * (distance_delta * self.distance_scale);
        let position = if self.signed_side(desired_position) < Self::LINE_MARGIN
            || !obstacles.camera_is_clear(desired_position)
        {
            static_position
        } else {
            desired_position
        };
        Transform::from_translation(position).looking_at(target, Vec3::Y)
    }

    fn director_shot(
        &self,
        target: Vec3,
        shot: DirectorShot,
        motion: DirectorMotion,
        progress: f32,
        obstacles: &ReplayCameraObstacles,
    ) -> Transform {
        let distance_delta = match motion {
            DirectorMotion::DollyIn => -0.35 * progress,
            DirectorMotion::DollyOut => 0.35 * progress,
            DirectorMotion::Static | DirectorMotion::DriftLeft | DirectorMotion::DriftRight => 0.0,
        };
        self.speaker_shot(target, shot, distance_delta, obstacles)
    }

    fn signed_side(&self, position: Vec3) -> f32 {
        horizontal(position - self.axis_origin).dot(self.camera_side)
    }

    fn visible_position(
        &self,
        target: Vec3,
        base_distance: f32,
        height: f32,
        obstacles: &ReplayCameraObstacles,
    ) -> Vec3 {
        const YAW_OFFSETS_DEGREES: [f32; 9] =
            [0.0, 15.0, -15.0, 30.0, -30.0, 45.0, -45.0, 60.0, -60.0];
        const DISTANCE_SCALES: [f32; 10] = [1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2, 0.12];
        const HEIGHT_SCALES: [f32; 3] = [1.0, 0.75, 1.25];

        let mut best = None::<(f32, Vec3)>;
        for yaw_offset_degrees in YAW_OFFSETS_DEGREES {
            let yaw_degrees = self.yaw_degrees + yaw_offset_degrees;
            let direction = Quat::from_rotation_y(yaw_degrees.to_radians()) * self.camera_side;
            for distance_scale in DISTANCE_SCALES {
                for height_scale in HEIGHT_SCALES {
                    let mut candidate = target
                        + direction * (base_distance * distance_scale)
                        + Vec3::Y * (height * height_scale);
                    let signed_side = self.signed_side(candidate);
                    if signed_side < Self::LINE_MARGIN {
                        candidate += self.camera_side
                            * (Self::LINE_MARGIN - signed_side).min(Self::MAX_SIDE_PUSH);
                    }
                    if !obstacles.camera_is_clear(candidate) {
                        continue;
                    }
                    let score = distance_scale * 10.0
                        - yaw_offset_degrees.abs() * 0.05
                        - (height_scale - 1.0).abs() * 0.5;
                    if best.is_none_or(|(best_score, _)| score > best_score) {
                        best = Some((score, candidate));
                    }
                }
            }
        }
        best.map(|(_, position)| position).unwrap_or_else(|| {
            let mut fallback =
                target + self.camera_side * (VOXEL_SIZE * 2.0) + Vec3::Y * VOXEL_SIZE;
            let signed_side = self.signed_side(fallback);
            if signed_side < Self::LINE_MARGIN {
                fallback +=
                    self.camera_side * (Self::LINE_MARGIN - signed_side).min(Self::MAX_SIDE_PUSH);
            }
            fallback
        })
    }
}

fn horizontal(vector: Vec3) -> Vec3 { Vec3::new(vector.x, 0.0, vector.z) }

fn interpolated_camera(
    frames: &[ReplayCameraKeyframe],
    time_ms: u64,
    transition_curve: f32,
) -> Option<Transform> {
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
    let linear_t = time_ms.saturating_sub(left.time_ms) as f32 / span as f32;
    let t = camera_easing_t(linear_t, transition_curve);
    let left_rotation = Quat::from_array(left.rotation).normalize();
    let right_rotation = Quat::from_array(right.rotation).normalize();
    Some(Transform {
        translation: Vec3::from_array(left.translation)
            .lerp(Vec3::from_array(right.translation), t),
        rotation: left_rotation.slerp(right_rotation, t),
        ..default()
    })
}

fn camera_easing_t(linear_t: f32, transition_curve: f32) -> f32 {
    let linear_t = linear_t.clamp(0.0, 1.0);
    let curve = normalized_camera_transition_curve(transition_curve);
    if linear_t <= 0.5 {
        0.5 * (linear_t * 2.0).powf(curve)
    } else {
        1.0 - 0.5 * ((1.0 - linear_t) * 2.0).powf(curve)
    }
}

fn interpolated_player_position(
    movements: &[ReplayPlayerMovement],
    user_id: u64,
    time_ms: u64,
    curve: f32,
) -> Option<Vec3> {
    let movement = movements.iter().rev().find(|movement| {
        movement.user_id == user_id
            && movement
                .keyframes
                .first()
                .is_some_and(|frame| frame.time_ms <= time_ms)
    })?;
    interpolated_player_movement(&movement.keyframes, time_ms, curve)
}

fn interpolated_player_movement(
    frames: &[ReplayPlayerMovementKeyframe],
    time_ms: u64,
    curve: f32,
) -> Option<Vec3> {
    let first = frames.first()?;
    let next_index = frames.partition_point(|frame| frame.time_ms <= time_ms);
    if next_index == 0 {
        return Some(Vec3::from_array(first.position_cells) * VOXEL_SIZE);
    }
    if next_index >= frames.len() {
        return Some(Vec3::from_array(frames.last()?.position_cells) * VOXEL_SIZE);
    }
    let left_index = next_index - 1;
    let right_index = next_index;
    let left = &frames[left_index];
    let right = &frames[right_index];
    let span = right.time_ms.saturating_sub(left.time_ms).max(1);
    let t = time_ms.saturating_sub(left.time_ms) as f32 / span as f32;
    let p0 = Vec3::from_array(frames[left_index.saturating_sub(1)].position_cells);
    let p1 = Vec3::from_array(left.position_cells);
    let p2 = Vec3::from_array(right.position_cells);
    let p3 = Vec3::from_array(frames.get(right_index + 1).unwrap_or(right).position_cells);
    let t2 = t * t;
    let t3 = t2 * t;
    let curved = (0.5
        * (2.0 * p1
            + (-p0 + p2) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3))
        .clamp(p1.min(p2), p1.max(p2));
    let linear = p1.lerp(p2, t);
    let smoothed = linear.lerp(
        curved,
        normalized_player_movement_curve(curve),
    );
    Some(smoothed * VOXEL_SIZE)
}

fn interpolated_standee_position(samples: &[ReplayStandeePosition], time_ms: u64) -> Option<Vec3> {
    let first = samples.first()?;
    if time_ms <= first.time_ms {
        return Some(Vec3::from_array(first.position));
    }
    let index = samples.partition_point(|sample| sample.time_ms <= time_ms);
    if index >= samples.len() {
        return Some(Vec3::from_array(
            samples.last()?.position,
        ));
    }
    let left = &samples[index - 1];
    let right = &samples[index];
    let span = right.time_ms.saturating_sub(left.time_ms).max(1);
    let fraction = ((time_ms - left.time_ms) as f32 / span as f32).clamp(0.0, 1.0);
    Some(Vec3::from_array(left.position).lerp(
        Vec3::from_array(right.position),
        fraction,
    ))
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
    snapshot: &ReplayMessageSnapshot,
    metadata_estimated: bool,
) -> Option<ReplayDialogue> {
    let text = message.text.trim();
    if text.is_empty() {
        return None;
    }
    let is_gm = replay_sender_is_gm(message.sender_id, manager);
    let character = if is_gm {
        None
    } else {
        manager
            .player_characters
            .get(&message.sender_id.to_string())
            .or_else(|| {
                message
                    .character_id
                    .as_ref()
                    .and_then(|character_id| manager.player_characters.get(character_id))
            })
    };
    let (name, role, avatar) = dialogue_identity(manager, message, character);
    let avatar = resolve_character_image_source(manager, &avatar);
    let side = speaker_side(is_gm);
    let camera_focus_id = is_gm
        .then(|| match message.visibility {
            Visibility::Player(player_id) => Some(player_id),
            _ => message
                .character_id
                .as_deref()
                .and_then(|character_id| character_id.parse().ok()),
        })
        .flatten();
    Some(ReplayDialogue {
        time_ms,
        duration_ms: dialogue_duration_ms(text),
        duration_locked: false,
        sender_id: message.sender_id,
        camera_focus_id,
        name,
        role,
        text: text.to_owned(),
        speech_text: None,
        speech_enabled: true,
        speech_rate: default_line_speech_rate(),
        speech_volume: default_line_speech_volume(),
        avatar,
        avatar_data_url: None,
        visibility: message.visibility.clone(),
        side,
        line_id: snapshot.line_id,
        source_time: message.time,
        turn_index: snapshot.turn_index,
        position_cells: snapshot.position_cells,
        area: String::new(),
        included: true,
        snapshot_recorded: true,
        metadata_estimated,
        forwarded: message.forwarded,
    })
}

fn estimated_replay_snapshot(
    message: &CampaignMessage,
    manager: &NapcatMessageManager,
    speaker_positions: &HashMap<u64, Vec3>,
) -> ReplayMessageSnapshot {
    let turn_index = manager
        .current_group()
        .map(|group| {
            group
                .player_turns
                .get(&message.sender_id.to_string())
                .map(|turn| turn.turns_passed)
                .unwrap_or(group.world_turn)
        })
        .unwrap_or_default();
    let position_cells = speaker_positions
        .get(&message.sender_id)
        .map(|position| (*position / VOXEL_SIZE).round().as_ivec3().to_array())
        .unwrap_or([0, 0, 0]);
    ReplayMessageSnapshot {
        line_id: 0,
        turn_index,
        position_cells,
    }
}

fn normalize_dialogue_sides(replay: &mut ReplayFile, manager: &NapcatMessageManager) {
    deduplicate_broadcast_dialogue(&mut replay.dialogue, manager);
    for dialogue in &mut replay.dialogue {
        dialogue.side = speaker_side(replay_sender_is_gm(
            dialogue.sender_id,
            manager,
        ));
    }
}

fn replay_sender_is_gm(sender_id: u64, manager: &NapcatMessageManager) -> bool {
    // Locally authored messages use zero until NapCat has supplied the bot's QQ id.
    sender_id == 0 || manager.is_gm_user(sender_id)
}

fn deduplicate_broadcast_dialogue(
    dialogue: &mut Vec<ReplayDialogue>,
    manager: &NapcatMessageManager,
) {
    let mut seen = HashSet::<(u64, u64, String)>::new();
    dialogue.retain(|line| {
        if line.source_time == 0
            || (!line.forwarded && !replay_sender_is_gm(line.sender_id, manager))
        {
            return true;
        }
        seen.insert((
            line.sender_id,
            line.source_time,
            line.text.trim().to_owned(),
        ))
    });
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
    manager: &NapcatMessageManager,
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
    let avatar =
        crate::napcat::resolve_player_portrait_image(&manager.player_characters, character);
    (name, role, avatar)
}

fn dialogue_duration_ms(text: &str) -> u64 {
    let reading_ms = text.chars().fold(300_u64, |total, character| {
        let character_ms = match character {
            '。' | '！' | '？' | '!' | '?' | '；' | ';' => 110,
            '，' | ',' | '、' | '：' | ':' => 50,
            character if character.is_whitespace() => 8,
            character if is_cjk_character(character) => 62,
            _ => 23,
        };
        total.saturating_add(character_ms)
    });
    reading_ms
        .saturating_mul(5)
        .saturating_div(2)
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

fn active_dialogue_index(dialogue: &[ReplayDialogue], time_ms: u64) -> Option<usize> {
    dialogue.iter().rposition(|line| {
        replay_dialogue_is_playable(line)
            && time_ms >= line.time_ms
            && time_ms < line.time_ms.saturating_add(line.duration_ms)
    })
}

fn replay_speech_preparation_indices(replay: &ReplayFile, playback_ms: u64) -> Vec<usize> {
    let priority = active_dialogue_index(&replay.dialogue, playback_ms)
        .filter(|index| replay.dialogue[*index].speech_enabled)
        .or_else(|| {
            replay.dialogue.iter().position(|line| {
                replay_dialogue_is_playable(line)
                    && line.speech_enabled
                    && line.time_ms >= playback_ms
            })
        });
    priority
        .into_iter()
        .chain(
            replay
                .dialogue
                .iter()
                .enumerate()
                .filter_map(|(index, line)| {
                    (replay_dialogue_is_playable(line)
                        && line.speech_enabled
                        && Some(index) != priority)
                        .then_some(index)
                }),
        )
        .collect()
}

fn replay_speech_generation_line_ids(replay: &ReplayFile) -> HashSet<u64> {
    replay
        .dialogue
        .iter()
        .filter(|line| replay_dialogue_is_playable(line) && line.speech_enabled)
        .map(|line| line.line_id)
        .collect()
}

fn speech_generation_requested(
    line_id: u64,
    cue: (u64, usize),
    generation_line_ids: &HashSet<u64>,
    generation_cues: &HashSet<(u64, usize)>,
) -> bool {
    generation_line_ids.contains(&line_id) || generation_cues.contains(&cue)
}

fn replay_dialogue_is_ready_for_display(
    studio: &ReplayStudio,
    speech: &PreviewSpeechController,
    dialogue_index: usize,
    tts_available: bool,
) -> bool {
    if studio.mode != ReplayMode::Playing || !studio.speech_enabled || !tts_available {
        return true;
    }
    let Some(replay) = studio.replay.as_ref() else {
        return false;
    };
    if dialogue_index >= replay.dialogue.len() {
        return false;
    }
    if !replay.dialogue[dialogue_index].speech_enabled {
        return true;
    }
    let cue = (
        replay.created_at_unix_ms,
        dialogue_index,
    );
    if studio.speech_bypass_cues.contains(&cue) {
        return true;
    }
    speech.onnx_cue_finished(
        replay_voice_signature(replay, studio.speech_volume),
        cue,
    )
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
    let mut image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let max_side = ctx.input(|i| i.max_texture_side).max(1) as u32;
    if image.width() > max_side || image.height() > max_side {
        let scale = max_side as f32 / image.width().max(image.height()) as f32;
        let new_width = ((image.width() as f32) * scale).round().max(1.0) as u32;
        let new_height = ((image.height() as f32) * scale).round().max(1.0) as u32;
        image = image::imageops::resize(
            &image,
            new_width,
            new_height,
            image::imageops::FilterType::Lanczos3,
        );
    }
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

fn import_replay(path: &str, manager: Option<&NapcatMessageManager>) -> Result<ReplayFile, String> {
    let path = normalized_path(path)?;
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    let mut replay: ReplayFile = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
    replay.camera_distance_scale =
        normalized_directed_camera_distance_scale(replay.camera_distance_scale);
    replay.camera_yaw_degrees = normalized_directed_camera_yaw_degrees(replay.camera_yaw_degrees);
    replay.camera_transition_curve =
        normalized_camera_transition_curve(replay.camera_transition_curve);
    replay.player_movement_curve = normalized_player_movement_curve(replay.player_movement_curve);
    if !matches!(
        replay.format_version,
        LEGACY_REPLAY_FORMAT_VERSION | AREA_REPLAY_FORMAT_VERSION | REPLAY_FORMAT_VERSION
    ) {
        return Err(format!(
            "不支持的回放版本 {}（当前支持 {}、{} 和 {}）",
            replay.format_version,
            LEGACY_REPLAY_FORMAT_VERSION,
            AREA_REPLAY_FORMAT_VERSION,
            REPLAY_FORMAT_VERSION
        ));
    }
    if replay.audience == ReplayAudience::Gm {
        replay.audience = ReplayAudience::All;
    }
    if replay.format_version != REPLAY_FORMAT_VERSION {
        if let Some(manager) = manager {
            normalize_dialogue_sides(&mut replay, manager);
        }
    }
    for line in &mut replay.dialogue {
        line.duration_ms = line.duration_ms.max(1);
        line.speech_rate = normalized_line_speech_rate(line.speech_rate);
        line.speech_volume = normalized_line_speech_volume(line.speech_volume);
    }
    assign_replay_line_ids(&mut replay.dialogue);
    if replay.format_version == LEGACY_REPLAY_FORMAT_VERSION {
        let legacy_duration_ms = replay.duration_ms.max(1);
        replay.dialogue.sort_by_key(|line| {
            (
                line.time_ms,
                line.source_time,
                line.line_id,
            )
        });
        for line in &mut replay.dialogue {
            line.area = "旧时间线".to_owned();
            line.included = true;
            line.snapshot_recorded = false;
        }
        replay.area_blocks = if replay.dialogue.is_empty() {
            Vec::new()
        } else {
            vec![ReplayAreaBlock {
                id: 1,
                area: "旧时间线".to_owned(),
                line_ids: replay.dialogue.iter().map(|line| line.line_id).collect(),
            }]
        };
        let compact_duration_ms = compile_area_block_timeline(&mut replay);
        for frame in &mut replay.camera {
            frame.time_ms = ((frame.time_ms as u128 * compact_duration_ms as u128)
                / legacy_duration_ms as u128) as u64;
        }
        for movement in &mut replay.player_movements {
            for frame in &mut movement.keyframes {
                frame.time_ms = ((frame.time_ms as u128 * compact_duration_ms as u128)
                    / legacy_duration_ms as u128) as u64;
            }
        }
        replay.duration_ms = compact_duration_ms;
        extend_replay_for_speech(&mut replay);
    }
    replay.format_version = REPLAY_FORMAT_VERSION;
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
    use std::time::Duration;

    use bevy::ecs::system::RunSystemOnce;
    use bevy_egui::egui::{
        Context,
        Event,
        Modifiers,
        PointerButton,
        RawInput,
        Shape,
    };
    use serde_json::{
        from_str,
        to_string,
        to_value,
    };

    use super::*;

    #[test]
    fn preview_and_export_movement_preparation_is_shared_and_idempotent() {
        let mut line = test_dialogue(0, 1_000, DialogueSide::Right);
        line.sender_id = 7;
        let mut replay = test_replay(vec![line]);
        replay.duration_ms = 1_000;
        replay.campaign_id = "default".to_owned();
        replay.created_at_unix_ms = 1_000;
        replay.ship_trajectory_history_cursor_unix_ms = 1_000;
        let history = ReplayShipTrajectoryHistory {
            sessions: vec![PersistedShipTrajectorySession {
                campaign_id: "default".to_owned(),
                ship_id: "ship".to_owned(),
                ship_name: "船".to_owned(),
                keyframes: vec![
                    PersistedShipKeyframe {
                        source_unix_ms: 2_000,
                        translation: [0.0; 3],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    PersistedShipKeyframe {
                        source_unix_ms: 3_000,
                        translation: [10.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                ],
                ..Default::default()
            }],
        };
        let movements = ReplayPlayerMovementHistory {
            sessions: vec![PersistedPlayerMovementSession {
                campaign_id: "default".to_owned(),
                user_id: 7,
                keyframes: vec![
                    PersistedPlayerMovementKeyframe {
                        source_unix_ms: 2_000,
                        position_cells: [0.0; 3],
                    },
                    PersistedPlayerMovementKeyframe {
                        source_unix_ms: 3_000,
                        position_cells: [10.0, 0.0, 0.0],
                    },
                ],
                ..Default::default()
            }],
        };
        let mut studio = ReplayStudio::default();
        studio.replay = Some(replay);
        prepare_replay_movement_for_playback(&mut studio, &movements, &history);
        assert_eq!(
            studio.replay.as_ref().unwrap().ship_trajectories.len(),
            1
        );
        assert_eq!(studio.replay.as_ref().unwrap().player_movements.len(), 1);
        assert!(studio.replay.as_ref().unwrap().duration_ms > 1_000);
        assert_eq!(studio.playback_ms, 0);
        let prepared = to_string(studio.replay.as_ref().unwrap()).unwrap();
        prepare_replay_movement_for_playback(&mut studio, &movements, &history);
        assert_eq!(
            to_string(studio.replay.as_ref().unwrap()).unwrap(),
            prepared
        );
    }

    #[test]
    fn cancelling_live_takes_restores_duration_and_preserves_separate_scene_edits() {
        for ship_take in [false, true] {
            for scene_edit in [false, true] {
                let mut studio = ReplayStudio::default();
                let mut replay = test_replay(Vec::new());
                replay.duration_ms = 1_000;
                studio.replay = Some(replay);
                studio.playback_ms = 900;
                if ship_take {
                    begin_live_ship_take(
                        &mut studio,
                        "ship".into(),
                        "船".into(),
                        Transform::default(),
                    );
                } else {
                    begin_live_movement_take(&mut studio, 7, Vec3::ZERO);
                }
                studio.playback_ms = 2_000;
                let replay = studio.replay.as_mut().unwrap();
                replay.duration_ms = 2_000;
                if scene_edit {
                    replay.terrain_changes.push(ReplayTerrainChange {
                        time_ms: 1_500,
                        position: [0; 3],
                        material: 0,
                        enabled: true,
                    });
                }
                cancel_live_replay_take(&mut studio);
                let replay = studio.replay.as_ref().unwrap();
                let expected_end = if scene_edit { 1_500 } else { 1_000 };
                assert_eq!(replay.duration_ms, expected_end);
                assert_eq!(studio.playback_ms, expected_end);
                assert_eq!(studio.mode, ReplayMode::Paused);
                assert!(!replay_has_live_take(&studio));
                assert!(replay.player_movements.is_empty());
                assert!(replay.ship_trajectories.is_empty());
                assert_eq!(
                    replay.terrain_changes.len(),
                    usize::from(scene_edit)
                );
            }
        }
    }

    #[test]
    fn stopping_preview_discards_the_pending_take_and_its_empty_tail() {
        let mut studio = ReplayStudio::default();
        let mut replay = test_replay(Vec::new());
        replay.duration_ms = 1_000;
        studio.replay = Some(replay);
        begin_live_movement_take(&mut studio, 7, Vec3::ZERO);
        studio.replay.as_mut().unwrap().duration_ms = 2_000;
        studio.playback_ms = 2_000;
        let mut world = World::new();
        world.insert_resource(studio);
        world
            .run_system_once(
                |mut studio: ResMut<ReplayStudio>,
                 mut grids: Query<&mut Grid<u8>, With<TrpgVoxelGrid>>| {
                    stop_playback(&mut studio, &mut grids);
                },
            )
            .unwrap();
        let studio = world.resource::<ReplayStudio>();
        assert_eq!(studio.mode, ReplayMode::Idle);
        assert_eq!(studio.playback_ms, 0);
        assert_eq!(
            studio.replay.as_ref().unwrap().duration_ms,
            1_000
        );
        assert!(!replay_has_live_take(studio));
    }

    #[test]
    fn starting_another_take_cannot_discard_unsaved_samples() {
        let mut studio = ReplayStudio::default();
        studio.replay = Some(test_replay(vec![test_dialogue(0, 1_000, DialogueSide::Left)]));
        studio.playback_ms = 500;
        begin_live_movement_take(&mut studio, 7, Vec3::X);
        let turns = replay_turns(studio.replay.as_ref().unwrap());
        jump_replay_to_turn(&mut studio, &turns, 0);
        assert_eq!(studio.playback_ms, 500);
        assert_eq!(studio.mode, ReplayMode::Playing);
        begin_live_movement_take(&mut studio, 8, Vec3::Y);
        begin_live_ship_take(
            &mut studio,
            "ship".into(),
            "船".into(),
            Transform::default(),
        );
        let take = studio.live_movement_punch_in.as_ref().unwrap();
        assert_eq!(take.user_id, 7);
        assert_eq!(
            take.samples[0].position,
            Vec3::X.to_array()
        );
        assert!(studio.live_ship_punch_in.is_none());
        cancel_live_replay_take(&mut studio);
        begin_live_ship_take(
            &mut studio,
            "ship".into(),
            "船".into(),
            Transform::default(),
        );
        begin_live_movement_take(&mut studio, 8, Vec3::Y);
        assert!(studio.live_movement_punch_in.is_none());
        assert_eq!(
            studio.live_ship_punch_in.as_ref().unwrap().ship_id,
            "ship"
        );
    }

    #[test]
    fn stopping_history_only_movement_preserves_the_earlier_path() {
        let mut replay = test_replay(Vec::new());
        replay.player_movements.push(ReplayPlayerMovement {
            user_id: 7,
            keyframes: [0, 100, 200]
                .into_iter()
                .map(|time_ms| ReplayPlayerMovementKeyframe {
                    time_ms,
                    position_cells: [time_ms as f32, 0.0, 0.0],
                })
                .collect(),
        });
        stop_replay_movement_at(&mut replay, 7, 150, Vec3::Y);
        assert_eq!(replay.standee_positions.len(), 3);
        assert_eq!(
            interpolated_standee_position(&replay.standee_positions, 0),
            Some(Vec3::ZERO)
        );
        assert_eq!(
            interpolated_standee_position(&replay.standee_positions, 100),
            Some(Vec3::X * 100.0 * VOXEL_SIZE)
        );
        assert_eq!(
            interpolated_standee_position(&replay.standee_positions, 200),
            Some(Vec3::Y)
        );
    }

    #[test]
    fn position_frame_controls_only_write_when_enabled_and_clicked() {
        for enabled in [false, true] {
            let context = Context::default();
            let position = Vec3::new(4.0, -2.0, 8.0) * VOXEL_SIZE;
            let mut result = None;
            let output = context.run_ui(RawInput::default(), |ui| {
                result = replay_position_frame_editor(ui, position, enabled);
            });
            assert!(
                result.is_none(),
                "rendering must not author a frame"
            );
            let button_center = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    Shape::Text(text) if text.galley.text() == "保存当前位置帧" => {
                        Some(text.visual_bounding_rect().center())
                    },
                    _ => None,
                })
                .expect("rendered position-frame button");
            for pressed in [true, false] {
                let _ = context.run_ui(
                    RawInput {
                        events: vec![Event::PointerMoved(button_center), Event::PointerButton {
                            pos: button_center,
                            button: PointerButton::Primary,
                            pressed,
                            modifiers: Modifiers::default(),
                        }],
                        ..default()
                    },
                    |ui| {
                        result = replay_position_frame_editor(ui, position, enabled);
                    },
                );
            }
            assert_eq!(result, enabled.then_some(position));
        }
    }

    #[test]
    fn live_takes_extend_past_the_end_and_normal_playback_still_stops() {
        for ship_take in [false, true] {
            let mut replay = test_replay(vec![test_dialogue(
                0,
                1_000,
                DialogueSide::Right,
            )]);
            replay.duration_ms = 1_000;
            let mut studio = ReplayStudio::default();
            studio.replay = Some(replay);
            studio.playback_ms = 900;
            studio.speech_enabled = false;
            studio.turn_playback_enabled = true;
            if ship_take {
                begin_live_ship_take(
                    &mut studio,
                    "ship".into(),
                    "Ship".into(),
                    Transform::default(),
                );
            } else {
                begin_live_movement_take(&mut studio, 7, Vec3::ZERO);
            }
            let mut time = Time::<()>::default();
            time.advance_by(Duration::from_millis(200));
            let mut world = World::new();
            world.insert_resource(time);
            world.insert_resource(studio);
            world.init_resource::<PreviewSpeechController>();
            world.run_system_once(advance_replay).unwrap();
            world.run_system_once(advance_replay).unwrap();
            let mut studio = world.resource_mut::<ReplayStudio>();
            assert_eq!(studio.playback_ms, 1_300);
            assert_eq!(studio.mode, ReplayMode::Playing);
            assert_eq!(
                studio.replay.as_ref().unwrap().duration_ms,
                1_300
            );
            if ship_take {
                finish_live_ship_take(
                    &mut studio,
                    Some(Transform::from_xyz(2.0, 0.0, 0.0)),
                );
                assert_eq!(
                    studio.replay.as_ref().unwrap().ship_trajectories[0]
                        .keyframes
                        .last()
                        .unwrap()
                        .time_ms,
                    1_300
                );
            } else {
                finish_live_movement_take(&mut studio, Some(Vec3::X));
                assert_eq!(
                    studio.replay.as_ref().unwrap().player_movements[0]
                        .keyframes
                        .last()
                        .unwrap()
                        .time_ms,
                    1_300
                );
            }
            world.run_system_once(advance_replay).unwrap();
            let studio = world.resource::<ReplayStudio>();
            assert_eq!(studio.playback_ms, 1_300);
            assert_eq!(studio.mode, ReplayMode::Paused);
        }
    }

    #[test]
    fn replay_occlusion_tracks_every_player_not_only_the_focused_speaker() {
        let camera = Vec3::new(1.0, 2.0, 3.0);
        let players = [
            Vec3::new(4.0, 5.0, 6.0),
            Vec3::new(-4.0, 5.0, 6.0),
            Vec3::new(0.0, 1.0, 8.0),
        ];
        let mut fade = VoxelReplayOcclusionFade::default();

        set_replay_occlusion_targets(&mut fade, camera, players);

        assert!(fade.active);
        assert_eq!(fade.camera, camera);
        assert_eq!(fade.targets, players);
        assert_eq!(
            fade.cast_width_cells,
            DEFAULT_VOXEL_OCCLUSION_CAST_WIDTH_CELLS
        );
        assert_eq!(
            fade.cast_height_cells,
            DEFAULT_VOXEL_OCCLUSION_CAST_HEIGHT_CELLS
        );
        assert_eq!(
            fade.cast_end_width_cells,
            DEFAULT_VOXEL_OCCLUSION_CAST_END_WIDTH_CELLS
        );
        assert_eq!(
            fade.cast_end_height_cells,
            DEFAULT_VOXEL_OCCLUSION_CAST_END_HEIGHT_CELLS
        );
    }

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
    fn all_replay_includes_every_visibility_scope() {
        for visibility in [
            Visibility::Public,
            Visibility::Party("split-a".to_owned()),
            Visibility::Player(7),
            Visibility::Gm,
            Visibility::System,
        ] {
            assert!(ReplayAudience::All.can_read_visibility(&visibility, None));
        }
    }

    #[test]
    fn replay_message_eligibility_never_crosses_campaigns() {
        let manager: NapcatMessageManager = serde_json::from_str(r#"{"messages":{}}"#).unwrap();
        let message = CampaignMessage {
            campaign_id: "campaign-a".to_owned(),
            sender_id: 7,
            sender_name: "player".to_owned(),
            source: crate::napcat::MessageSource::Gui,
            character_id: None,
            party_id: None,
            visibility: Visibility::Public,
            text: "line".to_owned(),
            time: 1_200,
            forwarded: false,
        };

        assert!(replay_message_is_eligible(
            &ReplayAudience::All,
            "campaign-a",
            &message,
            &manager,
        ));
        assert!(!replay_message_is_eligible(
            &ReplayAudience::All,
            "campaign-b",
            &message,
            &manager,
        ));
    }

    #[test]
    fn replay_snapshot_turn_comes_from_the_messages_own_campaign() {
        let mut manager: NapcatMessageManager = serde_json::from_str(r#"{"messages":{}}"#).unwrap();
        manager.trpg_groups.insert(
            "table-a".to_owned(),
            crate::napcat::TrpgGroup {
                campaign_id: "campaign-a".to_owned(),
                world_turn: 10,
                player_turns: HashMap::from([(
                    "7".to_owned(),
                    crate::napcat::TrpgPlayerTurnState {
                        turns_passed: 3,
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        );
        manager.trpg_groups.insert(
            "table-b".to_owned(),
            crate::napcat::TrpgGroup {
                campaign_id: "campaign-b".to_owned(),
                world_turn: 20,
                player_turns: HashMap::from([(
                    "7".to_owned(),
                    crate::napcat::TrpgPlayerTurnState {
                        turns_passed: 8,
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        );
        manager.current_trpg_group = Some("table-a".to_owned());

        assert_eq!(
            replay_message_turn_index(&manager, "campaign-a", 7),
            3
        );
        assert_eq!(
            replay_message_turn_index(&manager, "campaign-b", 7),
            8
        );
    }

    #[test]
    fn automatic_areas_use_horizontal_voxel_distance() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 1_200, [0, 100, 0]),
            positioned_dialogue(2, 1, 1_208, [12, -100, 0]),
            positioned_dialogue(3, 1, 1_205, [30, 0, 0]),
        ];
        assign_replay_line_ids(&mut lines);
        let mut replay = test_replay(lines);

        auto_group_replay_areas(&mut replay);

        assert_eq!(
            replay.dialogue[0].area,
            replay.dialogue[1].area
        );
        assert_ne!(
            replay.dialogue[0].area,
            replay.dialogue[2].area
        );
    }

    #[test]
    fn legacy_history_uses_an_editable_estimated_position() {
        let manager: NapcatMessageManager = serde_json::from_str(r#"{"messages":{}}"#).unwrap();
        let message = CampaignMessage {
            campaign_id: "campaign".to_owned(),
            sender_id: 7,
            sender_name: "player".to_owned(),
            source: crate::napcat::MessageSource::Gui,
            character_id: None,
            party_id: None,
            visibility: Visibility::Public,
            text: "old line".to_owned(),
            time: 1_200,
            forwarded: false,
        };
        let snapshot = estimated_replay_snapshot(
            &message,
            &manager,
            &HashMap::from([(7, Vec3::new(2.0, 3.0, -1.0))]),
        );
        let dialogue = dialogue_from_message(&message, &manager, 350, &snapshot, true).unwrap();

        assert_eq!(dialogue.position_cells, [8, 12, -4]);
        assert!(dialogue.included);
        assert!(dialogue.snapshot_recorded);
        assert!(dialogue.metadata_estimated);
    }

    #[test]
    fn camera_uses_the_position_saved_on_each_line() {
        let mut line = positioned_dialogue(1, 1, 1_200, [40, 4, -12]);
        line.time_ms = 350;
        line.duration_ms = 600;
        let saved_position = IVec3::from_array(line.position_cells).as_vec3() * VOXEL_SIZE;
        let frames = turn_based_camera_track(
            &Transform::from_xyz(0.0, 3.0, 4.0),
            &[line],
            1_000,
            &HashMap::from([(1, Vec3::new(-100.0, 0.0, 0.0))]),
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
            &ReplayCameraObstacles::default(),
        );
        let shot = frame_transform(frames.iter().find(|frame| frame.time_ms == 350).unwrap());
        let forward = shot.rotation * Vec3::NEG_Z;
        assert!(
            forward.dot((saved_position - shot.translation).normalize()) > 0.99,
            "camera must ignore the speaker's current standee position"
        );
    }

    #[test]
    fn recorded_voxel_origin_overrides_the_live_standee_position() {
        let mut line = positioned_dialogue(1, 1, 1_200, [0, 0, 0]);
        line.sender_id = 7;
        line.time_ms = 350;

        let position = replay_standee_position_at(
            &[line],
            7,
            350,
            &HashMap::from([(7, Vec3::new(99.0, 2.0, -50.0))]),
        );

        assert_eq!(position, Some(Vec3::ZERO));
    }

    #[test]
    fn gm_camera_line_focuses_on_the_addressed_player_standee() {
        let target = Vec3::new(8.0, 1.0, -3.0);
        let mut line = test_dialogue(350, 600, DialogueSide::Left);
        line.sender_id = 0;
        line.camera_focus_id = Some(7);
        line.snapshot_recorded = true;
        line.position_cells = [0, 0, 0];
        let frames = turn_based_camera_track(
            &Transform::from_xyz(0.0, 3.0, 4.0),
            &[line],
            1_000,
            &HashMap::from([(7, target)]),
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
            &ReplayCameraObstacles::default(),
        );

        let shot = frame_transform(frames.iter().find(|frame| frame.time_ms == 350).unwrap());
        let forward = shot.rotation * Vec3::NEG_Z;
        assert!(
            forward.dot((target - shot.translation).normalize()) > 0.99,
            "GM dialogue must frame the addressed player, not the GM origin"
        );
    }

    #[test]
    fn legacy_gm_camera_line_focuses_on_the_previous_player() {
        let target = Vec3::new(8.0, 1.0, -3.0);
        let mut player_line = test_dialogue(350, 600, DialogueSide::Right);
        player_line.sender_id = 7;
        player_line.snapshot_recorded = true;
        player_line.position_cells = [32, 4, -12];
        let mut gm_line = test_dialogue(1_000, 600, DialogueSide::Left);
        gm_line.sender_id = 0;
        gm_line.snapshot_recorded = true;
        gm_line.position_cells = [0, 0, 0];
        let dialogue = [player_line, gm_line];
        let speaker_positions = replay_speaker_positions(&dialogue);
        let frames = turn_based_camera_track(
            &Transform::from_xyz(0.0, 3.0, 4.0),
            &dialogue,
            1_700,
            &speaker_positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
            &ReplayCameraObstacles::default(),
        );

        let shot = frame_transform(frames.iter().find(|frame| frame.time_ms == 1_000).unwrap());
        let forward = shot.rotation * Vec3::NEG_Z;
        assert!(
            forward.dot((target - shot.translation).normalize()) > 0.99,
            "legacy GM dialogue must inherit the nearest player's standee"
        );
    }

    #[test]
    fn area_blocks_cap_distinct_turns_at_three() {
        let mut lines = (1..=4)
            .map(|turn| {
                let mut line = positioned_dialogue(turn as u64, 1, turn as u64, [0, 0, 0]);
                line.turn_index = turn;
                line.area = "area1".to_owned();
                line
            })
            .collect::<Vec<_>>();
        assign_replay_line_ids(&mut lines);
        let mut replay = test_replay(lines);

        rebuild_area_blocks(&mut replay);

        assert_eq!(replay.area_blocks.len(), 2);
        assert_eq!(replay.area_blocks[0].line_ids.len(), 3);
        assert_eq!(replay.area_blocks[1].line_ids.len(), 1);
    }

    #[test]
    fn generated_blocks_are_globally_ordered_by_their_earliest_source_time() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 1, [0, 0, 0]),
            positioned_dialogue(2, 2, 2, [0, 0, 0]),
            positioned_dialogue(3, 3, 3, [0, 0, 0]),
            positioned_dialogue(4, 4, 100, [0, 0, 0]),
            positioned_dialogue(5, 1, 50, [40, 0, 0]),
        ];
        for line in &mut lines[..4] {
            line.area = "区域 A".to_owned();
        }
        lines[4].area = "区域 B".to_owned();
        let mut replay = test_replay(lines);

        rebuild_area_blocks(&mut replay);

        assert_eq!(
            replay
                .area_blocks
                .iter()
                .map(|block| block.line_ids.clone())
                .collect::<Vec<_>>(),
            vec![vec![1, 2, 3], vec![5], vec![4]]
        );
        assert_eq!(
            replay
                .area_blocks
                .iter()
                .map(|block| block.id)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn dialogue_drag_moves_a_line_into_the_target_block() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 1_200, [0, 0, 0]),
            positioned_dialogue(2, 1, 1_201, [0, 0, 0]),
            positioned_dialogue(3, 1, 1_202, [30, 0, 0]),
        ];
        lines[0].area = "区域 1".to_owned();
        lines[1].area = "区域 1".to_owned();
        lines[2].area = "区域 2".to_owned();
        let mut replay = test_replay(lines);
        replay.area_blocks = vec![
            ReplayAreaBlock {
                id: 1,
                area: "区域 1".to_owned(),
                line_ids: vec![1, 2],
            },
            ReplayAreaBlock {
                id: 2,
                area: "区域 2".to_owned(),
                line_ids: vec![3],
            },
        ];

        assert!(move_replay_dialogue(
            &mut replay,
            1,
            3,
            true
        ));

        assert_eq!(replay.area_blocks[0].line_ids, vec![2]);
        assert_eq!(replay.area_blocks[1].line_ids, vec![
            3, 1
        ]);
        assert_eq!(
            replay
                .dialogue
                .iter()
                .map(|line| line.line_id)
                .collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
        assert_eq!(
            replay
                .dialogue
                .iter()
                .find(|line| line.line_id == 1)
                .unwrap()
                .area,
            "区域 2"
        );
    }

    #[test]
    fn manually_inserted_dm_dialogue_order_survives_appending_and_import() {
        let mut replay = test_replay(vec![
            positioned_dialogue(1, 1, 100, [0, 0, 0]),
            positioned_dialogue(2, 1, 200, [0, 0, 0]),
        ]);
        replay.area_blocks = vec![ReplayAreaBlock {
            id: 1,
            area: "甲板".to_owned(),
            line_ids: vec![1, 2],
        }];
        let inserted_id = append_dm_replay_dialogue(&mut replay);
        replay.dialogue.last_mut().unwrap().text = "门后传来脚步声。".to_owned();
        recompile_edited_replay_dialogue(&mut replay);
        assert!(!replay.manual_dialogue_order);
        assert_eq!(
            replay.dialogue.last().unwrap().line_id,
            inserted_id
        );

        assert!(move_replay_dialogue(
            &mut replay,
            inserted_id,
            2,
            false
        ));
        recompile_edited_replay_dialogue(&mut replay);
        assert!(replay.manual_dialogue_order);
        assert_eq!(
            replay
                .dialogue
                .iter()
                .map(|line| line.line_id)
                .collect::<Vec<_>>(),
            vec![1, inserted_id, 2]
        );

        let appended_id = append_dm_replay_dialogue(&mut replay);
        replay.dialogue.last_mut().unwrap().text = "脚步声停下了。".to_owned();
        recompile_edited_replay_dialogue(&mut replay);
        let expected_ids = vec![1, inserted_id, 2, appended_id];
        let expected_sources = vec![100, 201, 200, 202];
        assert_eq!(
            replay
                .dialogue
                .iter()
                .map(|line| line.line_id)
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(
            replay
                .dialogue
                .iter()
                .map(|line| line.source_time)
                .collect::<Vec<_>>(),
            expected_sources
        );

        let directory = TempDir::new().unwrap();
        let path = directory.path().join("manual-dm-order.willow-replay.json");
        export_replay(&replay, path.to_str().unwrap()).unwrap();
        let mut imported = import_replay(path.to_str().unwrap(), None).unwrap();
        recompile_edited_replay_dialogue(&mut imported);
        assert!(imported.manual_dialogue_order);
        assert_eq!(
            imported
                .dialogue
                .iter()
                .map(|line| line.line_id)
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(
            imported
                .dialogue
                .iter()
                .map(|line| line.source_time)
                .collect::<Vec<_>>(),
            expected_sources
        );

        // Projects saved before explicit ordering retain automatic GM boundaries.
        let mut old_json = to_value(&replay).unwrap();
        old_json
            .as_object_mut()
            .unwrap()
            .remove("manual_dialogue_order");
        fs::write(&path, to_string(&old_json).unwrap()).unwrap();
        let mut imported = import_replay(path.to_str().unwrap(), None).unwrap();
        recompile_edited_replay_dialogue(&mut imported);
        assert!(!imported.manual_dialogue_order);
        assert_eq!(
            imported
                .dialogue
                .iter()
                .map(|line| line.line_id)
                .collect::<Vec<_>>(),
            vec![1, 2, inserted_id, appended_id]
        );
    }

    #[test]
    fn deleting_dialogue_removes_empty_area_blocks() {
        let mut replay = test_replay(vec![
            positioned_dialogue(1, 1, 1_200, [0, 0, 0]),
            positioned_dialogue(2, 1, 1_201, [30, 0, 0]),
        ]);
        replay.area_blocks = vec![
            ReplayAreaBlock {
                id: 1,
                area: "区域 1".to_owned(),
                line_ids: vec![1],
            },
            ReplayAreaBlock {
                id: 2,
                area: "区域 2".to_owned(),
                line_ids: vec![2],
            },
        ];

        assert!(delete_replay_dialogue(&mut replay, 1));

        assert_eq!(replay.dialogue.len(), 1);
        assert_eq!(replay.area_blocks.len(), 1);
        assert_eq!(replay.area_blocks[0].line_ids, vec![2]);
    }

    #[test]
    fn select_all_and_clear_all_preserve_manual_block_layout() {
        let mut replay = test_replay(vec![
            positioned_dialogue(1, 1, 1_200, [0, 0, 0]),
            positioned_dialogue(2, 2, 1_201, [30, 0, 0]),
        ]);
        replay.area_blocks = vec![
            ReplayAreaBlock {
                id: 9,
                area: "后播放".to_owned(),
                line_ids: vec![2],
            },
            ReplayAreaBlock {
                id: 4,
                area: "先播放".to_owned(),
                line_ids: vec![1],
            },
        ];
        let expected = replay.area_blocks.clone();

        assert!(set_all_replay_dialogue_included(
            &mut replay,
            false
        ));
        replay.duration_ms = compile_area_block_timeline(&mut replay);
        assert_eq!(replay.area_blocks, expected);
        assert!(replay.dialogue.iter().all(|line| !line.included));

        assert!(set_all_replay_dialogue_included(
            &mut replay,
            true
        ));
        replay.duration_ms = compile_area_block_timeline(&mut replay);
        assert_eq!(replay.area_blocks, expected);
        assert!(replay.dialogue.iter().all(replay_dialogue_is_playable));
    }

    #[test]
    fn block_reassignment_prunes_the_empty_source_and_updates_area() {
        let mut replay = test_replay(vec![
            positioned_dialogue(1, 1, 1_200, [0, 0, 0]),
            positioned_dialogue(2, 1, 1_201, [30, 0, 0]),
        ]);
        replay.area_blocks = vec![
            ReplayAreaBlock {
                id: 1,
                area: "甲板".to_owned(),
                line_ids: vec![1],
            },
            ReplayAreaBlock {
                id: 2,
                area: "机库".to_owned(),
                line_ids: vec![2],
            },
        ];

        assert!(reassign_replay_dialogue_to_block(
            &mut replay,
            1,
            2
        ));

        assert_eq!(replay.area_blocks.len(), 1);
        assert_eq!(replay.area_blocks[0].id, 2);
        assert_eq!(replay.area_blocks[0].line_ids, vec![
            2, 1
        ]);
        assert_eq!(
            replay
                .dialogue
                .iter()
                .find(|line| line.line_id == 1)
                .unwrap()
                .area,
            "机库"
        );
    }

    #[test]
    fn line_area_edit_moves_only_that_line_without_rebuilding_other_blocks() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 1_200, [0, 0, 0]),
            positioned_dialogue(2, 1, 1_201, [0, 0, 0]),
            positioned_dialogue(3, 1, 1_202, [30, 0, 0]),
        ];
        lines[0].area = "甲板".to_owned();
        lines[1].area = "甲板".to_owned();
        lines[2].area = "机库".to_owned();
        let mut replay = test_replay(lines);
        replay.area_blocks = vec![
            ReplayAreaBlock {
                id: 1,
                area: "甲板".to_owned(),
                line_ids: vec![1, 2],
            },
            ReplayAreaBlock {
                id: 2,
                area: "机库".to_owned(),
                line_ids: vec![3],
            },
        ];

        assert!(apply_replay_dialogue_area_edit(
            &mut replay,
            1,
            "机库".to_owned()
        ));

        assert_eq!(replay.area_blocks[0].line_ids, vec![2]);
        assert_eq!(replay.area_blocks[1].line_ids, vec![
            3, 1
        ]);
        assert_eq!(replay.dialogue[0].area, "机库");
    }

    #[test]
    fn new_dm_dialogue_inherits_the_replay_scope_and_last_block() {
        let mut line = positioned_dialogue(4, 3, 1_200, [8, 4, -12]);
        line.sender_id = 77;
        line.area = "甲板".to_owned();
        let mut replay = test_replay(vec![line]);
        replay.audience = ReplayAudience::Party("split-a".to_owned());
        replay.area_blocks = vec![ReplayAreaBlock {
            id: 9,
            area: "甲板".to_owned(),
            line_ids: vec![4],
        }];

        let line_id = append_dm_replay_dialogue(&mut replay);
        let added = replay.dialogue.last().unwrap();

        assert_eq!(line_id, 5);
        assert_eq!(added.name, "DM");
        assert!(added.text.is_empty());
        assert_eq!(
            added.visibility,
            Visibility::Party("split-a".to_owned())
        );
        assert_eq!(added.camera_focus_id, Some(77));
        assert_eq!(added.turn_index, 3);
        assert_eq!(added.position_cells, [8, 4, -12]);
        assert_eq!(replay.area_blocks[0].line_ids, vec![
            4, 5
        ]);
    }

    #[test]
    fn dm_area_block_order_overrides_global_timestamps() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 1_200, [0, 0, 0]),
            positioned_dialogue(2, 2, 1_208, [1, 0, 0]),
            positioned_dialogue(3, 1, 1_205, [30, 0, 0]),
            positioned_dialogue(4, 2, 1_210, [30, 0, 0]),
        ];
        lines[0].area = "area1".to_owned();
        lines[1].area = "area1".to_owned();
        lines[2].area = "area2".to_owned();
        lines[3].area = "area2".to_owned();
        let mut replay = test_replay(lines);
        replay.area_blocks = vec![
            ReplayAreaBlock {
                id: 1,
                area: "area1".to_owned(),
                line_ids: vec![1, 2],
            },
            ReplayAreaBlock {
                id: 2,
                area: "area2".to_owned(),
                line_ids: vec![3, 4],
            },
        ];

        replay.duration_ms = compile_area_block_timeline(&mut replay);

        assert_eq!(
            replay
                .dialogue
                .iter()
                .map(|line| line.source_time)
                .collect::<Vec<_>>(),
            vec![1_200, 1_208, 1_205, 1_210]
        );
    }

    #[test]
    fn gm_lines_are_chronological_boundaries_between_area_groups() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 90, [30, 0, 0]),
            positioned_dialogue(2, 1, 100, [0, 0, 0]),
            positioned_dialogue(3, 1, 110, [0, 0, 0]),
        ];
        lines[0].area = "later block".to_owned();
        lines[1].area = "later block".to_owned();
        lines[1].side = DialogueSide::Left;
        lines[2].area = "first block".to_owned();
        let mut replay = test_replay(lines);
        replay.area_blocks = vec![
            ReplayAreaBlock {
                id: 1,
                area: "first block".to_owned(),
                line_ids: vec![3],
            },
            ReplayAreaBlock {
                id: 2,
                area: "later block".to_owned(),
                line_ids: vec![1, 2],
            },
        ];

        replay.duration_ms = compile_area_block_timeline(&mut replay);

        assert_eq!(
            replay
                .dialogue
                .iter()
                .map(|line| line.source_time)
                .collect::<Vec<_>>(),
            vec![90, 100, 110]
        );
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
            interpolated_camera(
                &frames,
                500,
                default_camera_transition_curve()
            )
            .unwrap()
            .translation
            .x,
            5.0
        );
        assert_eq!(
            interpolated_camera(
                &frames,
                250,
                default_camera_transition_curve()
            )
            .unwrap()
            .translation
            .x,
            1.25
        );
        assert_eq!(
            interpolated_camera(
                &frames,
                2_000,
                default_camera_transition_curve()
            )
            .unwrap()
            .translation
            .x,
            10.0
        );
        assert_eq!(
            interpolated_camera(&frames, 250, 1.0)
                .unwrap()
                .translation
                .x,
            2.5
        );
    }

    #[test]
    fn possessed_player_movement_is_sampled_in_voxel_cells_and_split_by_control_session() {
        let mut studio = ReplayStudio::default();
        studio.mode = ReplayMode::Recording;
        studio.replay = Some(test_replay(Vec::new()));
        let mut positions = HashMap::from([(42, Vec3::new(1.0, 2.0, 3.0))]);

        record_possessed_player_movement(&mut studio, Some(42), &positions, 0.0);
        studio.record_elapsed_ms = 100;
        positions.insert(42, Vec3::new(1.5, 2.0, 3.0));
        record_possessed_player_movement(
            &mut studio,
            Some(42),
            &positions,
            PLAYER_MOVEMENT_SAMPLE_SECONDS,
        );
        studio.record_elapsed_ms = 150;
        record_possessed_player_movement(&mut studio, None, &positions, 0.05);
        studio.record_elapsed_ms = 1_000;
        positions.insert(42, Vec3::new(4.0, 2.0, 3.0));
        record_possessed_player_movement(&mut studio, Some(42), &positions, 0.0);

        let movements = &studio.replay.as_ref().unwrap().player_movements;
        assert_eq!(movements.len(), 2);
        assert_eq!(movements[0].user_id, 42);
        assert_eq!(movements[0].keyframes.len(), 3);
        assert_eq!(
            movements[0].keyframes[0].position_cells,
            [4.0, 8.0, 12.0]
        );
        assert_eq!(
            movements[0].keyframes[1].position_cells,
            [6.0, 8.0, 12.0]
        );
        assert_eq!(movements[1].keyframes[0].time_ms, 1_000);
    }

    #[test]
    fn replay_player_movement_curves_between_samples_without_crossing_session_gaps() {
        let movements = vec![
            ReplayPlayerMovement {
                user_id: 42,
                keyframes: vec![
                    ReplayPlayerMovementKeyframe {
                        time_ms: 0,
                        position_cells: [0.0, 0.0, 0.0],
                    },
                    ReplayPlayerMovementKeyframe {
                        time_ms: 100,
                        position_cells: [1.0, 0.0, 0.0],
                    },
                    ReplayPlayerMovementKeyframe {
                        time_ms: 200,
                        position_cells: [1.0, 0.0, 1.0],
                    },
                    ReplayPlayerMovementKeyframe {
                        time_ms: 300,
                        position_cells: [2.0, 0.0, 1.0],
                    },
                ],
            },
            ReplayPlayerMovement {
                user_id: 42,
                keyframes: vec![ReplayPlayerMovementKeyframe {
                    time_ms: 1_000,
                    position_cells: [10.0, 0.0, 0.0],
                }],
            },
        ];

        let linear = interpolated_player_position(&movements, 42, 125, 0.0).unwrap() / VOXEL_SIZE;
        let curved = interpolated_player_position(&movements, 42, 125, 1.0).unwrap() / VOXEL_SIZE;
        assert!((linear - Vec3::new(1.0, 0.0, 0.25)).length() < 0.0001);
        assert!((curved - linear).length() > 0.01);
        assert!(curved.z < linear.z);
        assert_eq!(
            interpolated_player_position(&movements, 42, 500, 1.0).unwrap(),
            Vec3::new(2.0, 0.0, 1.0) * VOXEL_SIZE
        );
        assert_eq!(
            interpolated_player_position(&movements, 42, 1_000, 1.0).unwrap(),
            Vec3::new(10.0, 0.0, 0.0) * VOXEL_SIZE
        );
    }

    #[test]
    fn history_generated_replay_imports_campaign_movement_on_the_dialogue_timeline() {
        let mut first = test_dialogue(1_000, 600, DialogueSide::Right);
        first.source_time = 100;
        first.sender_id = 42;
        let mut second = test_dialogue(3_000, 600, DialogueSide::Right);
        second.source_time = 110;
        second.sender_id = 42;
        let history = ReplayPlayerMovementHistory {
            sessions: vec![
                PersistedPlayerMovementSession {
                    campaign_id: "campaign".to_owned(),
                    user_id: 42,
                    keyframes: vec![
                        PersistedPlayerMovementKeyframe {
                            source_unix_ms: 100_000,
                            position_cells: [0.0, 0.0, 0.0],
                        },
                        PersistedPlayerMovementKeyframe {
                            source_unix_ms: 105_000,
                            position_cells: [1.0, 0.0, 0.0],
                        },
                        PersistedPlayerMovementKeyframe {
                            source_unix_ms: 110_000,
                            position_cells: [2.0, 0.0, 0.0],
                        },
                    ],
                    ..default()
                },
                PersistedPlayerMovementSession {
                    campaign_id: "other".to_owned(),
                    user_id: 99,
                    keyframes: vec![PersistedPlayerMovementKeyframe {
                        source_unix_ms: 105_000,
                        position_cells: [9.0, 0.0, 0.0],
                    }],
                    ..default()
                },
            ],
        };

        let movements =
            replay_player_movements_from_history(&history, "campaign", &[first, second]);

        assert_eq!(movements.len(), 1);
        assert_eq!(movements[0].user_id, 42);
        assert_eq!(
            movements[0]
                .keyframes
                .iter()
                .map(|frame| frame.time_ms)
                .collect::<Vec<_>>(),
            vec![1_000, 6_000, 11_000]
        );
    }

    #[test]
    fn persisted_movement_anchor_and_delay_survive_regeneration() {
        let mut player = test_dialogue(1_000, 600, DialogueSide::Right);
        player.source_time = 100;
        player.sender_id = 42;
        let mut gm = test_dialogue(3_000, 700, DialogueSide::Left);
        gm.source_time = 105;
        gm.sender_id = 0;
        let history = ReplayPlayerMovementHistory {
            sessions: vec![PersistedPlayerMovementSession {
                campaign_id: "campaign".to_owned(),
                user_id: 42,
                turn_index: 3,
                start_after_source_time: Some(105),
                start_after_sender_id: Some(0),
                start_delay_ms: 400,
                keyframes: vec![
                    PersistedPlayerMovementKeyframe {
                        source_unix_ms: 10_000_000,
                        position_cells: [0.0, 0.0, 0.0],
                    },
                    PersistedPlayerMovementKeyframe {
                        source_unix_ms: 10_001_000,
                        position_cells: [4.0, 0.0, 0.0],
                    },
                ],
            }],
        };
        let encoded = serde_json::to_string(&history).unwrap();
        let restored: ReplayPlayerMovementHistory = serde_json::from_str(&encoded).unwrap();

        let first_build = replay_player_movements_from_history(&restored, "campaign", &[
            player.clone(),
            gm.clone(),
        ]);
        let second_build =
            replay_player_movements_from_history(&restored, "campaign", &[player, gm]);

        assert_eq!(
            first_build[0].keyframes[0].time_ms,
            3_400
        );
        assert_eq!(
            first_build[0].keyframes[1].time_ms,
            4_400
        );
        assert_eq!(
            first_build[0]
                .keyframes
                .iter()
                .map(|frame| frame.time_ms)
                .collect::<Vec<_>>(),
            second_build[0]
                .keyframes
                .iter()
                .map(|frame| frame.time_ms)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn clearing_test_progress_removes_only_matching_campaign_replay_movement() {
        let mut history = ReplayPlayerMovementHistory {
            sessions: vec![
                PersistedPlayerMovementSession {
                    campaign_id: "campaign-a".to_owned(),
                    user_id: 42,
                    ..default()
                },
                PersistedPlayerMovementSession {
                    campaign_id: "campaign-b".to_owned(),
                    user_id: 42,
                    ..default()
                },
            ],
        };

        assert_eq!(
            clear_campaign_replay_movement_history(&mut history, "campaign-a"),
            1
        );
        assert_eq!(history.sessions.len(), 1);
        assert_eq!(
            history.sessions[0].campaign_id,
            "campaign-b"
        );
    }

    #[test]
    fn play_imports_movement_recorded_after_replay_generation_without_duplicates() {
        let mut dialogue = test_dialogue(0, 600, DialogueSide::Right);
        dialogue.sender_id = 42;
        let mut replay = test_replay(vec![dialogue]);
        replay.created_at_unix_ms = 100_000;
        replay.player_movement_history_cursor_unix_ms = 100_000;
        replay.duration_ms = 3_000;
        let camera = Transform::from_xyz(0.0, 2.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y);
        replay.camera = vec![camera_keyframe(3_000, &camera)];
        let mut history = ReplayPlayerMovementHistory {
            sessions: vec![PersistedPlayerMovementSession {
                campaign_id: "campaign".to_owned(),
                user_id: 42,
                keyframes: vec![
                    PersistedPlayerMovementKeyframe {
                        source_unix_ms: 100_100,
                        position_cells: [0.0, 0.0, 0.0],
                    },
                    PersistedPlayerMovementKeyframe {
                        source_unix_ms: 101_100,
                        position_cells: [4.0, 2.0, 0.0],
                    },
                ],
                ..default()
            }],
        };

        assert_eq!(
            append_new_player_movements_from_history(&mut replay, &history),
            1
        );
        assert_eq!(replay.player_movements.len(), 1);
        assert_eq!(
            replay.player_movements[0]
                .keyframes
                .iter()
                .map(|frame| frame.time_ms)
                .collect::<Vec<_>>(),
            vec![3_000 + FOCUS_TRANSITION_MS, 4_000 + FOCUS_TRANSITION_MS]
        );
        assert_eq!(
            replay.duration_ms,
            4_000 + FOCUS_TRANSITION_MS
        );
        assert_eq!(
            interpolated_player_position(
                &replay.player_movements,
                42,
                3_500 + FOCUS_TRANSITION_MS,
                0.0,
            ),
            Some(Vec3::new(2.0, 1.0, 0.0) * VOXEL_SIZE)
        );

        let duration_after_first_import = replay.duration_ms;
        assert_eq!(
            append_new_player_movements_from_history(&mut replay, &history),
            0
        );
        assert_eq!(
            replay.duration_ms,
            duration_after_first_import
        );
        assert_eq!(replay.player_movements.len(), 1);

        history.sessions[0]
            .keyframes
            .push(PersistedPlayerMovementKeyframe {
                source_unix_ms: 101_200,
                position_cells: [5.0, 0.0, 0.0],
            });
        assert_eq!(
            append_new_player_movements_from_history(&mut replay, &history),
            1
        );
        assert_eq!(replay.player_movements.len(), 2);
        assert_eq!(
            replay.player_movements[1].keyframes[0].position_cells,
            [5.0, 0.0, 0.0]
        );
    }

    #[test]
    fn replay_playback_visibly_moves_the_player_standee() {
        let mut replay = test_replay(Vec::new());
        replay.duration_ms = 100;
        replay.player_movements = vec![ReplayPlayerMovement {
            user_id: 42,
            keyframes: vec![
                ReplayPlayerMovementKeyframe {
                    time_ms: 0,
                    position_cells: [0.0, 0.0, 0.0],
                },
                ReplayPlayerMovementKeyframe {
                    time_ms: 100,
                    position_cells: [4.0, 0.0, 0.0],
                },
            ],
        }];
        let mut studio = ReplayStudio::default();
        studio.mode = ReplayMode::Paused;
        studio.replay = Some(replay);
        let mut app = App::new();
        app.insert_resource(studio)
            .add_systems(Update, apply_replay_standee_positions);
        let standee = app
            .world_mut()
            .spawn((
                Transform::from_xyz(9.0, 0.0, 0.0),
                VoxelPlayerStandee::replay_test(42),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .entity(standee)
                .get::<Transform>()
                .unwrap()
                .translation,
            Vec3::ZERO
        );
        app.world_mut().resource_mut::<ReplayStudio>().playback_ms = 100;
        app.update();
        assert_eq!(
            app.world()
                .entity(standee)
                .get::<Transform>()
                .unwrap()
                .translation,
            Vec3::X
        );

        // Editing a frame during paused playback must invalidate the cached
        // world track without hiding the original movement on either side.
        {
            let mut studio = app.world_mut().resource_mut::<ReplayStudio>();
            replace_replay_movement_segment(
                studio.replay.as_mut().unwrap(),
                42,
                50,
                50,
                &[ReplayStandeePosition {
                    time_ms: 50,
                    user_id: 42,
                    position: Vec3::Y.to_array(),
                }],
            );
            studio.timeline_revision = studio.timeline_revision.wrapping_add(1);
        }
        for (time_ms, expected) in [(0, Vec3::ZERO), (50, Vec3::Y), (100, Vec3::X)] {
            app.world_mut().resource_mut::<ReplayStudio>().playback_ms = time_ms;
            app.update();
            assert_eq!(
                app.world()
                    .entity(standee)
                    .get::<Transform>()
                    .unwrap()
                    .translation,
                expected
            );
        }
    }

    #[test]
    fn replay_standee_faces_the_camera_without_tilting() {
        let rotation = replay_standee_facing_rotation(
            Vec3::new(2.0, 1.0, 3.0),
            Vec3::new(8.0, 20.0, -5.0),
        )
        .unwrap();
        let portrait_front = rotation * Vec3::NEG_Z;
        let labeled_back = rotation * Vec3::Z;
        let expected = Vec3::new(6.0, 0.0, -8.0).normalize();

        assert!(portrait_front.dot(expected) > 0.9999);
        assert!(labeled_back.dot(expected) < -0.9999);
        assert!(portrait_front.y.abs() < 0.0001);
        assert!(replay_standee_facing_rotation(Vec3::ZERO, Vec3::new(0.0, 10.0, 0.0)).is_none());
    }

    #[test]
    fn replay_system_keeps_the_standee_front_facing_the_replay_camera() {
        let camera = Transform::from_xyz(5.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y);
        let mut replay = test_replay(Vec::new());
        replay.duration_ms = 100;
        replay.camera = vec![camera_keyframe(0, &camera)];
        let mut studio = ReplayStudio::default();
        studio.mode = ReplayMode::Paused;
        studio.replay = Some(replay);
        let mut app = App::new();
        app.insert_resource(studio)
            .add_systems(Update, apply_replay_standee_positions);
        let standee = app
            .world_mut()
            .spawn((
                Transform::from_xyz(-2.0, 1.0, 1.0),
                VoxelPlayerStandee::replay_test(42),
            ))
            .id();

        app.update();

        let transform = app.world().entity(standee).get::<Transform>().unwrap();
        let toward_camera = horizontal(camera.translation - transform.translation).normalize();
        assert!((transform.rotation * Vec3::NEG_Z).dot(toward_camera) > 0.9999);
        assert!((transform.rotation * Vec3::Z).dot(toward_camera) < -0.9999);
    }

    #[test]
    fn turn_camera_cuts_between_known_speakers() {
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
        let obstacles = ReplayCameraObstacles::default();
        let frames = turn_based_camera_track(
            &base,
            &dialogue,
            5_430,
            &speaker_positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
            &obstacles,
        );
        assert!(frames
            .windows(2)
            .all(|pair| pair[0].time_ms < pair[1].time_ms));
        for (arrival_ms, target) in [
            (1_250, speaker_positions[&1]),
            (3_030, speaker_positions[&2]),
        ] {
            let frame = frames
                .iter()
                .find(|frame| frame.time_ms == arrival_ms)
                .unwrap();
            let shot = frame_transform(frame);
            let forward = shot.rotation * Vec3::NEG_Z;
            let to_speaker = (target - shot.translation).normalize();
            assert!(forward.dot(to_speaker) > 0.99);
        }
        let before_cut = frames.iter().find(|frame| frame.time_ms == 3_029).unwrap();
        let after_cut = frames.iter().find(|frame| frame.time_ms == 3_030).unwrap();
        assert!(
            Vec3::from_array(before_cut.translation)
                .distance(Vec3::from_array(after_cut.translation))
                > 1.0
        );
    }

    #[test]
    fn missing_director_dialogue_identifies_the_exact_line() {
        let mut available = test_dialogue(0, 600, DialogueSide::Right);
        available.sender_id = 7;
        available.name = "林青".to_owned();
        available.text = "我检查舱门。".to_owned();
        let mut missing = test_dialogue(1_000, 600, DialogueSide::Right);
        missing.sender_id = 8;
        missing.name = "周遥".to_owned();
        missing.text = "  这里没有我的立牌。\n请先处理。 ".to_owned();
        let replay = test_replay(vec![available, missing]);

        let issues = missing_director_dialogue(
            &replay,
            &HashMap::from([(7, Vec3::ZERO)]),
        );

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].0, 1);
        assert_eq!(
            issues[0].1,
            "第 2 句 · 周遥（QQ 8）：这里没有我的立牌。 请先处理。"
        );
    }

    #[test]
    fn missing_director_dialogue_ignores_unchecked_lines() {
        let mut line = test_dialogue(0, 600, DialogueSide::Right);
        line.sender_id = 8;
        line.included = false;
        let replay = test_replay(vec![line]);

        assert!(missing_director_dialogue(&replay, &HashMap::new()).is_empty());
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
        let obstacles = ReplayCameraObstacles::default();
        let frames = director_camera_track(
            &base,
            &dialogue,
            &cues,
            3_050,
            &HashMap::from([(1, target)]),
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
            &obstacles,
        );
        let shot = frame_transform(frames.last().unwrap());
        let forward = shot.rotation * Vec3::NEG_Z;
        let to_speaker = (target - shot.translation).normalize();
        assert!(forward.dot(to_speaker) > 0.99);
        assert!(frames.iter().any(|frame| frame.time_ms == 350));
    }

    #[test]
    fn directed_camera_stays_on_one_side_of_the_scene_axis() {
        let base = Transform::from_xyz(0.0, 6.0, 12.0);
        let mut dialogue = [
            test_dialogue(350, 2_700, DialogueSide::Left),
            test_dialogue(3_320, 2_700, DialogueSide::Right),
        ];
        dialogue[1].sender_id = 2;
        let speaker_positions = HashMap::from([
            (1, Vec3::new(-6.0, 1.0, 0.0)),
            (2, Vec3::new(6.0, 1.0, 0.0)),
        ]);
        let cues = [
            DirectorCue {
                index: 0,
                text: "左侧发言。".to_owned(),
                speech_text: "左侧发言。".to_owned(),
                shot: DirectorShot::SpeakerMedium,
                motion: DirectorMotion::DollyIn,
            },
            DirectorCue {
                index: 1,
                text: "右侧发言。".to_owned(),
                speech_text: "右侧发言。".to_owned(),
                shot: DirectorShot::SpeakerClose,
                motion: DirectorMotion::DriftRight,
            },
        ];
        let rig = DirectedCameraRig::for_dialogue(
            &base,
            &dialogue,
            &speaker_positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
        );
        let obstacles = ReplayCameraObstacles::default();
        let frames = director_camera_track(
            &base,
            &dialogue,
            &cues,
            6_020,
            &speaker_positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
            &obstacles,
        );

        assert!(frames
            .iter()
            .map(frame_transform)
            .all(|frame| rig.signed_side(frame.translation) >= DirectedCameraRig::LINE_MARGIN));
    }

    #[test]
    fn three_person_shot_uses_the_green_dot_group_center_position() {
        let base = Transform::from_xyz(0.0, 4.0, 12.0);
        let dialogue = [test_dialogue(350, 2_700, DialogueSide::Right)];
        let positions = HashMap::from([
            (1, Vec3::new(-6.0, 1.0, 0.0)),
            (2, Vec3::new(0.0, 1.0, 0.0)),
            (3, Vec3::new(6.0, 1.0, 0.0)),
        ]);
        let rig = DirectedCameraRig::for_dialogue(
            &base,
            &dialogue,
            &positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
        );

        let shot = rig.director_shot(
            positions[&3],
            DirectorShot::SpeakerMedium,
            DirectorMotion::Static,
            0.0,
            &ReplayCameraObstacles::default(),
        );

        assert_eq!(rig.subject_count, 3);
        assert!(shot.translation.x.abs() < 0.001);
        let directions = [positions[&1], positions[&2], positions[&3]]
            .map(|subject| horizontal(subject - shot.translation).normalize());
        assert!(directions[0].dot(directions[1]) < 0.99);
        assert!(directions[1].dot(directions[2]) < 0.99);
        assert!(
            (shot.rotation * Vec3::NEG_Z).dot((positions[&3] - shot.translation).normalize())
                > 0.999
        );
    }

    #[test]
    fn spread_out_standees_keep_the_camera_on_the_speaker() {
        // Three standees far apart: the centroid axis passes hundreds of units
        // from any single speaker, so the side-of-axis correction must never
        // teleport the camera across the scene.
        let base = Transform::from_xyz(0.0, 10.0, 100.0);
        let dialogue = [test_dialogue(350, 2_700, DialogueSide::Right)];
        let positions = HashMap::from([
            (1, Vec3::new(-150.0, 2.0, -160.0)),
            (2, Vec3::new(0.0, 2.0, 0.0)),
            (3, Vec3::new(150.0, 2.0, 160.0)),
        ]);
        let rig = DirectedCameraRig::for_dialogue(
            &base,
            &dialogue,
            &positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
        );
        assert!(
            rig.group_spread > DirectedCameraRig::GROUP_SPREAD_LIMIT,
            "test setup requires a spread-out group"
        );

        let shot = rig.speaker_shot(
            positions[&1],
            DirectorShot::SpeakerMedium,
            0.0,
            &ReplayCameraObstacles::default(),
        );
        let distance = horizontal(shot.translation - positions[&1]).length();
        assert!(
            distance < 15.0,
            "camera must stay near the speaker, ended {distance:.1} units away"
        );
        assert!(
            (shot.rotation * Vec3::NEG_Z).dot((positions[&1] - shot.translation).normalize())
                > 0.999,
            "camera must keep the speaker centered"
        );
    }

    #[test]
    fn speaker_change_cuts_instead_of_sliding_or_rotating_between_subjects() {
        let base = Transform::from_xyz(0.0, 4.0, 12.0);
        let mut dialogue = [
            test_dialogue(350, 2_700, DialogueSide::Right),
            test_dialogue(3_320, 2_700, DialogueSide::Right),
        ];
        dialogue[1].sender_id = 2;
        let positions = HashMap::from([
            (1, Vec3::new(-6.0, 1.0, 0.0)),
            (2, Vec3::new(6.0, 1.0, 0.0)),
        ]);
        let cues = [
            DirectorCue {
                index: 0,
                text: "左侧发言。".to_owned(),
                speech_text: "左侧发言。".to_owned(),
                shot: DirectorShot::SpeakerMedium,
                motion: DirectorMotion::Static,
            },
            DirectorCue {
                index: 1,
                text: "右侧发言。".to_owned(),
                speech_text: "右侧发言。".to_owned(),
                shot: DirectorShot::SpeakerMedium,
                motion: DirectorMotion::Static,
            },
        ];
        let frames = director_camera_track(
            &base,
            &dialogue,
            &cues,
            6_020,
            &positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
            &ReplayCameraObstacles::default(),
        );
        let before = frames
            .iter()
            .find(|frame| frame.time_ms == dialogue[1].time_ms - 1)
            .unwrap();
        let after = frames
            .iter()
            .find(|frame| frame.time_ms == dialogue[1].time_ms)
            .unwrap();

        let before = frame_transform(before);
        let after = frame_transform(after);
        assert!(before.translation.distance(after.translation) > 1.0);
        assert!(
            (before.rotation * Vec3::NEG_Z).dot((positions[&1] - before.translation).normalize())
                > 0.999
        );
        assert!(
            (after.rotation * Vec3::NEG_Z).dot((positions[&2] - after.translation).normalize())
                > 0.999
        );
    }

    #[test]
    fn directed_camera_keeps_full_distance_when_a_wall_can_be_dissolved() {
        let target = Vec3::new(0.125, 1.125, 0.125);
        let base = Transform::from_xyz(0.125, 3.0, 5.125);
        let dialogue = [test_dialogue(350, 2_700, DialogueSide::Right)];
        let positions = HashMap::from([(1, target)]);
        let rig = DirectedCameraRig::for_dialogue(
            &base,
            &dialogue,
            &positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
        );
        let scene = ReplayScene {
            voxels: (-20..=20)
                .flat_map(|x| {
                    (-4..=20).map(move |y| ReplayVoxel {
                        position: [x, y, 8],
                        material: 1,
                    })
                })
                .collect(),
        };
        let obstacles = ReplayCameraObstacles::from_scene(&scene);

        let shot = rig.director_shot(
            target,
            DirectorShot::SpeakerMedium,
            DirectorMotion::Static,
            0.0,
            &obstacles,
        );
        let forward = shot.rotation * Vec3::NEG_Z;
        let to_speaker = (target - shot.translation).normalize();

        assert!(shot.translation.z > 7.0);
        assert!(obstacles.camera_is_clear(shot.translation));
        assert!(!obstacles.segment_is_clear(shot.translation, target));
        assert!(forward.dot(to_speaker) > 0.999);
        assert!(rig.signed_side(shot.translation) >= DirectedCameraRig::LINE_MARGIN);
    }

    #[test]
    fn directed_camera_speaker_shots_are_half_the_previous_distance() {
        let target = Vec3::new(2.0, 1.0, -1.0);
        let base = Transform::from_xyz(2.0, 2.5, 6.0);
        let dialogue = [test_dialogue(350, 2_700, DialogueSide::Right)];
        let positions = HashMap::from([(1, target)]);
        let rig = DirectedCameraRig::for_dialogue(
            &base,
            &dialogue,
            &positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
        );

        let shot = rig.director_shot(
            target,
            DirectorShot::SpeakerMedium,
            DirectorMotion::Static,
            0.0,
            &ReplayCameraObstacles::default(),
        );

        assert!((horizontal(shot.translation - target).length() - 7.5).abs() < 0.001);
        assert!((shot.translation.y - target.y - 2.25).abs() < 0.001);
    }

    #[test]
    fn gm_distance_control_rescales_existing_shots_and_keeps_focus() {
        let target = Vec3::new(2.0, 1.0, -1.0);
        let line = test_dialogue(350, 2_700, DialogueSide::Right);
        let mut replay = test_replay(vec![line]);
        let original = Transform::from_xyz(2.0, 3.25, 6.5).looking_at(target, Vec3::Y);
        replay.camera = vec![camera_keyframe(350, &original)];

        rescale_replay_camera_distance(
            &mut replay,
            3.0,
            &HashMap::from([(1, target)]),
        );

        let adjusted = frame_transform(&replay.camera[0]);
        assert!(
            (adjusted.translation.distance(target) - original.translation.distance(target) * 2.0)
                .abs()
                < 0.001
        );
        assert!(
            (adjusted.rotation * Vec3::NEG_Z).dot((target - adjusted.translation).normalize())
                > 0.999
        );
        assert_eq!(replay.camera_distance_scale, 3.0);
    }

    #[test]
    fn focused_camera_yaw_control_rotates_existing_shots_and_keeps_focus() {
        let target = Vec3::new(2.0, 1.0, -1.0);
        let line = test_dialogue(350, 2_700, DialogueSide::Right);
        let mut replay = test_replay(vec![line]);
        let original = Transform::from_xyz(2.0, 3.25, 6.5).looking_at(target, Vec3::Y);
        replay.camera = vec![camera_keyframe(350, &original)];
        let speaker_positions = HashMap::from([(1, target)]);

        assert_eq!(
            rotate_replay_camera_yaw(&mut replay, 30.0, &speaker_positions),
            30.0
        );
        let rotated = frame_transform(&replay.camera[0]);
        let expected_offset =
            Quat::from_rotation_y(30.0_f32.to_radians()) * (original.translation - target);
        assert!((rotated.translation - target).abs_diff_eq(expected_offset, 0.000_1));
        assert!(
            (rotated.rotation * Vec3::NEG_Z).dot((target - rotated.translation).normalize())
                > 0.999
        );

        assert_eq!(
            rotate_replay_camera_yaw(&mut replay, -30.0, &speaker_positions),
            -30.0
        );
        let rotated = frame_transform(&replay.camera[0]);
        let expected_offset =
            Quat::from_rotation_y(-30.0_f32.to_radians()) * (original.translation - target);
        assert!((rotated.translation - target).abs_diff_eq(expected_offset, 0.000_1));
        assert_eq!(replay.camera_yaw_degrees, -30.0);
    }

    #[test]
    fn focused_camera_yaw_is_bounded() {
        assert_eq!(
            normalized_directed_camera_yaw_degrees(f32::INFINITY),
            default_directed_camera_yaw_degrees()
        );
        assert_eq!(
            normalized_directed_camera_yaw_degrees(100.0),
            MAX_DIRECTED_CAMERA_YAW_DEGREES
        );
        assert_eq!(
            normalized_directed_camera_yaw_degrees(-100.0),
            MIN_DIRECTED_CAMERA_YAW_DEGREES
        );
    }

    #[test]
    fn generated_focused_camera_yaw_stays_on_the_scene_side() {
        let base = Transform::from_xyz(0.0, 4.0, 12.0);
        let dialogue = [test_dialogue(350, 2_700, DialogueSide::Right)];
        let positions = HashMap::from([
            (1, Vec3::new(-6.0, 1.0, 0.0)),
            (2, Vec3::new(6.0, 1.0, 0.0)),
        ]);
        let default_rig = DirectedCameraRig::for_dialogue(
            &base,
            &dialogue,
            &positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
        );
        let rotated_rig = DirectedCameraRig::for_dialogue(
            &base,
            &dialogue,
            &positions,
            default_directed_camera_distance_scale(),
            45.0,
        );
        let target = positions[&1];
        let default_shot = default_rig.speaker_shot(
            target,
            DirectorShot::SpeakerMedium,
            0.0,
            &ReplayCameraObstacles::default(),
        );
        let rotated_shot = rotated_rig.speaker_shot(
            target,
            DirectorShot::SpeakerMedium,
            0.0,
            &ReplayCameraObstacles::default(),
        );
        let expected_direction = Quat::from_rotation_y(45.0_f32.to_radians())
            * horizontal(default_shot.translation - target);
        assert!(
            horizontal(rotated_shot.translation - target)
                .normalize()
                .dot(expected_direction.normalize())
                > 0.999
        );
        assert!(
            (rotated_shot.rotation * Vec3::NEG_Z)
                .dot((target - rotated_shot.translation).normalize())
                > 0.999
        );
        assert!(
            rotated_rig.signed_side(rotated_shot.translation) >= DirectedCameraRig::LINE_MARGIN
        );
    }

    #[test]
    fn directed_dolly_moves_along_the_locked_camera_axis_and_keeps_focus() {
        let base = Transform::from_xyz(0.0, 4.0, 10.0);
        let dialogue = [test_dialogue(350, 2_700, DialogueSide::Right)];
        let target = Vec3::new(2.0, 1.0, -1.0);
        let positions = HashMap::from([(1, target)]);
        let rig = DirectedCameraRig::for_dialogue(
            &base,
            &dialogue,
            &positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
        );
        let obstacles = ReplayCameraObstacles::default();
        let arrival = rig.director_shot(
            target,
            DirectorShot::SpeakerMedium,
            DirectorMotion::DollyIn,
            0.0,
            &obstacles,
        );
        let desired = rig.director_shot(
            target,
            DirectorShot::SpeakerMedium,
            DirectorMotion::DollyIn,
            1.0,
            &obstacles,
        );
        let settled = limit_camera_travel_toward(&arrival, &desired, 0.2, target);
        let movement = settled.translation - arrival.translation;
        let forward = settled.rotation * Vec3::NEG_Z;
        let to_speaker = (target - settled.translation).normalize();

        assert!(movement.normalize().dot(*arrival.forward()) > 0.999);
        assert!(settled.translation.distance(target) < arrival.translation.distance(target));
        assert!(forward.dot(to_speaker) > 0.999);
    }

    #[test]
    fn directed_dolly_becomes_static_when_the_line_margin_would_distort_it() {
        let base = Transform::from_xyz(0.0, 6.0, 12.0);
        let mut dialogue = [
            test_dialogue(350, 2_700, DialogueSide::Left),
            test_dialogue(3_320, 2_700, DialogueSide::Right),
        ];
        dialogue[1].sender_id = 2;
        let positions = HashMap::from([
            (1, Vec3::new(-6.0, 1.0, 0.0)),
            (2, Vec3::new(6.0, 1.0, 0.0)),
        ]);
        let rig = DirectedCameraRig::for_dialogue(
            &base,
            &dialogue,
            &positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
        );
        let obstacles = ReplayCameraObstacles::default();
        // Keep the speaker far enough onto the forbidden side that the scaled
        // shot must still clamp to the scene line.
        let off_axis_target = Vec3::new(0.0, 1.0, -30.0);
        let arrival = rig.director_shot(
            off_axis_target,
            DirectorShot::SpeakerMedium,
            DirectorMotion::DollyIn,
            0.0,
            &obstacles,
        );
        let settled = rig.director_shot(
            off_axis_target,
            DirectorShot::SpeakerMedium,
            DirectorMotion::DollyIn,
            1.0,
            &obstacles,
        );

        assert!(arrival
            .translation
            .abs_diff_eq(settled.translation, f32::EPSILON));
        // The dolly must not cross the scene line, and the camera must stay
        // close enough to frame the off-axis speaker instead of being pushed
        // across the whole scene to the "correct" side.
        assert!(horizontal(settled.translation - off_axis_target).length() < 15.0);
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
    fn local_discussion_group_broadcast_is_one_avatarless_gm_line_on_the_left() {
        let mut manager: NapcatMessageManager = serde_json::from_str(r#"{"messages":{}}"#).unwrap();
        manager
            .player_characters
            .insert("7".to_owned(), PlayerCharacter {
                name: "陌陌".to_owned(),
                image: "momo.png".to_owned(),
                ..Default::default()
            });
        let message = CampaignMessage {
            campaign_id: "campaign".to_owned(),
            sender_id: 0,
            sender_name: "GM".to_owned(),
            source: crate::napcat::MessageSource::Gui,
            character_id: Some("7".to_owned()),
            party_id: None,
            visibility: Visibility::Player(7),
            text: "冷冻舱紧急解除了休眠".to_owned(),
            time: 1_200,
            forwarded: false,
        };
        let snapshot = ReplayMessageSnapshot {
            line_id: 88,
            turn_index: 0,
            position_cells: [0, 0, 0],
        };

        let line = dialogue_from_message(
            &message, &manager, 350, &snapshot, false,
        )
        .unwrap();

        assert_eq!(line.side, DialogueSide::Left);
        assert_eq!(line.line_id, 88);
        assert_eq!(line.camera_focus_id, Some(7));
        assert_eq!(line.name, "GM");
        assert!(line.role.is_empty());
        assert!(line.avatar.is_empty());

        let mut dialogue = vec![line.clone(), line];
        deduplicate_broadcast_dialogue(&mut dialogue, &manager);
        assert_eq!(dialogue.len(), 1);
    }

    #[test]
    fn forwarded_broadcast_is_one_original_player_line() {
        let manager: NapcatMessageManager = serde_json::from_str(r#"{"messages":{}}"#).unwrap();
        let mut line = test_dialogue(1_200, 600, DialogueSide::Right);
        line.sender_id = 7;
        line.text = "hello".to_owned();
        line.source_time = 1_200;
        line.forwarded = true;

        let mut dialogue = vec![line.clone(), line];
        deduplicate_broadcast_dialogue(&mut dialogue, &manager);

        assert_eq!(dialogue.len(), 1);
        assert_eq!(dialogue[0].sender_id, 7);
        assert_eq!(dialogue[0].side, DialogueSide::Right);
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
        assert_eq!(MIN_DIALOGUE_MS, 1_500);
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
        assert_eq!(emotivoice_model_text("好好"), "好好。");
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
        assert_eq!(default_master_speech_speed(), 1.10);
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
            1.10
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
    fn replay_studio_defaults_to_all_audiences() {
        let studio = ReplayStudio::default();
        assert_eq!(studio.audience, ReplayAudience::All);
    }

    #[test]
    fn replay_audio_defaults_are_clearly_audible() {
        let studio = ReplayStudio::default();
        assert_eq!(studio.music_volume, 0.65);
        assert_eq!(studio.speech_volume, 1.25);
    }

    #[test]
    fn completed_replay_is_distinct_from_mid_replay_pause() {
        let mut studio = ReplayStudio::default();
        let mut replay = test_replay(Vec::new());
        replay.duration_ms = 10_000;
        studio.replay = Some(replay);
        studio.mode = ReplayMode::Paused;
        studio.playback_ms = 9_999;
        assert!(!replay_has_completed(&studio));

        studio.playback_ms = 10_000;
        assert!(replay_has_completed(&studio));
    }

    #[test]
    fn paused_take_at_the_extended_end_can_resume_without_restarting() {
        for ship_take in [false, true] {
            let mut studio = ReplayStudio::default();
            let mut replay = test_replay(Vec::new());
            replay.duration_ms = 10_000;
            studio.replay = Some(replay);
            studio.playback_ms = 9_000;
            if ship_take {
                begin_live_ship_take(
                    &mut studio,
                    "ship".to_owned(),
                    "飞船".to_owned(),
                    Transform::default(),
                );
            } else {
                begin_live_movement_take(&mut studio, 7, Vec3::ZERO);
            }
            studio.replay.as_mut().unwrap().duration_ms = 12_000;
            studio.playback_ms = 12_000;
            studio.mode = ReplayMode::Paused;

            assert!(!replay_has_completed(&studio));
            assert!(!can_start_director_export(
                &studio, true
            ));
            assert_eq!(studio.playback_ms, 12_000);

            cancel_live_replay_take(&mut studio);
            assert!(replay_has_completed(&studio));
            assert!(can_start_director_export(&studio, true));
        }
    }

    #[test]
    fn regenerated_replay_inherits_project_and_matching_voice_settings() {
        let retained_voice = test_voice_settings("9000", 42);
        let removed_voice = test_voice_settings("65", -12);
        let mut previous = test_replay(vec![test_dialogue_for_speaker(7)]);
        previous.area_radius_cells = 24;
        previous.master_speech_speed = 1.75;
        previous.master_dialogue_duration = 2.25;
        previous
            .speaker_voice_settings
            .insert(7, retained_voice.clone());
        previous.speaker_voice_settings.insert(99, removed_voice);

        let mut regenerated = test_replay(vec![
            test_dialogue_for_speaker(7),
            test_dialogue_for_speaker(8),
        ]);
        let new_voice = test_voice_settings("1001", 10);
        regenerated
            .speaker_voice_settings
            .insert(8, new_voice.clone());
        ReplayGenerationSettings::from_replay(&previous).apply_to(&mut regenerated);

        assert_eq!(regenerated.area_radius_cells, 24);
        assert_eq!(regenerated.master_speech_speed, 1.75);
        assert_eq!(
            regenerated.master_dialogue_duration,
            2.25
        );
        assert_eq!(
            regenerated.speaker_voice_settings.get(&7),
            Some(&retained_voice)
        );
        assert_eq!(
            regenerated.speaker_voice_settings.get(&8),
            Some(&new_voice)
        );
        assert!(!regenerated.speaker_voice_settings.contains_key(&99));
    }

    #[test]
    fn speech_cache_reuses_only_matching_voice_inputs() {
        let original = emotivoice_speech_cache_path("你好", "9000", "普通", 1.3);
        assert_eq!(
            original,
            emotivoice_speech_cache_path("你好", "9000", "普通", 1.3)
        );
        assert_ne!(
            original,
            emotivoice_speech_cache_path("再见", "9000", "普通", 1.3)
        );
        assert_ne!(
            original,
            emotivoice_speech_cache_path("你好", "65", "普通", 1.3)
        );
        assert_ne!(
            original,
            emotivoice_speech_cache_path("你好", "9000", "普通", 1.5)
        );
    }

    #[test]
    #[ignore = "requires the installed EmotiVoice Chinese runtime"]
    fn speech_cache_benchmark_avoids_repeat_gpu_synthesis() {
        let text = "这是角色语音持久缓存性能测试。";
        let speaker = "9000";
        let emotion = "普通";
        let speed = 1.3;
        let cache_path = emotivoice_speech_cache_path(text, speaker, emotion, speed);
        let _ = fs::remove_file(&cache_path);
        let directory = tempfile::tempdir().unwrap();
        let job = |name: &str| SpeechSynthesisJob {
            text: text.to_owned(),
            output_path: directory.path().join(name).to_string_lossy().into_owned(),
            speaker: speaker.to_owned(),
            emotion: emotion.to_owned(),
            onnx_speed: speed,
            duration_ms: 20_000,
            allow_truncate: false,
        };

        let cold_started = std::time::Instant::now();
        synthesize_speech_batch(directory.path(), &[job("cold.wav")]).unwrap();
        let cold = cold_started.elapsed();
        let warm_started = std::time::Instant::now();
        synthesize_speech_batch(directory.path(), &[job("warm.wav")]).unwrap();
        let warm = warm_started.elapsed();

        eprintln!("EmotiVoice cold={cold:?}, cache-hit={warm:?}");
        assert!(warm < cold);
        assert_eq!(
            fs::read(directory.path().join("cold.wav")).unwrap(),
            fs::read(directory.path().join("warm.wav")).unwrap()
        );
        let _ = fs::remove_file(cache_path);
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
    fn speech_preparation_prioritizes_current_and_queues_every_line() {
        let replay_json = r#"{"format_version":2,"title":"test","campaign_id":"c","created_at_unix_ms":1,"duration_ms":3000,"audience":{"scope":"public"},"scene":{"voxels":[]},"camera":[],"dialogue":[]}"#;
        let mut replay: ReplayFile = serde_json::from_str(replay_json).unwrap();
        replay.dialogue = vec![
            test_dialogue(0, 900, DialogueSide::Left),
            test_dialogue(1_000, 900, DialogueSide::Right),
            test_dialogue(2_000, 900, DialogueSide::Left),
        ];
        replay.dialogue[1].included = false;

        assert_eq!(
            replay_speech_preparation_indices(&replay, 2_100),
            vec![2, 0]
        );

        let mut speech = PreviewSpeechController::default();
        speech.prepared_signature = Some(replay_voice_signature(&replay, 1.0));
        speech.onnx_cache.insert(
            (replay.created_at_unix_ms, 0),
            (vec![1], 1.0),
        );
        assert_eq!(
            speech.preparation_progress(&replay, 1.0),
            (1, 0, 2)
        );
        speech.onnx_failures.insert(
            (replay.created_at_unix_ms, 2),
            "test failure".to_owned(),
        );
        assert_eq!(
            speech.preparation_progress(&replay, 1.0),
            (1, 1, 2)
        );
        assert!(speech.onnx_cue_finished(
            replay_voice_signature(&replay, 1.0),
            (replay.created_at_unix_ms, 2),
        ));
    }

    #[test]
    fn speech_wait_watchdog_keeps_replay_from_freezing() {
        let cue = (7, 0);
        let mut studio = ReplayStudio::default();
        assert!(!studio.turn_playback_enabled);

        assert!(replay_speech_wait_is_blocking(
            &mut studio,
            cue,
            4.9
        ));
        assert!(!studio.speech_bypass_cues.contains(&cue));
        assert!(!replay_speech_wait_is_blocking(
            &mut studio,
            cue,
            0.1
        ));
        assert!(studio.speech_bypass_cues.contains(&cue));

        let next_cue = (7, 1);
        assert!(replay_speech_wait_is_blocking(
            &mut studio,
            next_cue,
            0.1
        ));
        assert_eq!(studio.speech_wait_cue, Some(next_cue));
    }

    #[test]
    fn uncached_unqueued_speech_does_not_block_playback() {
        let replay_json = r#"{"format_version":2,"title":"test","campaign_id":"c","created_at_unix_ms":1,"duration_ms":1000,"audience":{"scope":"public"},"scene":{"voxels":[]},"camera":[],"dialogue":[]}"#;
        let mut replay: ReplayFile = serde_json::from_str(replay_json).unwrap();
        replay.dialogue.push(test_dialogue(
            0,
            1_000,
            DialogueSide::Right,
        ));
        let signature = replay_voice_signature(&replay, 1.0);
        let mut speech = PreviewSpeechController::default();
        speech.prepared_signature = Some(signature);

        assert!(speech.onnx_cue_finished(
            signature,
            (replay.created_at_unix_ms, 0),
        ));

        speech.onnx_queued.insert((replay.created_at_unix_ms, 0));
        assert!(!speech.onnx_cue_finished(
            signature,
            (replay.created_at_unix_ms, 0),
        ));
    }

    #[test]
    fn only_new_message_or_retry_cues_authorize_speech_generation() {
        let cue = (7, 2);
        assert!(!speech_generation_requested(
            3,
            cue,
            &HashSet::new(),
            &HashSet::new(),
        ));
        assert!(speech_generation_requested(
            3,
            cue,
            &HashSet::from([3]),
            &HashSet::new(),
        ));
        assert!(speech_generation_requested(
            3,
            cue,
            &HashSet::new(),
            &HashSet::from([cue]),
        ));
    }

    #[test]
    fn every_missing_included_line_is_immediately_authorized() {
        let mut replay = test_replay(vec![
            positioned_dialogue(10, 0, 0, [0, 0, 0]),
            positioned_dialogue(20, 0, 1_000, [1, 0, 0]),
            positioned_dialogue(30, 0, 2_000, [2, 0, 0]),
        ]);
        replay.dialogue[2].included = false;

        assert_eq!(
            replay_speech_generation_line_ids(&replay),
            HashSet::from([10, 20]),
        );
    }

    #[test]
    fn playing_dialogue_waits_for_its_speech_before_display() {
        let replay_json = r#"{"format_version":1,"title":"test","campaign_id":"c","created_at_unix_ms":1,"duration_ms":1000,"audience":{"scope":"public"},"scene":{"voxels":[]},"camera":[],"dialogue":[]}"#;
        let mut replay: ReplayFile = serde_json::from_str(replay_json).unwrap();
        replay.dialogue.push(test_dialogue(
            0,
            1_000,
            DialogueSide::Right,
        ));
        let mut studio = ReplayStudio::default();
        studio.mode = ReplayMode::Playing;
        studio.replay = Some(replay);
        let mut speech = PreviewSpeechController::default();

        assert!(!replay_dialogue_is_ready_for_display(
            &studio, &speech, 0, true
        ));

        let replay = studio.replay.as_ref().unwrap();
        let signature = replay_voice_signature(replay, studio.speech_volume);
        speech.prepared_signature = Some(signature);
        speech.onnx_cache.insert(
            (replay.created_at_unix_ms, 0),
            (vec![1], 1.0),
        );
        assert!(replay_dialogue_is_ready_for_display(
            &studio, &speech, 0, true
        ));

        speech.onnx_cache.clear();
        studio.mode = ReplayMode::Paused;
        assert!(replay_dialogue_is_ready_for_display(
            &studio, &speech, 0, true
        ));
    }

    #[test]
    fn replay_preview_blocks_world_mouse_interaction() {
        let mut studio = ReplayStudio::default();
        assert!(!replay_blocks_mouse_interaction(
            &studio
        ));

        studio.mode = ReplayMode::Playing;
        assert!(replay_blocks_mouse_interaction(&studio));

        studio.mode = ReplayMode::Paused;
        assert!(replay_blocks_mouse_interaction(&studio));

        studio.mode = ReplayMode::Recording;
        assert!(!replay_blocks_mouse_interaction(
            &studio
        ));
    }

    #[test]
    fn director_export_is_available_during_preview_and_reuses_an_applied_plan() {
        let mut studio = ReplayStudio::default();
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
    fn matching_director_plan_survives_manager_serialization() {
        let replay = test_replay(vec![test_dialogue(
            0,
            2_700,
            DialogueSide::Right,
        )]);
        let summary_key = replay_director_key(&replay);
        let request = ReplayDirectorRequest {
            summary_key: summary_key.clone(),
            message_count: 1,
            text: "request text".to_owned(),
            custom_prompt: "quiet cuts".to_owned(),
            fingerprint: "matching-fingerprint".to_owned(),
        };
        let mut manager = DeepseekManager::default();
        manager.director_request_fingerprints.insert(
            director_fingerprint_key(&summary_key, 1),
            request.fingerprint.clone(),
        );
        manager.summaries.insert(
            summary_key,
            crate::deepseek::DeepseekSummary {
                blocks: vec![DeepseekSummaryBlock {
                    latest: r#"{"dialogue":[]}"#.to_owned(),
                    message_count: 1,
                    pending: false,
                    error: None,
                }],
            },
        );

        let serialized = serde_json::to_string(&manager).unwrap();
        let restored: DeepseekManager = serde_json::from_str(&serialized).unwrap();

        assert!(saved_director_block(&replay, &restored, &request).is_some());
        let changed_request = ReplayDirectorRequest {
            fingerprint: "changed-fingerprint".to_owned(),
            ..request
        };
        assert!(saved_director_block(&replay, &restored, &changed_request).is_none());
    }

    #[test]
    fn pending_director_response_applies_only_to_the_requested_replay_edits() {
        for changed in 0..5 {
            let mut line = test_dialogue(0, 600, DialogueSide::Right);
            line.line_id = 1;
            line.sender_id = 7;
            let mut replay = test_replay(vec![line]);
            replay.duration_ms = 600;
            replay.area_blocks = vec![ReplayAreaBlock {
                id: 1,
                area: "甲板".to_owned(),
                line_ids: vec![1],
            }];
            let mut studio = ReplayStudio::default();
            studio.replay = Some(replay);
            studio.director_request_pending = true;
            studio.auto_export_after_director = true;
            let mut world = World::new();
            world.insert_resource(studio);
            world.spawn((
                Transform::default(),
                VoxelPlayerStandee::replay_test(7),
            ));
            world.spawn((
                Transform::from_xyz(0.0, 3.0, 5.0),
                VoxelViewportCamera,
            ));
            let request = world
                .run_system_once(
                    |studio: Res<ReplayStudio>,
                     standees: Query<
                        (&Transform, &VoxelPlayerStandee),
                        Without<VoxelViewportCamera>,
                    >| {
                        replay_director_request(
                            studio.replay.as_ref().unwrap(),
                            &studio.deepseek_custom_prompt,
                            &standees,
                        )
                    },
                )
                .unwrap()
                .unwrap();
            let mut manager = DeepseekManager::default();
            manager.director_request_fingerprints.insert(
                director_fingerprint_key(
                    &request.summary_key,
                    request.message_count,
                ),
                request.fingerprint,
            );
            manager.summaries.entry(request.summary_key).or_default().upsert_block(
                DeepseekSummaryBlock {
                    latest: r#"{"dialogue":[{"index":0,"text":"导演结果","speech_text":"导演结果","shot":"speaker_medium","motion":"static"}]}"#.to_owned(),
                    message_count: request.message_count,
                    pending: false,
                    error: None,
                },
            );
            world.insert_resource(manager);
            {
                let mut studio = world.resource_mut::<ReplayStudio>();
                match changed {
                    1 => studio.replay.as_mut().unwrap().dialogue[0].text = "DM 新台词".to_owned(),
                    2 => studio.replay.as_mut().unwrap().standee_positions.push(
                        ReplayStandeePosition {
                            time_ms: 300,
                            user_id: 7,
                            position: [3.0, 0.0, 0.0],
                        },
                    ),
                    3 => studio.deepseek_custom_prompt.push_str("保留当前镜头"),
                    4 => studio.replay.as_mut().unwrap().dialogue.clear(),
                    _ => {},
                }
            }
            let before =
                json_to_string(world.resource::<ReplayStudio>().replay.as_ref().unwrap()).unwrap();
            let result = world
                .run_system_once(
                    |mut studio: ResMut<ReplayStudio>,
                     manager: Res<DeepseekManager>,
                     camera: Query<&Transform, With<VoxelViewportCamera>>,
                     standees: Query<
                        (&Transform, &VoxelPlayerStandee),
                        Without<VoxelViewportCamera>,
                    >| {
                        apply_ready_director_plan(
                            &mut studio,
                            &manager,
                            &camera,
                            &standees,
                        )
                    },
                )
                .unwrap();
            let studio = world.resource::<ReplayStudio>();
            assert!(!studio.director_request_pending);
            if changed == 0 {
                assert_eq!(result, Ok(true));
                assert_eq!(
                    studio.replay.as_ref().unwrap().dialogue[0].text,
                    "导演结果"
                );
            } else {
                assert!(result.is_err());
                assert!(!studio.auto_export_after_director);
                assert!(studio.director_response_hash.is_none());
                assert_eq!(
                    json_to_string(studio.replay.as_ref().unwrap()).unwrap(),
                    before
                );
            }
        }
    }

    #[test]
    fn replay_edit_fingerprint_and_tts_signature_cover_all_editor_metadata() {
        let mut replay = test_replay(vec![
            positioned_dialogue(1, 1, 100, [1, 2, 3]),
            positioned_dialogue(2, 2, 200, [4, 5, 6]),
        ]);
        replay.dialogue[0].area = "甲板".to_owned();
        replay.dialogue[1].area = "机库".to_owned();
        replay.area_blocks = vec![
            ReplayAreaBlock {
                id: 1,
                area: "甲板".to_owned(),
                line_ids: vec![1],
            },
            ReplayAreaBlock {
                id: 2,
                area: "机库".to_owned(),
                line_ids: vec![2],
            },
        ];
        replay.duration_ms = compile_area_block_timeline(&mut replay);
        let baseline_edit = replay_edit_fingerprint_state(&replay).unwrap();
        let baseline_tts = replay_voice_signature(&replay, 1.0);

        let mut variants = Vec::new();
        let mut changed = replay.clone();
        changed.dialogue[0].text.push_str("修改");
        variants.push(changed);
        let mut changed = replay.clone();
        changed.dialogue[0].turn_index += 1;
        variants.push(changed);
        let mut changed = replay.clone();
        changed.dialogue[0].position_cells[0] += 1;
        variants.push(changed);
        let mut changed = replay.clone();
        changed.dialogue[0].area = "舰桥".to_owned();
        variants.push(changed);
        let mut changed = replay.clone();
        changed.dialogue[0].included = false;
        variants.push(changed);
        let mut changed = replay.clone();
        changed.area_blocks.swap(0, 1);
        variants.push(changed);

        for changed in variants {
            assert_ne!(
                replay_edit_fingerprint_state(&changed).unwrap(),
                baseline_edit
            );
            assert_ne!(
                replay_voice_signature(&changed, 1.0),
                baseline_tts
            );
        }
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
    fn project_loading_preserves_authored_sides_and_migrates_legacy_sides() {
        let manager: NapcatMessageManager = from_str(r#"{"messages":{}}"#).unwrap();
        for version in [
            LEGACY_REPLAY_FORMAT_VERSION,
            AREA_REPLAY_FORMAT_VERSION,
            REPLAY_FORMAT_VERSION,
        ] {
            let mut gm = test_dialogue(0, 600, DialogueSide::Right);
            gm.sender_id = 0;
            gm.line_id = 1;
            let mut unregistered_gm = test_dialogue(800, 600, DialogueSide::Left);
            unregistered_gm.sender_id = 7;
            unregistered_gm.line_id = 2;
            let mut replay = test_replay(vec![gm, unregistered_gm]);
            replay.duration_ms = 1_400;
            replay.format_version = version;
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("saved-sides.willow-replay.json");
            export_replay(&replay, path.to_str().unwrap()).unwrap();

            let imported = import_replay(path.to_str().unwrap(), Some(&manager)).unwrap();
            assert_eq!(
                imported.format_version,
                REPLAY_FORMAT_VERSION
            );
            let expected = if version == REPLAY_FORMAT_VERSION {
                [DialogueSide::Right, DialogueSide::Left]
            } else {
                [DialogueSide::Left, DialogueSide::Right]
            };
            assert_eq!(
                imported
                    .dialogue
                    .iter()
                    .map(|line| line.side)
                    .collect::<Vec<_>>(),
                expected,
            );
        }
    }

    #[test]
    fn area_replay_version_two_imports_without_losing_layout() {
        let mut first = positioned_dialogue(11, 2, 1_200, [0, 0, 0]);
        first.area = "舰桥".to_owned();
        let mut second = positioned_dialogue(22, 3, 1_100, [8, 4, -2]);
        second.area = "机库".to_owned();
        let mut replay = test_replay(vec![first, second]);
        replay.format_version = AREA_REPLAY_FORMAT_VERSION;
        replay.area_radius_cells = 24;
        replay.area_blocks = vec![
            ReplayAreaBlock {
                id: 7,
                area: "舰桥".to_owned(),
                line_ids: vec![11],
            },
            ReplayAreaBlock {
                id: 9,
                area: "机库".to_owned(),
                line_ids: vec![22],
            },
        ];

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("area-v2.willow-replay.json");
        fs::write(
            &path,
            serde_json::to_vec(&replay).unwrap(),
        )
        .unwrap();

        let imported = import_replay(path.to_str().unwrap(), None).unwrap();

        assert_eq!(
            imported.format_version,
            REPLAY_FORMAT_VERSION
        );
        assert_eq!(imported.area_radius_cells, 24);
        assert_eq!(
            imported
                .area_blocks
                .iter()
                .map(|block| (block.id, block.line_ids.clone()))
                .collect::<Vec<_>>(),
            vec![(7, vec![11]), (9, vec![22])]
        );
        assert_eq!(imported.dialogue[0].position_cells, [
            0, 0, 0
        ]);
        assert_eq!(imported.dialogue[1].position_cells, [
            8, 4, -2
        ]);
    }

    #[test]
    fn legacy_chronological_layout_cannot_be_erased_by_area_rebuilds() {
        let mut first = test_dialogue(350, 600, DialogueSide::Left);
        first.line_id = 1;
        first.area = "旧时间线".to_owned();
        let mut second = test_dialogue(1_220, 600, DialogueSide::Right);
        second.line_id = 2;
        second.area = "旧时间线".to_owned();
        let mut replay = test_replay(vec![first, second]);
        replay.area_blocks = vec![ReplayAreaBlock {
            id: 1,
            area: "旧时间线".to_owned(),
            line_ids: vec![1, 2],
        }];
        let expected = replay.area_blocks.clone();

        auto_group_replay_areas(&mut replay);
        rebuild_area_blocks(&mut replay);

        assert_eq!(replay.area_blocks, expected);
        assert!(replay_uses_legacy_chronological_layout(
            &replay
        ));
    }

    #[test]
    fn version_one_import_sorts_legacy_dialogue_chronologically() {
        let replay_json = r#"{"format_version":1,"title":"legacy","campaign_id":"c","created_at_unix_ms":1,"duration_ms":5000,"audience":{"scope":"public"},"scene":{"voxels":[]},"camera":[],"dialogue":[]}"#;
        let mut legacy: ReplayFile = serde_json::from_str(replay_json).unwrap();
        legacy.dialogue.push(test_dialogue(
            2_000,
            600,
            DialogueSide::Right,
        ));
        legacy.dialogue.push(test_dialogue(
            500,
            600,
            DialogueSide::Left,
        ));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("unordered-v1.willow-replay.json");
        fs::write(
            &path,
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();

        let imported = import_replay(path.to_str().unwrap(), None).unwrap();

        assert_eq!(
            imported
                .dialogue
                .iter()
                .map(|line| line.source_time)
                .collect::<Vec<_>>(),
            vec![500, 2_000]
        );
        assert!(imported.dialogue.iter().all(|line| !line.snapshot_recorded));
        assert_eq!(
            imported.area_blocks[0].line_ids,
            imported
                .dialogue
                .iter()
                .map(|line| line.line_id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn legacy_import_compacts_real_world_chat_gaps() {
        let replay_json = r#"{"format_version":1,"title":"legacy","campaign_id":"c","created_at_unix_ms":1,"duration_ms":61351511400,"audience":{"scope":"public"},"scene":{"voxels":[]},"camera":[{"time_ms":0,"translation":[0.0,0.0,0.0],"rotation":[0.0,0.0,0.0,1.0]},{"time_ms":61351511400,"translation":[1.0,0.0,0.0],"rotation":[0.0,0.0,0.0,1.0]}],"dialogue":[]}"#;
        let mut legacy: ReplayFile = serde_json::from_str(replay_json).unwrap();
        legacy.dialogue.push(test_dialogue(
            0,
            2_400,
            DialogueSide::Left,
        ));
        legacy.dialogue.push(test_dialogue(
            61_351_509_000,
            2_400,
            DialogueSide::Right,
        ));

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.willow-replay.json");
        fs::write(
            &path,
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        let imported = import_replay(path.to_str().unwrap(), None).unwrap();

        assert_eq!(
            imported.format_version,
            REPLAY_FORMAT_VERSION
        );
        assert!(imported.duration_ms < 30_000);
        assert!(imported.dialogue[1].time_ms < 15_000);
        assert!(imported
            .camera
            .iter()
            .all(|frame| frame.time_ms <= imported.duration_ms));
    }

    #[test]
    fn speaker_voice_profiles_use_fixed_emotivoice_speakers() {
        let profiles = (0..8).map(speaker_voice_profile).collect::<Vec<_>>();
        assert!(profiles.iter().all(|(pitch, _)| *pitch == 0));
        assert_eq!(DEFAULT_EMOTIVOICE_SPEAKERS.len(), 11);
        assert_eq!(DEFAULT_EMOTIVOICE_SPEAKERS[3], "6671");
        assert_eq!(DEFAULT_EMOTIVOICE_SPEAKERS[4], "6670");
        assert_eq!(emotivoice_voice_profiles().len(), 2_014);
        assert!(emotivoice_voice_profiles()
            .iter()
            .any(|profile| profile.id == "8051" && profile.name == "Maria Kasper"));
        assert_eq!(default_emotivoice_speaker(0), "9000");
        assert_eq!(default_emotivoice_speaker(1), "984");
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
                &["9000".to_owned(), "65".to_owned()],
                "9000"
            ),
            Some("65".to_owned())
        );
        assert_eq!(
            random_emotivoice_speaker(&[], "9000"),
            None
        );
        assert_eq!(
            resolved_emotivoice_speaker(Some("9000"), 0),
            "9000"
        );
        assert_eq!(
            resolved_emotivoice_speaker(Some("8051"), 0),
            "8051"
        );
    }

    #[test]
    fn replay_voice_favorites_round_trip_every_voice_setting() {
        let favorites = ReplayVoiceFavorites {
            favorites: vec![ReplayVoiceFavorite {
                name: "反派低语".to_owned(),
                sender_id: Some(42),
                settings: SpeakerVoiceSettings {
                    voice_name: Some("8051".to_owned()),
                    emotion: Some("悲伤".to_owned()),
                    onnx_speaker_id: None,
                    pitch: -3,
                    speech_rate: 42,
                    volume: 0.73,
                },
            }],
        };
        let serialized = serde_json::to_string(&favorites).unwrap();
        let restored: ReplayVoiceFavorites = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.favorites, favorites.favorites);

        let legacy: ReplayVoiceFavorites = serde_json::from_str(
            r#"{"favorites":[{"name":"旧收藏","settings":{"speech_rate":0,"volume":1.0}}]}"#,
        )
        .unwrap();
        assert_eq!(legacy.favorites[0].sender_id, None);
    }

    #[test]
    fn generated_replay_applies_latest_favorite_for_each_speaker() {
        let mut replay = test_replay(vec![
            test_dialogue_for_speaker(42),
            test_dialogue_for_speaker(7),
        ]);
        let earlier = test_voice_settings("9000", 10);
        let latest = test_voice_settings("8051", 25);
        let favorites = ReplayVoiceFavorites {
            favorites: vec![
                ReplayVoiceFavorite {
                    name: "旧设置".to_owned(),
                    sender_id: Some(42),
                    settings: earlier,
                },
                ReplayVoiceFavorite {
                    name: "未关联收藏".to_owned(),
                    sender_id: None,
                    settings: test_voice_settings("65", 0),
                },
                ReplayVoiceFavorite {
                    name: "当前设置".to_owned(),
                    sender_id: Some(42),
                    settings: latest.clone(),
                },
                ReplayVoiceFavorite {
                    name: "聊天中不存在".to_owned(),
                    sender_id: Some(99),
                    settings: test_voice_settings("65", 0),
                },
            ],
        };

        assert_eq!(
            apply_favorite_voice_settings(&mut replay, &favorites),
            1
        );
        assert_eq!(
            replay.speaker_voice_settings.get(&42),
            Some(&latest)
        );
        assert!(!replay.speaker_voice_settings.contains_key(&7));
        assert!(!replay.speaker_voice_settings.contains_key(&99));
    }

    #[test]
    fn replay_voice_settings_are_optional_and_round_trip() {
        let old_json = r#"{"format_version":1,"title":"test","campaign_id":"c","created_at_unix_ms":1,"duration_ms":0,"audience":{"scope":"public"},"scene":{"voxels":[]},"camera":[],"dialogue":[]}"#;
        let mut replay: ReplayFile = serde_json::from_str(old_json).unwrap();
        assert!(replay.speaker_voice_settings.is_empty());
        assert_eq!(replay.master_speech_speed, 1.10);
        assert_eq!(replay.master_dialogue_duration, 1.0);
        assert_eq!(
            replay.camera_distance_scale,
            default_directed_camera_distance_scale()
        );
        assert_eq!(
            replay.camera_yaw_degrees,
            default_directed_camera_yaw_degrees()
        );
        assert_eq!(
            replay.camera_transition_curve,
            default_camera_transition_curve()
        );
        assert_eq!(
            replay.player_movement_curve,
            default_player_movement_curve()
        );
        assert!(replay.player_movements.is_empty());
        replay.master_speech_speed = 1.15;
        replay.master_dialogue_duration = 2.75;
        replay.camera_distance_scale = 2.25;
        replay.camera_yaw_degrees = 37.5;
        replay.camera_transition_curve = 3.25;
        replay.player_movement_curve = 0.4;
        replay
            .speaker_voice_settings
            .insert(42, SpeakerVoiceSettings {
                voice_name: Some("9000".to_owned()),
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
            Some("9000")
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
        assert_eq!(restored.camera_distance_scale, 2.25);
        assert_eq!(restored.camera_yaw_degrees, 37.5);
        assert_eq!(restored.camera_transition_curve, 3.25);
        assert_eq!(restored.player_movement_curve, 0.4);
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
    fn local_bgm_is_a_non_silent_stereo_wav() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.wav");
        write_test_bgm(&source);
        let path = directory.path().join("music.wav");
        write_background_music_track(&source, &path, 250, 0.35).unwrap();
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[22..24], &2_u16.to_le_bytes());
        assert!(bytes[44..].iter().any(|byte| *byte != 0));
    }

    #[test]
    fn bgm_folder_loads_supported_music_only_in_name_order() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("z.MP3"), b"music").unwrap();
        fs::write(directory.path().join("A.ogg"), b"music").unwrap();
        fs::write(
            directory.path().join("notes.txt"),
            b"not music",
        )
        .unwrap();

        let files = discover_background_music_in(directory.path());
        assert_eq!(
            files
                .iter()
                .filter_map(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["A.ogg", "z.MP3"]
        );
    }

    #[test]
    #[ignore = "requires the installed EmotiVoice Chinese runtime"]
    fn emotivoice_synthesizes_distinct_fixed_chinese_voices() {
        let mut tts = create_onnx_tts().unwrap();
        let chinese = "你好诶艾，我在维护跑团回放。";
        let first = tts.synthesize(chinese, "9000", "普通", 1.4).unwrap();
        let repeated = tts.synthesize(chinese, "9000", "普通", 1.4).unwrap();
        let second = tts.synthesize(chinese, "65", "开心", 1.4).unwrap();
        let unrestricted_fast = tts.synthesize(chinese, "9000", "普通", 3.0).unwrap();
        assert_eq!(&first[0..4], b"RIFF");
        assert!(first.len() > 44);
        assert_eq!(first, repeated);
        assert!(unrestricted_fast.len() > 44);
        assert!(unrestricted_fast.len() < first.len());
        assert_ne!(first, second);
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "requires the installed EmotiVoice Chinese runtime"]
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
        let music = directory.path().join("music.wav");
        write_test_bgm(&music);
        let mut dialogue = test_dialogue(0, 900, DialogueSide::Right);
        dialogue.text = "你好，这是角色语音。".to_owned();
        encode_video_frames(
            directory.path(),
            &output,
            10,
            1_000,
            Some(&music),
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

    fn write_test_bgm(path: &Path) {
        let samples = (0..8_000)
            .flat_map(|index| {
                let sample = if index % 32 < 16 { 8_000_i16 } else { -8_000_i16 };
                [sample, sample]
            })
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut writer = BufWriter::new(fs::File::create(path).unwrap());
        write_wav_header(
            &mut writer,
            32_000,
            2,
            16,
            samples.len() as u32,
        )
        .unwrap();
        writer.write_all(&samples).unwrap();
        writer.flush().unwrap();
    }

    fn test_dialogue(time_ms: u64, duration_ms: u64, side: DialogueSide) -> ReplayDialogue {
        ReplayDialogue {
            time_ms,
            duration_ms,
            duration_locked: false,
            sender_id: 1,
            camera_focus_id: None,
            name: "测试".to_owned(),
            role: String::new(),
            text: "台词".to_owned(),
            speech_text: None,
            speech_enabled: true,
            speech_rate: default_line_speech_rate(),
            speech_volume: default_line_speech_volume(),
            avatar: String::new(),
            avatar_data_url: None,
            visibility: Visibility::Public,
            side,
            line_id: 0,
            source_time: time_ms,
            turn_index: 0,
            position_cells: [0, 0, 0],
            area: String::new(),
            included: true,
            snapshot_recorded: false,
            metadata_estimated: false,
            forwarded: false,
        }
    }

    fn test_dialogue_for_speaker(sender_id: u64) -> ReplayDialogue {
        let mut dialogue = test_dialogue(0, 600, DialogueSide::Right);
        dialogue.sender_id = sender_id;
        dialogue
    }

    fn test_voice_settings(voice_name: &str, speech_rate: i32) -> SpeakerVoiceSettings {
        SpeakerVoiceSettings {
            voice_name: Some(voice_name.to_owned()),
            emotion: Some("普通".to_owned()),
            onnx_speaker_id: None,
            pitch: 0,
            speech_rate,
            volume: 1.0,
        }
    }

    fn positioned_dialogue(
        line_id: u64,
        turn_index: u32,
        source_time: u64,
        position_cells: [i32; 3],
    ) -> ReplayDialogue {
        let mut line = test_dialogue(source_time, 600, DialogueSide::Right);
        line.line_id = line_id;
        line.turn_index = turn_index;
        line.source_time = source_time;
        line.position_cells = position_cells;
        line.snapshot_recorded = true;
        line
    }

    fn test_replay(dialogue: Vec<ReplayDialogue>) -> ReplayFile {
        ReplayFile {
            format_version: REPLAY_FORMAT_VERSION,
            title: "test".to_owned(),
            campaign_id: "campaign".to_owned(),
            created_at_unix_ms: 1,
            duration_ms: 0,
            audience: ReplayAudience::Public,
            scene: ReplayScene::default(),
            camera: Vec::new(),
            camera_distance_scale: default_directed_camera_distance_scale(),
            camera_yaw_degrees: default_directed_camera_yaw_degrees(),
            camera_transition_curve: default_camera_transition_curve(),
            player_movement_curve: default_player_movement_curve(),
            player_movements: Vec::new(),
            authored_player_movements: BTreeSet::new(),
            authored_ship_trajectories: BTreeSet::new(),
            player_movement_history_cursor_unix_ms: 1,
            ship_trajectories: Vec::new(),
            standee_positions: Vec::new(),
            ship_motion_speed: default_ship_motion_speed(),
            dialogue_waits_for_ship_motion: default_dialogue_waits_for_ship_motion(),
            terrain_changes: Vec::new(),
            ship_hull_changes: Vec::new(),
            ship_trajectory_history_cursor_unix_ms: 1,
            dialogue,
            area_blocks: Vec::new(),
            manual_dialogue_order: false,
            area_radius_cells: DEFAULT_AREA_RADIUS_CELLS,
            master_speech_speed: default_master_speech_speed(),
            master_dialogue_duration: default_master_dialogue_duration(),
            speaker_voice_settings: HashMap::new(),
        }
    }

    #[test]
    fn ship_trajectory_interpolates_and_holds_pose() {
        let trajectory = ReplayShipTrajectory {
            ship_id: "ship-1".to_owned(),
            ship_name: "测试舰".to_owned(),
            keyframes: vec![
                ReplayShipKeyframe {
                    time_ms: 0,
                    translation: [0.0, 0.0, 0.0],
                    rotation: Quat::IDENTITY.to_array(),
                },
                ReplayShipKeyframe {
                    time_ms: 1_000,
                    translation: [10.0, 0.0, 0.0],
                    rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array(),
                },
            ],
        };

        let (translation, _) = interpolated_ship_pose(&trajectory, 500).unwrap();
        assert!((translation - Vec3::new(5.0, 0.0, 0.0)).length() < 0.001);

        let (translation, rotation) = interpolated_ship_pose(&trajectory, 0).unwrap();
        assert_eq!(translation, Vec3::ZERO);
        assert_eq!(rotation, Quat::IDENTITY);

        let (translation, rotation) = interpolated_ship_pose(&trajectory, 5_000).unwrap();
        assert_eq!(translation, Vec3::new(10.0, 0.0, 0.0));
        assert!((rotation - Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)).length() < 0.001);
    }

    #[test]
    fn replay_turns_group_consecutive_lines_by_turn_index() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 1_000, [0, 0, 0]),
            positioned_dialogue(2, 1, 1_100, [0, 0, 0]),
            positioned_dialogue(3, 2, 1_200, [0, 0, 0]),
            positioned_dialogue(4, 2, 1_300, [0, 0, 0]),
        ];
        let mut replay = test_replay(lines.clone());
        let mut timeline = 350;
        for line in &mut replay.dialogue {
            line.time_ms = timeline;
            timeline += 700;
        }
        let mut turns = replay_turns(&replay);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].turn_index, 1);
        assert_eq!(turns[1].turn_index, 2);
        assert!(turns[0].end_ms < turns[1].start_ms);
        assert_eq!(
            current_replay_turn(&turns, 400),
            Some(0)
        );
        assert_eq!(next_replay_turn(&turns, 400), Some(1));
        assert_eq!(
            previous_replay_turn(&turns, 1_100),
            Some(0)
        );

        // A later segment with the same turn index becomes its own segment.
        lines.push(positioned_dialogue(5, 1, 1_400, [
            0, 0, 0,
        ]));
        let mut replay = test_replay(lines);
        let mut timeline = 350;
        for line in &mut replay.dialogue {
            line.time_ms = timeline;
            timeline += 700;
        }
        turns = replay_turns(&replay);
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[2].turn_index, 1);
    }

    #[test]
    fn replay_terrain_changes_apply_forward_and_rewind() {
        let mut world = World::new();
        let entity = world.spawn(Grid::<u8>::new()).id();
        let mut dirty = VoxelGeometryDirtyChunks::default();
        let mut state = ReplayTerrainPlaybackState {
            replay_key: Some(1),
            scene_revision: 0,
            baseline: HashMap::from([(IVec3::ZERO, 1_u8)]),
            applied: HashMap::from([(IVec3::ZERO, 1_u8)]),
            next_change: 0,
            covered_time_ms: 0,
        };
        let changes = vec![ReplayTerrainChange {
            time_ms: 300,
            position: [1, 0, 0],
            material: 2,
            enabled: true,
        }];

        let mut entity_mut = world.entity_mut(entity);
        let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
        grid.set(IVec3::ZERO, 1);
        apply_terrain_changes_at(
            &mut grid, &mut dirty, &mut state, &changes, 500,
        );
        assert_eq!(grid.get(IVec3::new(1, 0, 0)), Some(&2));
        assert_eq!(state.next_change, 1);

        apply_terrain_changes_at(
            &mut grid, &mut dirty, &mut state, &changes, 0,
        );
        assert_eq!(grid.get(IVec3::new(1, 0, 0)), Some(&0));
        assert_eq!(grid.get(IVec3::ZERO), Some(&1));
        assert_eq!(state.next_change, 0);

        apply_terrain_changes_at(
            &mut grid, &mut dirty, &mut state, &changes, 500,
        );
        assert_eq!(grid.get(IVec3::new(1, 0, 0)), Some(&2));
    }

    #[test]
    fn applying_replay_scene_only_writes_changed_voxels() {
        let mut world = World::new();
        let entity = world.spawn(Grid::<u8>::new()).id();
        let mut entity_mut = world.entity_mut(entity);
        let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();
        grid.set(IVec3::ZERO, 1);
        grid.set(IVec3::X, 2);

        let identical = ReplayScene {
            voxels: vec![
                ReplayVoxel {
                    position: IVec3::ZERO.to_array(),
                    material: 1,
                },
                ReplayVoxel {
                    position: IVec3::X.to_array(),
                    material: 2,
                },
            ],
        };
        assert_eq!(apply_scene(&mut grid, &identical), 0);

        let changed = ReplayScene {
            voxels: vec![
                ReplayVoxel {
                    position: IVec3::ZERO.to_array(),
                    material: 3,
                },
                ReplayVoxel {
                    position: IVec3::Y.to_array(),
                    material: 4,
                },
            ],
        };
        assert_eq!(apply_scene(&mut grid, &changed), 3);
        assert_eq!(grid.get(IVec3::ZERO), Some(&3));
        assert_eq!(grid.get(IVec3::X), Some(&0));
        assert_eq!(grid.get(IVec3::Y), Some(&4));
    }

    #[test]
    fn replay_ship_hull_changes_destroy_and_restore_cells() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &mut world);
        let mut occupancy = VoxelSpaceshipOccupancyCache::default();
        let original = vec![(IVec3::ZERO, 1_u8), (IVec3::X, 2_u8)];
        let mut state = ReplayShipHullPlaybackState {
            replay_key: Some(1),
            scene_revision: 0,
            original_cells: HashMap::from([(entity, original)]),
            applied: HashMap::from([(
                entity,
                HashMap::from([(IVec3::ZERO, 1_u8), (IVec3::X, 2_u8)]),
            )]),
            next_change: 0,
            covered_time_ms: 0,
            active: true,
        };
        let id_to_entity = HashMap::from([("ship-1".to_owned(), entity)]);
        let mut changes = vec![ReplayShipHullChange {
            time_ms: 500,
            ship_id: "ship-1".to_owned(),
            position: [1, 0, 0],
            material: 0,
            enabled: true,
        }];

        apply_ship_hull_changes_at(
            &mut commands,
            &mut occupancy,
            &mut state,
            &id_to_entity,
            &changes,
            1_000,
        );
        let entry = occupancy.ships.get(&entity).expect("ship occupancy");
        assert!(entry.contains_cell(IVec3::ZERO));
        assert!(!entry.contains_cell(IVec3::X));

        changes[0].enabled = false;
        state.covered_time_ms = u64::MAX;
        apply_ship_hull_changes_at(
            &mut commands,
            &mut occupancy,
            &mut state,
            &id_to_entity,
            &changes,
            1_000,
        );
        let entry = occupancy.ships.get(&entity).expect("ship occupancy");
        assert!(entry.contains_cell(IVec3::X));

        changes[0].enabled = true;
        state.covered_time_ms = u64::MAX;
        apply_ship_hull_changes_at(
            &mut commands,
            &mut occupancy,
            &mut state,
            &id_to_entity,
            &changes,
            1_000,
        );
        let entry = occupancy.ships.get(&entity).expect("ship occupancy");
        assert!(!entry.contains_cell(IVec3::X));

        apply_ship_hull_changes_at(
            &mut commands,
            &mut occupancy,
            &mut state,
            &id_to_entity,
            &changes,
            0,
        );
        let entry = occupancy.ships.get(&entity).expect("ship occupancy");
        assert!(entry.contains_cell(IVec3::ZERO));
        assert!(entry.contains_cell(IVec3::X));
    }

    #[test]
    fn ship_motion_events_split_at_stationary_gaps() {
        let history = ReplayShipTrajectoryHistory {
            sessions: vec![PersistedShipTrajectorySession {
                campaign_id: "campaign".to_owned(),
                ship_id: "ship-1".to_owned(),
                ship_name: "测试舰".to_owned(),
                turn_index: 1,
                start_after_source_time: Some(100),
                start_delay_ms: 0,
                keyframes: vec![
                    PersistedShipKeyframe {
                        source_unix_ms: 100_000,
                        translation: [0.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    PersistedShipKeyframe {
                        source_unix_ms: 100_100,
                        translation: [1.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    PersistedShipKeyframe {
                        source_unix_ms: 110_000,
                        translation: [2.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                ],
            }],
        };

        let events = ship_motion_events(&history, "campaign", 0);

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].start_source_ms, 100_000);
        assert_eq!(events[0].end_source_ms, 100_100);
        assert_eq!(events[1].start_source_ms, 109_999);
        assert_eq!(events[1].end_source_ms, 110_000);
    }

    #[test]
    fn stationary_ship_records_no_motion_events() {
        let history = ReplayShipTrajectoryHistory {
            sessions: vec![PersistedShipTrajectorySession {
                campaign_id: "campaign".to_owned(),
                ship_id: "ship-1".to_owned(),
                ship_name: "测试舰".to_owned(),
                turn_index: 1,
                start_after_source_time: Some(100),
                start_delay_ms: 0,
                keyframes: vec![
                    PersistedShipKeyframe {
                        source_unix_ms: 100_000,
                        translation: [1.0, 2.0, 3.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    PersistedShipKeyframe {
                        source_unix_ms: 105_000,
                        translation: [1.0, 2.0, 3.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    PersistedShipKeyframe {
                        source_unix_ms: 110_000,
                        translation: [1.0, 2.0, 3.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                ],
            }],
        };

        assert!(ship_motion_events(&history, "campaign", 0).is_empty());
    }

    #[test]
    fn movement_after_last_dialogue_line_stays_visible_in_the_turn() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 100, [0, 0, 0]),
            positioned_dialogue(2, 1, 200, [0, 0, 0]),
        ];
        assign_replay_line_ids(&mut lines);
        let mut replay = test_replay(lines);
        auto_group_replay_areas(&mut replay);
        rebuild_area_blocks(&mut replay);
        let history = ReplayShipTrajectoryHistory {
            sessions: vec![PersistedShipTrajectorySession {
                campaign_id: "campaign".to_owned(),
                ship_id: "ship-1".to_owned(),
                ship_name: "测试舰".to_owned(),
                turn_index: 1,
                start_after_source_time: Some(200),
                start_delay_ms: 0,
                keyframes: vec![
                    PersistedShipKeyframe {
                        source_unix_ms: 260_000,
                        translation: [0.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    PersistedShipKeyframe {
                        source_unix_ms: 260_100,
                        translation: [8.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                ],
            }],
        };

        compile_scene_dynamics_timeline(&mut replay, &history, 0, &[], &[]);

        let trajectory = replay
            .ship_trajectories
            .iter()
            .find(|trajectory| trajectory.ship_id == "ship-1")
            .expect("ship trajectory");
        let last_line = replay
            .dialogue
            .iter()
            .max_by_key(|line| line.time_ms)
            .expect("dialogue line");
        assert!(
            trajectory.keyframes.last().unwrap().time_ms
                > last_line.time_ms.saturating_add(last_line.duration_ms)
        );
        let turns = replay_turns(&replay);
        assert_eq!(turns.len(), 1);
        assert!(turns[0].end_ms >= trajectory.keyframes.last().unwrap().time_ms);
    }

    #[test]
    fn dynamic_timeline_inserts_ship_motion_between_dialogue_lines() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 100, [0, 0, 0]),
            positioned_dialogue(2, 1, 200, [0, 0, 0]),
        ];
        assign_replay_line_ids(&mut lines);
        let mut replay = test_replay(lines);
        auto_group_replay_areas(&mut replay);
        rebuild_area_blocks(&mut replay);
        let history = ReplayShipTrajectoryHistory {
            sessions: vec![PersistedShipTrajectorySession {
                campaign_id: "campaign".to_owned(),
                ship_id: "ship-1".to_owned(),
                ship_name: "测试舰".to_owned(),
                turn_index: 1,
                start_after_source_time: Some(100),
                start_delay_ms: 0,
                keyframes: vec![
                    PersistedShipKeyframe {
                        source_unix_ms: 150_000,
                        translation: [0.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    PersistedShipKeyframe {
                        source_unix_ms: 150_100,
                        translation: [10.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                ],
            }],
        };

        let imported = compile_scene_dynamics_timeline(&mut replay, &history, 0, &[], &[]);

        assert_eq!(imported, 1);
        let first = &replay.dialogue[0];
        let second = &replay.dialogue[1];
        assert!(second.time_ms > first.time_ms.saturating_add(first.duration_ms));
        let trajectory = replay
            .ship_trajectories
            .iter()
            .find(|trajectory| trajectory.ship_id == "ship-1")
            .expect("ship trajectory");
        assert_eq!(trajectory.keyframes.len(), 2);
        for frame in &trajectory.keyframes {
            assert!(frame.time_ms >= first.time_ms.saturating_add(first.duration_ms));
            assert!(frame.time_ms < second.time_ms);
        }
        assert!(trajectory.keyframes[1].time_ms > trajectory.keyframes[0].time_ms);
    }

    #[test]
    fn pending_terrain_maps_into_the_motion_segment() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 100, [0, 0, 0]),
            positioned_dialogue(2, 1, 200, [0, 0, 0]),
        ];
        assign_replay_line_ids(&mut lines);
        let mut replay = test_replay(lines);
        auto_group_replay_areas(&mut replay);
        rebuild_area_blocks(&mut replay);
        let history = ReplayShipTrajectoryHistory {
            sessions: vec![PersistedShipTrajectorySession {
                campaign_id: "campaign".to_owned(),
                ship_id: "ship-1".to_owned(),
                ship_name: "测试舰".to_owned(),
                turn_index: 1,
                start_after_source_time: Some(100),
                start_delay_ms: 0,
                keyframes: vec![
                    PersistedShipKeyframe {
                        source_unix_ms: 150_000,
                        translation: [0.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    PersistedShipKeyframe {
                        source_unix_ms: 150_100,
                        translation: [10.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                ],
            }],
        };
        let pending_terrain = vec![(150_050_u64, IVec3::new(3, 0, 0), 5_u8)];

        compile_scene_dynamics_timeline(
            &mut replay,
            &history,
            0,
            &pending_terrain,
            &[],
        );

        let first = &replay.dialogue[0];
        let second = &replay.dialogue[1];
        let change = replay.terrain_changes.first().expect("terrain change");
        assert!(change.time_ms > first.time_ms.saturating_add(first.duration_ms));
        assert!(change.time_ms < second.time_ms);
        assert_eq!(change.position, [3, 0, 0]);
    }

    #[test]
    fn clearing_ship_trajectory_history_removes_only_matching_campaign() {
        let mut history = ReplayShipTrajectoryHistory {
            sessions: vec![
                PersistedShipTrajectorySession {
                    campaign_id: "campaign-a".to_owned(),
                    ship_id: "ship-1".to_owned(),
                    ..default()
                },
                PersistedShipTrajectorySession {
                    campaign_id: "campaign-b".to_owned(),
                    ship_id: "ship-1".to_owned(),
                    ..default()
                },
            ],
        };

        assert_eq!(
            clear_campaign_replay_ship_trajectory_history(&mut history, "campaign-a"),
            1
        );
        assert_eq!(history.sessions.len(), 1);
        assert_eq!(
            history.sessions[0].campaign_id,
            "campaign-b"
        );
    }

    #[test]
    fn clearing_test_progress_resets_all_replay_data() {
        let mut studio = ReplayStudio::default();
        studio.mode = ReplayMode::Paused;
        studio.playback_ms = 4_000;
        studio.record_elapsed_ms = 1_000;
        studio.message_counts.insert("target".to_owned(), 5);
        studio.pending_terrain_changes.push((1, IVec3::ZERO, 1));
        studio
            .pending_ship_hull_changes
            .push((1, "ship".to_owned(), IVec3::X, 1));
        let mut replay = test_replay(vec![test_dialogue(
            350,
            2_400,
            DialogueSide::Right,
        )]);
        replay.camera.push(camera_keyframe(0, &Transform::IDENTITY));
        replay.camera.push(camera_keyframe(
            4_000,
            &Transform::from_xyz(1.0, 2.0, 3.0),
        ));
        replay.ship_trajectories.push(ReplayShipTrajectory {
            ship_id: "ship-1".to_owned(),
            ship_name: "测试舰".to_owned(),
            keyframes: vec![ReplayShipKeyframe {
                time_ms: 1_000,
                translation: [1.0, 0.0, 0.0],
                rotation: Quat::IDENTITY.to_array(),
            }],
        });
        replay.terrain_changes.push(ReplayTerrainChange {
            time_ms: 1_000,
            position: [0, 0, 0],
            material: 1,
            enabled: true,
        });
        studio.replay = Some(replay);

        let mut tracker = ReplaySnapshotTracker {
            initialized: true,
            message_counts: HashMap::from([("target".to_owned(), 3)]),
        };
        let mut movement_recorder = ReplayMovementHistoryRecorder {
            active_session: Some(("campaign".to_owned(), 42)),
            session_index: Some(0),
            sample_accumulator: 0.5,
            persist_accumulator: 0.2,
        };
        let mut ship_recorder = ReplayShipTrajectoryRecorder {
            sessions: HashMap::from([(
                (
                    "campaign".to_owned(),
                    "ship-1".to_owned(),
                ),
                0,
            )]),
            sample_accumulators: HashMap::from([(
                (
                    "campaign".to_owned(),
                    "ship-1".to_owned(),
                ),
                0.1,
            )]),
            pending_poses: HashMap::from([(
                (
                    "campaign".to_owned(),
                    "ship-1".to_owned(),
                ),
                PersistedShipKeyframe {
                    source_unix_ms: 1,
                    translation: [0.0; 3],
                    rotation: Quat::IDENTITY.to_array(),
                },
            )]),
            persist_accumulator: 0.1,
        };
        let mut player_history = ReplayPlayerMovementHistory {
            sessions: vec![PersistedPlayerMovementSession {
                campaign_id: "campaign".to_owned(),
                user_id: 42,
                ..default()
            }],
        };
        let mut ship_history = ReplayShipTrajectoryHistory {
            sessions: vec![PersistedShipTrajectorySession {
                campaign_id: "campaign".to_owned(),
                ship_id: "ship-1".to_owned(),
                ..default()
            }],
        };

        let removed = clear_campaign_replay_data(
            &mut studio,
            &mut tracker,
            &mut movement_recorder,
            &mut ship_recorder,
            &mut player_history,
            &mut ship_history,
            "campaign",
        );

        assert_eq!(removed, 2);
        assert!(studio.replay.is_none());
        assert_eq!(studio.mode, ReplayMode::Idle);
        assert_eq!(studio.playback_ms, 0);
        assert_eq!(studio.record_elapsed_ms, 0);
        assert!(studio.message_counts.is_empty());
        assert!(studio.pending_terrain_changes.is_empty());
        assert!(studio.pending_ship_hull_changes.is_empty());
        assert!(studio.pre_playback_scene.is_none());
        assert!(!tracker.initialized);
        assert!(tracker.message_counts.is_empty());
        assert!(movement_recorder.active_session.is_none());
        assert!(ship_recorder.sessions.is_empty());
        assert!(ship_recorder.pending_poses.is_empty());
        assert!(player_history.sessions.is_empty());
        assert!(ship_history.sessions.is_empty());
    }

    #[test]
    fn ship_history_records_only_gm_manipulated_ships() {
        let entity = Entity::from_bits(7);
        let mut control = VoxelSpaceshipControlState::default();
        control.driving_ship_id = Some("carrier".to_owned());
        let drag = VoxelToolGunDragState::default();
        assert!(ship_is_gm_manipulated(
            &control, &drag, entity, "carrier"
        ));
        assert!(!ship_is_gm_manipulated(
            &control, &drag, entity, "escort"
        ));

        // The tool-gun drag is also treated as GM manipulation.
        control.driving_ship_id = None;
        let mut drag = VoxelToolGunDragState::default();
        drag.target = Some(entity);
        assert!(ship_is_gm_manipulated(
            &control, &drag, entity, "escort"
        ));
        assert!(!ship_is_gm_manipulated(
            &control,
            &drag,
            Entity::from_bits(8),
            "escort"
        ));
    }

    /// A fixed simultaneous fleet flight must import without serializing the
    /// ships' durations or depending on the user's changing campaign history.
    #[test]
    fn fleet_history_imports_arrogance_and_gm_focuses_the_player_standee() {
        let history = ReplayShipTrajectoryHistory {
            sessions: ["usi-arrogance", "escort-1", "escort-2"]
                .into_iter()
                .map(|ship_id| PersistedShipTrajectorySession {
                    campaign_id: "default".to_owned(),
                    ship_id: ship_id.to_owned(),
                    ship_name: ship_id.to_owned(),
                    turn_index: 1,
                    start_after_source_time: None,
                    start_delay_ms: 0,
                    keyframes: (0..120)
                        .map(|index| PersistedShipKeyframe {
                            source_unix_ms: 1_785_749_276_000 + index * 100,
                            translation: [index as f32 * 2.0, 0.0, 0.0],
                            rotation: Quat::IDENTITY.to_array(),
                        })
                        .collect(),
                })
                .collect(),
        };

        // A GM line addressing a player and that player's reply.
        let mut gm = test_dialogue(350, 2_400, DialogueSide::Left);
        gm.sender_id = 0;
        gm.camera_focus_id = Some(1_670_426_821);
        gm.snapshot_recorded = true;
        gm.metadata_estimated = false;
        let mut player = test_dialogue(3_020, 2_400, DialogueSide::Right);
        player.sender_id = 1_670_426_821;
        player.snapshot_recorded = true;
        player.metadata_estimated = false;
        player.position_cells = [-415, 43, -722];
        let mut replay = test_replay(vec![gm, player]);
        replay.campaign_id = "default".to_owned();
        assign_replay_line_ids(&mut replay.dialogue);
        auto_group_replay_areas(&mut replay);
        rebuild_area_blocks(&mut replay);

        let imported = compile_scene_dynamics_timeline(&mut replay, &history, 0, &[], &[]);
        assert!(
            imported > 0,
            "ship history must import into the replay"
        );
        assert!(
            replay
                .ship_trajectories
                .iter()
                .any(|trajectory| trajectory.ship_id == "usi-arrogance"),
            "Arrogance trajectory must be in the replay"
        );
        let arrogance = replay
            .ship_trajectories
            .iter()
            .find(|trajectory| trajectory.ship_id == "usi-arrogance")
            .expect("arrogance trajectory");
        assert!(
            arrogance.keyframes.len() > 100,
            "Arrogance trajectory should carry its recorded motion frames"
        );
        assert!(
            replay.duration_ms < 60_000,
            "two dialogue lines plus one fleet flight must stay short, got {} ms",
            replay.duration_ms
        );

        // Append path: a replay created before the fleet flight imports the
        // newly recorded Arrogance trajectory when 播放 is pressed.
        let mut append_replay = test_replay(Vec::new());
        append_replay.campaign_id = "default".to_owned();
        append_replay.created_at_unix_ms = 1_785_749_275_000;
        append_replay.ship_trajectory_history_cursor_unix_ms = 1_785_749_275_000;
        let appended = append_new_ship_trajectories_from_history(&mut append_replay, &history);
        assert!(
            appended > 0,
            "newly recorded trajectories must append on play"
        );
        let appended_arrogance = append_replay
            .ship_trajectories
            .iter()
            .find(|trajectory| trajectory.ship_id == "usi-arrogance")
            .expect("appended Arrogance trajectory");
        let start_pose = interpolated_ship_pose(appended_arrogance, 0)
            .expect("start pose")
            .0;
        let end_pose = interpolated_ship_pose(
            appended_arrogance,
            append_replay.duration_ms,
        )
        .expect("end pose")
        .0;
        assert!(
            horizontal(start_pose - end_pose).length() > 10.0,
            "Arrogance must visibly move during playback (start {start_pose:?}, end {end_pose:?})"
        );
        assert!(
            append_replay.duration_ms < 45_000,
            "appending the fleet flight must not inflate the timeline, got {} ms",
            append_replay.duration_ms
        );

        // Camera: the GM line must frame the player standee.
        let standee_position = Vec3::new(-122.419_174, -21.016_474, 41.071_274);
        let speaker_positions = HashMap::from([(1_670_426_821, standee_position)]);
        let camera = turn_based_camera_track(
            &Transform::from_xyz(29.004_196, 24.248_276, 221.935),
            &replay.dialogue,
            replay.duration_ms,
            &speaker_positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
            &ReplayCameraObstacles::default(),
        );
        // During playback the standee starts at its earliest recorded
        // position, so the opening GM shot frames that location.
        let mut anchor =
            IVec3::from_array(replay.dialogue[1].position_cells).as_vec3() * VOXEL_SIZE;
        for frame in &camera {
            if let Some(line) = replay
                .dialogue
                .iter()
                .filter(|line| {
                    line.included
                        && line.snapshot_recorded
                        && line.time_ms <= frame.time_ms
                        && line.sender_id == 1_670_426_821
                        && line.position_cells != [0; 3]
                })
                .last()
            {
                anchor = IVec3::from_array(line.position_cells).as_vec3() * VOXEL_SIZE;
            }
            let distance = horizontal(Vec3::from_array(frame.translation) - anchor).length();
            assert!(
                distance < 25.0,
                "camera frame at {:?} is {distance:.1} units from the player standee",
                frame.translation
            );
        }
    }

    #[test]
    fn replay_carries_standees_inside_moving_ships() {
        let mut replay = test_replay(Vec::new());
        replay.ship_trajectories.push(ReplayShipTrajectory {
            ship_id: "carrier".to_owned(),
            ship_name: "运载舰".to_owned(),
            keyframes: vec![
                ReplayShipKeyframe {
                    time_ms: 1_000,
                    translation: [0.0, 0.0, 0.0],
                    rotation: Quat::IDENTITY.to_array(),
                },
                ReplayShipKeyframe {
                    time_ms: 5_000,
                    translation: [100.0, 0.0, 0.0],
                    rotation: Quat::IDENTITY.to_array(),
                },
            ],
        });
        let ship_bounds = HashMap::from([(
            "carrier".to_owned(),
            (
                Vec3::new(-2.0, 0.0, -2.0),
                Vec3::new(2.0, 4.0, 2.0),
            ),
        )]);

        // The standee stood inside the hull at the recorded time; during
        // playback it must travel with the ship instead of staying behind.
        let carried = replay_carried_standee_position(
            &replay,
            &ship_bounds,
            Vec3::new(1.0, 1.0, 0.0),
            1_000,
            5_000,
        )
        .expect("standee on board must be carried");
        assert!(
            (carried - Vec3::new(101.0, 1.0, 0.0)).length() < 0.01,
            "carried standee should follow the ship, got {carried:?}"
        );

        // A standee that was never on the ship stays where the replay says.
        assert!(replay_carried_standee_position(
            &replay,
            &ship_bounds,
            Vec3::new(50.0, 1.0, 0.0),
            1_000,
            5_000,
        )
        .is_none());
    }

    #[test]
    fn ship_anchor_line_and_delay_are_respected_on_import() {
        let mut lines = vec![
            positioned_dialogue(1, 1, 100, [0, 0, 0]),
            positioned_dialogue(2, 1, 200, [0, 0, 0]),
        ];
        assign_replay_line_ids(&mut lines);
        let mut replay = test_replay(lines);
        replay.campaign_id = "default".to_owned();
        auto_group_replay_areas(&mut replay);
        rebuild_area_blocks(&mut replay);
        let history = ReplayShipTrajectoryHistory {
            sessions: vec![PersistedShipTrajectorySession {
                campaign_id: "default".to_owned(),
                ship_id: "ship-1".to_owned(),
                ship_name: "测试舰".to_owned(),
                turn_index: 1,
                start_after_source_time: Some(100),
                start_delay_ms: 500,
                keyframes: vec![
                    PersistedShipKeyframe {
                        source_unix_ms: 150_000,
                        translation: [0.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    PersistedShipKeyframe {
                        source_unix_ms: 150_100,
                        translation: [10.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                ],
            }],
        };

        compile_scene_dynamics_timeline(&mut replay, &history, 0, &[], &[]);

        let anchor_line = replay
            .dialogue
            .iter()
            .find(|line| line.source_time == 100)
            .expect("anchor line");
        let trajectory = replay
            .ship_trajectories
            .iter()
            .find(|trajectory| trajectory.ship_id == "ship-1")
            .expect("ship trajectory");
        let anchor_end = anchor_line.time_ms.saturating_add(anchor_line.duration_ms);
        assert!(
            trajectory.keyframes[0].time_ms >= anchor_end.saturating_add(500),
            "ship motion must start after the chosen line plus its delay, got {} (anchor end {})",
            trajectory.keyframes[0].time_ms,
            anchor_end
        );
    }

    #[test]
    fn standee_world_track_interpolates() {
        let samples = vec![
            ReplayStandeePosition {
                time_ms: 0,
                user_id: 42,
                position: [0.0, 0.0, 0.0],
            },
            ReplayStandeePosition {
                time_ms: 1_000,
                user_id: 42,
                position: [10.0, 0.0, 0.0],
            },
            ReplayStandeePosition {
                time_ms: 2_000,
                user_id: 42,
                position: [10.0, 0.0, 5.0],
            },
        ];
        assert_eq!(
            interpolated_standee_position(&samples, 0),
            Some(Vec3::ZERO)
        );
        let mid = interpolated_standee_position(&samples, 500).unwrap();
        assert!((mid - Vec3::new(5.0, 0.0, 0.0)).length() < 0.001);
        assert_eq!(
            interpolated_standee_position(&samples, 3_000),
            Some(Vec3::new(10.0, 0.0, 5.0))
        );
    }

    #[test]
    fn short_lines_keep_their_reading_duration() {
        let mut replay = test_replay(Vec::new());
        let mut line = test_dialogue(350, 1_500, DialogueSide::Left);
        line.text = "你是狂妄号的船长。".to_owned();
        line.snapshot_recorded = true;
        line.duration_ms = dialogue_duration_ms(&line.text);
        replay.dialogue.push(line.clone());
        let required = minimum_speech_window_ms(&line.text, 18, 1.10);
        assert!(
            required > line.duration_ms,
            "test requires speech longer than the reading duration"
        );

        extend_replay_for_speech(&mut replay);

        assert!(
            replay.dialogue[0].duration_ms <= SHORT_DIALOGUE_MAX_MS,
            "short line must not be stretched to fit speech, got {} ms",
            replay.dialogue[0].duration_ms
        );
    }

    #[test]
    fn ship_motion_speed_compresses_the_segment() {
        let history = ReplayShipTrajectoryHistory {
            sessions: vec![PersistedShipTrajectorySession {
                campaign_id: "default".to_owned(),
                ship_id: "ship-1".to_owned(),
                ship_name: "测试舰".to_owned(),
                turn_index: 1,
                start_after_source_time: None,
                start_delay_ms: 0,
                keyframes: (0..20)
                    .map(|index| PersistedShipKeyframe {
                        source_unix_ms: 100_000 + index as u64 * 100,
                        translation: [index as f32 * 2.0, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    })
                    .collect(),
            }],
        };
        let mut fast = test_replay(Vec::new());
        fast.campaign_id = "default".to_owned();
        fast.ship_motion_speed = 4.0;
        compile_scene_dynamics_timeline(&mut fast, &history, 0, &[], &[]);
        let mut slow = test_replay(Vec::new());
        slow.campaign_id = "default".to_owned();
        slow.ship_motion_speed = 0.5;
        compile_scene_dynamics_timeline(&mut slow, &history, 0, &[], &[]);
        let fast_span = fast
            .ship_trajectories
            .first()
            .and_then(|trajectory| trajectory.keyframes.last())
            .map(|frame| frame.time_ms)
            .unwrap_or_default();
        let slow_span = slow
            .ship_trajectories
            .first()
            .and_then(|trajectory| trajectory.keyframes.last())
            .map(|frame| frame.time_ms)
            .unwrap_or_default();
        assert!(
            fast_span < slow_span,
            "faster ship motion must compress the segment ({fast_span} vs {slow_span})"
        );
    }

    #[test]
    fn long_ship_flight_keeps_a_visible_motion_segment() {
        let history = ReplayShipTrajectoryHistory {
            sessions: vec![PersistedShipTrajectorySession {
                campaign_id: "default".to_owned(),
                ship_id: "ship-1".to_owned(),
                ship_name: "测试舰".to_owned(),
                turn_index: 1,
                start_after_source_time: None,
                start_delay_ms: 0,
                keyframes: (0..200)
                    .map(|index| PersistedShipKeyframe {
                        source_unix_ms: 100_000 + index as u64 * 100,
                        translation: [index as f32 * 0.5, 0.0, 0.0],
                        rotation: Quat::IDENTITY.to_array(),
                    })
                    .collect(),
            }],
        };
        let mut replay = test_replay(Vec::new());
        replay.campaign_id = "default".to_owned();
        compile_scene_dynamics_timeline(&mut replay, &history, 0, &[], &[]);
        let trajectory = replay.ship_trajectories.first().expect("ship trajectory");
        let first = trajectory.keyframes.first().expect("first frame").time_ms;
        let last = trajectory.keyframes.last().expect("last frame").time_ms;
        let span = last.saturating_sub(first);
        assert!(
            span >= 2_000,
            "a long flight must keep a visible playback span instead of a teleport, got {span} ms"
        );
        assert!(
            trajectory.keyframes.len() > 100,
            "the compressed segment should still carry the flight frames"
        );
    }

    #[test]
    fn authored_player_tracks_survive_history_rebuilds_until_explicitly_restored() {
        let lines = [7, 8]
            .into_iter()
            .map(|id| {
                let mut line = test_dialogue(0, 600, DialogueSide::Right);
                line.sender_id = id;
                line.source_time = 1;
                line
            })
            .collect();
        let mut replay = test_replay(lines);
        let mut history = ReplayPlayerMovementHistory {
            sessions: [7, 8]
                .into_iter()
                .map(
                    |user_id| PersistedPlayerMovementSession {
                        campaign_id: "campaign".into(),
                        user_id,
                        keyframes: vec![
                            PersistedPlayerMovementKeyframe {
                                source_unix_ms: 1_000,
                                position_cells: [0.0; 3],
                            },
                            PersistedPlayerMovementKeyframe {
                                source_unix_ms: 1_100,
                                position_cells: [4.0, 0.0, 0.0],
                            },
                        ],
                        ..default()
                    },
                )
                .collect(),
        };
        rebuild_player_movements_from_history(&mut replay, &history);
        stop_replay_movement_at(&mut replay, 7, 50, Vec3::Y);
        let authored = replay
            .player_movements
            .iter()
            .find(|movement| movement.user_id == 7)
            .unwrap()
            .clone();
        let saved = to_string(&replay).unwrap();
        let mut replay: ReplayFile = from_str(&saved).unwrap();
        for session in &mut history.sessions {
            session.keyframes[0].position_cells = [99.0, 0.0, 0.0];
        }
        rebuild_player_movements_from_history(&mut replay, &history);
        assert_eq!(
            to_string(&replay.player_movements[0]).unwrap(),
            to_string(&authored).unwrap()
        );
        assert_eq!(
            replay.player_movements[1].keyframes[0].position_cells[0],
            99.0
        );
        let protected_only = ReplayPlayerMovementHistory {
            sessions: vec![history.sessions[0].clone()],
        };
        replay.player_movement_history_cursor_unix_ms = 1;
        assert_eq!(
            append_new_player_movements_from_history(&mut replay, &protected_only),
            0
        );
        restore_player_movement_history(&mut replay, 7);
        rebuild_player_movements_from_history(&mut replay, &history);
        assert!(replay
            .standee_positions
            .iter()
            .all(|sample| sample.user_id != 7));
        assert!(!replay.authored_player_movements.contains(&7));
        assert_eq!(
            replay.player_movements[0].keyframes[0].position_cells[0],
            99.0
        );
        assert_eq!(
            append_new_player_movements_from_history(&mut replay, &history),
            0
        );
    }

    #[test]
    fn authored_ship_tracks_survive_imports_and_old_history_frames_are_replaced() {
        let mut replay = test_replay(Vec::new());
        stop_replay_ship_at(
            &mut replay,
            "authored",
            "船",
            150,
            Transform::from_translation(Vec3::Y),
        );
        let mut history = ReplayShipTrajectoryHistory {
            sessions: ["authored", "history"]
                .into_iter()
                .map(
                    |ship_id| PersistedShipTrajectorySession {
                        campaign_id: "campaign".into(),
                        ship_id: ship_id.into(),
                        ship_name: ship_id.into(),
                        keyframes: vec![
                            PersistedShipKeyframe {
                                source_unix_ms: 1_000,
                                translation: [0.0; 3],
                                rotation: Quat::IDENTITY.to_array(),
                            },
                            PersistedShipKeyframe {
                                source_unix_ms: 1_100,
                                translation: [4.0, 0.0, 0.0],
                                rotation: Quat::IDENTITY.to_array(),
                            },
                        ],
                        ..default()
                    },
                )
                .collect(),
        };
        let saved = to_string(&replay).unwrap();
        let mut replay: ReplayFile = from_str(&saved).unwrap();
        rebuild_ship_trajectories_from_history(&mut replay, &history);
        for session in &mut history.sessions {
            session.start_delay_ms = 1_000;
            session.keyframes[0].translation = [99.0, 0.0, 0.0];
        }
        rebuild_ship_trajectories_from_history(&mut replay, &history);
        assert_eq!(replay.ship_trajectories.len(), 2);
        let authored = &replay.ship_trajectories[0];
        assert_eq!(authored.ship_id, "authored");
        assert_eq!(authored.keyframes.len(), 1);
        assert_eq!(
            authored.keyframes[0].translation,
            Vec3::Y.to_array()
        );
        let source = &replay.ship_trajectories[1];
        assert_eq!(
            source.keyframes.len(),
            2,
            "retiming must not leave old source frames behind"
        );
        assert_eq!(source.keyframes[0].translation[0], 99.0);
        replay.ship_trajectory_history_cursor_unix_ms = 1;
        let protected_only = ReplayShipTrajectoryHistory {
            sessions: vec![history.sessions[0].clone()],
        };
        let duration = replay.duration_ms;
        assert_eq!(
            append_new_ship_trajectories_from_history(&mut replay, &protected_only),
            0
        );
        assert_eq!(
            replay.duration_ms, duration,
            "skipped imports must not append empty time"
        );
        restore_ship_trajectory_history(&mut replay, "authored");
        rebuild_ship_trajectories_from_history(&mut replay, &history);
        let restored = replay
            .ship_trajectories
            .iter()
            .find(|ship| ship.ship_id == "authored")
            .unwrap();
        assert_eq!(
            restored.keyframes[0].translation[0],
            99.0
        );
    }

    #[test]
    fn older_projects_default_to_history_driven_tracks() {
        let saved = to_string(&test_replay(Vec::new())).unwrap();
        assert!(!saved.contains("authored_player_movements"));
        assert!(!saved.contains("authored_ship_trajectories"));
        let replay: ReplayFile = from_str(&saved).unwrap();
        assert!(replay.authored_player_movements.is_empty());
        assert!(replay.authored_ship_trajectories.is_empty());
    }

    #[test]
    fn ship_position_frames_replace_one_time_and_extend_the_saved_timeline() {
        let mut replay = test_replay(Vec::new());
        let rotation = Quat::from_rotation_y(0.7).to_array();
        for (time_ms, position) in [
            (0, Vec3::ZERO),
            (200, Vec3::X),
            (100, Vec3::Y),
            (100, Vec3::Z),
        ] {
            replace_replay_ship_segment(
                &mut replay,
                "ship",
                "船",
                time_ms,
                time_ms,
                &[ReplayShipKeyframe {
                    time_ms,
                    translation: position.to_array(),
                    rotation,
                }],
            );
        }
        assert_eq!(replay.duration_ms, 200);
        let frames = &replay.ship_trajectories[0].keyframes;
        assert_eq!(frames.len(), 3);
        assert_eq!(
            frames.iter().map(|frame| frame.time_ms).collect::<Vec<_>>(),
            vec![0, 100, 200]
        );
        assert_eq!(
            frames[0].translation,
            Vec3::ZERO.to_array()
        );
        assert_eq!(
            frames[1].translation,
            Vec3::Z.to_array()
        );
        assert_eq!(frames[1].rotation, rotation);
        assert_eq!(
            frames[2].translation,
            Vec3::X.to_array()
        );
        let saved = serde_json::to_string(&replay).unwrap();
        let restored: ReplayFile = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.duration_ms, 200);
        assert_eq!(
            restored.ship_trajectories[0].keyframes[1].translation,
            Vec3::Z.to_array()
        );
    }

    #[test]
    fn timeline_editors_preserve_the_end_of_every_authored_track() {
        for track in 0..6 {
            let mut replay = test_replay(vec![test_dialogue(
                0,
                1_000,
                DialogueSide::Left,
            )]);
            assign_replay_line_ids(&mut replay.dialogue);
            rebuild_area_blocks(&mut replay);
            match track {
                0 => replay.camera.push(ReplayCameraKeyframe {
                    time_ms: 9_000,
                    translation: [0.0; 3],
                    rotation: Quat::IDENTITY.to_array(),
                }),
                1 => replay.player_movements.push(ReplayPlayerMovement {
                    user_id: 7,
                    keyframes: vec![ReplayPlayerMovementKeyframe {
                        time_ms: 9_000,
                        position_cells: [1.0, 0.0, 0.0],
                    }],
                }),
                2 => replay.ship_trajectories.push(ReplayShipTrajectory {
                    ship_id: "ship".into(),
                    ship_name: "船".into(),
                    keyframes: vec![ReplayShipKeyframe {
                        time_ms: 9_000,
                        translation: [0.0; 3],
                        rotation: Quat::IDENTITY.to_array(),
                    }],
                }),
                3 => replay.standee_positions.push(ReplayStandeePosition {
                    time_ms: 9_000,
                    user_id: 7,
                    position: [0.0; 3],
                }),
                4 => replay.terrain_changes.push(ReplayTerrainChange {
                    time_ms: 9_000,
                    position: [0; 3],
                    material: 0,
                    enabled: true,
                }),
                _ => replay.ship_hull_changes.push(ReplayShipHullChange {
                    time_ms: 9_000,
                    ship_id: "ship".into(),
                    position: [0; 3],
                    material: 0,
                    enabled: false,
                }),
            }
            refresh_replay_duration(&mut replay, 5_000);
            assert_eq!(
                replay.duration_ms, 9_000,
                "track {track}"
            );
            recompile_edited_replay_dialogue(&mut replay);
            assert_eq!(
                replay.duration_ms, 9_000,
                "recompiled track {track}"
            );
        }
    }

    #[test]
    fn shortening_a_cue_keeps_excluded_and_unassigned_lines_unscheduled() {
        let active = test_dialogue(0, 2_000, DialogueSide::Left);
        let mut excluded = test_dialogue(u64::MAX, 1_000, DialogueSide::Right);
        excluded.included = false;
        let unassigned = test_dialogue(u64::MAX, 1_000, DialogueSide::Right);
        let mut replay = test_replay(vec![active, excluded, unassigned]);
        replay.duration_ms = 3_000;

        set_exact_dialogue_duration(&mut replay, 0, 1_000, 500).unwrap();

        assert_eq!(replay.dialogue[1].time_ms, u64::MAX);
        assert_eq!(replay.dialogue[2].time_ms, u64::MAX);
        assert!(!replay_dialogue_is_playable(
            &replay.dialogue[2]
        ));
        assert_eq!(replay.duration_ms, 2_000);
    }

    #[test]
    fn live_replay_exact_dialogue_duration_ripples_every_downstream_track() {
        let mut first = test_dialogue(1_000, 2_000, DialogueSide::Left);
        first.line_id = 1;
        let mut second = test_dialogue(3_500, 1_000, DialogueSide::Right);
        second.line_id = 2;
        let mut replay = test_replay(vec![first, second]);
        replay.duration_ms = 6_000;
        replay.camera = vec![
            ReplayCameraKeyframe {
                time_ms: 2_500,
                translation: [0.0; 3],
                rotation: Quat::IDENTITY.to_array(),
            },
            ReplayCameraKeyframe {
                time_ms: 3_000,
                translation: [1.0, 0.0, 0.0],
                rotation: Quat::IDENTITY.to_array(),
            },
        ];
        replay.player_movements.push(ReplayPlayerMovement {
            user_id: 7,
            keyframes: vec![ReplayPlayerMovementKeyframe {
                time_ms: 4_000,
                position_cells: [1.0, 0.0, 0.0],
            }],
        });
        replay.ship_trajectories.push(ReplayShipTrajectory {
            ship_id: "ship".to_owned(),
            ship_name: "船".to_owned(),
            keyframes: vec![ReplayShipKeyframe {
                time_ms: 4_500,
                translation: [0.0; 3],
                rotation: Quat::IDENTITY.to_array(),
            }],
        });
        replay.standee_positions.push(ReplayStandeePosition {
            time_ms: 5_000,
            user_id: 7,
            position: [0.0; 3],
        });
        replay.terrain_changes.push(ReplayTerrainChange {
            time_ms: 5_200,
            position: [0; 3],
            material: 0,
            enabled: true,
        });
        replay.ship_hull_changes.push(ReplayShipHullChange {
            time_ms: 5_400,
            ship_id: "ship".to_owned(),
            position: [0; 3],
            material: 0,
            enabled: true,
        });

        let playhead = set_exact_dialogue_duration(&mut replay, 0, 1_000, 2_800).unwrap();

        assert_eq!(playhead, 1_999, "playhead stays in the edited cue");
        assert_eq!(replay.dialogue[0].duration_ms, 1_000);
        assert!(replay.dialogue[0].duration_locked);
        assert_eq!(replay.dialogue[1].time_ms, 2_500);
        assert_eq!(replay.camera[0].time_ms, 2_000);
        assert_eq!(replay.camera[1].time_ms, 2_500);
        assert_eq!(replay.player_movements[0].keyframes[0].time_ms, 3_000);
        assert_eq!(replay.ship_trajectories[0].keyframes[0].time_ms, 3_500);
        assert_eq!(replay.standee_positions[0].time_ms, 4_000);
        assert_eq!(replay.terrain_changes[0].time_ms, 4_200);
        assert_eq!(replay.ship_hull_changes[0].time_ms, 4_400);
        assert_eq!(replay.duration_ms, 5_000);
    }

    #[test]
    fn live_replay_locked_or_muted_dialogue_is_not_stretched_for_speech() {
        let mut locked = test_dialogue(350, 3_000, DialogueSide::Left);
        locked.text = "这是一句会需要很长时间才能读完的测试台词。".repeat(8);
        locked.duration_locked = true;
        let mut muted = test_dialogue(3_620, 3_000, DialogueSide::Right);
        muted.text = locked.text.clone();
        muted.speech_enabled = false;
        let mut replay = test_replay(vec![locked, muted]);
        replay.duration_ms = 7_000;

        assert!(!extend_replay_for_speech(&mut replay));
        assert_eq!(replay.dialogue[0].duration_ms, 3_000);
        assert_eq!(replay.dialogue[1].duration_ms, 3_000);
        assert_eq!(replay.duration_ms, 7_000);
    }

    #[test]
    fn live_replay_movement_punch_in_replaces_only_the_recorded_window() {
        let mut replay = test_replay(Vec::new());
        replay.standee_positions = [0, 1_000, 2_000, 3_000]
            .into_iter()
            .map(|time_ms| ReplayStandeePosition {
                time_ms,
                user_id: 7,
                position: [time_ms as f32 / 1_000.0, 0.0, 0.0],
            })
            .collect();
        replay.player_movements.push(ReplayPlayerMovement {
            user_id: 7,
            keyframes: [0, 1_000, 2_000, 3_000]
                .into_iter()
                .map(|time_ms| ReplayPlayerMovementKeyframe {
                    time_ms,
                    position_cells: [time_ms as f32 / 1_000.0, 0.0, 0.0],
                })
                .collect(),
        });
        let samples = vec![
            ReplayStandeePosition {
                time_ms: 1_000,
                user_id: 7,
                position: [10.0, 0.0, 0.0],
            },
            ReplayStandeePosition {
                time_ms: 2_000,
                user_id: 7,
                position: [20.0, 0.0, 0.0],
            },
        ];

        assert_eq!(
            replace_replay_movement_segment(&mut replay, 7, 1_000, 2_000, &samples),
            2
        );
        let times = replay
            .standee_positions
            .iter()
            .map(|sample| sample.time_ms)
            .collect::<Vec<_>>();
        assert_eq!(times, vec![0, 1_000, 2_000, 3_000]);
        assert_eq!(replay.standee_positions[1].position, [10.0, 0.0, 0.0]);
        assert_eq!(replay.standee_positions[2].position, [20.0, 0.0, 0.0]);
        assert_eq!(replay.player_movements.len(), 1);
        assert_eq!(replay.player_movements[0].keyframes.len(), 4);
    }

    #[test]
    fn live_replay_disabled_scene_events_are_skipped_and_remain_reversible() {
        let mut world = World::new();
        let entity = world.spawn(Grid::<u8>::new()).id();
        let mut dirty = VoxelGeometryDirtyChunks::default();
        let mut state = ReplayTerrainPlaybackState {
            replay_key: Some(1),
            scene_revision: 0,
            baseline: HashMap::new(),
            applied: HashMap::new(),
            next_change: 0,
            covered_time_ms: 0,
        };
        let mut changes = vec![ReplayTerrainChange {
            time_ms: 100,
            position: [1, 0, 0],
            material: 2,
            enabled: false,
        }];
        let mut entity_mut = world.entity_mut(entity);
        let mut grid = entity_mut.get_mut::<Grid<u8>>().unwrap();

        // The viewport tool has already changed the real grid, while the
        // replay playback cache still represents the pre-explosion scene.
        grid.set(IVec3::X, 2);
        state.covered_time_ms = u64::MAX;
        apply_terrain_changes_at(&mut grid, &mut dirty, &mut state, &changes, 200);
        assert_eq!(grid.get(IVec3::X).copied().unwrap_or_default(), 0);
        changes[0].enabled = true;
        state.covered_time_ms = u64::MAX;
        apply_terrain_changes_at(&mut grid, &mut dirty, &mut state, &changes, 200);
        assert_eq!(grid.get(IVec3::X), Some(&2));
        changes[0].enabled = false;
        state.covered_time_ms = u64::MAX;
        apply_terrain_changes_at(&mut grid, &mut dirty, &mut state, &changes, 200);
        assert_eq!(grid.get(IVec3::X), Some(&0));
    }

    #[test]
    fn real_gm_player_exchange_camera_focuses_the_standee() {
        // Reproduce the on-disk session: one GM line addressing player
        // 1670426821 and one line from that player, with the snapshot
        // positions persisted in messages.toml and the standee at the
        // position persisted in voxel_player_cameras.toml.
        let mut gm = test_dialogue(350, 2_400, DialogueSide::Left);
        gm.sender_id = 0;
        gm.camera_focus_id = Some(1_670_426_821);
        gm.snapshot_recorded = true;
        gm.metadata_estimated = false;
        gm.position_cells = [-366, 60, 1_375];
        let mut player = test_dialogue(3_020, 2_400, DialogueSide::Right);
        player.sender_id = 1_670_426_821;
        player.snapshot_recorded = true;
        player.metadata_estimated = false;
        player.position_cells = [-415, 43, -722];
        let dialogue = [gm, player];
        let standee_position = Vec3::new(-122.419_174, -21.016_474, 41.071_274);
        let speaker_positions = HashMap::from([(1_670_426_821, standee_position)]);
        let base = Transform::from_xyz(29.004_196, 24.248_276, 221.935);

        let camera = turn_based_camera_track(
            &base,
            &dialogue,
            8_000,
            &speaker_positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
            &ReplayCameraObstacles::default(),
        );

        assert!(!camera.is_empty());
        let mut anchor = IVec3::from_array(dialogue[1].position_cells).as_vec3() * VOXEL_SIZE;
        for frame in &camera {
            if let Some(line) = dialogue
                .iter()
                .filter(|line| {
                    line.included
                        && line.snapshot_recorded
                        && line.time_ms <= frame.time_ms
                        && line.sender_id == 1_670_426_821
                        && line.position_cells != [0; 3]
                })
                .last()
            {
                anchor = IVec3::from_array(line.position_cells).as_vec3() * VOXEL_SIZE;
            }
            let distance = horizontal(Vec3::from_array(frame.translation) - anchor).length();
            assert!(
                distance < 20.0,
                "camera frame at {:?} is {distance:.1} units from the standee anchor",
                frame.translation
            );
        }
    }

    #[derive(serde::Deserialize)]
    struct PlayerCameraStore {
        cameras: Vec<PlayerCamera>,
    }

    #[derive(serde::Deserialize)]
    struct PlayerCamera {
        user_id: u64,
        translation: [f32; 3],
        #[allow(dead_code)]
        rotation: [f32; 4],
    }

    /// Loads the on-disk session (messages + replay snapshots + standee camera
    /// store) and generates the camera track exactly like "从现有聊天生成",
    /// then verifies the camera never drifts hundreds of units from the
    /// standees. Skipped when the local session data is absent.
    #[test]
    fn real_chat_generation_camera_stays_near_the_standees() {
        let manager_path = std::path::Path::new(".data/willowblossom/messages.toml");
        let camera_path = std::path::Path::new(".data/willowblossom/voxel_player_cameras.toml");
        if !manager_path.is_file() || !camera_path.is_file() {
            return;
        }
        let manager_text = std::fs::read_to_string(manager_path).expect("read messages.toml");
        let manager: NapcatMessageManager =
            toml::from_str(&manager_text).expect("parse messages.toml");
        let camera_text = std::fs::read_to_string(camera_path).expect("read player cameras");
        let cameras: PlayerCameraStore =
            toml::from_str(&camera_text).expect("parse player cameras");
        let speaker_positions = cameras
            .cameras
            .iter()
            .map(|camera| {
                (
                    camera.user_id,
                    Vec3::from_array(camera.translation),
                )
            })
            .collect::<HashMap<_, _>>();

        let campaign_id = manager
            .active_campaign_id()
            .unwrap_or_else(|| "default".to_owned());
        let mut visible = manager
            .messages
            .iter()
            .flat_map(|(target, messages)| {
                messages.iter().enumerate().map(|(index, message)| {
                    (
                        manager.campaign_message_for_target(target, message),
                        manager
                            .replay_snapshots
                            .get(target)
                            .and_then(|snapshots| snapshots.get(index))
                            .and_then(Option::as_ref)
                            .copied(),
                    )
                })
            })
            .filter(|(message, _)| message.campaign_id == campaign_id)
            .filter(|(message, _)| !message.text.trim().is_empty())
            .collect::<Vec<_>>();
        visible.sort_by_key(|(message, _)| message.time);
        if visible.is_empty() {
            // The on-disk session changed (e.g. test progress was cleared);
            // the deterministic standee-focus test covers this behavior.
            return;
        }

        let mut replay = test_replay(Vec::new());
        let mut timeline_ms = 350_u64;
        for (message, snapshot) in &visible {
            let estimated;
            let (snapshot, metadata_estimated) = if let Some(snapshot) = snapshot.as_ref() {
                (snapshot, false)
            } else {
                estimated = estimated_replay_snapshot(message, &manager, &speaker_positions);
                (&estimated, true)
            };
            if let Some(line) = dialogue_from_message(
                message,
                &manager,
                timeline_ms,
                snapshot,
                metadata_estimated,
            ) {
                timeline_ms = line
                    .time_ms
                    .saturating_add(line.duration_ms)
                    .saturating_add(HISTORY_DIALOGUE_GAP_MS);
                replay.dialogue.push(line);
            }
        }
        deduplicate_broadcast_dialogue(&mut replay.dialogue, &manager);
        assign_replay_line_ids(&mut replay.dialogue);
        auto_group_replay_areas(&mut replay);
        rebuild_area_blocks(&mut replay);
        compile_scene_dynamics_timeline(
            &mut replay,
            &ReplayShipTrajectoryHistory::default(),
            0,
            &[],
            &[],
        );
        let base = Transform::from_xyz(29.004_196, 24.248_276, 221.935);
        let scene_replay_path =
            std::path::Path::new(".data/willowblossom/replays/latest.willow-replay.json");
        let scene = if scene_replay_path.is_file() {
            let legacy: ReplayFile = serde_json::from_str(
                &std::fs::read_to_string(scene_replay_path).expect("read legacy replay"),
            )
            .expect("parse legacy replay");
            legacy.scene
        } else {
            ReplayScene::default()
        };
        replay.scene = scene;
        let obstacles = ReplayCameraObstacles::from_scene(&replay.scene);
        let camera = turn_based_camera_track(
            &base,
            &replay.dialogue,
            replay.duration_ms,
            &speaker_positions,
            default_directed_camera_distance_scale(),
            default_directed_camera_yaw_degrees(),
            &obstacles,
        );

        assert!(
            !camera.is_empty(),
            "camera track is empty for {} dialogue lines",
            replay.dialogue.len()
        );
        // Standees start at their earliest recorded position, matching the
        // opening camera shots.
        let mut standee_anchor = HashMap::<u64, Vec3>::new();
        for line in replay
            .dialogue
            .iter()
            .filter(|line| line.included && line.snapshot_recorded && line.position_cells != [0; 3])
        {
            standee_anchor
                .entry(line.sender_id)
                .or_insert_with(|| IVec3::from_array(line.position_cells).as_vec3() * VOXEL_SIZE);
        }
        for frame in &camera {
            for line in replay.dialogue.iter().filter(|line| {
                line.included
                    && line.snapshot_recorded
                    && line.time_ms <= frame.time_ms
                    && line.position_cells != [0; 3]
            }) {
                standee_anchor.insert(
                    line.sender_id,
                    IVec3::from_array(line.position_cells).as_vec3() * VOXEL_SIZE,
                );
            }
            let position = Vec3::from_array(frame.translation);
            let nearest = speaker_positions
                .iter()
                .map(|(user_id, standee)| {
                    let anchor = standee_anchor.get(user_id).copied().unwrap_or(*standee);
                    horizontal(position - anchor).length()
                })
                .fold(f32::INFINITY, f32::min);
            assert!(
                nearest < 25.0,
                "camera frame at {:?} is {nearest:.1} units from every standee",
                frame.translation
            );
        }
    }
}
