use bevy::log::info_span;
use bevy_egui::egui::{
    pos2,
    vec2,
    Align2,
    CentralPanel,
    Color32,
    FontId,
    Frame,
    Label,
    Panel,
    Rect,
    Stroke,
    StrokeKind,
    Ui,
};

use super::{
    timeline::timeline_time_label,
    ReplayMode,
    ReplayStudio,
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ReplayWorkspaceTab {
    #[default]
    Project,
    Edit,
    Camera,
    Director,
    Export,
}

pub(super) fn workspace_header(root_ui: &mut Ui, studio: &mut ReplayStudio) {
    let _profile = info_span!("workspace_header").entered();
    Panel::top("replay-workspace-header").show(root_ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.strong("回放合成");
            ui.label(match studio.mode {
                ReplayMode::Idle => "准备",
                ReplayMode::Recording => "● 录制中",
                ReplayMode::Playing => "▶ 预览中",
                ReplayMode::Paused => "预览已暂停",
            });
            ui.separator();
            for (tab, label) in [
                (ReplayWorkspaceTab::Project, "项目"),
                (ReplayWorkspaceTab::Edit, "编辑"),
                (ReplayWorkspaceTab::Camera, "镜头"),
                (ReplayWorkspaceTab::Director, "导演"),
                (ReplayWorkspaceTab::Export, "输出"),
            ] {
                let response = ui.selectable_value(&mut studio.workspace_tab, tab, label);
                if response.clicked() {
                    studio.active_custom_tab = None;
                }
            }
            // 快速分割创建的分段视图 tab：随回放项目持久化。
            if let Some(replay) = studio.replay.as_ref() {
                let mut removed_tab = None;
                for tab in &replay.custom_tabs {
                    let selected = studio.active_custom_tab == Some(tab.id);
                    let response = ui.selectable_label(
                        selected,
                        format!("✂ {}", tab.label),
                    );
                    if response.clicked() {
                        studio.workspace_tab = ReplayWorkspaceTab::Edit;
                        studio.active_custom_tab = Some(tab.id);
                    }
                    response.on_hover_text(format!(
                        "分段视图：只显示/编辑 {} ~ {} 的台词（随项目保存）",
                        timeline_time_label(tab.start_ms),
                        timeline_time_label(tab.end_ms),
                    ));
                    if ui.small_button("×").clicked() {
                        removed_tab = Some(tab.id);
                    }
                }
                if let Some(removed) = removed_tab {
                    if studio.active_custom_tab == Some(removed) {
                        studio.active_custom_tab = None;
                    }
                    if let Some(replay) = studio.replay.as_mut() {
                        replay.custom_tabs.retain(|tab| tab.id != removed);
                    }
                    // 分段移除也立即触发自动保存，避免重启后复活。
                    studio.pending_replay_autosave = true;
                }
            }
            ui.separator();
            if ui.button("返回主界面").clicked() {
                studio.panel_open = false;
            }
        });
        if !studio.status.is_empty() {
            ui.add(Label::new(&studio.status).truncate());
        }
    });
}

/// Keeps the whole export frame visible, regardless of panel sizes.
pub(super) fn fit_preview_rect(available: Rect, aspect: f32) -> Rect {
    let width = available
        .width()
        .max(1.0)
        .min(available.height().max(1.0) * aspect);
    Rect::from_center_size(
        available.center(),
        vec2(width, width / aspect),
    )
    .intersect(available)
}

pub(super) fn workspace_preview(root_ui: &mut Ui, studio: &mut ReplayStudio) {
    let _profile = info_span!("workspace_preview").entered();
    CentralPanel::default()
        .frame(Frame::NONE)
        .show(root_ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("合成预览");
                if let Some(replay) = &studio.replay {
                    ui.add(Label::new(&replay.title).truncate());
                } else {
                    ui.label("在“项目”中从聊天生成或载入回放");
                }
            });
            let screen = ui.ctx().content_rect();
            let available = ui.available_rect_before_wrap().shrink(8.0);
            let preview = fit_preview_rect(
                available,
                screen.width() / screen.height().max(1.0),
            );
            // The scene camera uses this rectangle; egui stays on its own full-window camera.
            studio.preview_rect = Some(preview);
            ui.painter().rect_stroke(
                preview,
                0.0,
                Stroke::new(1.0, Color32::from_gray(65)),
                StrokeKind::Outside,
            );
            if studio.replay.is_none() {
                ui.painter().text(
                    pos2(preview.center().x, preview.top() + 28.0),
                    Align2::CENTER_TOP,
                    "场景预览",
                    FontId::proportional(16.0),
                    Color32::LIGHT_GRAY,
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use bevy_egui::egui::{
        pos2,
        vec2,
        Rect,
    };

    use super::fit_preview_rect;

    #[test]
    fn composition_stays_inside_resized_panel_without_cropping() {
        for size in [vec2(900.0, 400.0), vec2(320.0, 700.0), vec2(800.0, 450.0)] {
            let available = Rect::from_min_size(pos2(350.0, 60.0), size);
            let preview = fit_preview_rect(available, 16.0 / 9.0);
            assert!(available.contains_rect(preview));
            assert!((preview.width() / preview.height() - 16.0 / 9.0).abs() < 0.001);
            assert_eq!(preview.center(), available.center());
        }
    }
}
