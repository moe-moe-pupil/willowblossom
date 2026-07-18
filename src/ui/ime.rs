use std::collections::HashMap;

use bevy::{
    prelude::*,
    window::Ime,
};
use bevy_egui::{
    egui,
    input::EguiContextImeState,
};
use serde_json::json;
use tungstenite::Message;

use crate::napcat::{
    NapcatIOSender,
    NapcatOutboundMessage,
    NapcatSendResult,
};

pub struct ImePlugin;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NapcatSendTarget {
    Private(u64),
    Group(u64),
}

impl Plugin for ImePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ImeManager::default()).add_systems(
            Update,
            reset_egui_ime_enabled_after_commit,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_text_send_tracks_private_target_until_acknowledged() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let sender = NapcatIOSender(sender);
        let mut ime = ImeManager::default();

        ime.queue_text_send("42", " 欢迎加入 ", &sender, vec![
            NapcatSendTarget::Private(42),
        ])
        .unwrap();

        let outbound = receiver.try_recv().unwrap();
        assert_eq!(outbound.request_id, 1);
        assert_eq!(outbound.target_id, "42");
        assert!(outbound.message.to_string().contains("send_private_msg"));
        assert!(outbound.message.to_string().contains("欢迎加入"));

        let sent = ime.apply_send_results([NapcatSendResult {
            request_id: outbound.request_id,
            target_id: "42".to_owned(),
            error: None,
        }]);

        assert_eq!(sent, vec![ChatInputSendCompletion {
            input_id: "42".to_owned(),
            text: "欢迎加入".to_owned(),
            successful_targets: vec![NapcatSendTarget::Private(42)],
            clear_input: true,
        }]);
    }

    #[test]
    fn batch_send_preserves_partial_success_and_error_after_out_of_order_failure() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let sender = NapcatIOSender(sender);
        let mut ime = ImeManager::default();

        ime.queue_text_send(
            "broadcast",
            "party update",
            &sender,
            vec![NapcatSendTarget::Private(42), NapcatSendTarget::Private(43)],
        )
        .unwrap();

        let first = receiver.try_recv().unwrap();
        let second = receiver.try_recv().unwrap();
        assert!(ime
            .apply_send_results([NapcatSendResult {
                request_id: first.request_id,
                target_id: "broadcast".to_owned(),
                error: Some("recipient rejected message".to_owned()),
            }])
            .is_empty());

        let completed = ime.apply_send_results([NapcatSendResult {
            request_id: second.request_id,
            target_id: "broadcast".to_owned(),
            error: None,
        }]);

        assert_eq!(completed, vec![
            ChatInputSendCompletion {
                input_id: "broadcast".to_owned(),
                text: "party update".to_owned(),
                successful_targets: vec![NapcatSendTarget::Private(43)],
                clear_input: false,
            }
        ]);
        assert_eq!(
            ime.send_states["broadcast"].error.as_deref(),
            Some("recipient rejected message")
        );
    }

    #[test]
    fn partially_queued_batch_keeps_draft_and_records_queued_successes() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let sender = NapcatIOSender(sender);
        let mut ime = ImeManager::default();

        assert!(ime
            .queue_text_send(
                "broadcast",
                "party update",
                &sender,
                vec![NapcatSendTarget::Private(42), NapcatSendTarget::Private(43),]
            )
            .is_err());

        let queued = receiver.try_recv().unwrap();
        let completed = ime.apply_send_results([NapcatSendResult {
            request_id: queued.request_id,
            target_id: "broadcast".to_owned(),
            error: None,
        }]);

        assert_eq!(completed, vec![
            ChatInputSendCompletion {
                input_id: "broadcast".to_owned(),
                text: "party update".to_owned(),
                successful_targets: vec![NapcatSendTarget::Private(42)],
                clear_input: false,
            }
        ]);
        assert!(ime.send_states["broadcast"].error.is_some());
    }
}

fn reset_egui_ime_enabled_after_commit(
    mut events: MessageReader<Ime>,
    mut ime_states: Query<&mut EguiContextImeState>,
) {
    let should_reset = events.read().any(|event| {
        matches!(
            event,
            Ime::Commit { .. } | Ime::Disabled { .. } | Ime::Preedit { cursor: None, .. }
        )
    });
    if !should_reset {
        return;
    }

    for mut ime_state in &mut ime_states {
        ime_state.is_ime_allowed = false;
        ime_state.ime_rect = None;
    }
}

#[derive(Debug, Resource)]
pub struct ImeManager {
    next_send_request_id: u64,
    send_states: HashMap<String, ChatInputSendState>,
}

#[derive(Debug, Default)]
struct ChatInputSendState {
    pending_requests: Vec<(u64, NapcatSendTarget)>,
    successful_targets: Vec<NapcatSendTarget>,
    pending_text: Option<String>,
    error: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChatInputSendCompletion {
    pub input_id: String,
    pub text: String,
    pub successful_targets: Vec<NapcatSendTarget>,
    pub clear_input: bool,
}

impl Default for ImeManager {
    fn default() -> ImeManager {
        ImeManager {
            next_send_request_id: 1,
            send_states: HashMap::new(),
        }
    }
}

impl ImeManager {
    pub fn chat_input_multiline(
        &mut self,
        target_id: &str,
        text: &mut String,
        width: f32,
        desired_rows: usize,
        ui: &mut egui::Ui,
        _ctx: &egui::Context,
        sender: &NapcatIOSender,
        targets: Vec<NapcatSendTarget>,
    ) -> egui::text_edit::TextEditOutput {
        let teo = egui::TextEdit::multiline(text)
            .id_salt((target_id, "chat_input"))
            .desired_width(width)
            .desired_rows(desired_rows)
            .lock_focus(true)
            .return_key(None)
            .show(ui);
        let send_on_enter = teo.response.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);

        if send_on_enter {
            ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            });
        }

        let send_state = self.send_states.entry(target_id.to_owned()).or_default();
        if send_state.pending_requests.is_empty() {
            if let Some(error) = &send_state.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        } else {
            ui.label("发送中...");
        }

        if send_on_enter {
            if self
                .send_states
                .get(target_id)
                .map(|state| !state.pending_requests.is_empty())
                .unwrap_or(false)
            {
                return teo;
            }

            let message_text = text.trim().to_owned();
            if message_text.is_empty() {
                text.clear();
                return teo;
            }

            let _ = self.queue_text_send(target_id, message_text, sender, targets);
        }

        teo
    }

    pub fn queue_text_send(
        &mut self,
        target_id: &str,
        text: impl AsRef<str>,
        sender: &NapcatIOSender,
        targets: Vec<NapcatSendTarget>,
    ) -> Result<(), String> {
        let message_text = text.as_ref().trim().to_owned();
        if message_text.is_empty() {
            return Ok(());
        }

        if targets.is_empty() {
            let error = "没有可发送的NapCat目标".to_owned();
            self.send_states
                .entry(target_id.to_owned())
                .or_default()
                .error = Some(error.clone());
            return Err(error);
        }

        if self
            .send_states
            .get(target_id)
            .map(|state| !state.pending_requests.is_empty())
            .unwrap_or(false)
        {
            return Err("上一条NapCat消息仍在发送中".to_owned());
        }

        let mut pending_requests = Vec::new();
        let mut error = None;
        for target in targets {
            let (action, id_key, id) = match &target {
                NapcatSendTarget::Private(user_id) => ("send_private_msg", "user_id", user_id),
                NapcatSendTarget::Group(group_id) => ("send_group_msg", "group_id", group_id),
            };
            let request_id = self.next_send_request_id;
            self.next_send_request_id += 1;
            let message = Message::Text(
                json!({
                    "action": action,
                    "params": {
                        id_key: id,
                        "message": [
                            {
                                "type": "text",
                                "data": {
                                    "text": message_text
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
                target_id: target_id.to_owned(),
                message,
            }) {
                error = Some(format!(
                    "NapCat websocket消息入队失败：{err}"
                ));
                break;
            }
            pending_requests.push((request_id, target));
        }

        let send_state = self.send_states.entry(target_id.to_owned()).or_default();
        send_state.successful_targets.clear();
        if !pending_requests.is_empty() {
            send_state.pending_requests = pending_requests;
            send_state.pending_text = Some(message_text);
        } else {
            send_state.pending_requests.clear();
            send_state.pending_text = None;
        }

        match error {
            Some(error) => {
                send_state.error = Some(error.clone());
                Err(error)
            },
            None => {
                send_state.error = None;
                Ok(())
            },
        }
    }

    pub fn apply_send_results(
        &mut self,
        results: impl IntoIterator<Item = NapcatSendResult>,
    ) -> Vec<ChatInputSendCompletion> {
        let mut completions = Vec::new();
        for result in results {
            let Some(state) = self.send_states.get_mut(&result.target_id) else {
                continue;
            };
            let Some(request_index) = state
                .pending_requests
                .iter()
                .position(|(request_id, _)| *request_id == result.request_id)
            else {
                continue;
            };
            let (_, target) = state.pending_requests.remove(request_index);
            if let Some(error) = result.error {
                state.error = Some(error);
            } else {
                state.successful_targets.push(target);
            }
            if state.pending_requests.is_empty() {
                let Some(text) = state.pending_text.take() else {
                    continue;
                };
                completions.push(ChatInputSendCompletion {
                    input_id: result.target_id.clone(),
                    text,
                    successful_targets: std::mem::take(&mut state.successful_targets),
                    clear_input: state.error.is_none(),
                });
            }
        }
        completions
    }
}
