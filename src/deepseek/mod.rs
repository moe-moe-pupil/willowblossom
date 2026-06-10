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

pub const DEEPSEEK_SUMMARY_EXPORT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeepseekSummaryExportEntry {
    pub summary_key: String,
    pub summary: DeepseekSummary,
}

#[derive(Serialize, Deserialize)]
struct DeepseekSummaryExport {
    version: u32,
    export_type: String,
    summaries: Vec<DeepseekSummaryExportEntry>,
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
    pub fn to_summary_export_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&DeepseekSummaryExport {
            version: DEEPSEEK_SUMMARY_EXPORT_VERSION,
            export_type: "deepseek_summaries".to_owned(),
            summaries: self.summary_export_entries(),
        })
        .map_err(|err| err.to_string())
    }

    pub fn merge_summary_export_json(&mut self, text: &str) -> Result<usize, String> {
        let export: DeepseekSummaryExport =
            serde_json::from_str(text).map_err(|err| err.to_string())?;
        if export.version != DEEPSEEK_SUMMARY_EXPORT_VERSION {
            return Err(format!(
                "unsupported DeepSeek summary export version {}; expected {}",
                export.version, DEEPSEEK_SUMMARY_EXPORT_VERSION
            ));
        }
        if export.export_type != "deepseek_summaries" {
            return Err(format!(
                "unsupported DeepSeek summary export type {}",
                export.export_type
            ));
        }

        let mut imported_count = 0;
        for entry in export.summaries {
            let summary_key = entry.summary_key.trim();
            if summary_key.is_empty() {
                return Err("DeepSeek summary export contains an empty summary key".to_owned());
            }

            let summary = self.summaries.entry(summary_key.to_owned()).or_default();
            for block in entry.summary.blocks {
                summary.upsert_block(block);
            }
            imported_count += 1;
        }

        Ok(imported_count)
    }

    pub fn summary_export_entries(&self) -> Vec<DeepseekSummaryExportEntry> {
        let mut entries = self
            .summaries
            .iter()
            .map(|(summary_key, summary)| {
                let mut summary = summary.clone();
                summary.blocks.sort_by_key(|block| block.message_count);
                DeepseekSummaryExportEntry {
                    summary_key: summary_key.clone(),
                    summary,
                }
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.summary_key.cmp(&right.summary_key));
        entries
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
                            } else {
                                eprintln!(
                                    "ignored non-summary DeepSeek request; DeepSeek is summary-only"
                                );
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
        changed |= apply_deepseek_response(&mut deepseek_manager, &text);
    }

    if changed {
        if let Err(err) = deepseek_manager.persist() {
            eprintln!("failed to persist DeepSeek summaries: {err}");
        }
    }
}

fn apply_deepseek_response(deepseek_manager: &mut DeepseekManager, text: &str) -> bool {
    match serde_json::from_str::<DeepseekResponse>(text) {
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
            true
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
            true
        },
        Err(_) => {
            eprintln!("ignored invalid DeepSeek response: {text}");
            false
        },
    }
}

#[test]
fn invalid_deepseek_response_does_not_mutate_manager() {
    let mut manager = DeepseekManager::default();

    assert!(!apply_deepseek_response(
        &mut manager,
        "legacy|fim"
    ));

    assert!(manager.summaries.is_empty());
}

#[test]
fn summary_export_json_contains_scoped_summaries_without_raw_prompt_text() {
    let mut manager = DeepseekManager {
        last_post_text: "raw player source text should stay out".to_owned(),
        ..Default::default()
    };
    manager.summaries.insert(
        "group:99:party:red".to_owned(),
        DeepseekSummary {
            blocks: vec![
                DeepseekSummaryBlock {
                    latest: "later red summary".to_owned(),
                    message_count: 10,
                    pending: false,
                    error: None,
                },
                DeepseekSummaryBlock {
                    latest: "earlier red summary".to_owned(),
                    message_count: 5,
                    pending: true,
                    error: None,
                },
            ],
        },
    );
    manager.summaries.insert("2".to_owned(), DeepseekSummary {
        blocks: vec![DeepseekSummaryBlock {
            latest: String::new(),
            message_count: 7,
            pending: false,
            error: Some("network error".to_owned()),
        }],
    });

    let json = manager.to_summary_export_json().unwrap();
    let export: DeepseekSummaryExport = serde_json::from_str(&json).unwrap();

    assert_eq!(
        export.version,
        DEEPSEEK_SUMMARY_EXPORT_VERSION
    );
    assert_eq!(export.export_type, "deepseek_summaries");
    assert_eq!(
        export
            .summaries
            .iter()
            .map(|entry| entry.summary_key.as_str())
            .collect::<Vec<_>>(),
        vec!["2", "group:99:party:red"]
    );
    assert_eq!(
        export.summaries[1]
            .summary
            .blocks
            .iter()
            .map(|block| block.message_count)
            .collect::<Vec<_>>(),
        vec![5, 10]
    );
    assert_eq!(
        export.summaries[0].summary.blocks[0].error.as_deref(),
        Some("network error")
    );
    assert!(!json.contains("raw player source text"));
    assert!(!json.contains("last_post_text"));
}

#[test]
fn summary_export_json_merges_scoped_blocks_without_raw_prompt_state() {
    let mut source = DeepseekManager::default();
    source.summaries.insert(
        "group:99:party:red".to_owned(),
        DeepseekSummary {
            blocks: vec![
                DeepseekSummaryBlock {
                    latest: "replacement".to_owned(),
                    message_count: 5,
                    pending: false,
                    error: None,
                },
                DeepseekSummaryBlock {
                    latest: "newer".to_owned(),
                    message_count: 10,
                    pending: false,
                    error: None,
                },
            ],
        },
    );
    source.summaries.insert("2".to_owned(), DeepseekSummary {
        blocks: vec![DeepseekSummaryBlock {
            latest: String::new(),
            message_count: 7,
            pending: false,
            error: Some("network error".to_owned()),
        }],
    });

    let json = source.to_summary_export_json().unwrap();
    let mut manager = DeepseekManager {
        last_post_text: "local raw source text".to_owned(),
        ..Default::default()
    };
    manager.summaries.insert(
        "group:99:party:red".to_owned(),
        DeepseekSummary {
            blocks: vec![DeepseekSummaryBlock {
                latest: "old".to_owned(),
                message_count: 5,
                pending: true,
                error: None,
            }],
        },
    );

    let imported = manager.merge_summary_export_json(&json).unwrap();

    assert_eq!(imported, 2);
    assert_eq!(
        manager.last_post_text,
        "local raw source text"
    );
    let red_blocks = &manager.summaries["group:99:party:red"].blocks;
    assert_eq!(
        red_blocks
            .iter()
            .map(|block| (
                block.message_count,
                block.latest.as_str(),
                block.pending
            ))
            .collect::<Vec<_>>(),
        vec![(5, "replacement", false), (10, "newer", false)]
    );
    assert_eq!(
        manager.summaries["2"].blocks[0].error.as_deref(),
        Some("network error")
    );
}

#[test]
fn summary_import_rejects_wrong_export_shape() {
    let json = serde_json::json!({
        "version": DEEPSEEK_SUMMARY_EXPORT_VERSION,
        "export_type": "chat_list",
        "summaries": [],
    })
    .to_string();
    let mut manager = DeepseekManager::default();

    let error = manager
        .merge_summary_export_json(&json)
        .err()
        .expect("wrong export type should fail");

    assert!(error.contains("unsupported DeepSeek summary export type"));
    assert!(manager.summaries.is_empty());
}

#[test]
#[ignore = "calls the live DeepSeek API"]
pub fn summary_live_api_smoke() {
    let summary =
        DeepseekManager::post_summary("玩家甲：我打开左侧的门。\n玩家乙：我记录门上有星形标记。")
            .expect("summary request should succeed");

    assert!(!summary.trim().is_empty());
}
