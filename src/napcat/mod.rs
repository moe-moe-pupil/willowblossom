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
    Text { data: TextData },
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
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NapcatMessageData {
    pub time: u64,
    pub message_type: NapcatMessageType,
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

    let message_manager = NapcatMessageManager {
        messages: HashMap::default(),
        groups: HashMap::default(),
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
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
) {
    if let Ok(msg) = receiver.0.try_recv() {
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

            if manager.messages.contains_key(&target_id.to_string()) {
                manager
                    .messages
                    .get_mut(&target_id.to_string())
                    .unwrap()
                    .push(json)
            } else {
                manager.messages.insert(target_id.to_string(), vec![json]);
            }

            manager.persist().ok();
        } else {
            eprintln!("NapCat websocket response: {}", msg);
        }
    }
}
