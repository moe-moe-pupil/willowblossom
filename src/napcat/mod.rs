use std::{
    collections::HashMap,
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

#[derive(Resource, Serialize, Deserialize)]
pub struct NapcatMessageManager {
    pub messages: HashMap<String, Vec<NapcatMessage>>,
    #[serde(default)]
    pub groups: HashMap<String, ChatGroup>,
    #[serde(default)]
    pub read_message_counts: HashMap<String, usize>,
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
        groups: HashMap::default(),
        read_message_counts: HashMap::default(),
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

            let auto_forward = auto_forward_request(&manager, &json, &target_id);

            manager
                .messages
                .entry(target_id.clone())
                .or_default()
                .push(json);

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
}
