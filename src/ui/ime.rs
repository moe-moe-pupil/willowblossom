use std::collections::HashMap;

use bevy::prelude::*;
use bevy_egui::egui;
use serde_json::json;
use tungstenite::Message;

use crate::napcat::{
    NapcatIOSender,
    NapcatOutboundMessage,
    NapcatSendResult,
};

pub struct ImePlugin;

#[derive(Clone, Debug)]
pub enum NapcatSendTarget {
    Private(u64),
    Group(u64),
}

impl Plugin for ImePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ImeManager::default())
            .add_systems(PreUpdate, reset_unused_ime)
            .add_systems(Update, listen_ime_events)
            .add_systems(PostUpdate, clear_unused_ime);
    }
}

fn reset_unused_ime(mut ime: ResMut<ImeManager>) {
    // Make all ImeText unused before update
    for i in &mut ime.ime_texts {
        i.is_used = false;
    }
    ime.count = 0;
}

fn listen_ime_events(
    // ime look
    mut events: MessageReader<Ime>,
    mut ime: ResMut<ImeManager>,
    mut windows: Query<&mut Window>,
) {
    for event in events.read() {
        ime.listen_ime_event(event);
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    window.ime_position = ime
        .get_focused_text()
        .and_then(|text| Some(text.screen_pos))
        .unwrap_or(Vec2::new(0.0, 0.0));
}

fn clear_unused_ime(
    // delete unused ImeText after update
    mut ime: ResMut<ImeManager>,
) {
    ime.ime_texts.retain(|i| i.is_used == true);
}
//////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Resource)]
pub struct ImeManager {
    count: usize,
    ime_texts: Vec<ImeText>,
    next_send_request_id: u64,
    send_states: HashMap<String, ChatInputSendState>,
}

#[derive(Debug, Default)]
struct ChatInputSendState {
    pending_request_ids: Vec<u64>,
    pending_text: Option<String>,
    error: Option<String>,
}
impl Default for ImeManager {
    fn default() -> ImeManager {
        ImeManager {
            count: 0,
            ime_texts: Vec::new(),
            next_send_request_id: 1,
            send_states: HashMap::new(),
        }
    }
}
impl ImeManager {
    /// ```
    /// let teo = ime.text_edit_multiline(&mut text, 200.0, ui, ctx);
    /// if teo.response.changed() {
    ///     println!("{:?}", text);
    /// }
    /// ```
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
        if self.count >= self.ime_texts.len() {
            self.add();
            self.ime_texts[self.count].text = text.to_string();
        }
        self.ime_texts[self.count].target_id = target_id.to_owned();
        let text_before_edit = self.ime_texts[self.count].text.clone();
        let teo = self.ime_texts[self.count].get_text_edit_output(
            width,
            text,
            EditType::MultiLine,
            desired_rows,
            ui,
            ctx,
            "",
        );
        let send_on_enter = teo.response.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);

        if send_on_enter {
            ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            });
            self.ime_texts[self.count].text = text_before_edit;
            *text = self.ime_texts[self.count].text.clone();
        }

        let send_state = self.send_states.entry(target_id.to_owned()).or_default();
        if send_state.pending_request_ids.is_empty() {
            if let Some(error) = &send_state.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        } else {
            ui.label("发送中...");
        }

        if send_on_enter {
            if !send_state.pending_request_ids.is_empty() {
                self.ime_texts[self.count].id = teo.response.id.short_debug_format();
                self.count += 1;
                return teo;
            }

            let message_text = self.ime_texts[self.count].text.trim().to_owned();
            if message_text.is_empty() {
                self.ime_texts[self.count].text.clear();
                *text = String::new();
                self.count += 1;
                return teo;
            }

            if targets.is_empty() {
                send_state.error = Some("no valid NapCat target for outbound message".to_owned());
                self.ime_texts[self.count].id = teo.response.id.short_debug_format();
                self.count += 1;
                return teo;
            }

            let mut pending_request_ids = Vec::new();
            send_state.error = None;
            for target in targets {
                let (action, id_key, id) = match target {
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
                    send_state.error = Some(format!(
                        "failed to queue NapCat websocket message: {err}"
                    ));
                    break;
                }
                pending_request_ids.push(request_id);
            }

            if send_state.error.is_none() {
                send_state.pending_request_ids = pending_request_ids;
                send_state.pending_text = Some(message_text);
            } else if !pending_request_ids.is_empty() {
                send_state.pending_request_ids = pending_request_ids;
                send_state.pending_text = Some(message_text);
            }
        }
        self.ime_texts[self.count].id = teo.response.id.short_debug_format();
        self.count += 1;
        return teo;
    }

    pub fn apply_send_results(
        &mut self,
        results: impl IntoIterator<Item = NapcatSendResult>,
    ) -> Vec<(String, Option<String>)> {
        let mut sent_targets = Vec::new();
        for result in results {
            let Some(state) = self.send_states.get_mut(&result.target_id) else {
                continue;
            };
            state
                .pending_request_ids
                .retain(|request_id| *request_id != result.request_id);
            if let Some(error) = result.error {
                state.error = Some(error);
                state.pending_text = None;
            } else if state.pending_request_ids.is_empty() {
                state.error = None;
                let pending_text = state.pending_text.take();
                sent_targets.push((
                    result.target_id.clone(),
                    pending_text.clone(),
                ));
                if let Some(text) = self
                    .ime_texts
                    .iter_mut()
                    .find(|text| text.target_id == result.target_id)
                {
                    let should_clear = match pending_text.as_deref() {
                        Some(pending_text) => text.text.trim() == pending_text,
                        None => true,
                    };
                    if should_clear {
                        text.text.clear();
                    }
                }
            }
        }
        sent_targets
    }

    /// ```
    /// let teo = ime.text_edit_multiline(&mut text, 200.0, ui, ctx);
    /// let id = teo.response.id.short_debug_format()
    /// teo.set_text(&id, "あいうえお");
    /// ```
    pub fn set_text(&mut self, id: &str, text: &str) {
        let res = self.ime_texts.iter().position(|i| &i.id == id);
        if res.is_none() {
            return;
        }
        self.ime_texts[res.unwrap()].text = text.to_string();
    }

    fn add(&mut self) {
        // add Ime
        let it = ImeText::new();
        self.ime_texts.push(it);
    }

    fn get_focused_text(&mut self) -> Option<&ImeText> {
        self.ime_texts.iter().find(|&text| text.is_focus == true)
    }

    pub fn listen_ime_event(&mut self, event: &Ime) {
        // ime event look
        for i in &mut self.ime_texts {
            i.listen_ime_event(event);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum EditType {
    SingleLine,
    MultiLine,
}

#[derive(Debug)]
struct ImeText {
    id: String,
    target_id: String,
    screen_pos: Vec2,
    text: String,
    ime_string: String,
    ime_string_index: usize,
    cursor_index: usize,
    is_ime_input: bool,
    is_focus: bool,
    is_ime: bool,
    is_cursor_move: bool,
    edit_type: EditType,
    is_used: bool,
}
impl Default for ImeText {
    fn default() -> Self {
        ImeText {
            id: String::new(),
            target_id: String::new(),
            text: String::new(),
            ime_string: String::new(),
            ime_string_index: 0,
            cursor_index: 0,
            is_ime_input: false,
            is_focus: false,
            is_ime: false,
            is_cursor_move: true,
            edit_type: EditType::SingleLine,
            is_used: false,
            screen_pos: Vec2 { x: 0.0, y: 0.0 },
        }
    }
}

impl ImeText {
    fn new() -> ImeText { return ImeText::default(); }

    fn listen_ime_event(&mut self, event: &Ime) {
        if !self.is_focus {
            return;
        }
        match event {
            Ime::Preedit { value, cursor, .. } => {
                if cursor.is_some() {
                    // if self.is_focus {
                    //     self.ime_string = value.to_string();
                    //     self.ime_string_index = self.ime_string.chars().count();
                    // }
                } else {
                    self.is_ime = false;
                }
            },
            Ime::Commit { value, .. } => {
                if value.is_empty() {
                    self.is_cursor_move = false;
                }
            },
            Ime::Enabled { .. } => {
                self.is_ime = true;
            },
            Ime::Disabled { .. } => {
                self.is_ime = false;
            },
        }
    }

    fn get_text_edit_output(
        &mut self,
        width: f32,
        text: &mut String,
        edit_type: EditType,
        desired_rows: usize,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        autocompletion_text_len: &str,
    ) -> egui::text_edit::TextEditOutput {
        self.edit_type = edit_type;
        self.is_used = true;
        let mut lyt = |ui: &egui::Ui, string: &dyn egui::TextBuffer, wrap_width: f32| {
            let loj = self.get_layoutjob(
                string.as_str(),
                wrap_width,
                autocompletion_text_len,
            );
            ui.fonts_mut(|f| f.layout_job(loj))
        };
        let mut tmp_text = match self.ime_string.len() {
            0 => self.text.to_string(),
            _ => {
                let mut front = String::new();
                let mut back = String::new();
                let mut cnt = 0;
                for c in text.chars() {
                    if cnt < self.cursor_index {
                        front.push_str(&c.to_string());
                    } else {
                        back.push_str(&c.to_string());
                    }
                    cnt += 1;
                }
                format!("{}{}{}", front, self.ime_string, back)
            },
        };

        let mut teo = match self.edit_type {
            EditType::SingleLine => egui::TextEdit::singleline(&mut tmp_text)
                .desired_width(width)
                .layouter(&mut lyt)
                .show(ui),
            _ => egui::TextEdit::multiline(&mut tmp_text)
                .desired_width(width)
                .desired_rows(desired_rows)
                .layouter(&mut lyt)
                .lock_focus(true)
                .show(ui),
        };
        self.is_focus = teo.response.has_focus();
        if !self.is_ime {
            self.text = tmp_text.to_string();
        }
        if teo.cursor_range.is_some() {
            self.cursor_index = teo.cursor_range.unwrap().primary.index;
        }
        if self.is_ime_input {
            // respose.changed()=true
            teo.response.mark_changed();
        }
        if self.is_ime_input {
            self.is_ime_input = false;
            if self.is_cursor_move {
                let mut res_cursor = teo.cursor_range.unwrap().primary.clone();
                for _ in 0..self.ime_string_index {
                    res_cursor = teo.galley.cursor_right_one_character(&res_cursor);
                }
                let cr = egui::text_selection::CCursorRange::one(res_cursor);
                teo.state.cursor.set_char_range(Some(cr));
            }
        }
        if !self.is_cursor_move {
            self.is_cursor_move = true;
        }
        ui.ctx().output(|o| {
            self.screen_pos = Vec2::new(
                o.ime
                    .and_then(|p| Some(p.cursor_rect.right()))
                    .unwrap_or(0.0),
                o.ime
                    .and_then(|p| Some(p.cursor_rect.bottom()))
                    .unwrap_or(0.0),
            );
        });
        teo.state.clone().store(ctx, teo.response.id);
        *text = self.text.to_string();
        teo
    }

    fn get_layoutjob(
        &self,
        string: &str,
        width: f32,
        autocompletion_text_len: &str,
    ) -> egui::text::LayoutJob {
        let layout_job = match self.is_ime {
            false => {
                let mut lss: Vec<egui::text::LayoutSection> = vec![];
                let mut front = String::new();
                let mut back = String::new();
                let mut cnt = 0;
                for c in string.chars() {
                    if cnt < self.cursor_index {
                        front.push_str(&c.to_string());
                    } else {
                        back.push_str(&c.to_string());
                    }
                    cnt += 1;
                }

                let mut f_cnt = 0;
                let mut b_cnt = 0;
                b_cnt = b_cnt + front.len();
                let ls_front = egui::text::LayoutSection {
                    leading_space: 0.0,
                    byte_range: f_cnt..b_cnt,
                    format: egui::TextFormat {
                        color: egui::Color32::WHITE,
                        ..Default::default()
                    },
                };
                lss.push(ls_front);

                f_cnt = b_cnt;
                b_cnt = b_cnt + autocompletion_text_len.len();
                let ls_autocompletion = egui::text::LayoutSection {
                    leading_space: 0.0,
                    byte_range: f_cnt..b_cnt,
                    format: egui::TextFormat {
                        color: egui::Color32::GRAY,
                        ..Default::default()
                    },
                };
                lss.push(ls_autocompletion);

                f_cnt = b_cnt;
                b_cnt = b_cnt + back.len();
                let ls_back = egui::text::LayoutSection {
                    leading_space: 0.0,
                    byte_range: f_cnt..b_cnt,
                    format: egui::TextFormat {
                        color: egui::Color32::WHITE,
                        ..Default::default()
                    },
                };
                lss.push(ls_back);

                egui::text::LayoutJob {
                    sections: lss,
                    text: format!(
                        "{}{}{}",
                        front, autocompletion_text_len, back
                    ),
                    wrap: egui::text::TextWrapping {
                        max_width: width,
                        ..Default::default()
                    },
                    ..Default::default()
                }
            },
            _ => {
                let mut front = String::new();
                let mut back = String::new();
                let mut cnt = 0;
                for c in self.text.chars() {
                    if cnt < self.cursor_index {
                        front.push_str(&c.to_string());
                    } else {
                        back.push_str(&c.to_string());
                    }
                    cnt += 1;
                }

                let mut lss: Vec<egui::text::LayoutSection> = vec![];
                let mut f_cnt = 0;
                let mut b_cnt = 0;
                b_cnt = b_cnt + front.len();
                let ls_front = egui::text::LayoutSection {
                    leading_space: 0.0,
                    byte_range: f_cnt..b_cnt,
                    format: egui::TextFormat {
                        color: egui::Color32::WHITE,
                        ..Default::default()
                    },
                };
                lss.push(ls_front);

                f_cnt = b_cnt;
                b_cnt = b_cnt + self.ime_string.len();
                let ls_text = egui::text::LayoutSection {
                    leading_space: 0.0,
                    byte_range: f_cnt..b_cnt,
                    format: egui::TextFormat {
                        color: egui::Color32::GREEN,
                        background: egui::Color32::from_rgb(0, 128, 64),
                        ..Default::default()
                    },
                };
                lss.push(ls_text);

                f_cnt = b_cnt;
                b_cnt = b_cnt + autocompletion_text_len.len();
                let ls_autocompletion = egui::text::LayoutSection {
                    leading_space: 0.0,
                    byte_range: f_cnt..b_cnt,
                    format: egui::TextFormat {
                        color: egui::Color32::GRAY,
                        ..Default::default()
                    },
                };
                lss.push(ls_autocompletion);

                f_cnt = b_cnt;
                b_cnt = b_cnt + back.len();
                let ls_back = egui::text::LayoutSection {
                    leading_space: 0.0,
                    byte_range: f_cnt..b_cnt,
                    format: egui::TextFormat {
                        color: egui::Color32::WHITE,
                        ..Default::default()
                    },
                };
                lss.push(ls_back);
                let break_on_newline = match self.edit_type {
                    EditType::SingleLine => false,
                    _ => true,
                };
                egui::text::LayoutJob {
                    sections: lss,
                    text: format!(
                        "{}{}{}{}",
                        front, self.ime_string, autocompletion_text_len, back
                    ),
                    break_on_newline,
                    wrap: egui::text::TextWrapping {
                        max_width: width,
                        ..Default::default()
                    },
                    ..Default::default()
                }
            },
        };
        return layout_job;
    }
}
