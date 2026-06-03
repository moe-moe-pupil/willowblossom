use std::{
    collections::{
        hash_map::DefaultHasher,
        HashMap,
        HashSet,
    },
    fs,
    hash::{
        Hash,
        Hasher,
    },
    path::{
        Path,
        PathBuf,
    },
    thread,
    time::{
        Duration,
        SystemTime,
        UNIX_EPOCH,
    },
};

use bevy_persistent::prelude::*;
extern crate dirs;

use bevy::prelude::*;
use crossbeam_channel::{
    unbounded,
    Receiver as CBReceiver,
    Sender as CBSender,
};
use futures_util::{
    SinkExt,
    StreamExt,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    json,
    Value,
};
use tokio::{
    runtime::Builder,
    sync::mpsc::{
        Receiver,
        Sender,
    },
    time::sleep,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
};

use crate::{
    rule_engine::BuffSpec,
    scene::{
        SceneCaptureRequest,
        SceneCaptureRequests,
    },
};

#[derive(States, Debug, Default, Clone, Eq, PartialEq, Hash)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connected,
}

#[derive(Resource)]
struct NapcatIOReceiver(CBReceiver<Message>);

#[derive(Resource)]
struct NapcatSendResultReceiver(CBReceiver<NapcatSendResult>);

#[derive(Resource)]
pub struct NapcatIOSender(pub Sender<NapcatOutboundMessage>);

#[derive(Debug)]
pub struct NapcatOutboundMessage {
    pub request_id: u64,
    pub target_id: String,
    pub message: Message,
}

#[derive(Debug, Clone)]
pub struct NapcatSendResult {
    pub request_id: u64,
    pub target_id: String,
    pub error: Option<String>,
}

#[derive(Resource, Default)]
pub struct NapcatSendManager {
    pub results: Vec<NapcatSendResult>,
}

#[derive(Resource)]
struct NapcatAutoForwardRequestIds {
    next_request_id: u64,
}

#[derive(Resource)]
struct NapcatGroupInfoRequests {
    next_request_id: u64,
    pending_group_ids: HashSet<String>,
}

pub struct NapcatPlugin;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NapcatMessage {
    #[serde(flatten)]
    pub data: NapcatMessageData,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TextData {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImageData {
    #[serde(default)]
    #[serde(rename = "subType")]
    pub sub_type: usize,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub file_size: String,
    #[serde(default)]
    pub local_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Source {
    id: u64,
    time: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum NapcatMessageChainType {
    Source(Source),
    Text {
        data: TextData,
    },
    Image {
        data: ImageData,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum NapcatMessageType {
    Private,
    Group,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NapcatMessageChain {
    #[serde(flatten)]
    pub variant: NapcatMessageChainType,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NapcatSender {
    pub user_id: u64,
    pub nickname: String,
}

fn deserialize_message_chains<'de, D>(deserializer: D) -> Result<Vec<NapcatMessageChain>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MessageChains {
        Segments(Vec<NapcatMessageChain>),
        Text(String),
    }

    match MessageChains::deserialize(deserializer)? {
        MessageChains::Segments(segments) => Ok(segments),
        MessageChains::Text(text) => Ok(vec![NapcatMessageChain {
            variant: NapcatMessageChainType::Text {
                data: TextData { text },
            },
        }]),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NapcatMessageData {
    pub time: u64,
    pub message_type: NapcatMessageType,
    #[serde(deserialize_with = "deserialize_message_chains")]
    pub message: Vec<NapcatMessageChain>,
    pub self_id: u64,
    pub user_id: u64,
    pub group_id: Option<u64>,
    #[serde(default)]
    pub group_name: Option<String>,
    pub target_id: Option<u64>,
    pub sender: NapcatSender,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatGroup {
    pub members: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ChatTargetMetadata {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub automatic_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CharacterStatus {
    #[serde(default, rename = "str")]
    pub str_: i32,
    #[serde(default)]
    pub agi: i32,
    #[serde(default)]
    pub dex: i32,
    #[serde(default)]
    pub vit: i32,
    #[serde(default, rename = "int")]
    pub int_: i32,
    #[serde(default)]
    pub wis: i32,
    #[serde(default)]
    pub k: i32,
    #[serde(default)]
    pub cha: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerCharacter {
    #[serde(default)]
    pub inited: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub creation_step: CharacterCreationStep,
    #[serde(default = "default_status_points")]
    pub status_points: i32,
    #[serde(default = "default_exchange_points")]
    pub exchange_points: i32,
    #[serde(default = "default_character_hp")]
    pub hp: f32,
    #[serde(default = "default_character_hp")]
    pub max_hp: f32,
    #[serde(default)]
    pub hp_regen: f32,
    #[serde(default)]
    pub mp: f32,
    #[serde(default)]
    pub max_mp: f32,
    #[serde(default)]
    pub mp_regen: f32,
    #[serde(default = "default_character_level")]
    pub level: i32,
    #[serde(default)]
    pub exp: i32,
    #[serde(default = "default_character_speed")]
    pub speed: f32,
    #[serde(default = "default_modifier")]
    pub damage_dealt_modifier: f32,
    #[serde(default = "default_modifier")]
    pub healing_dealt_modifier: f32,
    #[serde(default = "default_modifier")]
    pub damage_taken_modifier: f32,
    #[serde(default = "default_modifier")]
    pub healing_taken_modifier: f32,
    #[serde(default)]
    pub status: CharacterStatus,
    #[serde(default)]
    pub extra_status: CharacterStatus,
    #[serde(default)]
    pub skill_names: Vec<String>,
    #[serde(default)]
    pub skill_notes: Vec<String>,
    #[serde(default)]
    pub skill_mp_costs: Vec<f32>,
    #[serde(default)]
    pub skill_cooldown_turns: Vec<u32>,
    #[serde(default)]
    pub skill_last_cast_turns: HashMap<String, u32>,
    #[serde(default)]
    pub active_buffs: Vec<BuffSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buff_base_stats: Option<CharacterBuffBaseStats>,
}

impl Default for PlayerCharacter {
    fn default() -> Self {
        Self {
            inited: false,
            name: String::new(),
            nickname: String::new(),
            image: String::new(),
            creation_step: CharacterCreationStep::Normal,
            status_points: default_status_points(),
            exchange_points: default_exchange_points(),
            hp: default_character_hp(),
            max_hp: default_character_hp(),
            hp_regen: 0.0,
            mp: 0.0,
            max_mp: 0.0,
            mp_regen: 0.0,
            level: default_character_level(),
            exp: 0,
            speed: default_character_speed(),
            damage_dealt_modifier: default_modifier(),
            healing_dealt_modifier: default_modifier(),
            damage_taken_modifier: default_modifier(),
            healing_taken_modifier: default_modifier(),
            status: CharacterStatus::default(),
            extra_status: CharacterStatus::default(),
            skill_names: Vec::new(),
            skill_notes: Vec::new(),
            skill_mp_costs: Vec::new(),
            skill_cooldown_turns: Vec::new(),
            skill_last_cast_turns: HashMap::new(),
            active_buffs: Vec::new(),
            buff_base_stats: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct CharacterBuffBaseStats {
    #[serde(default = "default_character_hp")]
    pub hp: f32,
    #[serde(default = "default_character_hp")]
    pub max_hp: f32,
    #[serde(default = "default_modifier")]
    pub damage_dealt_modifier: f32,
    #[serde(default = "default_modifier")]
    pub damage_taken_modifier: f32,
    #[serde(default = "default_modifier")]
    pub healing_dealt_modifier: f32,
    #[serde(default = "default_modifier")]
    pub healing_taken_modifier: f32,
}

impl CharacterBuffBaseStats {
    pub fn from_character(character: &PlayerCharacter) -> Self {
        Self {
            hp: character.hp,
            max_hp: character.max_hp,
            damage_dealt_modifier: character.damage_dealt_modifier,
            damage_taken_modifier: character.damage_taken_modifier,
            healing_dealt_modifier: character.healing_dealt_modifier,
            healing_taken_modifier: character.healing_taken_modifier,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CharacterCreationStep {
    #[default]
    Normal,
    Str,
    Agi,
    Dex,
    Vit,
    Int,
    Wis,
    K,
    Cha,
    ConfirmStatus,
    Skill,
    ConfirmSkill,
    Image,
    Nickname,
}

impl CharacterCreationStep {
    fn status_key(self) -> Option<StatusKey> {
        match self {
            CharacterCreationStep::Str => Some(StatusKey::Str),
            CharacterCreationStep::Agi => Some(StatusKey::Agi),
            CharacterCreationStep::Dex => Some(StatusKey::Dex),
            CharacterCreationStep::Vit => Some(StatusKey::Vit),
            CharacterCreationStep::Int => Some(StatusKey::Int),
            CharacterCreationStep::Wis => Some(StatusKey::Wis),
            CharacterCreationStep::K => Some(StatusKey::K),
            CharacterCreationStep::Cha => Some(StatusKey::Cha),
            _ => None,
        }
    }

    fn next_status_step(self) -> Self {
        match self {
            CharacterCreationStep::Str => CharacterCreationStep::Agi,
            CharacterCreationStep::Agi => CharacterCreationStep::Dex,
            CharacterCreationStep::Dex => CharacterCreationStep::Vit,
            CharacterCreationStep::Vit => CharacterCreationStep::Int,
            CharacterCreationStep::Int => CharacterCreationStep::Wis,
            CharacterCreationStep::Wis => CharacterCreationStep::K,
            CharacterCreationStep::K => CharacterCreationStep::Cha,
            CharacterCreationStep::Cha => CharacterCreationStep::ConfirmStatus,
            _ => self,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum StatusKey {
    Str,
    Agi,
    Dex,
    Vit,
    Int,
    Wis,
    K,
    Cha,
}

impl StatusKey {
    fn zh(self) -> &'static str {
        match self {
            StatusKey::Str => "力量",
            StatusKey::Agi => "敏捷",
            StatusKey::Dex => "灵巧",
            StatusKey::Vit => "体质",
            StatusKey::Int => "智力",
            StatusKey::Wis => "智慧",
            StatusKey::K => "知识",
            StatusKey::Cha => "魅力",
        }
    }
}

fn default_character_hp() -> f32 { 5.0 }

fn default_character_level() -> i32 { 1 }

fn default_character_speed() -> f32 { 3.0 }

fn default_modifier() -> f32 { 1.0 }

fn default_status_points() -> i32 { 5 }

fn default_exchange_points() -> i32 { 6 }

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TrpgGroup {
    #[serde(default)]
    pub players: Vec<String>,
    #[serde(default)]
    pub group_chats: Vec<String>,
    #[serde(default)]
    pub world_turn: u32,
    #[serde(default)]
    pub player_turns: HashMap<String, TrpgPlayerTurnState>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TrpgPlayerTurnState {
    #[serde(default)]
    pub turns_passed: u32,
    #[serde(default)]
    pub acted: bool,
    #[serde(default)]
    pub skipped: bool,
}

impl TrpgGroup {
    pub fn sync_turn_players(&mut self) -> bool {
        let before_len = self.player_turns.len();
        self.player_turns
            .retain(|target_id, _| self.players.contains(target_id));
        let mut changed = before_len != self.player_turns.len();

        for target_id in &self.players {
            if !self.player_turns.contains_key(target_id) {
                self.player_turns.insert(
                    target_id.clone(),
                    TrpgPlayerTurnState::default(),
                );
                changed = true;
            }
        }
        changed
    }

    pub fn mark_player_acted(&mut self, target_id: &str) -> bool {
        self.mark_player_turn(target_id, true)
    }

    pub fn mark_player_skipped(&mut self, target_id: &str) -> bool {
        self.mark_player_turn(target_id, false)
    }

    pub fn reset_current_turn(&mut self) -> bool {
        let mut changed = false;
        for turn in self.player_turns.values_mut() {
            if turn.acted || turn.skipped {
                turn.acted = false;
                turn.skipped = false;
                changed = true;
            }
        }
        changed
    }

    pub fn set_player_turns_passed(&mut self, target_id: &str, turns_passed: u32) -> bool {
        if !self.players.iter().any(|player_id| player_id == target_id) {
            return false;
        }

        self.sync_turn_players();
        let Some(turn) = self.player_turns.get_mut(target_id) else {
            return false;
        };
        if turn.turns_passed == turns_passed {
            return false;
        }

        turn.turns_passed = turns_passed;
        true
    }

    pub fn advance_world_turn(&mut self) -> bool {
        self.sync_turn_players();
        self.world_turn += 1;
        for turn in self.player_turns.values_mut() {
            turn.turns_passed += 1;
            turn.acted = false;
            turn.skipped = false;
        }
        true
    }

    fn mark_player_turn(&mut self, target_id: &str, acted: bool) -> bool {
        if !self.players.iter().any(|player_id| player_id == target_id) {
            return false;
        }

        let sync_changed = self.sync_turn_players();
        let Some(turn) = self.player_turns.get_mut(target_id) else {
            return sync_changed;
        };
        let already_set =
            if acted { turn.acted && !turn.skipped } else { !turn.acted && turn.skipped };
        if already_set {
            return sync_changed;
        }

        if acted {
            turn.acted = true;
            turn.skipped = false;
        } else {
            turn.acted = false;
            turn.skipped = true;
        }

        if self.all_players_finished_turn() {
            self.advance_world_turn();
        }
        true
    }

    fn all_players_finished_turn(&self) -> bool {
        !self.players.is_empty()
            && self.players.iter().all(|target_id| {
                self.player_turns
                    .get(target_id)
                    .is_some_and(|turn| turn.acted || turn.skipped)
            })
    }
}

#[derive(Resource, Serialize, Deserialize)]
pub struct NapcatMessageManager {
    pub messages: HashMap<String, Vec<NapcatMessage>>,
    #[serde(default)]
    pub chat_targets: HashMap<String, ChatTargetMetadata>,
    #[serde(default)]
    pub player_characters: HashMap<String, PlayerCharacter>,
    #[serde(default)]
    pub trpg_groups: HashMap<String, TrpgGroup>,
    #[serde(default)]
    pub current_trpg_group: Option<String>,
    #[serde(default)]
    pub groups: HashMap<String, ChatGroup>,
    #[serde(default)]
    pub read_message_counts: HashMap<String, usize>,
    #[serde(default)]
    pub summarized_message_counts: HashMap<String, usize>,
    #[serde(default)]
    pub open_chat_targets: HashSet<String>,
    #[serde(default)]
    pub pending_chat_targets: HashSet<String>,
}

impl NapcatMessageManager {
    pub fn migrate_chat_window_state(&mut self) -> bool {
        if !self.open_chat_targets.is_empty() || !self.pending_chat_targets.is_empty() {
            return false;
        }

        if !self.chat_targets.is_empty() {
            return false;
        }

        if self.messages.is_empty() {
            return false;
        }

        self.open_chat_targets.extend(self.messages.keys().cloned());
        true
    }

    pub fn sync_chat_targets(&mut self) -> bool {
        let mut changed = false;
        for target_id in self.messages.keys() {
            if !self.chat_targets.contains_key(target_id) {
                self.chat_targets.insert(
                    target_id.clone(),
                    ChatTargetMetadata::default(),
                );
                changed = true;
            }
            if is_private_message_target(self.messages.get(target_id))
                && !self.player_characters.contains_key(target_id)
            {
                self.player_characters.insert(
                    target_id.clone(),
                    PlayerCharacter::default(),
                );
                changed = true;
            }
        }
        let character_len = self.player_characters.len();
        self.player_characters
            .retain(|target_id, _| is_private_message_target(self.messages.get(target_id)));
        changed |= character_len != self.player_characters.len();

        for group in self.trpg_groups.values_mut() {
            let player_len = group.players.len();
            group
                .players
                .retain(|target_id| self.messages.contains_key(target_id));
            changed |= player_len != group.players.len();

            let group_chat_len = group.group_chats.len();
            group
                .group_chats
                .retain(|target_id| self.messages.contains_key(target_id));
            changed |= group_chat_len != group.group_chats.len();

            changed |= group.sync_turn_players();
        }
        if self
            .current_trpg_group
            .as_ref()
            .is_some_and(|group_name| !self.trpg_groups.contains_key(group_name))
        {
            self.current_trpg_group = None;
            changed = true;
        }
        if self.current_trpg_group.is_none() && self.trpg_groups.len() == 1 {
            if let Some(group_name) = self.trpg_groups.keys().next().cloned() {
                self.current_trpg_group = Some(group_name);
                changed = true;
            }
        }
        changed
    }

    pub fn register_incoming_target(&mut self, target_id: &str, is_new_target: bool) {
        self.chat_targets.entry(target_id.to_owned()).or_default();

        if !is_new_target || self.open_chat_targets.contains(target_id) {
            return;
        }

        self.pending_chat_targets.insert(target_id.to_owned());
    }

    pub fn set_automatic_target_name(&mut self, target_id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || name == target_id {
            return false;
        }

        let metadata = self.chat_targets.entry(target_id.to_owned()).or_default();
        if metadata.automatic_name.trim() == name {
            return false;
        }

        metadata.automatic_name = name.to_owned();
        true
    }
}

fn is_private_message_target(messages: Option<&Vec<NapcatMessage>>) -> bool {
    matches!(
        messages
            .and_then(|messages| messages.first())
            .map(|message| &message.data.message_type),
        Some(NapcatMessageType::Private)
    )
}

impl Plugin for NapcatPlugin {
    fn build(&self, app: &mut App) {
        app.insert_state(ConnectionState::Disconnected)
            // .insert_resource(NapcatSocket { ..default() })
            .add_systems(Startup, setup)
            .add_systems(Update, message_system)
            .add_systems(Update, request_missing_group_info_system)
            .add_systems(Update, send_result_system);
    }
}

fn setup(mut commands: Commands) {
    let (client_to_game_sender, client_to_game_receiver) = unbounded::<Message>();
    let (game_to_client_sender, game_to_client_receiver) = tokio::sync::mpsc::channel(100);
    let (send_result_sender, send_result_receiver) = unbounded::<NapcatSendResult>();
    let napcat_io = NapcatIOReceiver(client_to_game_receiver.clone());
    let napcat_send_results = NapcatSendResultReceiver(send_result_receiver);
    spawn_napcat_connection(
        client_to_game_sender.clone(),
        game_to_client_receiver,
        send_result_sender,
    );
    commands.insert_resource(napcat_io);
    commands.insert_resource(napcat_send_results);
    commands.insert_resource(NapcatIOSender(game_to_client_sender));
    commands.insert_resource(NapcatSendManager::default());
    commands.insert_resource(NapcatAutoForwardRequestIds {
        next_request_id: 1_000_000,
    });
    commands.insert_resource(NapcatGroupInfoRequests {
        next_request_id: 2_000_000,
        pending_group_ids: HashSet::default(),
    });

    let message_manager = NapcatMessageManager {
        messages: HashMap::default(),
        chat_targets: HashMap::default(),
        player_characters: HashMap::default(),
        trpg_groups: HashMap::default(),
        current_trpg_group: None,
        groups: HashMap::default(),
        read_message_counts: HashMap::default(),
        summarized_message_counts: HashMap::default(),
        open_chat_targets: HashSet::default(),
        pending_chat_targets: HashSet::default(),
    };
    let config_dir = Path::new(".data").join("willowblossom");
    commands.insert_resource(
        Persistent::<NapcatMessageManager>::builder()
            .name("messages")
            .format(StorageFormat::Toml)
            .path(config_dir.join("messages.toml"))
            .default(message_manager)
            .build()
            .expect("failed to init messages"),
    );
}

fn spawn_napcat_connection(
    client_to_game_sender: CBSender<Message>,
    game_to_client_receiver: Receiver<NapcatOutboundMessage>,
    send_result_sender: CBSender<NapcatSendResult>,
) {
    thread::Builder::new()
        .name("napcat-websocket".to_owned())
        .spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create NapCat Tokio runtime");
            runtime.block_on(run_napcat_connection(
                client_to_game_sender,
                game_to_client_receiver,
                send_result_sender,
            ));
        })
        .expect("failed to spawn NapCat websocket thread");
}

async fn run_napcat_connection(
    client_to_game_sender: CBSender<Message>,
    mut game_to_client_receiver: Receiver<NapcatOutboundMessage>,
    send_result_sender: CBSender<NapcatSendResult>,
) {
    const NAPCAT_WS_URL: &str = "ws://localhost:3001";

    loop {
        let (ws_stream, _) = match connect_async(NAPCAT_WS_URL).await {
            Ok(connection) => connection,
            Err(err) => {
                eprintln!("failed to connect NapCat websocket: {err}");
                sleep(Duration::from_secs(2)).await;
                continue;
            },
        };

        eprintln!("connected to NapCat websocket at {NAPCAT_WS_URL}");
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        loop {
            tokio::select! {
                //Receive messages from the websocket
                msg = ws_receiver.next() => {
                    match msg {
                        Some(Ok(msg)) => {
                            if msg.is_text() || msg.is_binary() {
                                if client_to_game_sender.send(msg).is_err() {
                                    return;
                                }
                            } else if msg.is_close() {
                                break;
                            }
                        }
                        Some(Err(err)) => {
                            eprintln!("NapCat websocket receive error: {err}");
                            break;
                        },
                        None => break,
                    }
                }
                //Receive messages from the game
                outbound = game_to_client_receiver.recv() => {
                    let Some(outbound) = outbound else {
                        return;
                    };
                    if let Err(err) = ws_sender.send(outbound.message.clone()).await {
                        let error = format!("NapCat websocket send error: {err}");
                        eprintln!("{error}");
                        let _ = send_result_sender.send(NapcatSendResult {
                            request_id: outbound.request_id,
                            target_id: outbound.target_id,
                            error: Some(error),
                        });
                        break;
                    }
                    let _ = send_result_sender.send(NapcatSendResult {
                        request_id: outbound.request_id,
                        target_id: outbound.target_id,
                        error: None,
                    });
                    eprintln!("sent NapCat websocket message");
                }
            }
        }

        sleep(Duration::from_secs(2)).await;
    }
}

fn send_result_system(
    receiver: Res<NapcatSendResultReceiver>,
    mut send_manager: ResMut<NapcatSendManager>,
) {
    while let Ok(result) = receiver.0.try_recv() {
        send_manager.results.push(result);
    }
}

#[derive(Debug, Deserialize)]
struct NapcatActionResponse {
    #[serde(default)]
    data: Option<NapcatActionResponseData>,
    #[serde(default)]
    echo: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NapcatActionResponseData {
    #[serde(default)]
    group_id: Option<Value>,
    #[serde(default)]
    group_name: Option<String>,
}

fn request_missing_group_info_system(
    sender: Option<Res<NapcatIOSender>>,
    mut group_info_requests: ResMut<NapcatGroupInfoRequests>,
    manager: Res<Persistent<NapcatMessageManager>>,
) {
    let Some(sender) = sender.as_deref() else {
        return;
    };

    for (target_id, messages) in &manager.messages {
        if !matches!(
            messages.first().map(|message| &message.data.message_type),
            Some(NapcatMessageType::Group)
        ) {
            continue;
        }

        let has_name = manager
            .chat_targets
            .get(target_id)
            .map(|metadata| !metadata.automatic_name.trim().is_empty())
            .unwrap_or_default();
        if has_name || group_info_requests.pending_group_ids.contains(target_id) {
            continue;
        }

        let Ok(group_id) = target_id.parse::<u64>() else {
            continue;
        };
        queue_group_info_request(
            sender,
            &mut group_info_requests,
            group_id,
        );
    }
}

fn queue_group_info_request(
    sender: &NapcatIOSender,
    group_info_requests: &mut NapcatGroupInfoRequests,
    group_id: u64,
) {
    let target_id = group_id.to_string();
    let request_id = group_info_requests.next_request_id;
    group_info_requests.next_request_id += 1;
    let message = Message::Text(
        json!({
            "action": "get_group_info",
            "params": {
                "group_id": group_id,
                "no_cache": false
            },
            "echo": format!("group-info:{target_id}")
        })
        .to_string()
        .into(),
    );

    match sender.0.try_send(NapcatOutboundMessage {
        request_id,
        target_id: target_id.clone(),
        message,
    }) {
        Ok(()) => {
            group_info_requests.pending_group_ids.insert(target_id);
        },
        Err(err) => eprintln!("failed to queue NapCat group info request: {err}"),
    }
}

fn apply_group_info_response(
    response: &NapcatActionResponse,
    manager: &mut NapcatMessageManager,
    group_info_requests: &mut NapcatGroupInfoRequests,
) -> bool {
    let Some(target_id) = response_group_target_id(response) else {
        return false;
    };
    group_info_requests.pending_group_ids.remove(&target_id);

    let Some(group_name) = response
        .data
        .as_ref()
        .and_then(|data| data.group_name.as_deref())
    else {
        return false;
    };

    manager.set_automatic_target_name(&target_id, group_name)
}

fn response_group_target_id(response: &NapcatActionResponse) -> Option<String> {
    if let Some(group_id) = response
        .data
        .as_ref()
        .and_then(|data| data.group_id.as_ref())
        .and_then(value_to_target_id)
    {
        return Some(group_id);
    }

    response
        .echo
        .as_deref()
        .and_then(|echo| echo.strip_prefix("group-info:"))
        .map(str::to_owned)
}

fn value_to_target_id(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => number.as_u64().map(|number| number.to_string()),
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_owned())
        },
        _ => None,
    }
}

fn message_system(
    receiver: Res<NapcatIOReceiver>,
    sender: Option<Res<NapcatIOSender>>,
    mut auto_forward_ids: ResMut<NapcatAutoForwardRequestIds>,
    mut group_info_requests: ResMut<NapcatGroupInfoRequests>,
    mut scene_capture_requests: Option<ResMut<SceneCaptureRequests>>,
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
) {
    while let Ok(msg) = receiver.0.try_recv() {
        let json_res = serde_json::from_str::<NapcatMessage>(&msg.to_string());
        if let Ok(mut json) = json_res {
            dbg!(&json);
            cache_message_images(&mut json);
            let target_id = match json.data.message_type {
                NapcatMessageType::Private => {
                    if json.data.user_id == json.data.self_id {
                        json.data.target_id.unwrap_or(json.data.user_id)
                    } else {
                        json.data.user_id
                    }
                },
                NapcatMessageType::Group => json.data.group_id.unwrap_or(json.data.user_id),
            };
            let target_id = target_id.to_string();
            let is_new_target = !manager.messages.contains_key(&target_id);
            let is_incoming_message = json.data.user_id != json.data.self_id;
            let incoming_user_id = json.data.user_id;

            let auto_forward = auto_forward_request(&manager, &json, &target_id);
            let character_creation_response = if is_incoming_message
                && matches!(
                    json.data.message_type,
                    NapcatMessageType::Private
                ) {
                handle_character_creation_message(&mut manager, &json, &target_id)
            } else {
                None
            };
            if is_scene_capture_command(&json) && json.data.user_id != json.data.self_id {
                if let Some(scene_capture_requests) = scene_capture_requests.as_deref_mut() {
                    scene_capture_requests.requests.push(SceneCaptureRequest {
                        user_id: json.data.user_id,
                    });
                }
            }

            manager
                .messages
                .entry(target_id.clone())
                .or_default()
                .push(json);
            manager.chat_targets.entry(target_id.clone()).or_default();
            let group_name = manager
                .messages
                .get(&target_id)
                .and_then(|messages| messages.last())
                .and_then(|message| message.data.group_name.as_deref())
                .map(str::to_owned);
            if let Some(group_name) = group_name {
                manager.set_automatic_target_name(&target_id, &group_name);
            }
            if is_incoming_message {
                manager.register_incoming_target(&target_id, is_new_target);
            }

            if let (Some(sender), Some(response)) = (
                sender.as_deref(),
                character_creation_response.as_deref(),
            ) {
                if queue_private_text_response(
                    sender,
                    &mut auto_forward_ids,
                    incoming_user_id,
                    response.to_owned(),
                ) {
                    append_local_private_text_response(
                        &mut manager,
                        &target_id,
                        incoming_user_id,
                        response,
                    );
                }
            }

            if let Err(err) = manager.persist() {
                eprintln!("failed to persist NapCat messages: {err}");
            }

            if let (Some(sender), Some(auto_forward)) = (sender.as_deref(), auto_forward) {
                for user_id in auto_forward.recipients {
                    queue_private_text_response(
                        sender,
                        &mut auto_forward_ids,
                        user_id,
                        auto_forward.text.clone(),
                    );
                }
            }
        } else {
            let response_res = serde_json::from_str::<NapcatActionResponse>(&msg.to_string());
            if let Ok(response) = response_res {
                if apply_group_info_response(
                    &response,
                    &mut manager,
                    &mut group_info_requests,
                ) {
                    if let Err(err) = manager.persist() {
                        eprintln!("failed to persist NapCat group info: {err}");
                    }
                }
            } else {
                eprintln!(
                    "NapCat websocket response was not a persisted chat message: {}; parse error: {:?}",
                    msg,
                    json_res.err()
                );
            }
        }
    }
}

fn cache_message_images(message: &mut NapcatMessage) {
    for chain in &mut message.data.message {
        let NapcatMessageChainType::Image { data } = &mut chain.variant else {
            continue;
        };
        if !data.local_path.trim().is_empty() || data.url.trim().is_empty() {
            continue;
        }

        match cache_remote_image(data.url.trim()) {
            Ok(path) => data.local_path = path.to_string_lossy().to_string(),
            Err(err) => eprintln!(
                "failed to cache NapCat image {}: {err}",
                data.url
            ),
        }
    }
}

fn cache_remote_image(url: &str) -> Result<PathBuf, String> {
    let response = reqwest::blocking::get(url).map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let bytes = response.bytes().map_err(|err| err.to_string())?;
    let format = image::guess_format(&bytes).map_err(|err| err.to_string())?;
    let extension = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Bmp => "bmp",
        _ => "img",
    };

    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let cache_dir = Path::new(".data").join("willowblossom").join("image_cache");
    fs::create_dir_all(&cache_dir).map_err(|err| err.to_string())?;
    let path = cache_dir.join(format!(
        "{:016x}.{extension}",
        hasher.finish()
    ));
    if !path.exists() {
        fs::write(&path, &bytes).map_err(|err| err.to_string())?;
    }
    Ok(path)
}

fn queue_private_text_response(
    sender: &NapcatIOSender,
    auto_forward_ids: &mut NapcatAutoForwardRequestIds,
    user_id: u64,
    text: String,
) -> bool {
    let request_id = auto_forward_ids.next_request_id;
    auto_forward_ids.next_request_id += 1;
    let message = Message::Text(
        json!({
            "action": "send_private_msg",
            "params": {
                "user_id": user_id,
                "message": [
                    {
                        "type": "text",
                        "data": {
                            "text": text
                        }
                    }
                ]
            }
        })
        .to_string()
        .into(),
    );

    if let Err(err) = sender.0.try_send(NapcatOutboundMessage {
        request_id,
        target_id: user_id.to_string(),
        message,
    }) {
        eprintln!("failed to queue NapCat private text response: {err}");
        false
    } else {
        true
    }
}

fn append_local_private_text_response(
    manager: &mut NapcatMessageManager,
    target_id: &str,
    recipient_id: u64,
    text: &str,
) {
    let Some(self_id) = manager
        .messages
        .get(target_id)
        .and_then(|messages| messages.first())
        .map(|message| message.data.self_id)
    else {
        return;
    };

    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let message = NapcatMessage {
        data: NapcatMessageData {
            time,
            message_type: NapcatMessageType::Private,
            message: vec![NapcatMessageChain {
                variant: NapcatMessageChainType::Text {
                    data: TextData {
                        text: text.to_owned(),
                    },
                },
            }],
            self_id,
            user_id: self_id,
            group_id: None,
            group_name: None,
            target_id: Some(recipient_id),
            sender: NapcatSender {
                user_id: self_id,
                nickname: "GM".to_owned(),
            },
        },
    };

    manager
        .messages
        .entry(target_id.to_owned())
        .or_default()
        .push(message);
}

fn is_scene_capture_command(message: &NapcatMessage) -> bool {
    let text = message_text(message);
    matches!(
        text.trim(),
        "#观察" | "#gc" | ".观察" | ".gc"
    )
}

fn handle_character_creation_message(
    manager: &mut NapcatMessageManager,
    message: &NapcatMessage,
    target_id: &str,
) -> Option<String> {
    let text = message_text(message).trim().to_owned();
    let image_reference = message_image_reference(message);
    let current_step = manager
        .player_characters
        .get(target_id)
        .map(|character| character.creation_step)
        .unwrap_or_default();
    let nickname_taken = current_step == CharacterCreationStep::Nickname
        && !text.is_empty()
        && manager.player_characters.iter().any(|(other_id, other)| {
            other_id != target_id && other.inited && other.nickname == text
        });
    let character = manager
        .player_characters
        .entry(target_id.to_owned())
        .or_insert_with(PlayerCharacter::default);

    if is_exchange_command(&text) {
        if character.inited {
            return Some("你已经有完成的角色卡了，如需修改请联系GM在角色编辑器中调整。".to_owned());
        }
        if character.creation_step != CharacterCreationStep::Normal {
            return Some(character_creation_prompt(character));
        }

        *character = PlayerCharacter::default();
        character.name = message.data.sender.nickname.clone();
        character.creation_step = CharacterCreationStep::Str;
        return Some(format!(
            "你还没有角色卡呢，接下来会开始建卡。\n你拥有{}点属性点，请将它分配到力量/敏捷/灵巧/体质/智力/智慧/知识/魅力上。\n请直接输入数字来增加当前属性；输入【..】退回上一个属性。\n{}",
            character.status_points,
            character_creation_prompt(character)
        ));
    }

    if character.inited || character.creation_step == CharacterCreationStep::Normal {
        return None;
    }

    if matches!(
        text.as_str(),
        ".." | ".。" | "。." | "。。"
    ) {
        return Some(character_creation_back(character));
    }
    if matches!(text.as_str(), "." | "。") {
        return Some(character_creation_next(character));
    }

    if let Some(status_key) = character.creation_step.status_key() {
        let Ok(points) = text.parse::<i32>() else {
            return Some(format!(
                "请输入0到{}之间的数字来分配{}。",
                character.status_points,
                status_key.zh()
            ));
        };
        if points < 0 || points > character.status_points {
            return Some(format!(
                "输入不合法。你剩余{}点属性点，但试图投入{}点。",
                character.status_points, points
            ));
        }
        set_character_status_value(
            &mut character.status,
            status_key,
            points,
        );
        character.status_points -= points;
        if character.status_points <= 0 {
            character.creation_step = CharacterCreationStep::ConfirmStatus;
            return Some(character_status_confirmation(character));
        }
        character.creation_step = character.creation_step.next_status_step();
        return Some(character_creation_prompt(character));
    }

    match character.creation_step {
        CharacterCreationStep::Skill => {
            character.skill_names.push(String::new());
            character.skill_notes.push(text);
            Some(format!(
                "技能兑换数据已录入，目前记录{}条。继续发送技能，或输入【.】结束技能录入。",
                character.skill_notes.len()
            ))
        },
        CharacterCreationStep::Image => {
            let image = image_reference.unwrap_or(text);
            if image.is_empty() {
                return Some(
                    "请发送人物立绘图片，或发送图片链接；如果暂时没有，输入【.】跳过。".to_owned(),
                );
            }
            character.image = image;
            character.creation_step = CharacterCreationStep::Nickname;
            Some(character_creation_prompt(character))
        },
        CharacterCreationStep::Nickname => {
            if text.is_empty() {
                return Some("角色名不能为空，请重新输入。".to_owned());
            }
            if nickname_taken {
                return Some(format!(
                    "很抱歉，「{text}」昵称已经被人使用了，请更换一个昵称。"
                ));
            }
            character.nickname = text;
            character.inited = true;
            character.creation_step = CharacterCreationStep::Normal;
            update_character_from_status(character);
            Some(format!(
                "是吗？「{}」真是个好名字呢，我十分期待您以后的表现。\n——兑换结束——",
                character.nickname
            ))
        },
        _ => None,
    }
}

fn is_exchange_command(text: &str) -> bool { matches!(text.trim(), ".兑换" | "。兑换") }

fn character_creation_next(character: &mut PlayerCharacter) -> String {
    match character.creation_step {
        CharacterCreationStep::ConfirmStatus => {
            character.creation_step = CharacterCreationStep::Skill;
            "属性数据已录入。\n现在是技能兑换，请按你的技能描述发送文本；输入【.】可以跳过或结束技能录入。".to_owned()
        },
        CharacterCreationStep::Skill => {
            character.creation_step = CharacterCreationStep::ConfirmSkill;
            character_creation_next(character)
        },
        CharacterCreationStep::ConfirmSkill => {
            character.creation_step = CharacterCreationStep::Image;
            "技能数据已录入。现在请发送人物立绘图片链接；如果暂时没有，输入【.】跳过。".to_owned()
        },
        CharacterCreationStep::Image => {
            character.creation_step = CharacterCreationStep::Nickname;
            "图片已跳过。\n最后，请告诉我你的角色名，兑换即将结束。".to_owned()
        },
        CharacterCreationStep::Nickname => "请直接发送角色名完成建卡。".to_owned(),
        _ => character_creation_prompt(character),
    }
}

fn character_creation_back(character: &mut PlayerCharacter) -> String {
    if character.creation_step.status_key().is_some()
        || character.creation_step == CharacterCreationStep::ConfirmStatus
    {
        reset_character_status_phase(character);
        return format!(
            "已退回属性兑换第一步，属性点已全部返还。\n{}",
            character_creation_prompt(character)
        );
    }

    match character.creation_step {
        CharacterCreationStep::Skill | CharacterCreationStep::ConfirmSkill => {
            reset_character_status_phase(character);
            format!(
                "已退回属性兑换第一步，属性点已全部返还。\n{}",
                character_creation_prompt(character)
            )
        },
        CharacterCreationStep::Image => {
            character.creation_step = CharacterCreationStep::Skill;
            "已退回技能兑换。请继续发送技能描述；输入【.】结束技能录入。".to_owned()
        },
        CharacterCreationStep::Nickname => {
            character.creation_step = CharacterCreationStep::Image;
            character.image.clear();
            "已退回图片录入。请发送人物立绘图片链接；如果暂时没有，输入【.】跳过。".to_owned()
        },
        CharacterCreationStep::Normal => "未处于建卡流程。输入【.兑换】开始。".to_owned(),
        CharacterCreationStep::ConfirmStatus
        | CharacterCreationStep::Str
        | CharacterCreationStep::Agi
        | CharacterCreationStep::Dex
        | CharacterCreationStep::Vit
        | CharacterCreationStep::Int
        | CharacterCreationStep::Wis
        | CharacterCreationStep::K
        | CharacterCreationStep::Cha => unreachable!("status steps return before match"),
    }
}

fn character_creation_prompt(character: &PlayerCharacter) -> String {
    if let Some(status_key) = character.creation_step.status_key() {
        return format!(
            "当前{}:「{}」 剩余属性点:「{}」",
            status_key.zh(),
            get_character_status_value(&character.status, status_key),
            character.status_points
        );
    }

    match character.creation_step {
        CharacterCreationStep::ConfirmStatus => character_status_confirmation(character),
        CharacterCreationStep::Skill => {
            format!(
                "现在是技能兑换。你还剩余{}分，请发送技能描述；输入【.】结束技能录入。",
                character.exchange_points
            )
        },
        CharacterCreationStep::ConfirmSkill => "技能数据已录入，输入【.】继续。".to_owned(),
        CharacterCreationStep::Image => "请发送人物立绘图片链接；输入【.】跳过。".to_owned(),
        CharacterCreationStep::Nickname => "最后，请告诉我你的角色名。".to_owned(),
        CharacterCreationStep::Normal => "未处于建卡流程。输入【.兑换】开始。".to_owned(),
        _ => "请继续当前建卡步骤。".to_owned(),
    }
}

fn character_status_confirmation(character: &PlayerCharacter) -> String {
    format!(
        "属性兑换全部完成，输入【.】确认；输入【..】退回上一步。\n{}",
        format_character_status(character)
    )
}

fn format_character_status(character: &PlayerCharacter) -> String {
    [
        StatusKey::Str,
        StatusKey::Agi,
        StatusKey::Dex,
        StatusKey::Vit,
        StatusKey::Int,
        StatusKey::Wis,
        StatusKey::K,
        StatusKey::Cha,
    ]
    .iter()
    .map(|status_key| {
        format!(
            "{}:「{}」",
            status_key.zh(),
            get_character_status_value(&character.status, *status_key)
        )
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn get_character_status_value(status: &CharacterStatus, status_key: StatusKey) -> i32 {
    match status_key {
        StatusKey::Str => status.str_,
        StatusKey::Agi => status.agi,
        StatusKey::Dex => status.dex,
        StatusKey::Vit => status.vit,
        StatusKey::Int => status.int_,
        StatusKey::Wis => status.wis,
        StatusKey::K => status.k,
        StatusKey::Cha => status.cha,
    }
}

fn set_character_status_value(status: &mut CharacterStatus, status_key: StatusKey, value: i32) {
    match status_key {
        StatusKey::Str => status.str_ = value,
        StatusKey::Agi => status.agi = value,
        StatusKey::Dex => status.dex = value,
        StatusKey::Vit => status.vit = value,
        StatusKey::Int => status.int_ = value,
        StatusKey::Wis => status.wis = value,
        StatusKey::K => status.k = value,
        StatusKey::Cha => status.cha = value,
    }
}

fn reset_character_status_phase(character: &mut PlayerCharacter) {
    character.creation_step = CharacterCreationStep::Str;
    character.status_points = default_status_points();
    character.status = CharacterStatus::default();
}

pub fn update_character_from_status(character: &mut PlayerCharacter) {
    let total_str = character.status.str_ + character.extra_status.str_;
    let total_agi = character.status.agi + character.extra_status.agi;
    let total_dex = character.status.dex + character.extra_status.dex;
    let total_vit = character.status.vit + character.extra_status.vit;
    let total_int = character.status.int_ + character.extra_status.int_;
    let total_wis = character.status.wis + character.extra_status.wis;

    character.max_hp = (5 + character.level * 5 + total_str + total_vit * 3).max(1) as f32;
    character.hp = character.max_hp;
    character.hp_regen = total_vit.max(0) as f32;
    character.max_mp = (total_int * 5) as f32 + total_wis as f32 * 2.5;
    character.mp = character.max_mp.max(0.0);
    character.mp_regen = total_wis.max(0) as f32;
    character.speed = 3.0
        + total_str.max(0) as f32 * 0.5
        + total_agi.max(0) as f32
        + total_dex.max(0) as f32 * 0.5;
}

fn message_text(message: &NapcatMessage) -> String {
    message
        .data
        .message
        .iter()
        .filter_map(|chain| match &chain.variant {
            NapcatMessageChainType::Text { data } => Some(data.text.as_str()),
            NapcatMessageChainType::Source(_) => None,
            NapcatMessageChainType::Image { .. } => None,
            NapcatMessageChainType::Unsupported => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn message_image_reference(message: &NapcatMessage) -> Option<String> {
    message.data.message.iter().find_map(|chain| {
        let NapcatMessageChainType::Image { data } = &chain.variant else {
            return None;
        };
        if !data.local_path.trim().is_empty() {
            Some(data.local_path.trim().to_owned())
        } else if !data.url.trim().is_empty() {
            Some(data.url.trim().to_owned())
        } else if !data.file_id.trim().is_empty() {
            Some(data.file_id.trim().to_owned())
        } else {
            None
        }
    })
}

struct AutoForwardRequest {
    recipients: Vec<u64>,
    text: String,
}

fn auto_forward_request(
    manager: &NapcatMessageManager,
    message: &NapcatMessage,
    target_id: &str,
) -> Option<AutoForwardRequest> {
    if !matches!(
        message.data.message_type,
        NapcatMessageType::Private
    ) || message.data.user_id == message.data.self_id
    {
        return None;
    }

    let text = quoted_auto_forward_text(message)?;
    let recipients = manager
        .groups
        .values()
        .find(|group| group.members.iter().any(|member_id| member_id == target_id))?
        .members
        .iter()
        .filter(|member_id| member_id.as_str() != target_id)
        .filter_map(|member_id| {
            let is_private_member = matches!(
                manager
                    .messages
                    .get(member_id)
                    .and_then(|messages| messages.first())
                    .map(|message| &message.data.message_type),
                Some(NapcatMessageType::Private)
            );
            if !is_private_member {
                return None;
            }
            member_id.parse::<u64>().ok()
        })
        .collect::<Vec<_>>();

    if recipients.is_empty() {
        return None;
    }

    Some(AutoForwardRequest {
        recipients,
        text: format!(
            "{}: {}",
            message.data.sender.nickname, text
        ),
    })
}

fn quoted_auto_forward_text(message: &NapcatMessage) -> Option<String> {
    let mut text = String::new();
    for chain in &message.data.message {
        if let NapcatMessageChainType::Text { data } = &chain.variant {
            text.push_str(&data.text);
        }
    }

    let text = text.trim();
    let mut indexed_chars = text.char_indices();
    let (_, start_quote) = indexed_chars.next()?;
    let (end_quote_index, end_quote) = indexed_chars.next_back()?;
    if !is_auto_forward_quote(start_quote) || !is_auto_forward_quote(end_quote) {
        return None;
    }

    let inner = text[start_quote.len_utf8()..end_quote_index].trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_owned())
    }
}

fn is_auto_forward_quote(character: char) -> bool {
    matches!(character, '"' | '“' | '”' | '＂')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_manager() -> NapcatMessageManager {
        NapcatMessageManager {
            messages: HashMap::default(),
            chat_targets: HashMap::default(),
            player_characters: HashMap::default(),
            trpg_groups: HashMap::default(),
            current_trpg_group: None,
            groups: HashMap::default(),
            read_message_counts: HashMap::default(),
            summarized_message_counts: HashMap::default(),
            open_chat_targets: HashSet::default(),
            pending_chat_targets: HashSet::default(),
        }
    }

    fn test_message(message_type: NapcatMessageType) -> NapcatMessage {
        test_message_with_text(message_type, "hello")
    }

    fn test_message_with_text(message_type: NapcatMessageType, text: &str) -> NapcatMessage {
        NapcatMessage {
            data: NapcatMessageData {
                time: 1780132600,
                message_type,
                message: vec![NapcatMessageChain {
                    variant: NapcatMessageChainType::Text {
                        data: TextData {
                            text: text.to_owned(),
                        },
                    },
                }],
                self_id: 1,
                user_id: 2,
                group_id: None,
                group_name: None,
                target_id: None,
                sender: NapcatSender {
                    user_id: 2,
                    nickname: "tester".to_owned(),
                },
            },
        }
    }

    fn test_private_image(url: &str) -> NapcatMessage {
        NapcatMessage {
            data: NapcatMessageData {
                time: 1780132600,
                message_type: NapcatMessageType::Private,
                message: vec![NapcatMessageChain {
                    variant: NapcatMessageChainType::Image {
                        data: ImageData {
                            sub_type: 0,
                            file: String::new(),
                            url: url.to_owned(),
                            file_id: String::new(),
                            file_size: String::new(),
                            local_path: String::new(),
                        },
                    },
                }],
                self_id: 1,
                user_id: 2,
                group_id: None,
                group_name: None,
                target_id: None,
                sender: NapcatSender {
                    user_id: 2,
                    nickname: "tester".to_owned(),
                },
            },
        }
    }

    #[test]
    fn scene_capture_command_accepts_hash_and_dot_aliases() {
        for command in ["#观察", "#gc", ".观察", ".gc"] {
            let message = test_message_with_text(NapcatMessageType::Private, command);
            assert!(
                is_scene_capture_command(&message),
                "{command} should trigger capture"
            );
        }
    }

    #[test]
    fn parses_group_message_with_unsupported_segments() {
        let message = serde_json::from_str::<NapcatMessage>(
            r#"{
                "time": 1780132600,
                "message_type": "group",
                "message": [
                    { "type": "at", "data": { "qq": "123" } },
                    { "type": "text", "data": { "text": "hello group" } }
                ],
                "self_id": 3432505351,
                "user_id": 1670426821,
                "group_id": 123456,
                "sender": {
                    "user_id": 1670426821,
                    "nickname": "tester"
                }
            }"#,
        )
        .expect("group message should parse");

        assert_eq!(message.data.message.len(), 2);
        assert!(matches!(
            message.data.message[0].variant,
            NapcatMessageChainType::Unsupported
        ));
        assert!(matches!(
            message.data.message[1].variant,
            NapcatMessageChainType::Text { .. }
        ));
    }

    #[test]
    fn parses_string_message_payload() {
        let message = serde_json::from_str::<NapcatMessage>(
            r#"{
                "time": 1780132600,
                "message_type": "group",
                "message": "plain group text",
                "self_id": 3432505351,
                "user_id": 1670426821,
                "group_id": 123456,
                "sender": {
                    "user_id": 1670426821,
                    "nickname": "tester"
                }
            }"#,
        )
        .expect("string message should parse");

        let NapcatMessageChainType::Text { data } = &message.data.message[0].variant else {
            panic!("string payload should become a text segment");
        };
        assert_eq!(data.text, "plain group text");
    }

    #[test]
    fn detects_auto_forward_text_with_mixed_quote_styles() {
        let message = serde_json::from_str::<NapcatMessage>(
            r#"{
                "time": 1780132600,
                "message_type": "private",
                "message": "“hello players\"",
                "self_id": 3432505351,
                "user_id": 1670426821,
                "sender": {
                    "user_id": 1670426821,
                    "nickname": "tester"
                }
            }"#,
        )
        .expect("private message should parse");

        assert_eq!(
            quoted_auto_forward_text(&message),
            Some("hello players".to_owned())
        );
    }

    #[test]
    fn rejects_auto_forward_text_without_strict_boundary_quotes() {
        let message = serde_json::from_str::<NapcatMessage>(
            r#"{
                "time": 1780132600,
                "message_type": "private",
                "message": "say \"hello players\"",
                "self_id": 3432505351,
                "user_id": 1670426821,
                "sender": {
                    "user_id": 1670426821,
                    "nickname": "tester"
                }
            }"#,
        )
        .expect("private message should parse");

        assert_eq!(quoted_auto_forward_text(&message), None);
    }

    #[test]
    fn rejects_single_quote_character_as_auto_forward_text() {
        let message = serde_json::from_str::<NapcatMessage>(
            r#"{
                "time": 1780132600,
                "message_type": "private",
                "message": "\"",
                "self_id": 3432505351,
                "user_id": 1670426821,
                "sender": {
                    "user_id": 1670426821,
                    "nickname": "tester"
                }
            }"#,
        )
        .expect("private message should parse");

        assert_eq!(quoted_auto_forward_text(&message), None);
    }

    #[test]
    fn new_incoming_target_waits_for_chat_window_approval() {
        let mut manager = empty_manager();

        manager.register_incoming_target("12345", true);

        assert!(manager.pending_chat_targets.contains("12345"));
        assert!(!manager.open_chat_targets.contains("12345"));
    }

    #[test]
    fn existing_message_targets_migrate_to_open_chat_windows() {
        let mut manager = empty_manager();
        manager.messages.insert("12345".to_owned(), Vec::new());

        assert!(manager.migrate_chat_window_state());
        assert!(manager.open_chat_targets.contains("12345"));
        assert!(manager.pending_chat_targets.is_empty());
    }

    #[test]
    fn synced_message_targets_do_not_reopen_after_all_windows_closed() {
        let mut manager = empty_manager();
        manager.messages.insert("12345".to_owned(), Vec::new());
        manager.chat_targets.insert(
            "12345".to_owned(),
            ChatTargetMetadata::default(),
        );

        assert!(!manager.migrate_chat_window_state());
        assert!(manager.open_chat_targets.is_empty());
        assert!(manager.pending_chat_targets.is_empty());
    }

    #[test]
    fn message_targets_sync_to_editable_chat_metadata() {
        let mut manager = empty_manager();
        manager.messages.insert("12345".to_owned(), Vec::new());

        assert!(manager.sync_chat_targets());
        assert!(manager.chat_targets.contains_key("12345"));
        assert!(!manager.sync_chat_targets());
    }

    #[test]
    fn group_info_response_updates_automatic_chat_name() {
        let mut manager = empty_manager();
        let mut requests = NapcatGroupInfoRequests {
            next_request_id: 2_000_000,
            pending_group_ids: HashSet::from(["976886808".to_owned()]),
        };
        let response = serde_json::from_str::<NapcatActionResponse>(
            r#"{
                "status": "ok",
                "retcode": 0,
                "data": {
                    "group_id": 976886808,
                    "group_name": "柳絮，只是另一个跑团软件"
                },
                "echo": "group-info:976886808"
            }"#,
        )
        .expect("group info response should parse");

        assert!(apply_group_info_response(
            &response,
            &mut manager,
            &mut requests
        ));
        assert_eq!(
            manager.chat_targets["976886808"].automatic_name,
            "柳絮，只是另一个跑团软件"
        );
        assert!(!requests.pending_group_ids.contains("976886808"));
    }

    #[test]
    fn private_message_targets_sync_to_player_characters() {
        let mut manager = empty_manager();
        manager.messages.insert("player-1".to_owned(), vec![
            test_message(NapcatMessageType::Private),
        ]);
        manager.messages.insert("group-1".to_owned(), vec![
            test_message(NapcatMessageType::Group),
        ]);

        assert!(manager.sync_chat_targets());
        assert!(manager.player_characters.contains_key("player-1"));
        assert!(!manager.player_characters.contains_key("group-1"));
        assert!(!manager.sync_chat_targets());
    }

    #[test]
    fn private_exchange_command_runs_character_creation_workflow() {
        let mut manager = empty_manager();
        let target_id = "2";

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            target_id,
        )
        .unwrap();
        assert!(response.contains("开始建卡"));
        assert_eq!(
            manager.player_characters[target_id].creation_step,
            CharacterCreationStep::Str
        );

        for value in ["2", "1", "1", "1"] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        let character = manager.player_characters.get(target_id).unwrap();
        assert_eq!(
            character.creation_step,
            CharacterCreationStep::ConfirmStatus
        );
        assert_eq!(character.status.str_, 2);
        assert_eq!(character.status.vit, 1);

        for value in [".", "."] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        handle_character_creation_message(
            &mut manager,
            &test_private_image("https://example.test/pc.png"),
            target_id,
        );
        assert_eq!(
            manager.player_characters[target_id].creation_step,
            CharacterCreationStep::Nickname
        );
        assert_eq!(
            manager.player_characters[target_id].image,
            "https://example.test/pc.png"
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, "柳生"),
            target_id,
        )
        .unwrap();
        let character = manager.player_characters.get(target_id).unwrap();
        assert!(response.contains("兑换结束"));
        assert!(character.inited);
        assert_eq!(character.nickname, "柳生");
        assert_eq!(
            character.creation_step,
            CharacterCreationStep::Normal
        );
        assert_eq!(character.max_hp, 15.0);
    }

    #[test]
    fn character_stats_are_derived_from_status_and_level() {
        let mut character = PlayerCharacter::default();
        character.level = 2;
        character.status.str_ = 2;
        character.status.vit = 3;
        character.status.int_ = 1;
        character.status.wis = 2;
        character.extra_status.agi = 1;
        character.extra_status.dex = 2;
        character.extra_status.vit = 1;

        update_character_from_status(&mut character);

        assert_eq!(character.max_hp, 29.0);
        assert_eq!(character.hp, 29.0);
        assert_eq!(character.hp_regen, 4.0);
        assert_eq!(character.max_mp, 10.0);
        assert_eq!(character.mp, 10.0);
        assert_eq!(character.mp_regen, 2.0);
        assert_eq!(character.speed, 6.0);
    }

    #[test]
    fn local_private_text_response_is_appended_as_self_message() {
        let mut manager = empty_manager();
        let target_id = "2";
        manager.messages.insert(target_id.to_owned(), vec![
            test_message_with_text(NapcatMessageType::Private, ".兑换"),
        ]);

        append_local_private_text_response(&mut manager, target_id, 2, "兑换回复");

        let messages = manager.messages.get(target_id).unwrap();
        assert_eq!(messages.len(), 2);
        let response = &messages[1];
        assert!(matches!(
            response.data.message_type,
            NapcatMessageType::Private
        ));
        assert_eq!(
            response.data.user_id,
            response.data.self_id
        );
        assert_eq!(response.data.target_id, Some(2));
        assert_eq!(response.data.sender.nickname, "GM");
        assert_eq!(message_text(response), "兑换回复");
    }

    #[test]
    fn character_creation_back_resets_status_phase_and_refunds_points() {
        let mut manager = empty_manager();
        let target_id = "2";

        handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            target_id,
        );
        for value in ["5"] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        assert_eq!(
            manager.player_characters[target_id].creation_step,
            CharacterCreationStep::ConfirmStatus
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".."),
            target_id,
        )
        .unwrap();
        let character = manager.player_characters.get(target_id).unwrap();

        assert!(response.contains("属性点已全部返还"));
        assert_eq!(
            character.creation_step,
            CharacterCreationStep::Str
        );
        assert_eq!(
            character.status_points,
            default_status_points()
        );
        assert_eq!(character.status.str_, 0);
        assert_eq!(character.status.cha, 0);
    }

    #[test]
    fn character_creation_back_from_nickname_returns_to_image_phase() {
        let mut manager = empty_manager();
        let target_id = "2";

        handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            target_id,
        );
        for value in ["2", "1", "1", "1", ".", "."] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        handle_character_creation_message(
            &mut manager,
            &test_private_image("https://example.test/pc.png"),
            target_id,
        );
        assert_eq!(
            manager.player_characters[target_id].creation_step,
            CharacterCreationStep::Nickname
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".."),
            target_id,
        )
        .unwrap();
        let character = manager.player_characters.get(target_id).unwrap();

        assert!(response.contains("图片录入"));
        assert_eq!(
            character.creation_step,
            CharacterCreationStep::Image
        );
        assert!(character.image.is_empty());
    }

    #[test]
    fn character_creation_back_from_image_returns_to_skill_phase() {
        let mut manager = empty_manager();
        let target_id = "2";

        handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            target_id,
        );
        for value in ["2", "1", "1", "1", ".", "."] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        assert_eq!(
            manager.player_characters[target_id].creation_step,
            CharacterCreationStep::Image
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".."),
            target_id,
        )
        .unwrap();
        let character = manager.player_characters.get(target_id).unwrap();

        assert!(response.contains("技能兑换"));
        assert_eq!(
            character.creation_step,
            CharacterCreationStep::Skill
        );
    }

    #[test]
    fn chat_target_sync_prunes_missing_trpg_group_members() {
        let mut manager = empty_manager();
        manager.messages.insert("player-1".to_owned(), Vec::new());
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["player-1".to_owned(), "missing-player".to_owned()],
            group_chats: vec!["missing-group".to_owned()],
            ..Default::default()
        });

        assert!(manager.sync_chat_targets());
        let group = manager.trpg_groups.get("table").unwrap();
        assert_eq!(group.players, vec!["player-1"]);
        assert!(group.group_chats.is_empty());
        assert!(group.player_turns.contains_key("player-1"));
        assert!(!group.player_turns.contains_key("missing-player"));
    }

    #[test]
    fn chat_target_sync_selects_only_trpg_group_as_current() {
        let mut manager = empty_manager();
        manager
            .trpg_groups
            .insert("table".to_owned(), TrpgGroup::default());

        assert!(manager.sync_chat_targets());
        assert_eq!(
            manager.current_trpg_group.as_deref(),
            Some("table")
        );
    }

    #[test]
    fn chat_target_sync_clears_deleted_current_group() {
        let mut manager = empty_manager();
        manager.current_trpg_group = Some("deleted".to_owned());
        manager
            .trpg_groups
            .insert("table".to_owned(), TrpgGroup::default());

        assert!(manager.sync_chat_targets());
        assert_eq!(
            manager.current_trpg_group.as_deref(),
            Some("table")
        );
    }

    #[test]
    fn trpg_group_advances_world_turn_after_all_players_finish() {
        let mut group = TrpgGroup {
            players: vec!["a".to_owned(), "b".to_owned()],
            ..Default::default()
        };

        assert!(group.mark_player_acted("a"));
        assert_eq!(group.world_turn, 0);
        assert!(group.player_turns["a"].acted);
        assert!(!group.player_turns["b"].acted);

        assert!(group.mark_player_skipped("b"));
        assert_eq!(group.world_turn, 1);
        assert_eq!(group.player_turns["a"].turns_passed, 1);
        assert_eq!(group.player_turns["b"].turns_passed, 1);
        assert!(!group.player_turns["a"].acted);
        assert!(!group.player_turns["b"].skipped);
    }

    #[test]
    fn trpg_group_turn_action_is_idempotent() {
        let mut group = TrpgGroup {
            players: vec!["a".to_owned(), "b".to_owned()],
            ..Default::default()
        };

        assert!(group.mark_player_acted("a"));
        assert!(!group.mark_player_acted("a"));
        assert_eq!(group.world_turn, 0);
        assert!(group.player_turns["a"].acted);
        assert!(!group.player_turns["a"].skipped);
        assert!(!group.player_turns["b"].acted);
    }

    #[test]
    fn trpg_group_rejects_turn_action_for_non_member() {
        let mut group = TrpgGroup {
            players: vec!["a".to_owned()],
            ..Default::default()
        };

        assert!(!group.mark_player_acted("missing"));
        assert_eq!(group.world_turn, 0);
        assert!(group.player_turns.is_empty());
    }

    #[test]
    fn trpg_group_sets_player_turn_count() {
        let mut group = TrpgGroup {
            players: vec!["a".to_owned()],
            ..Default::default()
        };

        assert!(group.set_player_turns_passed("a", 7));
        assert_eq!(group.player_turns["a"].turns_passed, 7);
        assert!(!group.set_player_turns_passed("missing", 3));
        assert_eq!(group.world_turn, 0);
    }

    #[test]
    fn trpg_group_manual_advance_counts_all_current_players() {
        let mut group = TrpgGroup {
            players: vec!["a".to_owned(), "b".to_owned()],
            ..Default::default()
        };

        assert!(group.advance_world_turn());

        assert_eq!(group.world_turn, 1);
        assert_eq!(group.player_turns["a"].turns_passed, 1);
        assert_eq!(group.player_turns["b"].turns_passed, 1);
    }
}
