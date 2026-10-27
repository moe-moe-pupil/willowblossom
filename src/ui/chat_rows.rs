use std::{
    collections::hash_map::DefaultHasher,
    hash::{
        Hash,
        Hasher,
    },
    sync::Arc,
};

use egui::{
    Rect,
    Style,
    Ui,
    Vec2,
};

use crate::napcat::{
    NapcatMessage,
    NapcatMessageChainType,
};

/// Cache geometry, never message contents or visibility decisions. Changed rows
/// are measured again, while unchanged offscreen rows only reserve their space.
#[derive(Default)]
pub(super) struct ChatRowCache {
    rows: Vec<Option<(u64, f32)>>,
    width: f32,
    pixels_per_point: f32,
    style: Option<Arc<Style>>,
}

impl ChatRowCache {
    pub(super) fn prepare(&mut self, ui: &Ui, count: usize) {
        let width = ui.available_width();
        let pixels_per_point = ui.ctx().pixels_per_point();
        if self.width != width
            || self.pixels_per_point != pixels_per_point
            || self.style.as_ref() != Some(ui.style())
        {
            self.rows.clear();
            self.width = width;
            self.pixels_per_point = pixels_per_point;
            self.style = Some(ui.style().clone());
        }
        self.rows.resize(count, None);
    }

    pub(super) fn height_at(&self, index: usize) -> Option<f32> {
        self.rows
            .get(index)
            .and_then(|row| row.as_ref().map(|(_, height)| *height))
    }

    /// 已测量行的平均高度：未测量行的估算值（虚拟滚动用）。
    pub(super) fn average_height(&self) -> Option<f32> {
        let mut total = 0.0;
        let mut count = 0;
        for row in &self.rows {
            if let Some((_, height)) = row {
                total += height;
                count += 1;
            }
        }
        (count > 0).then(|| total / count as f32)
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        index: usize,
        fingerprint: u64,
        render: impl FnOnce(&mut Ui),
    ) {
        if let Some((cached_fingerprint, height)) = self.rows[index] {
            let rect = Rect::from_min_size(
                ui.next_widget_position(),
                Vec2::new(self.width, height),
            );
            if cached_fingerprint == fingerprint && !ui.is_rect_visible(rect) {
                ui.allocate_space(rect.size());
                return;
            }
        }
        let response = ui.push_id(index, |ui| {
            render(ui);
            ui.add_space(ui.spacing().item_spacing.y);
        });
        self.rows[index] = Some((
            fingerprint,
            response.response.rect.height(),
        ));
    }
}

/// 只哈希文本长度与首尾各 16 字节：O(1) 的内容变化检测，
/// 长文本也只付常数开销（消息内容在会话内不可变，这已足够）。
fn hash_text_head_tail(hasher: &mut DefaultHasher, text: &str) {
    const SLICE: usize = 16;
    let bytes = text.as_bytes();
    bytes.len().hash(hasher);
    let head = &bytes[..bytes.len().min(SLICE)];
    head.hash(hasher);
    if bytes.len() > SLICE {
        bytes[bytes.len() - SLICE..].hash(hasher);
    }
}

pub(super) fn message_layout_fingerprint(message: &NapcatMessage) -> u64 {
    let mut hash = DefaultHasher::new();
    message.data.sender.nickname.hash(&mut hash);
    (message.data.self_id == message.data.user_id).hash(&mut hash);
    for chain in &message.data.message {
        match &chain.variant {
            NapcatMessageChainType::Text { data } => {
                0_u8.hash(&mut hash);
                // NapCat 消息到达后内容不变（无编辑功能）：只哈希长度与
                // 首尾片段，避免长会话每帧对全部历史消息做全文哈希。
                hash_text_head_tail(&mut hash, &data.text);
            },
            NapcatMessageChainType::Image { data } => {
                1_u8.hash(&mut hash);
                data.local_path.hash(&mut hash);
                data.url.hash(&mut hash);
            },
            _ => {},
        }
    }
    hash.finish()
}
