use std::{
    collections::{
        HashMap,
        HashSet,
    },
    path::Path,
    thread,
    time::Duration,
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
use serde_json::json;
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

use crate::scene::{
    SceneCaptureRequest,
    SceneCaptureRequests,
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
    pub url: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub file_size: String,
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
    pub skill_notes: Vec<String>,
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
            skill_notes: Vec::new(),
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

    fn previous_status_step(self) -> Option<Self> {
        match self {
            CharacterCreationStep::Agi => Some(CharacterCreationStep::Str),
            CharacterCreationStep::Dex => Some(CharacterCreationStep::Agi),
            CharacterCreationStep::Vit => Some(CharacterCreationStep::Dex),
            CharacterCreationStep::Int => Some(CharacterCreationStep::Vit),
            CharacterCreationStep::Wis => Some(CharacterCreationStep::Int),
            CharacterCreationStep::K => Some(CharacterCreationStep::Wis),
            CharacterCreationStep::Cha => Some(CharacterCreationStep::K),
            CharacterCreationStep::ConfirmStatus => Some(CharacterCreationStep::Cha),
            _ => None,
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

    let message_manager = NapcatMessageManager {
        messages: HashMap::default(),
        chat_targets: HashMap::default(),
        player_characters: HashMap::default(),
        trpg_groups: HashMap::default(),
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

fn message_system(
    receiver: Res<NapcatIOReceiver>,
    sender: Option<Res<NapcatIOSender>>,
    mut auto_forward_ids: ResMut<NapcatAutoForwardRequestIds>,
    mut scene_capture_requests: Option<ResMut<SceneCaptureRequests>>,
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
) {
    while let Ok(msg) = receiver.0.try_recv() {
        let json_res = serde_json::from_str::<NapcatMessage>(&msg.to_string());
        if let Ok(json) = json_res {
            dbg!(&json);
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
            if is_incoming_message {
                manager.register_incoming_target(&target_id, is_new_target);
            }

            if let Err(err) = manager.persist() {
                eprintln!("failed to persist NapCat messages: {err}");
            }

            if let (Some(sender), Some(response)) = (
                sender.as_deref(),
                character_creation_response,
            ) {
                queue_private_text_response(
                    sender,
                    &mut auto_forward_ids,
                    incoming_user_id,
                    response,
                );
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
            eprintln!(
                "NapCat websocket response was not a persisted chat message: {}; parse error: {:?}",
                msg,
                json_res.err()
            );
        }
    }
}

fn queue_private_text_response(
    sender: &NapcatIOSender,
    auto_forward_ids: &mut NapcatAutoForwardRequestIds,
    user_id: u64,
    text: String,
) {
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
    }
}

fn is_scene_capture_command(message: &NapcatMessage) -> bool {
    let text = message_text(message);
    matches!(text.trim(), "#观察" | "#gc")
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
    if let Some(previous_step) = character.creation_step.previous_status_step() {
        character.creation_step = previous_step;
        if let Some(status_key) = previous_step.status_key() {
            let previous_value = get_character_status_value(&character.status, status_key);
            character.status_points += previous_value;
            set_character_status_value(&mut character.status, status_key, 0);
        }
        character_creation_prompt(character)
    } else {
        "当前步骤不能退回。".to_owned()
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

fn update_character_from_status(character: &mut PlayerCharacter) {
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
        if !data.url.trim().is_empty() {
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
                            url: url.to_owned(),
                            file_id: String::new(),
                            file_size: String::new(),
                        },
                    },
                }],
                self_id: 1,
                user_id: 2,
                group_id: None,
                target_id: None,
                sender: NapcatSender {
                    user_id: 2,
                    nickname: "tester".to_owned(),
                },
            },
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
    fn message_targets_sync_to_editable_chat_metadata() {
        let mut manager = empty_manager();
        manager.messages.insert("12345".to_owned(), Vec::new());

        assert!(manager.sync_chat_targets());
        assert!(manager.chat_targets.contains_key("12345"));
        assert!(!manager.sync_chat_targets());
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
    fn chat_target_sync_prunes_missing_trpg_group_members() {
        let mut manager = empty_manager();
        manager.messages.insert("player-1".to_owned(), Vec::new());
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["player-1".to_owned(), "missing-player".to_owned()],
            group_chats: vec!["missing-group".to_owned()],
        });

        assert!(manager.sync_chat_targets());
        let group = manager.trpg_groups.get("table").unwrap();
        assert_eq!(group.players, vec!["player-1"]);
        assert!(group.group_chats.is_empty());
    }
}
