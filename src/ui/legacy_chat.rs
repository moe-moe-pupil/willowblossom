use super::*;

const LEGACY_SIDEBAR_WIDTH: f32 = 238.0;
const LEGACY_CHAT_CARD_HEIGHT: f32 = 68.0;
const LEGACY_SIDEBAR_CONTROLS_HEIGHT: f32 = 116.0;

#[derive(Default)]
pub(super) struct ChatWorkspaceState {
    selected_target: Option<String>,
    search: String,
}

pub(super) fn show_chat_workspace(
    ctx: &Context,
    root_ui: &mut Ui,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    deepseek_manager: &DeepseekManager,
    napcat_sender: Option<&NapcatIOSender>,
    ime: &mut ImeManager,
    chat_input_msgs: &mut HashMap<String, String>,
    chat_scroll_states: &mut HashMap<String, ChatScrollState>,
    image_textures: &mut HashMap<String, TextureHandle>,
    state: &mut ChatWorkspaceState,
    legacy_layout: bool,
) -> Option<String> {
    let views = workspace_target_views(manager);
    repair_selected_target(state, &views);

    if !legacy_layout {
        egui::Panel::right("chat_workspace_summary_panel")
            .resizable(true)
            .default_size(260.0)
            .size_range(200.0..=420.0)
            .show(root_ui, |ui| {
                summary_panel(ui, manager, deepseek_manager);
            });
    }

    let mut selected_from_sidebar = None;
    let sidebar_frame = if legacy_layout {
        egui::Frame::new()
            .fill(egui::Color32::WHITE)
            .inner_margin(egui::Margin::same(10))
    } else {
        egui::Frame::side_top_panel(&ctx.style_of(ctx.theme())).inner_margin(egui::Margin::same(10))
    };
    egui::Panel::left("chat_workspace_sidebar")
        .resizable(true)
        .default_size(LEGACY_SIDEBAR_WIDTH)
        .size_range(190.0..=360.0)
        .frame(sidebar_frame)
        .show(root_ui, |ui| {
            if legacy_layout {
                use_legacy_light_visuals(ui);
                ui.heading("Moonberry 聊天");
            } else {
                ui.heading("聊天工作区");
            }
            if let Some(group_name) = manager.current_trpg_group.as_deref() {
                ui.small(format!("当前TRPG组：{group_name}"));
            } else {
                ui.small("尚未选择TRPG组");
            }
            ui.separator();

            if !legacy_layout {
                ui.add(
                    egui::TextEdit::singleline(&mut state.search)
                        .hint_text("搜索聊天")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
            }

            let reserved_height = if legacy_layout { LEGACY_SIDEBAR_CONTROLS_HEIGHT } else { 0.0 };
            let list_height = (ui.available_height() - reserved_height).max(80.0);
            egui::ScrollArea::vertical()
                .id_salt(("chat_workspace_targets", legacy_layout))
                .auto_shrink([false, false])
                .max_height(list_height)
                .show(ui, |ui| {
                    if views.is_empty() {
                        ui.label("还没有保存的聊天。");
                        return;
                    }

                    let normalized_search = state.search.trim().to_lowercase();
                    let mut visible_count = 0usize;
                    for target in &views {
                        let display_name = target_display_name(manager, &target.target_id);
                        let preview = manager
                            .messages
                            .get(&target.target_id)
                            .and_then(|messages| messages.last())
                            .map(last_message_preview)
                            .unwrap_or_else(|| "还没有消息".to_owned());
                        if !normalized_search.is_empty()
                            && !display_name.to_lowercase().contains(&normalized_search)
                            && !target.target_id.to_lowercase().contains(&normalized_search)
                            && !preview.to_lowercase().contains(&normalized_search)
                        {
                            continue;
                        }
                        visible_count += 1;

                        let selected =
                            state.selected_target.as_deref() == Some(target.target_id.as_str());
                        let turn = target_turn_label(manager, &target.target_id);
                        let response = chat_target_card(
                            ui,
                            manager,
                            &target.target_id,
                            &display_name,
                            &preview,
                            target.unread_count,
                            turn.as_deref(),
                            selected,
                            legacy_layout,
                        );
                        if response.clicked() {
                            selected_from_sidebar = Some(target.target_id.clone());
                        }
                        ui.add_space(4.0);
                    }

                    if visible_count == 0 {
                        ui.label("没有匹配的聊天。");
                    }
                });

            if legacy_layout {
                ui.separator();
                legacy_sidebar_controls(ui, manager, &mut state.search);
            }
        });

    if let Some(target_id) = selected_from_sidebar {
        state.selected_target = Some(target_id.clone());
        manager.pending_chat_targets.remove(&target_id);
    }

    let active_target = state.selected_target.clone();
    let central_frame = if legacy_layout {
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(245, 246, 248))
            .inner_margin(egui::Margin::same(12))
    } else {
        egui::Frame::central_panel(&ctx.style_of(ctx.theme())).inner_margin(egui::Margin::same(12))
    };
    egui::CentralPanel::default()
        .frame(central_frame)
        .show(root_ui, |ui| {
            if legacy_layout {
                use_legacy_light_visuals(ui);
            }
            let Some(target_id) = active_target.as_deref() else {
                ui.centered_and_justified(|ui| {
                    ui.label("从左侧选择一个聊天。");
                });
                return;
            };

            let display_name = target_display_name(manager, target_id);
            ui.horizontal(|ui| {
                ui.heading(display_name);
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.small(target_id);
                        ui.small(chat_target_kind_label(
                            manager, target_id,
                        ));
                    },
                );
            });
            ui.separator();

            let messages = manager.messages.get(target_id).cloned().unwrap_or_default();
            let unread_count = target_unread_count(manager, target_id);
            chat_body_ui(
                ui,
                ctx,
                &messages,
                unread_count,
                napcat_sender,
                target_id,
                chat_input_msgs,
                targets_for_target(manager, target_id),
                ime,
                chat_scroll_states,
                image_textures,
                None,
            );
            mark_target_read(manager, target_id, messages.len());
        });

    active_target
}

fn workspace_target_views(manager: &NapcatMessageManager) -> Vec<ChatListTargetView> {
    let mut views = chat_list_target_views(manager, None);
    let Some(group) = manager
        .current_group()
        .filter(|group| group.battle_sort_by_turn)
    else {
        return views;
    };

    views.sort_by(|left, right| {
        let left_turn = group
            .player_turns
            .get(&left.target_id)
            .map(|turn| turn.turns_passed)
            .unwrap_or(u32::MAX);
        let right_turn = group
            .player_turns
            .get(&right.target_id)
            .map(|turn| turn.turns_passed)
            .unwrap_or(u32::MAX);
        left_turn
            .cmp(&right_turn)
            .then_with(|| right.last_time.cmp(&left.last_time))
            .then_with(|| left.target_id.cmp(&right.target_id))
    });
    views
}

fn repair_selected_target(state: &mut ChatWorkspaceState, views: &[ChatListTargetView]) {
    let selection_is_valid = state
        .selected_target
        .as_deref()
        .is_some_and(|selected| views.iter().any(|target| target.target_id == selected));
    if selection_is_valid {
        return;
    }
    state.selected_target = views.first().map(|target| target.target_id.clone());
}

fn legacy_sidebar_controls(
    ui: &mut Ui,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    search: &mut String,
) {
    let mut changed = false;
    if let Some(group_name) = manager.current_trpg_group.clone() {
        if let Some(group) = manager.trpg_groups.get_mut(&group_name) {
            changed |= ui
                .checkbox(
                    &mut group.allow_join_requests,
                    "接受新团员？",
                )
                .on_hover_text("允许未加入当前TRPG组的好友申请加入")
                .changed();
            changed |= ui
                .checkbox(
                    &mut group.battle_sort_by_turn,
                    "按回合数排列？",
                )
                .on_hover_text("保持玩家按回合数从小到大排列")
                .changed();
            changed |= ui
                .checkbox(
                    &mut group.battle_negative_enabled,
                    "启用消极？",
                )
                .on_hover_text("启用当前TRPG组的旧版消极倒计时规则")
                .changed();
        }
    } else {
        ui.small("选择TRPG组后可使用旧版团务开关。");
    }
    ui.add(
        egui::TextEdit::singleline(search)
            .hint_text("搜索聊天")
            .desired_width(f32::INFINITY),
    );
    if changed {
        manager.persist().ok();
    }
}

#[allow(clippy::too_many_arguments)]
fn chat_target_card(
    ui: &mut Ui,
    manager: &NapcatMessageManager,
    target_id: &str,
    display_name: &str,
    preview: &str,
    unread_count: usize,
    turn: Option<&str>,
    selected: bool,
    legacy_layout: bool,
) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, LEGACY_CHAT_CARD_HEIGHT),
        Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let fill = if selected {
            if legacy_layout {
                egui::Color32::from_rgb(235, 235, 235)
            } else {
                visuals.selection.bg_fill
            }
        } else if response.hovered() {
            visuals.widgets.hovered.weak_bg_fill
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 5.0, fill);

        let accent = manager
            .player_chat_window_color(target_id)
            .map(|[red, green, blue]| egui::Color32::from_rgb(red, green, blue))
            .unwrap_or_else(|| egui::Color32::from_rgb(91, 126, 188));
        let avatar_center = egui::pos2(rect.left() + 24.0, rect.center().y);
        ui.painter().circle_filled(avatar_center, 19.0, accent);
        let initial = display_name.chars().next().unwrap_or('?').to_string();
        ui.painter().text(
            avatar_center,
            egui::Align2::CENTER_CENTER,
            initial,
            egui::FontId::proportional(18.0),
            egui::Color32::WHITE,
        );

        let text_left = rect.left() + 51.0;
        let primary_color =
            if legacy_layout { egui::Color32::from_gray(35) } else { visuals.text_color() };
        let secondary_color = if legacy_layout {
            egui::Color32::from_gray(105)
        } else {
            visuals.weak_text_color()
        };
        ui.painter().text(
            egui::pos2(text_left, rect.top() + 10.0),
            egui::Align2::LEFT_TOP,
            truncate_preview(display_name, 22),
            egui::FontId::proportional(14.0),
            primary_color,
        );
        ui.painter().text(
            egui::pos2(text_left, rect.top() + 36.0),
            egui::Align2::LEFT_TOP,
            truncate_preview(preview, 30),
            egui::FontId::proportional(11.0),
            secondary_color,
        );

        if let Some(turn) = turn {
            ui.painter().text(
                egui::pos2(rect.right() - 8.0, rect.top() + 9.0),
                egui::Align2::RIGHT_TOP,
                turn,
                egui::FontId::proportional(10.0),
                accent,
            );
        }
        if unread_count > 0 {
            let badge_center = egui::pos2(
                rect.right() - 14.0,
                rect.bottom() - 15.0,
            );
            ui.painter().circle_filled(
                badge_center,
                10.0,
                egui::Color32::from_rgb(220, 70, 70),
            );
            ui.painter().text(
                badge_center,
                egui::Align2::CENTER_CENTER,
                unread_count.min(99).to_string(),
                egui::FontId::proportional(9.0),
                egui::Color32::WHITE,
            );
        }
    }

    response.on_hover_text(target_id)
}

fn target_turn_label(manager: &NapcatMessageManager, target_id: &str) -> Option<String> {
    manager
        .current_group()
        .and_then(|group| group.player_turns.get(target_id))
        .map(|turn| format!("{}轮", turn.turns_passed))
}

fn last_message_preview(message: &NapcatMessage) -> String {
    let mut parts = Vec::new();
    for chain in &message.data.message {
        match &chain.variant {
            NapcatMessageChainType::Text { data } => parts.push(data.text.trim().to_owned()),
            NapcatMessageChainType::Image { .. } => parts.push("[图片]".to_owned()),
            NapcatMessageChainType::ForwardedReplay { .. } => parts.push("[转发记录]".to_owned()),
            NapcatMessageChainType::Unsupported => parts.push("[不支持的消息]".to_owned()),
            NapcatMessageChainType::Source(_) => {},
        }
    }
    let preview = parts.join(" ").replace(['\r', '\n'], " ");
    if preview.trim().is_empty() {
        "还没有可预览的文字".to_owned()
    } else {
        preview
    }
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        truncated.push('…');
    }
    truncated
}

fn use_legacy_light_visuals(ui: &mut Ui) { *ui.visuals_mut() = egui::Visuals::light(); }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_truncation_is_unicode_safe() {
        assert_eq!(
            truncate_preview("柳絮聊天窗口", 4),
            "柳絮聊天…"
        );
        assert_eq!(truncate_preview("short", 8), "short");
    }

    #[test]
    fn selected_target_repairs_to_first_available_chat() {
        let mut state = ChatWorkspaceState {
            selected_target: Some("missing".to_owned()),
            search: String::new(),
        };
        let views = vec![ChatListTargetView {
            target_id: "42".to_owned(),
            message_count: 1,
            total_message_count: 1,
            unread_count: 0,
            last_time: 1,
            creation_incomplete: false,
        }];

        repair_selected_target(&mut state, &views);

        assert_eq!(
            state.selected_target.as_deref(),
            Some("42")
        );
    }
}
