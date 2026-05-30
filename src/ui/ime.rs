use std::cmp::min;

use bevy::prelude::*;
use bevy_egui::egui::{
    self,
    Color32,
    FontSelection,
    Pos2,
    TextWrapMode,
    WidgetText,
};
use bevy_persistent::Persistent;
use serde_json::json;
use tungstenite::Message;

use crate::{
    deepseek::filter_control_characters,
    napcat::{
        NapcatIOSender,
        NapcatMessageManager,
    },
};

pub struct ImePlugin;

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
}
impl Default for ImeManager {
    fn default() -> ImeManager {
        ImeManager {
            count: 0,
            ime_texts: Vec::new(),
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
        text: &mut String,
        width: f32,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        sender: &NapcatIOSender,
        target_ids: Vec<String>,
        autocompletion_text: &mut String,
    ) -> egui::text_edit::TextEditOutput {
        if self.count >= self.ime_texts.len() {
            self.add();
            self.ime_texts[self.count].text = text.to_string();
        }
        let teo = self.ime_texts[self.count].get_text_edit_output(
            width,
            text,
            EditType::MultiLine,
            ui,
            ctx,
            autocompletion_text,
        );

        if self.ime_texts[self.count].is_focus && ui.input(|i| i.key_pressed(egui::Key::Tab)) {
            let cursor_idx = teo.cursor_range.unwrap().primary.index;
            // Find the byte index corresponding to the 4th character
            let byte_index = self.ime_texts[self.count]
                .text
                .char_indices()
                .nth(cursor_idx)
                .map(|(idx, _)| idx)
                .unwrap_or(self.ime_texts[self.count].text.len());
            self.ime_texts[self.count]
                .text
                .insert_str(byte_index, autocompletion_text);
            self.ime_texts[self.count].text =
                filter_control_characters(&self.ime_texts[self.count].text);
            *autocompletion_text = "".to_owned();
        }

        if self.ime_texts[self.count].is_focus
            && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
        {
            println!("{}", self.ime_texts[self.count].text);
            for target_qq in target_ids {
                let err = sender
                    .0
                    .try_send(Message::Text(
                        json!({
                            "action": "send_private_msg",
                            "params": {
                                "user_id": target_qq,
                                "message_type": "private",
                                "message": [
                                    {
                                        "type": "text",
                                        "data": {
                                            "text": self.ime_texts[self.count].text
                                        }
                                    }
                                ]
                            }
                        })
                        .to_string()
                        .into(),
                    ))
                    .expect("can't send message");
            }

            self.ime_texts[self.count].text = "".to_string();
        }
        self.ime_texts[self.count].id = teo.response.id.short_debug_format();
        self.count += 1;
        return teo;
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
                .desired_rows(min(
                    20,
                    (ui.max_rect().height()
                        / ui.style()
                            .text_styles
                            .get(&egui::TextStyle::Body)
                            .unwrap()
                            .size)
                        .floor() as usize,
                ))
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
