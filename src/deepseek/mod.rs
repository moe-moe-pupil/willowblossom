use std::io::Read;

use async_compat::Compat;
use bevy::{
    app::{
        Plugin,
        Startup,
    },
    ecs::world::CommandQueue,
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
use curl::easy::{
    Easy,
    List,
};
use futures_lite::future;
use futures_util::{
    SinkExt,
    StreamExt,
};
use serde::Deserialize;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
};

pub struct DeepseekPlugin;

#[derive(Resource)]
struct DeepseekIOReceiver(CBReceiver<Message>);

#[derive(Resource)]
pub struct DeepseekIOSender(pub Sender<Message>);

#[derive(Resource)]
struct DeepseekTask(Task<CommandQueue>);

#[derive(Resource, Default)]
pub struct DeepseekManager {
    pub last_post_text: String,
    pub last_fim_response: String,
}

pub fn filter_control_characters(input: &str) -> String {
    input.chars()
      .filter(|&c| !c.is_control()) // Filter out control characters
      .collect() // Collect the remaining characters into a new String
}

impl DeepseekManager {
    pub fn post_fim(text: &str) -> String {
      return String::new();
        let preload_text = format!(
            r#"{{
        "model": "deepseek-chat",
        "prompt": "纯科幻太空世界，仅描述场景，不要续写任何人物故事，不要描述你的回答，比如，”下面是一段对你内容的续写：“，这种句式不被允许。必须记住仅描述场景。狂妄号是一艘退役的太空战列舰，装载有大量火炮，不过在星际战争结束后就被封存了，如今狂妄号仅保留了极其坚固的外壳和能量护盾。狂妄号船身较长，内部通道众多。有热熔炸弹的自动生成工厂，太空服自动售货机，以及全舰的监控和可以上锁的自动太空门。飞船上没有任何npc，飞船收到了女皇号的求救信号，解除了全舱的休眠。{}",
        "echo": false,
        "frequency_penalty": 0,
        "logprobs": 0,
        "max_tokens": 100,
        "presence_penalty": 0,
        "stop": null,
        "stream": false,
        "stream_options": null,
        "suffix": null,
        "temperature": 1.3,
        "top_p": 1
      }}"#,
            filter_control_characters(text)
        );

        dbg!(&preload_text);
        let mut data = preload_text.as_bytes();

        let mut easy = Easy::new();
        easy.url("https://api.deepseek.com/beta/completions")
            .unwrap();

        let mut list = List::new();
        list.append("Content-Type: application/json").unwrap();
        list.append("Accept: application/json").unwrap();
        list.append("Authorization: Bearer sk-04a95ae20ae24481a1908ba93be69de5")
            .unwrap();
        easy.http_headers(list).unwrap();
        easy.post(true).unwrap();
        easy.post_field_size(data.len() as u64).unwrap();

        // Perform the request and capture the response

        let mut dst = Vec::new();

        {
            let mut transfer = easy.transfer();
            transfer
                .read_function(|buf| Ok(data.read(buf).unwrap_or(0)))
                .unwrap();
            transfer
                .write_function(|data| {
                    dst.extend_from_slice(data);
                    Ok(data.len())
                })
                .unwrap();
            transfer.perform().unwrap();
        }

        let json_response = String::from_utf8(dst).unwrap();
        dbg!(&json_response);
        // Parse the JSON response
        let response: ApiResponse = serde_json::from_str(&json_response).unwrap();

        // Extract the text from the first choice
        let extracted_text = &response.choices[0].text;
        return extracted_text.to_string();
    }
}

impl Plugin for DeepseekPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_systems(Startup, setup)
            .add_systems(
                Update,
                handle_tasks.run_if(resource_exists::<DeepseekTask>),
            )
            .add_systems(Update, message_system);
    }
}

#[derive(Deserialize)]
struct ApiResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    text: String,
}

pub fn setup(mut commands: Commands) {
    commands.insert_resource(DeepseekManager::default());
    let thread_pool = AsyncComputeTaskPool::get();
    let (client_to_game_sender, client_to_game_receiver) = unbounded::<Message>();
    let napcat_io = DeepseekIOReceiver(client_to_game_receiver.clone());
    let task = thread_pool.spawn(Compat::new(handle_connection(
        client_to_game_sender.clone(),
    )));
    commands.insert_resource(napcat_io);
    commands.insert_resource(DeepseekTask(task));
}

fn handle_tasks(mut commands: Commands, mut task: ResMut<DeepseekTask>) {
    if let Some(mut commands_queue) = block_on(future::poll_once(&mut task.0)) {
        // append the returned command queue to have it execute later
        commands.append(&mut commands_queue);
    }
}

async fn handle_connection<'a>(client_to_game_sender: CBSender<Message>) -> CommandQueue {
    let (game_to_deepseek_sender, mut game_to_deepseek_receiver) = tokio::sync::mpsc::channel(100);

    let mut command_queue = CommandQueue::default();
    command_queue.push(move |world: &mut World| {
        world.insert_resource(DeepseekIOSender(
            game_to_deepseek_sender,
        ));
        world.remove_resource::<DeepseekTask>();
    });
    let task_pool = IoTaskPool::get();
    let _ = task_pool
        .spawn(async move {
            loop {
                tokio::select! {
                    //Receive messages from the game
                    game_msg = game_to_deepseek_receiver.recv() => {
                        let game_msg = game_msg.unwrap();
                        if let Message::Text(text) = game_msg {
                          client_to_game_sender.send(DeepseekManager::post_fim(&text).into()).expect("Could not send message");
                        }
                    }
                }
            }
        })
        .detach();

    command_queue
}

fn message_system(
    receiver: Res<DeepseekIOReceiver>,
    mut deepseek_manager: ResMut<DeepseekManager>,
) {
    if let Ok(msg) = receiver.0.try_recv() {
        deepseek_manager.last_fim_response = msg.to_string();
    }
}

#[test]
pub fn arrogance_ship() {
    let mut data = r#"{
  "model": "deepseek-chat",
  "prompt": "纯科幻太空世界，仅描述场景，不要续写任何人物故事，必须记住仅描述场景。狂妄号是一艘退役的太空战列舰，装载有大量火炮，不过在星际战争结束后就被封存了，如今狂妄号仅保留了极其坚固的外壳和能量护盾。狂妄号船身较长，内部通道众多。有热熔炸弹的自动生成工厂，太空服自动售货机，以及全舰的监控和可以上锁的自动太空门。飞船上没有任何npc，飞船收到了女皇号的求救信号，解除了全舱的休眠。你刚从休眠舱中醒来，你动了动还有些麻木的手脚，从休眠舱里起身，看见四周的休眠舱有几个早已打开，通向外侧的舱门也敞开着，你走了出去，通道上",
  "echo": false,
  "frequency_penalty": 0,
  "logprobs": 0,
  "max_tokens": 100,
  "presence_penalty": 0,
  "stop": null,
  "stream": false,
  "stream_options": null,
  "suffix": null,
  "temperature": 1.3,
  "top_p": 1
}"#
    .as_bytes();

    let mut easy = Easy::new();
    easy.url("https://api.deepseek.com/beta/completions")
        .unwrap();

    let mut list = List::new();
    list.append("Content-Type: application/json").unwrap();
    list.append("Accept: application/json").unwrap();
    list.append("Authorization: Bearer sk-04a95ae20ae24481a1908ba93be69de5")
        .unwrap();
    easy.http_headers(list).unwrap();
    easy.post(true).unwrap();
    easy.post_field_size(data.len() as u64).unwrap();

    // Perform the request and capture the response

    let mut dst = Vec::new();

    {
        let mut transfer = easy.transfer();
        transfer
            .read_function(|buf| Ok(data.read(buf).unwrap_or(0)))
            .unwrap();
        transfer
            .write_function(|data| {
                dst.extend_from_slice(data);
                Ok(data.len())
            })
            .unwrap();
        transfer.perform().unwrap();
    }

    let json_response = String::from_utf8(dst).unwrap();
    // Parse the JSON response
    let response: ApiResponse = serde_json::from_str(&json_response).unwrap();

    // Extract the text from the first choice
    let extracted_text = &response.choices[0].text;

    // Print the extracted text
    println!("Extracted Text: {}", extracted_text);
}
