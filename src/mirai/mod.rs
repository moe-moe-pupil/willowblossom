use std::net::TcpStream;

use bevy::prelude::*;
use tungstenite::{
    connect,
    stream::MaybeTlsStream,
    Message,
    WebSocket,
};

#[derive(States, Debug, Default, Clone, Eq, PartialEq, Hash)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connected,
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
        app.add_state::<ConnectionState>();
        app.insert_resource(MiraiSocket { ..default() });
        app.add_systems(Update, read_socket);
    }
}

fn read_socket(mut socket: ResMut<MiraiSocket>) {
    if socket.0.can_write() {
        let msg = socket.0.read().unwrap();
        if msg.is_text() {
            println!("received message {}", msg);
        }
    }
}
