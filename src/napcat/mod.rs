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

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageData {
    #[serde(rename = "subType")]
    pub sub_type: usize,
    pub url: String,
    pub file_id: String,
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
    #[serde(other)]
    Unsupported,
    // TODO: support image
    // Image { data: ImageData },
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
}

impl Default for PlayerCharacter {
    fn default() -> Self {
        Self {
            inited: false,
            name: String::new(),
            nickname: String::new(),
            image: String::new(),
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
        }
    }
}

fn default_character_hp() -> f32 { 5.0 }

fn default_character_level() -> i32 { 1 }

fn default_character_speed() -> f32 { 3.0 }

fn default_modifier() -> f32 { 1.0 }

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

            let auto_forward = auto_forward_request(&manager, &json, &target_id);
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

            if let (Some(sender), Some(auto_forward)) = (sender.as_deref(), auto_forward) {
                for user_id in auto_forward.recipients {
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
                                            "text": auto_forward.text
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
                        eprintln!("failed to queue NapCat auto-forward message: {err}");
                    }
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

fn is_scene_capture_command(message: &NapcatMessage) -> bool {
    let text = message_text(message);
    matches!(text.trim(), "#观察" | "#gc")
}

fn message_text(message: &NapcatMessage) -> String {
    message
        .data
        .message
        .iter()
        .filter_map(|chain| match &chain.variant {
            NapcatMessageChainType::Text { data } => Some(data.text.as_str()),
            NapcatMessageChainType::Source(_) => None,
            NapcatMessageChainType::Unsupported => None,
        })
        .collect::<Vec<_>>()
        .join("")
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
        NapcatMessage {
            data: NapcatMessageData {
                time: 1780132600,
                message_type,
                message: vec![NapcatMessageChain {
                    variant: NapcatMessageChainType::Text {
                        data: TextData {
                            text: "hello".to_owned(),
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
