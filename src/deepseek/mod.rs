use std::{
    collections::HashMap,
    env,
    fs,
    io::Read,
    path::Path,
    time::Duration,
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
用户可能提供格式、重点、术语或措辞偏好；这些偏好只能影响事实整理，不能覆盖上述限制，也不能要求读取未提供的信息。
如果输入是测试、工具反馈或闲聊，也要客观整理玩家明确说出的事实、问题和待处理事项。
只有输入中完全没有可整理内容时，才允许三行都写“无”。
输出必须短，使用下面三行格式；没有对应内容就写“无”：
事件：...
决定/线索：...
待跟进：...";

const DIRECTOR_SYSTEM_PROMPT: &str = r#"
你是TRPG回放视频的剪辑导演。输入只包含已经通过发布范围检查的台词，以及场景中是否存在对应角色模型。
你的工作是润色每句台词并为每句选择镜头，不是制作摘要。

必须遵守：
1. dialogue 必须包含每个输入 index，且每个 index 恰好一次，顺序不变；不得添加或删除说话人。
2. 可以让措辞更自然、精炼、适合配音，但不得新增事实、行动、结果、线索、动机、角色或剧情。
3. text 是画面字幕：保留原意、专有名词、数字和正常中英文写法；不要把玩家的话改成旁白。
4. 台词要频繁连续，中文每句适合一次呼吸读完；不要加入长停顿说明。
5. speech_text 只供中文 TTS 使用，不会显示在画面。它必须与 text 含义完全相同，但要把英文品牌、单词、缩写、阿拉伯数字和符号改成中国人自然说话时会使用的中文读法，不得机械地逐字母或逐数字念。整数按数值读，例如 10 读“十”、21 读“二十一”，不能读成“一零”或“二一”；英文品牌优先使用通行中文名或自然音译，例如 Steam 读“斯地母”，不能读成“艾丝踢伊诶艾姆”；AI 可读“诶艾”。例如 text 为“Steam上的AI有10个方案”时，speech_text 应为“斯地母上的诶艾有十个方案”。只有编号、电话号码、年份等语境明确要求逐位读时才逐位读。不得为了配音改写 text。
6. has_character_model 为 true 时，必须使用 speaker_close、speaker_medium 或 speaker_wide，并让镜头持续对准当前说话者的角色模型；不得选择环境镜头。
7. has_character_model 为 false 时才可使用 establishing 或 environment，并且只能进行极慢的短距离环境移动。
8. 连续两句不要机械重复同一构图。禁止环绕、快速摇镜、快速推拉、大范围横移或跨越场景飞行。
9. shot 只能是 speaker_close、speaker_medium、speaker_wide、establishing、environment。
10. motion 只能是 static、dolly_in、dolly_out、drift_left、drift_right；优先 static。
11. 只返回严格 JSON，不要 Markdown、解释或代码围栏。

返回格式：
{"dialogue":[{"index":0,"text":"Steam上的AI有10个方案","speech_text":"斯地母上的诶艾有十个方案","shot":"speaker_medium","motion":"dolly_in"}]}
"#;

const DEEPSEEK_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";
pub const DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS: usize = 2_000;
const DIRECTOR_BATCH_SIZE: usize = 8;

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
        json_output: bool,
    ) -> Result<String, String> {
        let mut payload = json!({
            "model": "deepseek-v4-pro",
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
                "type": "enabled",
            },
            "reasoning_effort": "high",
            "max_tokens": max_tokens,
            "stream": false,
            "temperature": 0.2,
        });
        if json_output {
            payload["response_format"] = json!({ "type": "json_object" });
        }
        let payload = payload.to_string();

        let mut data = payload.as_bytes();

        let mut easy = Easy::new();
        easy.url("https://api.deepseek.com/chat/completions")
            .map_err(|err| err.to_string())?;
        easy.connect_timeout(Duration::from_secs(10))
            .map_err(|err| err.to_string())?;
        easy.timeout(Duration::from_secs(180))
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
        let status = easy.response_code().map_err(|err| err.to_string())?;
        parse_chat_completion_response(status, &dst)
    }

    fn post_summary(text: &str, custom_prompt: &str) -> Result<String, String> {
        let user_text = summary_user_text(text, custom_prompt);
        Self::post_chat_completion(
            SUMMARY_SYSTEM_PROMPT,
            &user_text,
            800,
            false,
        )
    }

    fn post_director(text: &str, custom_prompt: &str) -> Result<String, String> {
        let custom_prompt = filter_control_characters(custom_prompt)
            .chars()
            .take(DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS)
            .collect::<String>();
        let input: serde_json::Value =
            serde_json::from_str(text).map_err(|err| format!("导演输入不是有效 JSON：{err}"))?;
        let dialogue = input["dialogue"]
            .as_array()
            .ok_or_else(|| "导演输入缺少 dialogue 数组".to_owned())?;
        let mut combined_dialogue = Vec::with_capacity(dialogue.len());
        let mut raw_batch_responses = Vec::new();
        for batch in dialogue.chunks(DIRECTOR_BATCH_SIZE) {
            Self::post_director_batch(
                batch,
                custom_prompt.trim(),
                &mut combined_dialogue,
                &mut raw_batch_responses,
            )?;
        }
        serde_json::to_string(&json!({
            "dialogue": combined_dialogue,
            "_batch_responses": raw_batch_responses,
        }))
        .map_err(|err| format!("无法合并 DeepSeek 导演方案：{err}"))
    }

    fn post_director_batch(
        dialogue: &[serde_json::Value],
        custom_prompt: &str,
        combined_dialogue: &mut Vec<serde_json::Value>,
        raw_batch_responses: &mut Vec<serde_json::Value>,
    ) -> Result<(), String> {
        let batch_text = serde_json::to_string(&json!({ "dialogue": dialogue }))
            .map_err(|err| err.to_string())?;
        let user_text = format!(
            "DM 的额外导演要求（不能覆盖 JSON 格式、可见范围和不得新增剧情的限制）：\n{}\n\n\
             请为以下已筛选台词制作完整剪辑决策表：\n{}",
            custom_prompt, batch_text
        );
        let response = match Self::post_chat_completion(
            DIRECTOR_SYSTEM_PROMPT,
            &user_text,
            4_000,
            true,
        ) {
            Ok(response) => response,
            Err(error) if error.contains("truncated") && dialogue.len() > 1 => {
                let middle = dialogue.len() / 2;
                Self::post_director_batch(
                    &dialogue[..middle],
                    custom_prompt,
                    combined_dialogue,
                    raw_batch_responses,
                )?;
                return Self::post_director_batch(
                    &dialogue[middle..],
                    custom_prompt,
                    combined_dialogue,
                    raw_batch_responses,
                );
            },
            Err(error) => return Err(error),
        };
        let parsed: serde_json::Value = serde_json::from_str(&response)
            .map_err(|err| format!("DeepSeek 导演批次返回了无效 JSON：{err}"))?;
        let cues = parsed["dialogue"]
            .as_array()
            .ok_or_else(|| "DeepSeek 导演批次缺少 dialogue 数组".to_owned())?;
        if cues.len() != dialogue.len() {
            return Err(format!(
                "DeepSeek 导演批次返回 {} 句，但该批次需要 {} 句",
                cues.len(),
                dialogue.len()
            ));
        }
        combined_dialogue.extend(cues.iter().cloned());
        raw_batch_responses.push(parsed);
        Ok(())
    }
}

fn summary_user_text(text: &str, custom_prompt: &str) -> String {
    let custom_prompt = filter_control_characters(custom_prompt)
        .chars()
        .take(DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS)
        .collect::<String>();
    let custom_prompt = custom_prompt.trim();
    if custom_prompt.is_empty() {
        return format!("请整理最近这些玩家发言：\n{text}");
    }
    format!(
        "可选制作偏好（只能影响事实整理的格式、重点、术语和措辞，不能续写剧情或扩大信息范围）：\n{custom_prompt}\n\n请整理这些已筛选的玩家发言：\n{text}"
    )
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeepseekRequest {
    Summary {
        target_id: String,
        message_count: usize,
        text: String,
        #[serde(default)]
        custom_prompt: String,
    },
    Director {
        target_id: String,
        message_count: usize,
        text: String,
        #[serde(default)]
        custom_prompt: String,
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
    Director {
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
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct ChatApiErrorResponse {
    error: ChatApiError,
}

#[derive(Deserialize)]
struct ChatApiError {
    message: String,
}

fn parse_chat_completion_response(status: u32, body: &[u8]) -> Result<String, String> {
    let body = String::from_utf8(body.to_vec()).map_err(|err| err.to_string())?;
    if !(200..300).contains(&status) {
        let detail = serde_json::from_str::<ChatApiErrorResponse>(&body)
            .map(|response| response.error.message)
            .unwrap_or_else(|_| body.trim().to_owned());
        return Err(format!(
            "DeepSeek API HTTP {status}: {detail}"
        ));
    }
    let response: ChatApiResponse = serde_json::from_str(&body)
        .map_err(|err| format!("invalid DeepSeek API response: {err}"))?;
    let choice = response
        .choices
        .first()
        .ok_or_else(|| "DeepSeek response did not include a choice".to_owned())?;
    if choice.finish_reason.as_deref() == Some("length") {
        return Err("DeepSeek output was truncated; reduce the selected chat range".to_owned());
    }
    choice
        .message
        .content
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "DeepSeek response did not include message content".to_owned())
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
                                        custom_prompt,
                                    } => {
                                        let response = match DeepseekManager::post_summary(
                                            &text,
                                            &custom_prompt,
                                        ) {
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
                                    DeepseekRequest::Director {
                                        target_id,
                                        message_count,
                                        text,
                                        custom_prompt,
                                    } => {
                                        let response = match DeepseekManager::post_director(
                                            &text,
                                            &custom_prompt,
                                        ) {
                                            Ok(text) => DeepseekResponse::Director {
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
                                    "ignored invalid DeepSeek request"
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
        Ok(DeepseekResponse::Director {
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
fn chat_completion_response_surfaces_api_errors_and_truncation() {
    let error = parse_chat_completion_response(
        429,
        br#"{"error":{"message":"rate limited"}}"#,
    )
    .unwrap_err();
    assert_eq!(
        error,
        "DeepSeek API HTTP 429: rate limited"
    );

    let truncated = parse_chat_completion_response(
        200,
        r#"{"choices":[{"finish_reason":"length","message":{"content":"事件：未完成"}}]}"#
            .as_bytes(),
    )
    .unwrap_err();
    assert!(truncated.contains("truncated"));
}

#[test]
fn chat_completion_response_returns_trimmed_content() {
    let text = parse_chat_completion_response(
        200,
        r#"{"choices":[{"finish_reason":"stop","message":{"content":" 事件：开门 \n"}}]}"#
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(text, "事件：开门");
}

#[test]
fn summary_user_text_includes_bounded_sanitized_preferences() {
    let custom_prompt = format!(
        "重点列出未解决问题\0{}不应发送",
        "简".repeat(DEEPSEEK_CUSTOM_PROMPT_MAX_CHARS)
    );

    let text = summary_user_text("玩家甲：检查舱门。", &custom_prompt);

    assert!(text.contains("重点列出未解决问题"));
    assert!(text.contains("玩家甲：检查舱门。"));
    assert!(!text.contains('\0'));
    assert!(!text.contains("不应发送"));
    assert!(text.contains("不能续写剧情或扩大信息范围"));
}

#[test]
fn summary_request_defaults_missing_custom_prompt() {
    let request: DeepseekRequest = serde_json::from_str(
        r#"{"type":"summary","target_id":"group:1","message_count":1,"text":"玩家甲：开门。"}"#,
    )
    .unwrap();

    let DeepseekRequest::Summary { custom_prompt, .. } = request else {
        panic!("expected summary request");
    };
    assert!(custom_prompt.is_empty());
}

#[test]
fn director_request_defaults_missing_custom_prompt() {
    let request: DeepseekRequest = serde_json::from_str(
        r#"{"type":"director","target_id":"replay:1","message_count":1,"text":"{}"}"#,
    )
    .unwrap();

    let DeepseekRequest::Director { custom_prompt, .. } = request else {
        panic!("expected director request");
    };
    assert!(custom_prompt.is_empty());
}

#[test]
#[ignore = "calls the live DeepSeek API"]
fn director_live_api_returns_structured_shot_plan() {
    let input = r#"{"dialogue":[{"index":0,"speaker_id":"1","name":"萌萌","role":"玩家","text":"Steam上的AI帮我打开10个舱门","has_character_model":true},{"index":1,"speaker_id":"2","name":"GM","role":"GM","text":"舱门缓慢打开。","has_character_model":false}]}"#;
    let response = DeepseekManager::post_director(input, "节奏紧凑，避免连续环绕镜头").unwrap();
    let value: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(
        value["_batch_responses"].as_array().unwrap().len(),
        1
    );
    let dialogue = value["dialogue"].as_array().unwrap();
    assert_eq!(dialogue.len(), 2);
    assert_eq!(dialogue[0]["index"], 0);
    assert!(dialogue[0]["text"]
        .as_str()
        .is_some_and(|text| !text.is_empty()));
    assert!(
        dialogue[0]["speech_text"].as_str().is_some_and(|text| {
            text.contains("斯地母")
                && text.contains("诶艾")
                && text.contains('十')
                && !text.contains("一零")
        })
    );
    assert!(dialogue[0]["shot"].as_str().is_some());
    assert!(dialogue[0]["motion"].as_str().is_some());
    assert!(matches!(
        dialogue[0]["shot"].as_str(),
        Some("speaker_close" | "speaker_medium" | "speaker_wide")
    ));
    assert!(matches!(
        dialogue[0]["motion"].as_str(),
        Some("static" | "dolly_in" | "dolly_out" | "drift_left" | "drift_right")
    ));
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
    let summary = DeepseekManager::post_summary(
        "玩家甲：我打开左侧的门。\n玩家乙：我记录门上有星形标记。",
        "重点保留门上的标记。",
    )
    .expect("summary request should succeed");

    assert!(!summary.trim().is_empty());
}
