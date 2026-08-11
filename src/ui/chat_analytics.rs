use std::{
    collections::{
        BTreeMap,
        HashSet,
    },
    time::{
        Duration,
        SystemTime,
        UNIX_EPOCH,
    },
};

use bevy_egui::egui;

use crate::{
    napcat::{
        NapcatMessage,
        NapcatMessageChainType,
        NapcatMessageManager,
        NapcatMessageType,
        TrpgGroup,
    },
    voxel::{
        VoxelEditorState,
        VoxelTeleportDestination,
    },
};

const GM_AUTO_CHAT_STAY_WARNING_SECS: f64 = 60.0;
const GM_AUTO_CHAT_TOAST_VISIBLE_SECS: f64 = 8.0;

#[derive(Debug, Default, PartialEq, Eq)]
struct TimingSummary {
    completed_samples: usize,
    average_secs: Option<u64>,
    shortest_secs: Option<u64>,
    longest_secs: Option<u64>,
    current_elapsed_secs: Option<u64>,
    estimated_remaining_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PrivateChatTiming {
    pub(super) target_id: String,
    pub(super) completed_samples: usize,
    pub(super) average_secs: Option<u64>,
}

#[derive(Debug)]
struct GmAutoChatSession {
    group_name: String,
    world_turn: u32,
    queue: Vec<String>,
    active_index: usize,
    active_since_secs: f64,
    stay_warning_shown: bool,
}

#[derive(Debug)]
struct GmAutoChatToast {
    message: String,
    shown_at_secs: f64,
}

#[derive(Debug, Default)]
pub(super) struct GmAutoChatModeState {
    enabled: bool,
    session: Option<GmAutoChatSession>,
    toast: Option<GmAutoChatToast>,
}

impl GmAutoChatModeState {
    pub(super) fn enabled(&self) -> bool { self.enabled }

    pub(super) fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.session = None;
        if !enabled {
            self.toast = None;
        }
    }

    pub(super) fn active_target(&self) -> Option<&str> {
        let session = self.session.as_ref()?;
        session.queue.get(session.active_index).map(String::as_str)
    }

    pub(super) fn progress(&self) -> Option<(usize, usize)> {
        let session = self.session.as_ref()?;
        (session.active_index < session.queue.len()).then_some((
            session.active_index + 1,
            session.queue.len(),
        ))
    }

    fn sync_session(
        &mut self,
        group_name: &str,
        world_turn: u32,
        ranked_waiting_targets: &[String],
        waiting_targets: &HashSet<String>,
        now_secs: f64,
    ) -> Option<String> {
        if !self.enabled {
            self.session = None;
            return None;
        }

        let starts_new_session = self.session.as_ref().is_none_or(|session| {
            session.group_name != group_name || session.world_turn != world_turn
        });
        if starts_new_session {
            self.session = (!ranked_waiting_targets.is_empty()).then(|| GmAutoChatSession {
                group_name: group_name.to_owned(),
                world_turn,
                queue: ranked_waiting_targets.to_vec(),
                active_index: 0,
                active_since_secs: now_secs,
                stay_warning_shown: false,
            });
            return self.active_target().map(str::to_owned);
        }

        let Some(session) = self.session.as_mut() else {
            return None;
        };
        let previous_index = session.active_index;
        while session
            .queue
            .get(session.active_index)
            .is_some_and(|target_id| !waiting_targets.contains(target_id))
        {
            session.active_index += 1;
        }
        if session.active_index == previous_index {
            return None;
        }

        session.active_since_secs = now_secs;
        session.stay_warning_shown = false;
        session.queue.get(session.active_index).cloned()
    }

    fn take_stay_warning(&mut self, now_secs: f64) -> Option<String> {
        let session = self.session.as_mut()?;
        if now_secs < session.active_since_secs {
            session.active_since_secs = now_secs;
            return None;
        }
        if session.stay_warning_shown
            || now_secs - session.active_since_secs < GM_AUTO_CHAT_STAY_WARNING_SECS
        {
            return None;
        }

        let target_id = session.queue.get(session.active_index)?.clone();
        session.stay_warning_shown = true;
        Some(target_id)
    }

    fn show_toast(&mut self, message: String, now_secs: f64) {
        self.toast = Some(GmAutoChatToast {
            message,
            shown_at_secs: now_secs,
        });
    }
}

pub(super) fn show_chat_analytics_window(
    ctx: &egui::Context,
    manager: &NapcatMessageManager,
    target_id: &str,
    display_name: &str,
    group_name: Option<&str>,
    open: &mut bool,
) {
    let now = unix_timestamp_secs();
    let messages = manager
        .messages
        .get(target_id)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let group = group_name.and_then(|name| manager.trpg_groups.get(name));
    let session_started_at = group.and_then(|group| current_session_started_at(manager, group));
    let response = session_started_at
        .map(|started_at| private_response_timing(messages, started_at, now))
        .unwrap_or_default();
    let world_turn = group
        .zip(session_started_at)
        .map(|(group, started_at)| world_turn_timing(manager, group, started_at, now));
    let title = format!("响应数据 · {display_name}");

    egui::Window::new(title)
        .id(egui::Id::new((
            "chat_analytics",
            target_id,
        )))
        .open(open)
        .default_size(egui::vec2(280.0, 170.0))
        .min_width(250.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.heading("私聊回复");
            timing_average_ui(ui, "平均回复时间", &response);
            if let (Some(shortest), Some(longest)) = (
                response.shortest_secs,
                response.longest_secs,
            ) {
                ui.small(format!(
                    "最快 {} · 最慢 {}",
                    format_duration(shortest),
                    format_duration(longest)
                ));
            }
            match response.current_elapsed_secs {
                Some(elapsed) => {
                    ui.label(format!(
                        "当前已等待：{}",
                        format_duration(elapsed)
                    ));
                    if let Some(remaining) = response.estimated_remaining_secs {
                        ui.small(format!(
                            "按平均速度预计还需 {}",
                            format_duration(remaining)
                        ));
                    }
                },
                None => {
                    ui.small("当前没有等待回复的玩家消息");
                },
            }

            ui.separator();
            ui.heading("世界回合");
            match world_turn {
                Some(summary) => {
                    timing_average_ui(ui, "平均完成时间", &summary);
                    match summary.current_elapsed_secs {
                        Some(elapsed) => {
                            ui.label(format!(
                                "当前回合已进行：{}",
                                format_duration(elapsed)
                            ));
                            if let Some(remaining) = summary.estimated_remaining_secs {
                                ui.small(format!(
                                    "按平均速度预计还需 {}",
                                    format_duration(remaining)
                                ));
                            }
                        },
                        None => {
                            ui.small("当前回合还没有可计时的聊天记录");
                        },
                    };
                },
                None => {
                    ui.small("此私聊不在当前TRPG组中，暂无世界回合数据");
                },
            }
            if let Some(started_at) = session_started_at {
                ui.small(format!(
                    "仅统计本次开团（起点 {}）；连续玩家消息按一次回复计算。",
                    format_clock(started_at)
                ));
            } else {
                ui.small("尚未开团，当前没有团期统计数据。");
            }
        });
}

pub(super) fn ranked_private_chat_timings(
    manager: &NapcatMessageManager,
    group: &TrpgGroup,
) -> Vec<PrivateChatTiming> {
    let now = unix_timestamp_secs();
    let session_started_at = current_session_started_at(manager, group);
    let mut seen = HashSet::new();
    let mut timings = group
        .players
        .iter()
        .filter(|target_id| seen.insert((*target_id).clone()))
        .map(|target_id| {
            let summary = session_started_at
                .map(|started_at| {
                    private_response_timing(
                        manager
                            .messages
                            .get(target_id)
                            .map(Vec::as_slice)
                            .unwrap_or_default(),
                        started_at,
                        now,
                    )
                })
                .unwrap_or_default();
            PrivateChatTiming {
                target_id: target_id.clone(),
                completed_samples: summary.completed_samples,
                average_secs: summary.average_secs,
            }
        })
        .collect::<Vec<_>>();
    sort_private_chat_timings(&mut timings);
    timings
}

fn sort_private_chat_timings(timings: &mut [PrivateChatTiming]) {
    timings.sort_by(|left, right| {
        right
            .average_secs
            .cmp(&left.average_secs)
            .then_with(|| left.target_id.cmp(&right.target_id))
    });
}

fn current_session_started_at(manager: &NapcatMessageManager, group: &TrpgGroup) -> Option<u64> {
    if !group.campaign_active {
        return None;
    }
    (group.campaign_started_at > 0)
        .then_some(group.campaign_started_at)
        .or_else(|| inferred_current_session_start(manager, group))
}

pub(super) fn update_gm_auto_chat_mode(
    ctx: &egui::Context,
    manager: &mut NapcatMessageManager,
    state: &mut GmAutoChatModeState,
    voxel_editor: &mut VoxelEditorState,
) -> bool {
    let now_secs = ctx.input(|input| input.time);
    if !state.enabled() {
        state.session = None;
        return false;
    }

    let Some((group_name, world_turn, ranked_waiting, waiting_targets)) = manager
        .current_trpg_group
        .as_deref()
        .and_then(|group_name| {
            let group = manager.trpg_groups.get(group_name)?;
            let waiting_targets = group
                .players
                .iter()
                .filter(|target_id| {
                    group
                        .player_turns
                        .get(*target_id)
                        .map(|turn| !turn.acted && !turn.skipped)
                        .unwrap_or(true)
                })
                .cloned()
                .collect::<HashSet<_>>();
            let ranked_waiting = ranked_private_chat_timings(manager, group)
                .into_iter()
                .map(|timing| timing.target_id)
                .filter(|target_id| {
                    target_id.parse::<u64>().is_ok() && waiting_targets.contains(target_id)
                })
                .collect::<Vec<_>>();
            Some((
                group_name.to_owned(),
                group.world_turn,
                ranked_waiting,
                waiting_targets,
            ))
        })
    else {
        state.session = None;
        return false;
    };

    let selected_target = state.sync_session(
        &group_name,
        world_turn,
        &ranked_waiting,
        &waiting_targets,
        now_secs,
    );
    let mut manager_changed = false;
    if let Some(target_id) = selected_target {
        manager_changed |= super::focus_gm_auto_chat_target_window(ctx, manager, &target_id);
        if let Ok(user_id) = target_id.parse::<u64>() {
            voxel_editor.request_teleport(VoxelTeleportDestination::PlayerStandee(
                user_id,
            ));
        }
    }

    if let Some(target_id) = state.take_stay_warning(now_secs) {
        let display_name = super::target_display_name(manager, &target_id);
        state.show_toast(
            format!(
                "你已在 {display_name} 的聊天窗口停留 1 分钟，请处理消息或点击“行动”继续巡查。"
            ),
            now_secs,
        );
    }
    ctx.request_repaint_after(Duration::from_secs(1));
    manager_changed
}

pub(super) fn keep_gm_auto_chat_window_topmost(
    ctx: &egui::Context,
    manager: &mut NapcatMessageManager,
    state: &GmAutoChatModeState,
) -> bool {
    let Some(target_id) = state.active_target() else {
        return false;
    };
    super::focus_gm_auto_chat_target_window(ctx, manager, target_id)
}

pub(super) fn show_gm_auto_chat_toast(ctx: &egui::Context, state: &mut GmAutoChatModeState) {
    let now_secs = ctx.input(|input| input.time);
    let Some(toast) = state.toast.as_ref() else {
        return;
    };
    if now_secs < toast.shown_at_secs
        || now_secs - toast.shown_at_secs >= GM_AUTO_CHAT_TOAST_VISIBLE_SECS
    {
        state.toast = None;
        return;
    }
    let message = toast.message.clone();

    egui::Area::new(egui::Id::new("gm_auto_chat_stay_toast"))
        .anchor(
            egui::Align2::RIGHT_TOP,
            egui::vec2(-16.0, 64.0),
        )
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(
                    48, 35, 12, 245,
                ))
                .stroke(egui::Stroke::new(
                    1.5,
                    egui::Color32::from_rgb(245, 180, 60),
                ))
                .corner_radius(7)
                .inner_margin(egui::Margin::symmetric(12, 9))
                .show(ui, |ui| {
                    ui.set_max_width(360.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 211, 115),
                        "自动巡查提醒",
                    );
                    ui.label(message);
                });
        });
    ctx.request_repaint_after(Duration::from_millis(250));
}

fn timing_average_ui(ui: &mut egui::Ui, label: &str, summary: &TimingSummary) {
    if let Some(average) = summary.average_secs {
        ui.label(format!(
            "{label}：{}（{}次）",
            format_duration(average),
            summary.completed_samples
        ));
    } else {
        ui.label(format!("{label}：数据不足"));
    }
}

fn private_response_timing(
    messages: &[NapcatMessage],
    session_started_at: u64,
    now: u64,
) -> TimingSummary {
    let mut ordered = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.data.time >= session_started_at)
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(index, message)| (message.data.time, *index));

    let mut pending_since = None;
    let mut samples = Vec::new();
    for (_, message) in ordered {
        if !matches!(
            message.data.message_type,
            NapcatMessageType::Private
        ) {
            continue;
        }
        if message.data.user_id == message.data.self_id {
            if let Some(started_at) = pending_since.take() {
                samples.push(message.data.time.saturating_sub(started_at));
            }
        } else {
            // A burst of player messages needs one response. Measure from the latest
            // message, which is when the GM had the complete prompt to answer.
            pending_since = Some(message.data.time);
        }
    }

    timing_summary(&samples, pending_since, now)
}

fn world_turn_timing(
    manager: &NapcatMessageManager,
    group: &TrpgGroup,
    session_started_at: u64,
    now: u64,
) -> TimingSummary {
    let target_ids = group.players.iter().chain(group.group_chats.iter());
    let mut turn_starts = BTreeMap::<u32, u64>::new();

    for target_id in target_ids {
        let Some(messages) = manager.messages.get(target_id) else {
            continue;
        };
        let Some(snapshots) = manager.replay_snapshots.get(target_id) else {
            continue;
        };
        for (message, snapshot) in messages.iter().zip(snapshots.iter()) {
            let Some(snapshot) = snapshot else {
                continue;
            };
            if message.data.time < session_started_at
                || message.data.campaign_id != group.campaign_id
            {
                continue;
            }
            turn_starts
                .entry(snapshot.turn_index)
                .and_modify(|started_at| *started_at = (*started_at).min(message.data.time))
                .or_insert(message.data.time);
        }
    }

    world_turn_summary(&turn_starts, group.world_turn, now)
}

fn inferred_current_session_start(
    manager: &NapcatMessageManager,
    group: &TrpgGroup,
) -> Option<u64> {
    group
        .players
        .iter()
        .filter_map(|target_id| manager.messages.get(target_id))
        .flatten()
        .filter(|message| message.data.user_id == message.data.self_id)
        .filter(|message| {
            let normalized = message
                .data
                .message
                .iter()
                .filter_map(|chain| match &chain.variant {
                    NapcatMessageChainType::Text { data } => Some(data.text.as_str()),
                    _ => None,
                })
                .collect::<String>()
                .to_lowercase()
                .replace([' ', ',', '，'], "");
            normalized.contains("ok兄弟萌开团了")
        })
        .map(|message| message.data.time)
        .max()
}

fn world_turn_summary(
    turn_starts: &BTreeMap<u32, u64>,
    current_turn: u32,
    now: u64,
) -> TimingSummary {
    let mut samples = Vec::new();
    let starts = turn_starts.iter().collect::<Vec<_>>();
    for pair in starts.windows(2) {
        let [(turn, started_at), (next_turn, next_started_at)] = pair else {
            continue;
        };
        if **next_turn == turn.saturating_add(1) && **turn < current_turn {
            samples.push(next_started_at.saturating_sub(**started_at));
        }
    }
    let current_started_at = starts
        .iter()
        .find_map(|(turn, started_at)| (**turn == current_turn).then_some(**started_at));

    timing_summary(&samples, current_started_at, now)
}

fn timing_summary(samples: &[u64], current_started_at: Option<u64>, now: u64) -> TimingSummary {
    let average_secs = (!samples.is_empty()).then(|| {
        samples
            .iter()
            .copied()
            .map(u128::from)
            .sum::<u128>()
            .checked_div(samples.len() as u128)
            .unwrap_or_default()
            .min(u64::MAX as u128) as u64
    });
    let current_elapsed_secs = current_started_at.map(|started_at| now.saturating_sub(started_at));
    let estimated_remaining_secs = average_secs
        .zip(current_elapsed_secs)
        .map(|(average, elapsed)| average.saturating_sub(elapsed));
    TimingSummary {
        completed_samples: samples.len(),
        average_secs,
        shortest_secs: samples.iter().copied().min(),
        longest_secs: samples.iter().copied().max(),
        current_elapsed_secs,
        estimated_remaining_secs,
    }
}

pub(super) fn format_duration(total_secs: u64) -> String {
    let days = total_secs / 86_400;
    let hours = total_secs % 86_400 / 3_600;
    let minutes = total_secs % 3_600 / 60;
    let seconds = total_secs % 60;
    if days > 0 {
        format!("{days}天 {hours}小时")
    } else if hours > 0 {
        format!("{hours}小时 {minutes}分")
    } else if minutes > 0 {
        format!("{minutes}分 {seconds}秒")
    } else {
        format!("{seconds}秒")
    }
}

fn format_clock(timestamp: u64) -> String {
    let seconds = timestamp % 86_400;
    let hour = (seconds / 3_600 + 8) % 24;
    let minute = seconds % 3_600 / 60;
    let second = seconds % 60;
    format!("{hour:02}:{minute:02}:{second:02}")
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::napcat::{
        NapcatMessageData,
        NapcatSender,
        Visibility,
    };

    fn private_message(time: u64, user_id: u64, self_id: u64) -> NapcatMessage {
        NapcatMessage {
            data: NapcatMessageData {
                time,
                message_type: NapcatMessageType::Private,
                message_id: None,
                message: Vec::new(),
                self_id,
                user_id,
                group_id: None,
                group_name: None,
                target_id: Some(if user_id == self_id { 2 } else { self_id }),
                sender: NapcatSender {
                    user_id,
                    nickname: String::new(),
                },
                campaign_id: "default".to_owned(),
                character_id: None,
                party_id: None,
                visibility: Visibility::default(),
                access_scope_resolved: true,
            },
        }
    }

    #[test]
    fn response_timing_pairs_message_bursts_with_next_reply() {
        let messages = vec![
            private_message(10, 2, 99),
            private_message(90, 99, 99),
            private_message(100, 2, 99),
            private_message(110, 2, 99),
            private_message(140, 99, 99),
            private_message(200, 2, 99),
            private_message(260, 99, 99),
            private_message(300, 2, 99),
        ];

        assert_eq!(
            private_response_timing(&messages, 100, 330),
            TimingSummary {
                completed_samples: 2,
                average_secs: Some(45),
                shortest_secs: Some(30),
                longest_secs: Some(60),
                current_elapsed_secs: Some(30),
                estimated_remaining_secs: Some(15),
            }
        );
    }

    #[test]
    fn private_chat_timings_rank_longest_average_first_and_missing_data_last() {
        let mut timings = vec![
            PrivateChatTiming {
                target_id: "30".to_owned(),
                completed_samples: 0,
                average_secs: None,
            },
            PrivateChatTiming {
                target_id: "20".to_owned(),
                completed_samples: 2,
                average_secs: Some(30),
            },
            PrivateChatTiming {
                target_id: "10".to_owned(),
                completed_samples: 4,
                average_secs: Some(90),
            },
            PrivateChatTiming {
                target_id: "40".to_owned(),
                completed_samples: 1,
                average_secs: Some(30),
            },
        ];

        sort_private_chat_timings(&mut timings);

        assert_eq!(
            timings
                .iter()
                .map(|timing| timing.target_id.as_str())
                .collect::<Vec<_>>(),
            vec!["10", "20", "40", "30"]
        );
    }

    #[test]
    fn auto_chat_advances_in_captured_rank_order_when_current_player_finishes() {
        let mut state = GmAutoChatModeState::default();
        state.set_enabled(true);
        let ranked = vec!["slow".to_owned(), "middle".to_owned(), "fast".to_owned()];
        let mut waiting = ranked.iter().cloned().collect::<HashSet<_>>();

        assert_eq!(
            state.sync_session("table", 7, &ranked, &waiting, 10.0),
            Some("slow".to_owned())
        );
        waiting.remove("slow");
        assert_eq!(
            state.sync_session("table", 7, &[], &waiting, 20.0),
            Some("middle".to_owned())
        );
        waiting.remove("middle");
        assert_eq!(
            state.sync_session("table", 7, &[], &waiting, 30.0),
            Some("fast".to_owned())
        );
        assert_eq!(state.progress(), Some((3, 3)));
    }

    #[test]
    fn auto_chat_starts_a_fresh_ranking_when_the_world_turn_advances() {
        let mut state = GmAutoChatModeState::default();
        state.set_enabled(true);
        let ranked = vec!["slow".to_owned(), "fast".to_owned()];
        let waiting = ranked.iter().cloned().collect::<HashSet<_>>();

        assert_eq!(
            state.sync_session("table", 3, &ranked, &waiting, 10.0),
            Some("slow".to_owned())
        );
        assert_eq!(
            state.sync_session("table", 4, &ranked, &waiting, 20.0),
            Some("slow".to_owned())
        );
        assert_eq!(state.progress(), Some((1, 2)));
    }

    #[test]
    fn auto_chat_warns_once_after_one_minute_on_each_selected_player() {
        let mut state = GmAutoChatModeState::default();
        state.set_enabled(true);
        let ranked = vec!["slow".to_owned(), "fast".to_owned()];
        let mut waiting = ranked.iter().cloned().collect::<HashSet<_>>();
        state.sync_session("table", 1, &ranked, &waiting, 10.0);

        assert_eq!(state.take_stay_warning(69.9), None);
        assert_eq!(
            state.take_stay_warning(70.0),
            Some("slow".to_owned())
        );
        assert_eq!(state.take_stay_warning(90.0), None);

        waiting.remove("slow");
        state.sync_session("table", 1, &[], &waiting, 100.0);
        assert_eq!(state.take_stay_warning(159.9), None);
        assert_eq!(
            state.take_stay_warning(160.0),
            Some("fast".to_owned())
        );
    }

    #[test]
    fn duration_format_stays_compact() {
        assert_eq!(format_duration(9), "9秒");
        assert_eq!(format_duration(125), "2分 5秒");
        assert_eq!(format_duration(3_725), "1小时 2分");
        assert_eq!(format_duration(90_000), "1天 1小时");
    }

    #[test]
    fn world_turn_timing_uses_consecutive_turn_starts_and_current_elapsed() {
        let starts = BTreeMap::from([(3, 100), (4, 160), (5, 250)]);

        assert_eq!(
            world_turn_summary(&starts, 5, 280),
            TimingSummary {
                completed_samples: 2,
                average_secs: Some(75),
                shortest_secs: Some(60),
                longest_secs: Some(90),
                current_elapsed_secs: Some(30),
                estimated_remaining_secs: Some(45),
            }
        );
    }
}
