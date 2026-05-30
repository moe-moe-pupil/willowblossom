use std::{
    io::Read,
    path::Path,
};

use async_compat::Compat;
use bevy_egui::egui::{
    self,
    Memory,
    Modifiers,
    TextureHandle,
    Ui,
};
use bevy_persistent::prelude::*;
extern crate dirs;
use std::collections::HashMap;

use bevy::{
    ecs::world::CommandQueue,
    prelude::*,
    tasks::{
        block_on,
        AsyncComputeTaskPool,
        IoTaskPool,
        Task,
    },
};
use crossbeam_channel::{
    unbounded,
    Receiver as CBReceiver,
    Sender as CBSender,
};

use futures_lite::future;
use futures_util::{
    SinkExt,
    StreamExt,
};

use tokio::sync::mpsc::Sender;
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
};

use serde::{
    Deserialize,
    Serialize,
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
pub struct NapcatIOSender(pub Sender<Message>);

#[derive(Resource)]
struct NapcatTask(Task<CommandQueue>);

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
            .add_systems(Update, handle_tasks.run_if(resource_exists::<NapcatTask>))
            .add_systems(Update, message_system);
    }
}

fn setup(mut commands: Commands) {
    let thread_pool = AsyncComputeTaskPool::get();
    let (client_to_game_sender, client_to_game_receiver) = unbounded::<Message>();
    let napcat_io = NapcatIOReceiver(client_to_game_receiver.clone());
    let task = thread_pool.spawn(Compat::new(handle_connection(
        client_to_game_sender.clone(),
    )));
    commands.insert_resource(napcat_io);
    commands.insert_resource(NapcatTask(task));

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

fn handle_tasks(mut commands: Commands, mut task: ResMut<NapcatTask>) {
    if let Some(mut commands_queue) = block_on(future::poll_once(&mut task.0)) {
        // append the returned command queue to have it execute later
        commands.append(&mut commands_queue);
    }
}

async fn handle_connection<'a>(client_to_game_sender: CBSender<Message>) -> CommandQueue {
    let (ws_stream, _) = connect_async("ws://localhost:3001")
        .await
        .expect("Failed to connect");
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let (game_to_client_sender, mut game_to_client_receiver) = tokio::sync::mpsc::channel(100);

    let mut command_queue = CommandQueue::default();
    command_queue.push(move |world: &mut World| {
        world.insert_resource(NapcatIOSender(game_to_client_sender));
        world.remove_resource::<NapcatTask>();
    });
    let task_pool = IoTaskPool::get();
    let _ = task_pool.spawn(async move {
        loop {
            tokio::select! {
                //Receive messages from the websocket
                msg = ws_receiver.next() => {
                    match msg {
                        Some(msg) => {
                            let msg = msg.unwrap();
                            if msg.is_text() || msg.is_binary() {
                                client_to_game_sender.send(msg).expect("Could not send message");
                            } else if msg.is_close() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                //Receive messages from the game
                game_msg = game_to_client_receiver.recv() => {
                    let game_msg = game_msg.unwrap();
                    let _ = ws_sender.send(game_msg).await;
                }
            }
        }
    }).detach();

    command_queue
}

fn message_system(
    receiver: Res<NapcatIOReceiver>,
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
) {
    if let Ok(msg) = receiver.0.try_recv() {
        let json_res = serde_json::from_str::<NapcatMessage>(&msg.to_string());
        if let Ok(json) = json_res {
            dbg!(&json);
            let target_id = if json.data.user_id == json.data.self_id {
                json.data.target_id.unwrap()
            } else {
                json.data.user_id
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
            // dbg!(json_res.err());
        }
    }
}
