use std::path::Path;

use async_compat::Compat;
use bevy_persistent::prelude::*;
extern crate dirs;
use bevy::{
    ecs::system::CommandQueue,
    prelude::*,
    tasks::{
        block_on,
        AsyncComputeTaskPool,
        IoTaskPool,
        Task,
    },
    utils::hashbrown::HashMap,
};
use crossbeam_channel::{
    unbounded,
    Receiver as CBReceiver,
    Sender as CBSender,
};
use dirs::state_dir;
use futures_lite::future;
use futures_util::{
    SinkExt,
    StreamExt,
};
use serde::{
    Deserialize,
    Serialize,
};
use tokio::sync::mpsc::Sender;
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
struct MiraiIOReceiver(CBReceiver<Message>);

#[derive(Resource)]
pub struct MiraiIOSender(pub Sender<Message>);

#[derive(Resource)]
struct MiraiTask(Task<CommandQueue>);

pub struct MiraiPlugin;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiraiMessage {
    pub(crate) sync_id: String,
    pub data: MiraiMessageData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Plain {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Image {
    pub image: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Source {
    id: u64,
    time: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MiraiMessageChainType {
    Source(Source),
    Plain(Plain),
    Image(Image),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum MiraiMessageType {
    FriendMessage,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiraiMessageChain {
    #[serde(flatten)]
    pub variant: MiraiMessageChainType,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiraiSender {
    pub id: u64,
    pub nickname: String,
    pub remark: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiraiMessageData {
    pub r#type: MiraiMessageType,
    pub message_chain: Vec<MiraiMessageChain>,
    pub sender: MiraiSender,
}

#[derive(Resource, Serialize, Deserialize)]
pub struct MiraiMessageManager {
    pub messages: HashMap<String, Vec<MiraiMessage>>,
}

impl Plugin for MiraiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_state(ConnectionState::Disconnected)
            // .insert_resource(MiraiSocket { ..default() })
            .add_systems(Startup, setup)
            .add_systems(Update, handle_tasks.run_if(resource_exists::<MiraiTask>))
            .add_systems(Update, (send_message.run_if(resource_exists::<MiraiIOSender>), message_system));
    }
}

fn setup(mut commands: Commands) {
    println!("start to setup");
    let thread_pool = AsyncComputeTaskPool::get();
    let (client_to_game_sender, client_to_game_receiver) = unbounded::<Message>();
    let mirai_io = MiraiIOReceiver(client_to_game_receiver.clone());
    let task = thread_pool.spawn(Compat::new(handle_connection(
        client_to_game_sender.clone(),
    )));
    let message_manager = MiraiMessageManager {
        messages: HashMap::default(),
    };
    let config_dir = Path::new(".data").join("willowblossom");
    commands.insert_resource(
        Persistent::<MiraiMessageManager>::builder()
            .name("messages")
            .format(StorageFormat::Toml)
            .path(config_dir.join("messages.toml"))
            .default(message_manager)
            .build()
            .expect("failed to init messages"),
    );
    commands.insert_resource(mirai_io);
    commands.insert_resource(MiraiTask(task));
}

fn handle_tasks(mut commands: Commands, mut task: ResMut<MiraiTask>) {
    if let Some(mut commands_queue) = block_on(future::poll_once(&mut task.0)) {
        // append the returned command queue to have it execute later
        commands.append(&mut commands_queue);
    }
}

async fn handle_connection<'a>(client_to_game_sender: CBSender<Message>) -> CommandQueue {
    let url = url::Url::parse("ws://localhost:5005/message").unwrap();

    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let (game_to_client_sender, mut game_to_client_receiver) = tokio::sync::mpsc::channel(100);

    let mut command_queue = CommandQueue::default();
    command_queue.push(move |world: &mut World| {
        println!("work!!!");
        world.insert_resource(MiraiIOSender(game_to_client_sender));
        world.remove_resource::<MiraiTask>();
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

fn send_message(buttons: Res<ButtonInput<MouseButton>>, sender: Res<MiraiIOSender>) {
    if buttons.just_pressed(MouseButton::Left) {
        // Left button was pressed
        // let err = sender
        //     .0
        //     .try_send(Message::Text(
        //         (r#"{"syncId":123,"command":"sendFriendMessage","subCommand":null,"content":{"
        // target":1670426821,"messageChain":[{"type":"Plain","text":"你好~"}]}}"#)
        //         .to_string(),
        //     ))
        //     .expect("can't send message");
    }
}

fn message_system(
    receiver: Res<MiraiIOReceiver>,
    mut manager: ResMut<Persistent<MiraiMessageManager>>,
) {
    if let Ok(msg) = receiver.0.try_recv() {
        println!("msg => {:?}", msg);
        if let Ok(json) = serde_json::from_str::<MiraiMessage>(&msg.to_string()) {
            println!("json => {:?}", json);
            if manager
                .messages
                .contains_key(&json.data.sender.id.to_string())
            {
                manager
                    .messages
                    .get_mut(&json.data.sender.id.to_string())
                    .unwrap()
                    .push(json)
            } else {
                manager
                    .messages
                    .insert(json.data.sender.id.to_string(), vec![
                        json,
                    ]);
            }
            manager.persist().ok();
        }
    }
}
