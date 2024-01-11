mod bevy_tokio;

use std::{
    net::TcpStream,
    time::Duration,
};

use bevy::{
    prelude::*,
    tasks::{
        block_on,
        AsyncComputeTaskPool,
        IoTaskPool,
        Task,
    },
    transform::commands,
};
use async_compat::Compat;
use bevy_tokio::{
    TaskContext,
    TokioTasksPlugin,
    TokioTasksRuntime,
};
use crossbeam_channel::{
    unbounded,
    Receiver as CBReceiver,
    Sender as CBSender,
};
use futures_util::{
    future,
    pin_mut,
    stream::{
        SplitSink,
        SplitStream,
    },
    SinkExt,
    StreamExt,
};
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
    WebSocketStream,
};
use tungstenite::{
    connect,
    stream::MaybeTlsStream,
    WebSocket,
};

#[derive(States, Debug, Default, Clone, Eq, PartialEq, Hash)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connected,
}

#[derive(Resource)]
struct MiraiIO {
    read: CBReceiver<Message>,
    write: CBSender<Message>,
}

#[derive(Resource)]
struct MiraiSocket(WebSocket<MaybeTlsStream<TcpStream>>);

// custom implementation for unusual values
impl Default for MiraiSocket {
    fn default() -> Self {
        let url = url::Url::parse("ws://localhost:5005/message").unwrap();
        let (socket, response) = connect(url).unwrap();

        println!("Connected to the server");
        println!(
            "Response HTTP code: {}",
            response.status()
        );
        println!("Response contains the following headers:");
        for (ref header, _value) in response.headers() {
            println!("* {}", header);
        }
        MiraiSocket(socket)
    }
}

pub struct MiraiPlugin;

impl Plugin for MiraiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(TokioTasksPlugin::default())
            .add_state::<ConnectionState>()
            // .insert_resource(MiraiSocket { ..default() })
            .add_systems(Startup, setup);
    }
}

fn setup(mut commands: Commands) {
    println!("start to setup");
    let task_pool = IoTaskPool::get();
    let (client_to_game_sender, client_to_game_receiver) = unbounded::<Message>();
    task_pool
        .spawn(Compat::new((hanlde_conneciton(
            client_to_game_sender.clone(),
        ))))
        .detach();
    commands.insert_resource(MiraiIO {
        read: client_to_game_receiver,
        write: client_to_game_sender,
    });
}

fn print_type_of<T>(_: &T) { println!("{}", std::any::type_name::<T>()) }

async fn hanlde_conneciton(client_to_game_sender: CBSender<Message>) -> Result<(), String> {
    let url = url::Url::parse("ws://localhost:5005/message").unwrap();
    println!("connected");
    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let (game_to_client_sender, mut game_to_client_receiver) = tokio::sync::mpsc::channel(100);

    loop {
        tokio::select! {
            //Receive messages from the websocket
            msg = ws_receiver.next() => {
                match msg {
                    Some(msg) => {
                        let msg = msg.unwrap();
                        println!("{}", msg);
                        if msg.is_text() ||msg.is_binary() {
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
                let _ = ws_sender.send(Message::Text(game_msg)).await;
            }

        }
    }
    Ok(())
}

fn read_socket(runtime: ResMut<TokioTasksRuntime>) { runtime.spawn_background_task(async_tokio); }

async fn async_read_socket(mut ctx: TaskContext) {
    ctx.run_on_main_thread(move |ctx| {
        let mut socket = ctx.world.get_resource_mut::<MiraiSocket>().unwrap();
        if socket.0.can_read() {
            let msg = socket.0.read().unwrap();
            if msg.is_text() {
                println!("received message {}", msg);
            }
        }
    })
    .await;
}

async fn async_tokio(mut ctx: TaskContext) {
    let url = url::Url::parse("ws://localhost:5005/message").unwrap();

    let (stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();

    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");

    let (write, read) = ws_stream.split();

    let stdin_to_ws = stdin_rx.map(Ok).forward(write);
    let ws_to_stdout = {
        read.for_each(|message| async {
            let data = message.unwrap();
            println!("{}", data);
        })
    };

    pin_mut!(stdin_to_ws, ws_to_stdout);
    future::select(stdin_to_ws, ws_to_stdout).await;
}
