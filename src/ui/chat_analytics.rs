use std::{
    collections::BTreeMap,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use bevy_egui::egui;

use crate::napcat::{
    NapcatMessage,
    NapcatMessageChainType,
    NapcatMessageManager,
    NapcatMessageType,
    TrpgGroup,
};

#[derive(Debug, Default, PartialEq, Eq)]
struct TimingSummary {
    completed_samples: usize,
    average_secs: Option<u64>,
    shortest_secs: Option<u64>,
    longest_secs: Option<u64>,
    current_elapsed_secs: Option<u64>,
    estimated_remaining_secs: Option<u64>,
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
    let session_started_at = group
        .filter(|group| group.campaign_active)
        .and_then(|group| {
            (group.campaign_started_at > 0)
                .then_some(group.campaign_started_at)
                .or_else(|| inferred_current_session_start(manager, group))
        });
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

fn format_duration(total_secs: u64) -> String {
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
