use std::{
    collections::HashMap,
    env,
    fs,
    io::Read,
    path::Path,
};

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
};
use bevy_persistent::{
    Persistent,
    StorageFormat,
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
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::json;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::tungstenite::protocol::Message;

pub struct DeepseekPlugin;

#[derive(Resource)]
struct DeepseekIOReceiver(CBReceiver<Message>);

#[derive(Resource)]
pub struct DeepseekIOSender(pub Sender<Message>);

#[derive(Resource)]
struct DeepseekTask(Task<CommandQueue>);

#[derive(Resource, Default, Serialize, Deserialize)]
pub struct DeepseekManager {
    #[serde(default)]
    pub last_post_text: String,
    #[serde(default)]
    pub last_fim_response: String,
    #[serde(default)]
    pub summaries: HashMap<String, DeepseekSummary>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DeepseekSummary {
    #[serde(default)]
    pub blocks: Vec<DeepseekSummaryBlock>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DeepseekSummaryBlock {
    #[serde(default)]
    pub latest: String,
    #[serde(default)]
    pub message_count: usize,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub error: Option<String>,
}

impl DeepseekSummary {
    pub fn upsert_block(&mut self, block: DeepseekSummaryBlock) {
        if let Some(existing) = self
            .blocks
            .iter_mut()
            .find(|existing| existing.message_count == block.message_count)
        {
            *existing = block;
        } else {
            self.blocks.push(block);
            self.blocks.sort_by_key(|block| block.message_count);
        }
    }
}

const SUMMARY_SYSTEM_PROMPT: &str = "\
你是TRPG聊天记录整理器，只整理输入中已经明确发生或明确说过的内容。
禁止解释你的任务，禁止提到“聊天记录”“上下文”“我会”“总结如下”等元话语。
禁止推测、创作剧情、补全动机、决定行动结果、扮演旁白或NPC。
如果输入是测试、工具反馈或闲聊，也要客观整理玩家明确说出的事实、问题和待处理事项。
只有输入中完全没有可整理内容时，才允许三行都写“无”。
输出必须短，使用下面三行格式；没有对应内容就写“无”：
事件：...
决定/线索：...
待跟进：...";

const DEEPSEEK_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";

pub fn filter_control_characters(input: &str) -> String {
    input.chars()
      .filter(|&c| !c.is_control()) // Filter out control characters
      .collect() // Collect the remaining characters into a new String
}

fn deepseek_api_key() -> Result<String, String> {
    env::var(DEEPSEEK_API_KEY_ENV)
        .ok()
        .filter(|key| !key.trim().is_empty())
        .or_else(|| env_file_value(DEEPSEEK_API_KEY_ENV))
        .ok_or_else(|| format!("{DEEPSEEK_API_KEY_ENV} is not set"))
}

fn env_file_value(key: &str) -> Option<String> {
    let env_file = fs::read_to_string(".env").ok()?;
    for line in env_file.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }

        let value = value.trim().trim_matches('"').trim_matches('\'');
        if value.is_empty() {
            return None;
        }
        return Some(value.to_owned());
    }

    None
}

fn deepseek_authorization_header() -> Result<String, String> {
    Ok(format!(
        "Authorization: Bearer {}",
        deepseek_api_key()?
    ))
}

impl DeepseekManager {
    fn post_completion(
        prompt: &str,
        suffix: Option<&str>,
        max_tokens: u32,
    ) -> Result<String, String> {
        let payload = json!({
            "model": "deepseek-chat",
            "prompt": filter_control_characters(prompt),
            "echo": false,
            "frequency_penalty": 0,
            "logprobs": 0,
            "max_tokens": max_tokens,
            "presence_penalty": 0,
            "stop": null,
            "stream": false,
            "stream_options": null,
            "suffix": suffix.map(filter_control_characters),
            "temperature": 1.3,
            "top_p": 1
        })
        .to_string();

        let mut data = payload.as_bytes();

        let mut easy = Easy::new();
        easy.url("https://api.deepseek.com/beta/completions")
            .map_err(|err| err.to_string())?;

        let mut list = List::new();
        list.append("Content-Type: application/json")
            .map_err(|err| err.to_string())?;
        list.append("Accept: application/json")
            .map_err(|err| err.to_string())?;
        list.append(&deepseek_authorization_header()?)
            .map_err(|err| err.to_string())?;
        easy.http_headers(list).map_err(|err| err.to_string())?;
        easy.post(true).map_err(|err| err.to_string())?;
        easy.post_field_size(data.len() as u64)
            .map_err(|err| err.to_string())?;

        let mut dst = Vec::new();

        {
            let mut transfer = easy.transfer();
            transfer
                .read_function(|buf| Ok(data.read(buf).unwrap_or(0)))
                .map_err(|err| err.to_string())?;
            transfer
                .write_function(|data| {
                    dst.extend_from_slice(data);
                    Ok(data.len())
                })
                .map_err(|err| err.to_string())?;
            transfer.perform().map_err(|err| err.to_string())?;
        }

        let json_response = String::from_utf8(dst).map_err(|err| err.to_string())?;
        let response: ApiResponse =
            serde_json::from_str(&json_response).map_err(|err| err.to_string())?;

        response
            .choices
            .first()
            .map(|choice| choice.text.to_string())
            .ok_or_else(|| "DeepSeek response did not include choices".to_owned())
    }

    fn post_chat_completion(
        system_prompt: &str,
        user_text: &str,
        max_tokens: u32,
    ) -> Result<String, String> {
        let payload = json!({
            "model": "deepseek-v4-flash",
            "messages": [
                {
                    "role": "system",
                    "content": filter_control_characters(system_prompt),
                },
                {
                    "role": "user",
                    "content": filter_control_characters(user_text),
                },
            ],
            "thinking": {
                "type": "disabled",
            },
            "frequency_penalty": 0,
            "max_tokens": max_tokens,
            "presence_penalty": 0,
            "stream": false,
            "temperature": 0.2,
            "top_p": 1
        })
        .to_string();

        let mut data = payload.as_bytes();

        let mut easy = Easy::new();
        easy.url("https://api.deepseek.com/chat/completions")
            .map_err(|err| err.to_string())?;

        let mut list = List::new();
        list.append("Content-Type: application/json")
            .map_err(|err| err.to_string())?;
        list.append("Accept: application/json")
            .map_err(|err| err.to_string())?;
        list.append(&deepseek_authorization_header()?)
            .map_err(|err| err.to_string())?;
        easy.http_headers(list).map_err(|err| err.to_string())?;
        easy.post(true).map_err(|err| err.to_string())?;
        easy.post_field_size(data.len() as u64)
            .map_err(|err| err.to_string())?;

        let mut dst = Vec::new();

        {
            let mut transfer = easy.transfer();
            transfer
                .read_function(|buf| Ok(data.read(buf).unwrap_or(0)))
                .map_err(|err| err.to_string())?;
            transfer
                .write_function(|data| {
                    dst.extend_from_slice(data);
                    Ok(data.len())
                })
                .map_err(|err| err.to_string())?;
            transfer.perform().map_err(|err| err.to_string())?;
        }

        let json_response = String::from_utf8(dst).map_err(|err| err.to_string())?;
        let response: ChatApiResponse =
            serde_json::from_str(&json_response).map_err(|err| err.to_string())?;

        response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
            .ok_or_else(|| "DeepSeek response did not include message content".to_owned())
    }

    pub fn post_fim(text: &str, suffix: &str) -> String {
        let prompt = format!(
            "纯科幻太空世界，仅描述场景，不要续写任何人物故事，不要描述你的回答，比如，”下面是一段对你内容的续写：“，这种句式不被允许。必须记住仅描述场景。狂妄号是一艘退役的太空战列舰，装载有大量火炮，不过在星际战争结束后就被封存了，如今狂妄号仅保留了极其坚固的外壳和能量护盾。狂妄号船身较长，内部通道众多。有热熔炸弹的自动生成工厂，太空服自动售货机，以及全舰的监控和可以上锁的自动太空门。飞船上没有任何npc，飞船收到了女皇号的求救信号，解除了全舱的休眠。{}",
            text
        );
        Self::post_completion(&prompt, Some(suffix), 20).unwrap_or_default()
    }

    fn post_summary(text: &str) -> Result<String, String> {
        let user_text = format!("请整理最近这些玩家发言：\n{}", text);
        Self::post_chat_completion(SUMMARY_SYSTEM_PROMPT, &user_text, 120)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeepseekRequest {
    Summary {
        target_id: String,
        message_count: usize,
        text: String,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DeepseekResponse {
    Summary {
        target_id: String,
        message_count: usize,
        text: String,
    },
    Error {
        target_id: String,
        message_count: usize,
        text: String,
    },
    Fim {
        text: String,
    },
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

#[derive(Deserialize)]
struct ChatApiResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

pub fn setup(mut commands: Commands) {
    let config_dir = Path::new(".data").join("willowblossom");
    commands.insert_resource(
        Persistent::<DeepseekManager>::builder()
            .name("deepseek_summaries")
            .format(StorageFormat::Toml)
            .path(config_dir.join("deepseek_summaries.toml"))
            .default(DeepseekManager::default())
            .build()
            .expect("failed to init DeepSeek summaries"),
    );
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
                        let Some(game_msg) = game_msg else {
                            break;
                        };
                        if let Message::Text(text) = game_msg {
                            if let Ok(request) = serde_json::from_str::<DeepseekRequest>(&text) {
                                match request {
                                    DeepseekRequest::Summary {
                                        target_id,
                                        message_count,
                                        text,
                                    } => {
                                        let response = match DeepseekManager::post_summary(&text) {
                                            Ok(text) => DeepseekResponse::Summary {
                                                target_id,
                                                message_count,
                                                text,
                                            },
                                            Err(text) => DeepseekResponse::Error {
                                                target_id,
                                                message_count,
                                                text,
                                            },
                                        };
                                        let response = serde_json::to_string(&response)
                                            .expect("failed to serialize DeepSeek response");
                                        client_to_game_sender
                                            .send(response.into())
                                            .expect("Could not send message");
                                    },
                                }
                            } else if let Some((prefix, suffix)) = text.split_once('|') {
                                let response = DeepseekResponse::Fim {
                                    text: DeepseekManager::post_fim(prefix, suffix),
                                };
                                let response = serde_json::to_string(&response)
                                    .expect("failed to serialize DeepSeek response");
                                client_to_game_sender
                                    .send(response.into())
                                    .expect("Could not send message");
                            }
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
    mut deepseek_manager: ResMut<Persistent<DeepseekManager>>,
) {
    let mut changed = false;
    while let Ok(msg) = receiver.0.try_recv() {
        let text = msg.to_string();
        match serde_json::from_str::<DeepseekResponse>(&text) {
            Ok(DeepseekResponse::Summary {
                target_id,
                message_count,
                text,
            }) => {
                deepseek_manager
                    .summaries
                    .entry(target_id)
                    .or_default()
                    .upsert_block(DeepseekSummaryBlock {
                        latest: text,
                        message_count,
                        pending: false,
                        error: None,
                    });
                changed = true;
            },
            Ok(DeepseekResponse::Error {
                target_id,
                message_count,
                text,
            }) => {
                deepseek_manager
                    .summaries
                    .entry(target_id)
                    .or_default()
                    .upsert_block(DeepseekSummaryBlock {
                        latest: String::new(),
                        message_count,
                        pending: false,
                        error: Some(text),
                    });
                changed = true;
            },
            Ok(DeepseekResponse::Fim { text }) => {
                deepseek_manager.last_fim_response = text;
                changed = true;
            },
            Err(_) => {
                deepseek_manager.last_fim_response = text;
                changed = true;
            },
        }
    }

    if changed {
        if let Err(err) = deepseek_manager.persist() {
            eprintln!("failed to persist DeepSeek summaries: {err}");
        }
    }
}

#[test]
#[ignore = "calls the live DeepSeek API"]
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
    list.append(&deepseek_authorization_header().unwrap())
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
