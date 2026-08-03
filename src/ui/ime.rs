use std::collections::HashMap;

use base64::{
    engine::general_purpose::STANDARD,
    Engine as _,
};
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

/// An image attached to a chat composer draft.
///
/// `png_bytes` is the encoded payload that gets sent to NapCat as a
/// `base64://` image segment and mirrored into the local chat history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatInputImage {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub png_bytes: Vec<u8>,
}

/// Fixed composer space reserved above the text input while pasted images are
/// attached, so the message pane keeps its own height inside the viewport.
pub const CHAT_PASTE_PREVIEW_HEIGHT: f32 = 108.0;

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
            images: Vec::new(),
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
                images: Vec::new(),
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
                images: Vec::new(),
                successful_targets: vec![NapcatSendTarget::Private(42)],
                clear_input: false,
            }
        ]);
        assert!(ime.send_states["broadcast"].error.is_some());

        ime.queue_text_send(
            "broadcast",
            "party update",
            &sender,
            vec![NapcatSendTarget::Private(42), NapcatSendTarget::Private(43)],
        )
        .unwrap();
        let retry = receiver.try_recv().unwrap();
        assert!(receiver.try_recv().is_err());
        assert!(retry.message.to_string().contains("\"user_id\":43"));
    }

    #[test]
    fn retry_after_partial_batch_sends_only_to_unacknowledged_targets() {
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
        let completed = ime.apply_send_results([
            NapcatSendResult {
                request_id: first.request_id,
                target_id: "broadcast".to_owned(),
                error: None,
            },
            NapcatSendResult {
                request_id: second.request_id,
                target_id: "broadcast".to_owned(),
                error: Some("recipient rejected message".to_owned()),
            },
        ]);
        assert_eq!(completed[0].successful_targets, vec![
            NapcatSendTarget::Private(42)
        ]);
        assert!(!completed[0].clear_input);

        ime.queue_text_send(
            "broadcast",
            "party update",
            &sender,
            vec![NapcatSendTarget::Private(42), NapcatSendTarget::Private(43)],
        )
        .unwrap();

        let retry = receiver.try_recv().unwrap();
        assert!(receiver.try_recv().is_err());
        assert!(retry.message.to_string().contains("\"user_id\":43"));

        let completed = ime.apply_send_results([NapcatSendResult {
            request_id: retry.request_id,
            target_id: "broadcast".to_owned(),
            error: None,
        }]);
        assert_eq!(completed, vec![
            ChatInputSendCompletion {
                input_id: "broadcast".to_owned(),
                text: "party update".to_owned(),
                images: Vec::new(),
                successful_targets: vec![NapcatSendTarget::Private(43)],
                clear_input: true,
            }
        ]);
    }

    #[test]
    fn changing_partial_batch_draft_starts_a_fresh_send_to_all_targets() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let sender = NapcatIOSender(sender);
        let mut ime = ImeManager::default();

        ime.queue_text_send(
            "broadcast",
            "first update",
            &sender,
            vec![NapcatSendTarget::Private(42), NapcatSendTarget::Private(43)],
        )
        .unwrap();
        let first = receiver.try_recv().unwrap();
        let second = receiver.try_recv().unwrap();
        ime.apply_send_results([
            NapcatSendResult {
                request_id: first.request_id,
                target_id: "broadcast".to_owned(),
                error: None,
            },
            NapcatSendResult {
                request_id: second.request_id,
                target_id: "broadcast".to_owned(),
                error: Some("recipient rejected message".to_owned()),
            },
        ]);

        ime.queue_text_send(
            "broadcast",
            "corrected update",
            &sender,
            vec![NapcatSendTarget::Private(42), NapcatSendTarget::Private(43)],
        )
        .unwrap();

        let first_retry = receiver.try_recv().unwrap();
        let second_retry = receiver.try_recv().unwrap();
        assert!(receiver.try_recv().is_err());
        assert!(first_retry.message.to_string().contains("\"user_id\":42"));
        assert!(second_retry.message.to_string().contains("\"user_id\":43"));
    }

    #[test]
    fn enter_does_not_send_while_ime_composition_is_active() {
        assert!(!should_send_chat_input(
            true, true, false, true, None
        ));
        assert!(!should_send_chat_input(
            true,
            true,
            false,
            false,
            Some(false)
        ));
        assert!(should_send_chat_input(
            true, true, false, false, None
        ));
    }

    #[test]
    fn ime_composition_update_tracks_preedit_and_commit_events() {
        let preedit = egui::Event::Ime(egui::ImeEvent::Preedit {
            text: "xspace".to_owned(),
            active_range_chars: Some(0..6),
        });
        assert_eq!(
            ime_composition_update(&[preedit]),
            Some(true)
        );

        let commit = egui::Event::Ime(egui::ImeEvent::Commit("小".to_owned()));
        assert_eq!(
            ime_composition_update(&[commit]),
            Some(false)
        );
    }

    #[test]
    fn image_send_emits_base64_image_segment_before_text() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let sender = NapcatIOSender(sender);
        let mut ime = ImeManager::default();
        let image = ChatInputImage {
            id: 1,
            width: 1,
            height: 1,
            png_bytes: vec![0x89, 0x50, 0x4e, 0x47],
        };

        ime.queue_send(
            "42",
            "look at this",
            &[image],
            &sender,
            vec![NapcatSendTarget::Private(42)],
        )
        .unwrap();

        let outbound = receiver.try_recv().unwrap();
        let json = outbound.message.to_string();
        assert!(json.contains("\"action\":\"send_private_msg\""));
        assert!(json.contains("base64://iVBO"));
        assert!(json.contains("\"type\":\"image\""));
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("look at this"));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn image_only_send_queues_and_completes_with_clear_input() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let sender = NapcatIOSender(sender);
        let mut ime = ImeManager::default();
        let image = ChatInputImage {
            id: 1,
            width: 1,
            height: 1,
            png_bytes: vec![0x89, 0x50, 0x4e, 0x47],
        };

        ime.queue_send(
            "42",
            "",
            &[image.clone()],
            &sender,
            vec![NapcatSendTarget::Private(42)],
        )
        .unwrap();

        let outbound = receiver.try_recv().unwrap();
        assert!(!outbound.message.to_string().contains("\"type\":\"text\""));
        let completed = ime.apply_send_results([NapcatSendResult {
            request_id: outbound.request_id,
            target_id: "42".to_owned(),
            error: None,
        }]);

        assert_eq!(completed, vec![
            ChatInputSendCompletion {
                input_id: "42".to_owned(),
                text: String::new(),
                images: vec![image],
                successful_targets: vec![NapcatSendTarget::Private(42)],
                clear_input: true,
            }
        ]);
    }

    #[test]
    fn image_retry_after_partial_batch_sends_only_to_unacknowledged_targets() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let sender = NapcatIOSender(sender);
        let mut ime = ImeManager::default();
        let image = ChatInputImage {
            id: 1,
            width: 1,
            height: 1,
            png_bytes: vec![0x89, 0x50, 0x4e, 0x47],
        };

        ime.queue_send(
            "broadcast",
            "with image",
            &[image.clone()],
            &sender,
            vec![NapcatSendTarget::Private(42), NapcatSendTarget::Private(43)],
        )
        .unwrap();
        let first = receiver.try_recv().unwrap();
        let second = receiver.try_recv().unwrap();
        let completed = ime.apply_send_results([
            NapcatSendResult {
                request_id: first.request_id,
                target_id: "broadcast".to_owned(),
                error: None,
            },
            NapcatSendResult {
                request_id: second.request_id,
                target_id: "broadcast".to_owned(),
                error: Some("recipient rejected message".to_owned()),
            },
        ]);
        assert_eq!(completed[0].images, vec![image.clone()]);
        assert_eq!(completed[0].successful_targets, vec![
            NapcatSendTarget::Private(42)
        ]);
        assert!(!completed[0].clear_input);

        ime.queue_send(
            "broadcast",
            "with image",
            &[image],
            &sender,
            vec![NapcatSendTarget::Private(42), NapcatSendTarget::Private(43)],
        )
        .unwrap();

        let retry = receiver.try_recv().unwrap();
        assert!(receiver.try_recv().is_err());
        assert!(retry.message.to_string().contains("\"user_id\":43"));
    }

    #[test]
    fn attached_images_can_be_removed_and_cleared() {
        let mut ime = ImeManager::default();
        let first = ChatInputImage {
            id: 1,
            width: 1,
            height: 1,
            png_bytes: vec![1],
        };
        let second = ChatInputImage {
            id: 2,
            width: 1,
            height: 1,
            png_bytes: vec![2],
        };
        ime.attached_images.insert("42".to_owned(), vec![
            first.clone(),
            second.clone(),
        ]);

        assert!(ime.has_attachments("42"));
        ime.remove_attached_image("42", 0);
        assert_eq!(ime.attached_images("42"), &[second]);

        ime.clear_attached_images("42");
        assert!(!ime.has_attachments("42"));
        assert!(ime.attached_images("42").is_empty());
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

fn ime_composition_update(events: &[egui::Event]) -> Option<bool> {
    events
        .iter()
        .filter_map(|event| match event {
            egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => Some(!text.is_empty()),
            egui::Event::Ime(egui::ImeEvent::Commit(_)) => Some(false),
            _ => None,
        })
        .last()
}

fn should_send_chat_input(
    has_focus: bool,
    enter_pressed: bool,
    shift_pressed: bool,
    was_ime_composing: bool,
    ime_update: Option<bool>,
) -> bool {
    has_focus && enter_pressed && !shift_pressed && !was_ime_composing && ime_update.is_none()
}

#[derive(Resource)]
pub struct ImeManager {
    next_send_request_id: u64,
    next_attachment_id: u64,
    send_states: HashMap<String, ChatInputSendState>,
    attached_images: HashMap<String, Vec<ChatInputImage>>,
    attachment_previews: HashMap<u64, egui::TextureHandle>,
    ime_composition_input: Option<String>,
}

impl std::fmt::Debug for ImeManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ImeManager")
            .field(
                "next_send_request_id",
                &self.next_send_request_id,
            )
            .field(
                "next_attachment_id",
                &self.next_attachment_id,
            )
            .field("send_states", &self.send_states)
            .field("attached_images", &self.attached_images)
            .field(
                "attachment_count",
                &self.attachment_previews.len(),
            )
            .field(
                "ime_composition_input",
                &self.ime_composition_input,
            )
            .finish()
    }
}

#[derive(Debug, Default)]
struct ChatInputSendState {
    pending_requests: Vec<(u64, NapcatSendTarget)>,
    successful_targets: Vec<NapcatSendTarget>,
    delivered_text: Option<String>,
    delivered_images: Vec<ChatInputImage>,
    delivered_targets: Vec<NapcatSendTarget>,
    pending_text: Option<String>,
    pending_images: Vec<ChatInputImage>,
    error: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChatInputSendCompletion {
    pub input_id: String,
    pub text: String,
    pub images: Vec<ChatInputImage>,
    pub successful_targets: Vec<NapcatSendTarget>,
    pub clear_input: bool,
}

impl Default for ImeManager {
    fn default() -> ImeManager {
        ImeManager {
            next_send_request_id: 1,
            next_attachment_id: 1,
            send_states: HashMap::new(),
            attached_images: HashMap::new(),
            attachment_previews: HashMap::new(),
            ime_composition_input: None,
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
        ctx: &egui::Context,
        sender: &NapcatIOSender,
        targets: Vec<NapcatSendTarget>,
    ) -> egui::text_edit::TextEditOutput {
        let mut remove_attachment: Option<usize> = None;
        let attachment_count = self.attached_images(target_id).len();
        if attachment_count > 0 {
            egui::ScrollArea::horizontal()
                .id_salt((target_id, "chat_paste_previews"))
                .max_height(96.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("待发送图片：");
                        for index in 0..attachment_count {
                            let Some(image) = self.attached_images(target_id).get(index) else {
                                break;
                            };
                            let Some(preview) = self.attachment_previews.get(&image.id) else {
                                continue;
                            };
                            let size = fit_attachment_preview_size(preview.size_vec2());
                            let response = ui.add(
                                egui::Image::from_texture((preview.id(), size)).corner_radius(4),
                            );
                            if response.on_hover_text("点击移除图片").clicked() {
                                remove_attachment = Some(index);
                            }
                        }
                    });
                });
        }
        if let Some(index) = remove_attachment {
            self.remove_attached_image(target_id, index);
        }

        let teo = egui::TextEdit::multiline(text)
            .id_salt((target_id, "chat_input"))
            .desired_width(width)
            .desired_rows(desired_rows)
            .lock_focus(true)
            .return_key(None)
            .show(ui);
        let (enter_pressed, shift_pressed, ime_update, paste_shortcut, text_paste_happened) = ui
            .input(|i| {
                (
                    i.key_pressed(egui::Key::Enter),
                    i.modifiers.shift,
                    ime_composition_update(&i.events),
                    i.modifiers.command && i.key_pressed(egui::Key::V),
                    i.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Paste(_) | egui::Event::Text(_)
                        )
                    }),
                )
            });
        let was_ime_composing = self.ime_composition_input.as_deref() == Some(target_id);
        let send_on_enter = should_send_chat_input(
            teo.response.has_focus(),
            enter_pressed,
            shift_pressed,
            was_ime_composing,
            ime_update,
        );

        let mut paste_error: Option<String> = None;
        if teo.response.has_focus() {
            if paste_shortcut {
                if let Err(err) = self.paste_clipboard_image(target_id, ctx) {
                    // A Text event in the same frame means text was pasted, so a
                    // missing clipboard image is expected and not worth an error.
                    if !text_paste_happened {
                        paste_error = Some(err);
                    }
                }
            }
            if let Some(is_composing) = ime_update {
                self.ime_composition_input = is_composing.then(|| target_id.to_owned());
            }
        } else if was_ime_composing {
            self.ime_composition_input = None;
        }

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
            let images = self
                .attached_images
                .get(target_id)
                .cloned()
                .unwrap_or_default();
            if message_text.is_empty() && images.is_empty() {
                text.clear();
                return teo;
            }

            let _ = self.queue_send(
                target_id,
                message_text,
                &images,
                sender,
                targets,
            );
        } else if let Some(error) = paste_error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
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
        self.queue_send(target_id, text, &[], sender, targets)
    }

    pub fn queue_send(
        &mut self,
        target_id: &str,
        text: impl AsRef<str>,
        images: &[ChatInputImage],
        sender: &NapcatIOSender,
        targets: Vec<NapcatSendTarget>,
    ) -> Result<(), String> {
        let message_text = text.as_ref().trim().to_owned();
        let images = images.to_vec();
        if message_text.is_empty() && images.is_empty() {
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

        let mut targets = targets;
        {
            let send_state = self.send_states.entry(target_id.to_owned()).or_default();
            let is_retry = send_state.error.is_some()
                && send_state.delivered_text.as_deref() == Some(message_text.as_str())
                && send_state.delivered_images == images;
            if is_retry {
                targets.retain(|target| !send_state.delivered_targets.contains(target));
            } else {
                send_state.delivered_text = None;
                send_state.delivered_images.clear();
                send_state.delivered_targets.clear();
            }
            send_state.successful_targets.clear();
        }
        if targets.is_empty() {
            let error = "当前目标均已确认送达；如需再次发送，请修改消息内容".to_owned();
            self.send_states
                .entry(target_id.to_owned())
                .or_default()
                .error = Some(error.clone());
            return Err(error);
        }

        let mut pending_requests = Vec::new();
        let mut error = None;
        for target in targets {
            let (action, id_key, id) = match &target {
                NapcatSendTarget::Private(user_id) => ("send_private_msg", "user_id", user_id),
                NapcatSendTarget::Group(group_id) => ("send_group_msg", "group_id", group_id),
            };
            let mut message_segments = Vec::new();
            for image in &images {
                message_segments.push(json!({
                    "type": "image",
                    "data": {
                        "file": format!("base64://{}", STANDARD.encode(&image.png_bytes))
                    }
                }));
            }
            if !message_text.is_empty() {
                message_segments.push(json!({
                    "type": "text",
                    "data": {
                        "text": message_text
                    }
                }));
            }
            let request_id = self.next_send_request_id;
            self.next_send_request_id += 1;
            let message = Message::Text(
                json!({
                    "action": action,
                    "params": {
                        id_key: id,
                        "message": message_segments
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
        if !pending_requests.is_empty() {
            send_state.pending_requests = pending_requests;
            send_state.pending_text = Some(message_text);
            send_state.pending_images = images;
        } else {
            send_state.pending_requests.clear();
            send_state.pending_text = None;
            send_state.pending_images.clear();
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
                let images = std::mem::take(&mut state.pending_images);
                state.delivered_text = Some(text.clone());
                state.delivered_images = images.clone();
                for target in &state.successful_targets {
                    if !state.delivered_targets.contains(target) {
                        state.delivered_targets.push(target.clone());
                    }
                }
                let clear_input = state.error.is_none();
                completions.push(ChatInputSendCompletion {
                    input_id: result.target_id.clone(),
                    text,
                    images,
                    successful_targets: std::mem::take(&mut state.successful_targets),
                    clear_input,
                });
                if clear_input {
                    state.delivered_text = None;
                    state.delivered_images.clear();
                    state.delivered_targets.clear();
                }
            }
        }
        completions
    }

    pub fn has_attachments(&self, target_id: &str) -> bool {
        self.attached_images
            .get(target_id)
            .is_some_and(|images| !images.is_empty())
    }

    pub fn attached_images(&self, target_id: &str) -> &[ChatInputImage] {
        self.attached_images
            .get(target_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn remove_attached_image(&mut self, target_id: &str, index: usize) {
        let Some(images) = self.attached_images.get_mut(target_id) else {
            return;
        };
        if let Some(image) = images.get(index) {
            self.attachment_previews.remove(&image.id);
        }
        images.remove(index);
        if images.is_empty() {
            self.attached_images.remove(target_id);
        }
    }

    pub fn clear_attached_images(&mut self, target_id: &str) {
        if let Some(images) = self.attached_images.remove(target_id) {
            for image in images {
                self.attachment_previews.remove(&image.id);
            }
        }
    }

    pub fn paste_clipboard_image(
        &mut self,
        target_id: &str,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (target_id, ctx);
            return Err("网页版暂不支持粘贴图片".to_owned());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let image = read_clipboard_image()?;
            let (width, height, rgba) = image;
            let pixel_count = width as u64 * height as u64;
            if pixel_count == 0 {
                return Err("剪贴板图片为空".to_owned());
            }
            if pixel_count > 40_000_000 {
                return Err("剪贴板图片过大，请缩小后再粘贴".to_owned());
            }
            let png_bytes = encode_png_bytes(width, height, &rgba)?;
            let id = self.next_attachment_id;
            self.next_attachment_id += 1;
            let color_image =
                egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
            let preview = ctx.load_texture(
                format!("chat-paste:{target_id}:{id}"),
                color_image,
                egui::TextureOptions::LINEAR,
            );
            self.attached_images
                .entry(target_id.to_owned())
                .or_default()
                .push(ChatInputImage {
                    id,
                    width,
                    height,
                    png_bytes,
                });
            self.attachment_previews.insert(id, preview);
            Ok(())
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_clipboard_image() -> Result<(u32, u32, Vec<u8>), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|err| format!("无法访问剪贴板：{err}"))?;
    let image = clipboard.get_image().map_err(|err| match err {
        arboard::Error::ContentNotAvailable => "剪贴板中没有图片".to_owned(),
        other => format!("读取剪贴板图片失败：{other}"),
    })?;
    Ok((
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn encode_png_bytes(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "剪贴板图片像素数据无效".to_owned())?;
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|err| format!("图片编码失败：{err}"))?;
    Ok(png_bytes)
}

fn fit_attachment_preview_size(original: egui::Vec2) -> egui::Vec2 {
    const MAX_PREVIEW: egui::Vec2 = egui::Vec2::new(84.0, 84.0);
    if original.x <= 0.0 || original.y <= 0.0 {
        return MAX_PREVIEW;
    }
    let scale = (MAX_PREVIEW.x / original.x)
        .min(MAX_PREVIEW.y / original.y)
        .min(1.0);
    original * scale
}
