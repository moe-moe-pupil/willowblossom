use std::collections::{
    BTreeMap,
    HashMap,
};

use bevy_egui::egui;

use super::{
    format_number,
    BattleEncounter,
    BattleRoundStore,
    CombatLogEntry,
    CombatLogKind,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MatchScope {
    #[default]
    CurrentMatch,
    FullHistory,
}

impl MatchScope {
    const ALL: [Self; 2] = [Self::CurrentMatch, Self::FullHistory];

    fn label(self) -> &'static str {
        match self {
            Self::CurrentMatch => "本场战斗",
            Self::FullHistory => "全部历史",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ParticipantScope {
    #[default]
    All,
    Players,
    Units,
}

impl ParticipantScope {
    const ALL: [Self; 3] = [Self::All, Self::Players, Self::Units];

    fn label(self) -> &'static str {
        match self {
            Self::All => "全部参战者",
            Self::Players => "仅玩家角色",
            Self::Units => "仅单位/NPC",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SummaryMetric {
    #[default]
    DamageDealt,
    DamageTaken,
    RawDamage,
    HealingDone,
    HealingReceived,
    RawHealing,
    ExperienceGained,
    Kills,
    Assists,
    Deaths,
    BuffsApplied,
    EffectEvents,
}

impl SummaryMetric {
    const ALL: [Self; 12] = [
        Self::DamageDealt,
        Self::DamageTaken,
        Self::RawDamage,
        Self::HealingDone,
        Self::HealingReceived,
        Self::RawHealing,
        Self::ExperienceGained,
        Self::Kills,
        Self::Assists,
        Self::Deaths,
        Self::BuffsApplied,
        Self::EffectEvents,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::DamageDealt => "有效伤害",
            Self::DamageTaken => "承受伤害",
            Self::RawDamage => "伤害基础值",
            Self::HealingDone => "有效治疗/护盾",
            Self::HealingReceived => "受到治疗/护盾",
            Self::RawHealing => "治疗基础值",
            Self::ExperienceGained => "获得经验",
            Self::Kills => "击杀",
            Self::Assists => "助攻",
            Self::Deaths => "死亡",
            Self::BuffsApplied => "施加状态",
            Self::EffectEvents => "结算事件",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::Kills
            | Self::Assists
            | Self::Deaths
            | Self::BuffsApplied
            | Self::EffectEvents => "次",
            Self::ExperienceGained => " EXP",
            _ => "",
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Self::DamageDealt | Self::DamageTaken | Self::RawDamage => {
                egui::Color32::from_rgb(214, 75, 72)
            },
            Self::HealingDone | Self::HealingReceived | Self::RawHealing => {
                egui::Color32::from_rgb(63, 174, 112)
            },
            Self::ExperienceGained => egui::Color32::from_rgb(120, 108, 220),
            Self::Kills | Self::Assists | Self::Deaths => egui::Color32::from_rgb(225, 154, 61),
            Self::BuffsApplied | Self::EffectEvents => egui::Color32::from_rgb(67, 145, 205),
        }
    }

    fn subject_is_target(self) -> bool {
        matches!(
            self,
            Self::DamageTaken | Self::HealingReceived | Self::Deaths
        )
    }

    fn event_value(self, entry: &CombatLogEntry) -> Option<f32> {
        match self {
            Self::DamageDealt | Self::DamageTaken if entry.kind == CombatLogKind::Damage => {
                Some(entry.effective_amount.max(0.0))
            },
            Self::RawDamage if entry.kind == CombatLogKind::Damage => {
                Some(entry.base_amount.max(0.0))
            },
            Self::HealingDone | Self::HealingReceived if entry.kind == CombatLogKind::Healing => {
                Some(entry.effective_amount.max(0.0))
            },
            Self::RawHealing if entry.kind == CombatLogKind::Healing => {
                Some(entry.base_amount.max(0.0))
            },
            Self::ExperienceGained if entry.kind == CombatLogKind::Experience => {
                Some(entry.effective_amount.max(0.0))
            },
            Self::Kills if entry.kind == CombatLogKind::Elimination => Some(1.0),
            Self::Assists if entry.kind == CombatLogKind::Assist => Some(1.0),
            Self::Deaths if entry.kind == CombatLogKind::Elimination => Some(1.0),
            Self::BuffsApplied if entry.kind == CombatLogKind::Buff => Some(1.0),
            Self::EffectEvents
                if matches!(
                    entry.kind,
                    CombatLogKind::Damage
                        | CombatLogKind::Healing
                        | CombatLogKind::Buff
                        | CombatLogKind::Resource
                ) =>
            {
                Some(1.0)
            },
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SummaryGroupBy {
    #[default]
    Participant,
    Action,
    ParticipantAction,
    Target,
    Round,
}

impl SummaryGroupBy {
    const ALL: [Self; 5] = [
        Self::Participant,
        Self::Action,
        Self::ParticipantAction,
        Self::Target,
        Self::Round,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Participant => "参战者",
            Self::Action => "技能/来源",
            Self::ParticipantAction => "参战者 + 技能",
            Self::Target => "目标",
            Self::Round => "回合",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SummarySort {
    #[default]
    ValueDescending,
    LabelAscending,
}

impl SummarySort {
    const ALL: [Self; 2] = [Self::ValueDescending, Self::LabelAscending];

    fn label(self) -> &'static str {
        match self {
            Self::ValueDescending => "数值从高到低",
            Self::LabelAscending => "名称/回合顺序",
        }
    }
}

pub(super) struct PostMatchSummaryUiState {
    pub(super) open: bool,
    selected_encounter_id: String,
    scope: MatchScope,
    participant_scope: ParticipantScope,
    metric: SummaryMetric,
    group_by: SummaryGroupBy,
    sort: SummarySort,
    source_filter: String,
    action_filter: String,
    target_filter: String,
    round_from: u32,
    round_to: u32,
    top_n: usize,
    include_zero: bool,
}

impl Default for PostMatchSummaryUiState {
    fn default() -> Self {
        Self {
            open: false,
            selected_encounter_id: String::new(),
            scope: MatchScope::CurrentMatch,
            participant_scope: ParticipantScope::All,
            metric: SummaryMetric::DamageDealt,
            group_by: SummaryGroupBy::Participant,
            sort: SummarySort::ValueDescending,
            source_filter: String::new(),
            action_filter: String::new(),
            target_filter: String::new(),
            round_from: 0,
            round_to: 0,
            top_n: 10,
            include_zero: false,
        }
    }
}

impl PostMatchSummaryUiState {
    pub(super) fn open_for(&mut self, encounter_id: Option<&str>) {
        self.open = true;
        if let Some(encounter_id) = encounter_id {
            self.selected_encounter_id = encounter_id.to_owned();
        }
    }

    fn reset_filters(&mut self) {
        let open = self.open;
        let selected_encounter_id = std::mem::take(&mut self.selected_encounter_id);
        *self = Self::default();
        self.open = open;
        self.selected_encounter_id = selected_encounter_id;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ChartRow {
    label: String,
    value: f32,
    event_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ParticipantTotals {
    id: String,
    name: String,
    player_character: Option<bool>,
    level: Option<i32>,
    current_exp: Option<i32>,
    alive: Option<bool>,
    damage_dealt: f32,
    damage_taken: f32,
    healing_done: f32,
    healing_received: f32,
    experience: f32,
    kills: u32,
    assists: u32,
    deaths: u32,
    buffs_applied: u32,
    effect_events: u32,
}

impl ParticipantTotals {
    fn has_values(&self) -> bool {
        self.damage_dealt > f32::EPSILON
            || self.damage_taken > f32::EPSILON
            || self.healing_done > f32::EPSILON
            || self.healing_received > f32::EPSILON
            || self.experience > f32::EPSILON
            || self.kills > 0
            || self.assists > 0
            || self.deaths > 0
            || self.buffs_applied > 0
            || self.effect_events > 0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct HeadlineTotals {
    damage: f32,
    healing: f32,
    experience: f32,
    kills: u32,
    assists: u32,
    events: usize,
}

pub(super) fn show_window(
    ctx: &egui::Context,
    state: &mut PostMatchSummaryUiState,
    store: &BattleRoundStore,
) {
    if !state.open {
        return;
    }

    ensure_encounter_selection(state, store);
    let constraint_rect = ctx.content_rect();
    let min_size = egui::vec2(
        520.0_f32.min(constraint_rect.width()),
        360.0_f32.min(constraint_rect.height()),
    );
    let max_size = egui::vec2(
        constraint_rect.width().max(min_size.x),
        constraint_rect.height().max(min_size.y),
    );
    let mut open = state.open;

    egui::Window::new("GM赛后统计")
        .id(egui::Id::new(
            "battle_post_match_summary_v1",
        ))
        .default_size(egui::vec2(860.0, 680.0))
        .min_size(min_size)
        .max_size(max_size)
        .constrain_to(constraint_rect)
        .resizable(true)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_max_width(max_size.x);
            encounter_selector_ui(ui, state, store);
            let Some(encounter) = store.encounters.get(&state.selected_encounter_id) else {
                ui.label("还没有可统计的战斗轮。");
                return;
            };

            ui.horizontal_wrapped(|ui| {
                ui.heading(&encounter.name);
                ui.small(if encounter.active {
                    "实时统计（战斗进行中）"
                } else {
                    "赛后结果"
                });
                ui.small(format!("当前第{}轮", encounter.round));
                ui.small(format!(
                    "{}名参战者",
                    encounter.participants.len()
                ));
            });
            ui.small("仅在GM本地显示；不会发送到QQ、玩家窗口、AI或MCP。");
            ui.separator();

            filter_controls_ui(ui, state);

            let common_entries = filtered_common_entries(encounter, state);
            let headline = headline_totals(encounter, state, &common_entries);
            headline_ui(ui, &headline);
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let participants = aggregate_participants(encounter, state, &common_entries);
                    participant_overview_ui(ui, state, &participants);
                    ui.separator();

                    let rows = aggregate_chart(encounter, state);
                    chart_ui(ui, state, &rows);
                    ui.separator();
                    data_notes_ui(ui);
                });
        });

    state.open = open;
}

fn ensure_encounter_selection(state: &mut PostMatchSummaryUiState, store: &BattleRoundStore) {
    if store.encounters.contains_key(&state.selected_encounter_id) {
        return;
    }
    state.selected_encounter_id = store
        .active_encounter_id
        .as_ref()
        .filter(|id| store.encounters.contains_key(*id))
        .cloned()
        .or_else(|| {
            let mut encounters = store.encounters.iter().collect::<Vec<_>>();
            encounters.sort_by(|left, right| {
                left.1
                    .name
                    .cmp(&right.1.name)
                    .then_with(|| left.0.cmp(right.0))
            });
            encounters.first().map(|(id, _)| (*id).clone())
        })
        .unwrap_or_default();
}

fn encounter_selector_ui(
    ui: &mut egui::Ui,
    state: &mut PostMatchSummaryUiState,
    store: &BattleRoundStore,
) {
    let mut encounters = store.encounters.iter().collect::<Vec<_>>();
    encounters.sort_by(|left, right| {
        left.1
            .name
            .cmp(&right.1.name)
            .then_with(|| left.0.cmp(right.0))
    });
    let selected_text = store
        .encounters
        .get(&state.selected_encounter_id)
        .map(|encounter| encounter.name.as_str())
        .unwrap_or("选择战斗轮");

    ui.horizontal_wrapped(|ui| {
        ui.label("战斗轮");
        egui::ComboBox::from_id_salt("post_match_encounter")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                for (encounter_id, encounter) in &encounters {
                    ui.selectable_value(
                        &mut state.selected_encounter_id,
                        (*encounter_id).clone(),
                        format!(
                            "{} · {}",
                            encounter.name,
                            if encounter.active { "进行中" } else { "已结束" }
                        ),
                    );
                }
            });
        if ui.button("重置筛选").clicked() {
            state.reset_filters();
        }
    });
}

fn filter_controls_ui(ui: &mut egui::Ui, state: &mut PostMatchSummaryUiState) {
    ui.horizontal_wrapped(|ui| {
        ui.label("数据范围");
        egui::ComboBox::from_id_salt("post_match_scope")
            .selected_text(state.scope.label())
            .show_ui(ui, |ui| {
                for scope in MatchScope::ALL {
                    ui.selectable_value(&mut state.scope, scope, scope.label());
                }
            });

        ui.label("指标");
        egui::ComboBox::from_id_salt("post_match_metric")
            .selected_text(state.metric.label())
            .show_ui(ui, |ui| {
                for metric in SummaryMetric::ALL {
                    ui.selectable_value(
                        &mut state.metric,
                        metric,
                        metric.label(),
                    );
                }
            });

        ui.label("分组");
        egui::ComboBox::from_id_salt("post_match_group")
            .selected_text(state.group_by.label())
            .show_ui(ui, |ui| {
                for group_by in SummaryGroupBy::ALL {
                    ui.selectable_value(
                        &mut state.group_by,
                        group_by,
                        group_by.label(),
                    );
                }
            });
    });

    ui.horizontal_wrapped(|ui| {
        ui.label("参战者范围");
        egui::ComboBox::from_id_salt("post_match_participant_scope")
            .selected_text(state.participant_scope.label())
            .show_ui(ui, |ui| {
                for scope in ParticipantScope::ALL {
                    ui.selectable_value(
                        &mut state.participant_scope,
                        scope,
                        scope.label(),
                    );
                }
            });

        ui.label("排序");
        egui::ComboBox::from_id_salt("post_match_sort")
            .selected_text(state.sort.label())
            .show_ui(ui, |ui| {
                for sort in SummarySort::ALL {
                    ui.selectable_value(&mut state.sort, sort, sort.label());
                }
            });

        ui.label("Top");
        ui.add(
            egui::DragValue::new(&mut state.top_n)
                .range(1..=100)
                .speed(1),
        );
        ui.checkbox(&mut state.include_zero, "显示零值角色");
    });

    ui.horizontal_wrapped(|ui| {
        ui.label("来源角色");
        ui.add(
            egui::TextEdit::singleline(&mut state.source_filter)
                .hint_text("名称或ID包含")
                .desired_width(150.0),
        );
        ui.label("技能/来源");
        ui.add(
            egui::TextEdit::singleline(&mut state.action_filter)
                .hint_text("名称包含")
                .desired_width(150.0),
        );
        ui.label("目标");
        ui.add(
            egui::TextEdit::singleline(&mut state.target_filter)
                .hint_text("名称或ID包含")
                .desired_width(150.0),
        );
    });

    ui.horizontal_wrapped(|ui| {
        ui.label("回合");
        ui.add(
            egui::DragValue::new(&mut state.round_from)
                .prefix("从 ")
                .range(0..=u32::MAX),
        );
        ui.add(
            egui::DragValue::new(&mut state.round_to)
                .prefix("到 ")
                .range(0..=u32::MAX),
        );
        ui.small("0 表示不限；文本筛选忽略英文大小写。");
    });
}

fn headline_ui(ui: &mut egui::Ui, totals: &HeadlineTotals) {
    ui.horizontal_wrapped(|ui| {
        ui.strong("筛选后总计");
        ui.label(format!(
            "伤害 {}",
            format_number(totals.damage)
        ));
        ui.label(format!(
            "治疗/护盾 {}",
            format_number(totals.healing)
        ));
        ui.label(format!(
            "经验 {}",
            format_number(totals.experience)
        ));
        ui.label(format!(
            "击杀 {} · 助攻 {}",
            totals.kills, totals.assists
        ));
        ui.label(format!("事件 {} 条", totals.events));
    });
}

fn participant_overview_ui(
    ui: &mut egui::Ui,
    state: &PostMatchSummaryUiState,
    participants: &[ParticipantTotals],
) {
    egui::CollapsingHeader::new(format!(
        "参战者总览 · {}人",
        participants.len()
    ))
    .default_open(true)
    .show(ui, |ui| {
        if participants.is_empty() {
            ui.small("当前筛选条件下没有参战者数据。");
            return;
        }

        if ui.button("复制角色总览 TSV").clicked() {
            ui.ctx().copy_text(participant_totals_tsv(participants));
        }
        ui.small("伤害/治疗同时给出输出与承受值；K/A/D 为击杀/助攻/死亡。");

        for participant in participants {
            ui.group(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(&participant.name);
                    ui.small(match participant.player_character {
                        Some(true) => "玩家角色",
                        Some(false) => "单位/NPC",
                        None => "历史角色",
                    });
                    if participant.id != participant.name {
                        ui.small(format!("ID {}", participant.id));
                    }
                    if let Some(level) = participant.level {
                        ui.small(format!(
                            "Lv.{level} · 当前EXP {}",
                            participant.current_exp.unwrap_or_default()
                        ));
                    }
                    if let Some(alive) = participant.alive {
                        ui.colored_label(
                            if alive {
                                egui::Color32::LIGHT_GREEN
                            } else {
                                egui::Color32::LIGHT_RED
                            },
                            if alive { "存活" } else { "倒下" },
                        );
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "伤害 {} / 承受 {}",
                        format_number(participant.damage_dealt),
                        format_number(participant.damage_taken),
                    ));
                    ui.label(format!(
                        "治疗 {} / 受到 {}",
                        format_number(participant.healing_done),
                        format_number(participant.healing_received),
                    ));
                    ui.label(format!(
                        "K/A/D {}/{}/{}",
                        participant.kills, participant.assists, participant.deaths,
                    ));
                    ui.label(format!(
                        "EXP {}",
                        format_number(participant.experience)
                    ));
                    ui.label(format!(
                        "状态 {} 次",
                        participant.buffs_applied
                    ));
                    ui.label(format!(
                        "结算 {} 条",
                        participant.effect_events
                    ));
                });
            });
        }

        if !state.include_zero {
            ui.small("未产生筛选后数据的角色已隐藏；可启用“显示零值角色”。");
        }
    });
}

fn chart_ui(ui: &mut egui::Ui, state: &PostMatchSummaryUiState, rows: &[ChartRow]) {
    ui.horizontal_wrapped(|ui| {
        ui.heading(format!(
            "{} · 按{}横向条形图",
            state.metric.label(),
            state.group_by.label()
        ));
        if ui.button("复制当前图表 TSV").clicked() {
            ui.ctx().copy_text(chart_tsv(state, rows));
        }
    });

    if rows.is_empty() {
        ui.colored_label(
            egui::Color32::YELLOW,
            "当前筛选条件下没有可绘制的数据。",
        );
        return;
    }

    let total = rows.iter().map(|row| row.value).sum::<f32>();
    let max_value = rows.iter().map(|row| row.value).fold(0.0_f32, f32::max);
    for row in rows {
        let ratio = if max_value > f32::EPSILON { row.value / max_value } else { 0.0 };
        let share = if total > f32::EPSILON { row.value / total * 100.0 } else { 0.0 };
        let row_width = ui.available_width().max(180.0);
        let spacing = ui.spacing().item_spacing.x;
        let label_width = (row_width * 0.28).clamp(80.0, 170.0);
        let bar_width = (row_width - label_width - spacing).max(96.0);
        ui.horizontal(|ui| {
            let label = ui.add_sized(
                [label_width, 20.0],
                egui::Label::new(&row.label).truncate(),
            );
            label.on_hover_text(&row.label);
            ui.add(
                egui::ProgressBar::new(ratio)
                    .desired_width(bar_width)
                    .desired_height(20.0)
                    .fill(state.metric.color())
                    .text(format!(
                        "{}{} · {:.1}% · {}条",
                        format_number(row.value),
                        state.metric.unit(),
                        share,
                        row.event_count,
                    )),
            )
            .on_hover_text(format!(
                "{}\n精确值：{}\n占筛选总值：{share:.2}%\n贡献事件：{}",
                row.label, row.value, row.event_count,
            ));
        });
    }
}

fn data_notes_ui(ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("统计口径与数据说明")
        .default_open(false)
        .show(ui, |ui| {
            ui.label("• 有效伤害是实际扣除生命值后的数值；被护盾、免疫或闪避完全阻止的部分不会虚增输出。");
            ui.label("• 有效治疗沿用战斗结算值，包含实际回复生命与由过量治疗转化的护盾。");
            ui.label("• 技能范围伤害/治疗按每个目标各记一条，因此“结算事件”表示命中/生效次数，不等同于施法次数。");
            ui.label("• “本场战斗”从最近一次由休整切换为进行中时开始；“全部历史”包含该战斗轮保留的旧场次。");
            ui.label("• 经验、击杀与助攻使用结构化事件，从安装本功能后的新结算开始记录；旧备份仍可统计其中已有的伤害、治疗与状态事件。");
            ui.label("• 助攻包含对目标造成伤害的参与者，以及本场中曾治疗击杀者或对其施加有益状态的参战者。");
            ui.label("• 所有筛选和复制操作都只读取GM本地战斗数据，不会更改战斗结果。");
        });
}

fn entries_for_scope<'a>(
    encounter: &'a BattleEncounter,
    scope: MatchScope,
) -> impl Iterator<Item = &'a CombatLogEntry> {
    let start = match scope {
        MatchScope::CurrentMatch => encounter.combat_log_start.min(encounter.combat_log.len()),
        MatchScope::FullHistory => 0,
    };
    encounter.combat_log[start..].iter()
}

fn filtered_common_entries<'a>(
    encounter: &'a BattleEncounter,
    state: &PostMatchSummaryUiState,
) -> Vec<&'a CombatLogEntry> {
    entries_for_scope(encounter, state.scope)
        .filter(|entry| entry_matches_common_filters(entry, state))
        .collect()
}

fn entry_matches_common_filters(entry: &CombatLogEntry, state: &PostMatchSummaryUiState) -> bool {
    (state.round_from == 0 || entry.round >= state.round_from)
        && (state.round_to == 0 || entry.round <= state.round_to)
        && text_filter_matches(&state.source_filter, &[
            &entry.source_id,
            &entry.source_name,
        ])
        && text_filter_matches(&state.action_filter, &[
            &entry.action_name
        ])
        && text_filter_matches(&state.target_filter, &[
            &entry.target_id,
            &entry.target_name,
        ])
}

fn text_filter_matches(filter: &str, candidates: &[&str]) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let filter = filter.to_lowercase();
    candidates
        .iter()
        .any(|candidate| candidate.to_lowercase().contains(&filter))
}

fn participant_matches_scope(
    encounter: &BattleEncounter,
    target_id: &str,
    scope: ParticipantScope,
) -> bool {
    if scope == ParticipantScope::All {
        return true;
    }
    encounter
        .participants
        .iter()
        .find(|participant| participant.target_id == target_id)
        .is_some_and(|participant| {
            participant.player_character == (scope == ParticipantScope::Players)
        })
}

fn metric_subject<'a>(metric: SummaryMetric, entry: &'a CombatLogEntry) -> (&'a str, &'a str) {
    if metric.subject_is_target() {
        (&entry.target_id, &entry.target_name)
    } else {
        (&entry.source_id, &entry.source_name)
    }
}

fn chart_group(
    group_by: SummaryGroupBy,
    subject_id: &str,
    subject_name: &str,
    entry: &CombatLogEntry,
) -> (String, String) {
    let action_name = if entry.action_name.trim().is_empty() {
        "未命名来源"
    } else {
        entry.action_name.as_str()
    };
    match group_by {
        SummaryGroupBy::Participant => (
            format!("participant:{subject_id}"),
            subject_name.to_owned(),
        ),
        SummaryGroupBy::Action => (
            format!("action:{action_name}"),
            action_name.to_owned(),
        ),
        SummaryGroupBy::ParticipantAction => (
            format!("participant_action:{subject_id}\0{action_name}"),
            format!("{subject_name} · {action_name}"),
        ),
        SummaryGroupBy::Target => (
            format!("target:{}", entry.target_id),
            entry.target_name.clone(),
        ),
        SummaryGroupBy::Round => (
            format!("round:{:010}", entry.round),
            format!("R{}", entry.round),
        ),
    }
}

fn aggregate_chart(encounter: &BattleEncounter, state: &PostMatchSummaryUiState) -> Vec<ChartRow> {
    let mut rows = BTreeMap::<String, ChartRow>::new();
    for entry in entries_for_scope(encounter, state.scope) {
        if !entry_matches_common_filters(entry, state) {
            continue;
        }
        let Some(value) = state.metric.event_value(entry) else {
            continue;
        };
        let (subject_id, subject_name) = metric_subject(state.metric, entry);
        if !participant_matches_scope(
            encounter,
            subject_id,
            state.participant_scope,
        ) {
            continue;
        }
        let (key, label) = chart_group(
            state.group_by,
            subject_id,
            subject_name,
            entry,
        );
        let row = rows.entry(key).or_insert_with(|| ChartRow {
            label,
            ..Default::default()
        });
        row.value += value;
        row.event_count += 1;
    }

    if state.include_zero && state.group_by == SummaryGroupBy::Participant {
        let subject_filter = if state.metric.subject_is_target() {
            &state.target_filter
        } else {
            &state.source_filter
        };
        for participant in &encounter.participants {
            if !participant_matches_scope(
                encounter,
                &participant.target_id,
                state.participant_scope,
            ) || !text_filter_matches(subject_filter, &[
                &participant.target_id,
                &participant.display_name,
            ]) {
                continue;
            }
            rows.entry(format!(
                "participant:{}",
                participant.target_id
            ))
            .or_insert_with(|| ChartRow {
                label: participant.display_name.clone(),
                ..Default::default()
            });
        }
    }

    let duplicate_labels = rows.values().fold(
        HashMap::<String, usize>::new(),
        |mut counts, row| {
            *counts.entry(row.label.clone()).or_default() += 1;
            counts
        },
    );
    for (key, row) in &mut rows {
        if duplicate_labels.get(&row.label).copied().unwrap_or(0) <= 1 {
            continue;
        }
        let identity = match state.group_by {
            SummaryGroupBy::Participant => key.strip_prefix("participant:"),
            SummaryGroupBy::ParticipantAction => key
                .strip_prefix("participant_action:")
                .and_then(|value| value.split_once('\0').map(|(id, _)| id)),
            SummaryGroupBy::Target => key.strip_prefix("target:"),
            SummaryGroupBy::Action | SummaryGroupBy::Round => None,
        };
        if let Some(identity) = identity {
            row.label = format!("{} · {identity}", row.label);
        }
    }

    let mut rows = rows.into_values().collect::<Vec<_>>();
    match state.sort {
        SummarySort::ValueDescending => rows.sort_by(|left, right| {
            right
                .value
                .total_cmp(&left.value)
                .then_with(|| left.label.cmp(&right.label))
        }),
        SummarySort::LabelAscending if state.group_by == SummaryGroupBy::Round => {
            rows.sort_by_key(|row| round_label_number(&row.label));
        },
        SummarySort::LabelAscending => {
            rows.sort_by(|left, right| left.label.cmp(&right.label));
        },
    }
    rows.truncate(state.top_n.clamp(1, 100));
    rows
}

fn round_label_number(label: &str) -> u32 {
    label
        .strip_prefix('R')
        .and_then(|value| value.parse().ok())
        .unwrap_or(u32::MAX)
}

fn headline_totals(
    encounter: &BattleEncounter,
    state: &PostMatchSummaryUiState,
    entries: &[&CombatLogEntry],
) -> HeadlineTotals {
    let mut totals = HeadlineTotals::default();
    for entry in entries {
        if !participant_matches_scope(
            encounter,
            &entry.source_id,
            state.participant_scope,
        ) {
            continue;
        }
        match entry.kind {
            CombatLogKind::Damage => {
                totals.damage += entry.effective_amount.max(0.0);
                totals.events += 1;
            },
            CombatLogKind::Healing => {
                totals.healing += entry.effective_amount.max(0.0);
                totals.events += 1;
            },
            CombatLogKind::Experience => totals.experience += entry.effective_amount.max(0.0),
            CombatLogKind::Elimination => totals.kills += 1,
            CombatLogKind::Assist => totals.assists += 1,
            CombatLogKind::Buff | CombatLogKind::Resource => totals.events += 1,
        }
    }
    totals
}

fn aggregate_participants(
    encounter: &BattleEncounter,
    state: &PostMatchSummaryUiState,
    entries: &[&CombatLogEntry],
) -> Vec<ParticipantTotals> {
    let participant_types = encounter
        .participants
        .iter()
        .map(|participant| {
            (
                participant.target_id.as_str(),
                (
                    participant.display_name.as_str(),
                    participant.player_character,
                    participant.level,
                    participant.exp,
                    participant.alive,
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut totals = BTreeMap::<String, ParticipantTotals>::new();

    if state.include_zero {
        for participant in &encounter.participants {
            if participant_matches_scope(
                encounter,
                &participant.target_id,
                state.participant_scope,
            ) {
                totals.insert(
                    participant.target_id.clone(),
                    ParticipantTotals {
                        id: participant.target_id.clone(),
                        name: participant.display_name.clone(),
                        player_character: Some(participant.player_character),
                        level: Some(participant.level),
                        current_exp: Some(participant.exp),
                        alive: Some(participant.alive),
                        ..Default::default()
                    },
                );
            }
        }
    }

    for entry in entries {
        let source_allowed = participant_matches_scope(
            encounter,
            &entry.source_id,
            state.participant_scope,
        );
        let target_allowed = participant_matches_scope(
            encounter,
            &entry.target_id,
            state.participant_scope,
        );

        if source_allowed {
            let source = participant_totals_entry(
                &mut totals,
                &participant_types,
                &entry.source_id,
                &entry.source_name,
            );
            match entry.kind {
                CombatLogKind::Damage => {
                    source.damage_dealt += entry.effective_amount.max(0.0);
                    source.effect_events = source.effect_events.saturating_add(1);
                },
                CombatLogKind::Healing => {
                    source.healing_done += entry.effective_amount.max(0.0);
                    source.effect_events = source.effect_events.saturating_add(1);
                },
                CombatLogKind::Experience => {
                    source.experience += entry.effective_amount.max(0.0);
                },
                CombatLogKind::Elimination => {
                    source.kills = source.kills.saturating_add(1);
                },
                CombatLogKind::Assist => {
                    source.assists = source.assists.saturating_add(1);
                },
                CombatLogKind::Buff => {
                    source.buffs_applied = source.buffs_applied.saturating_add(1);
                    source.effect_events = source.effect_events.saturating_add(1);
                },
                CombatLogKind::Resource => {
                    source.effect_events = source.effect_events.saturating_add(1);
                },
            }
        }

        if target_allowed {
            let target = participant_totals_entry(
                &mut totals,
                &participant_types,
                &entry.target_id,
                &entry.target_name,
            );
            match entry.kind {
                CombatLogKind::Damage => {
                    target.damage_taken += entry.effective_amount.max(0.0);
                },
                CombatLogKind::Healing => {
                    target.healing_received += entry.effective_amount.max(0.0);
                },
                CombatLogKind::Elimination => {
                    target.deaths = target.deaths.saturating_add(1);
                },
                CombatLogKind::Buff
                | CombatLogKind::Resource
                | CombatLogKind::Experience
                | CombatLogKind::Assist => {},
            }
        }
    }

    let mut totals = totals
        .into_values()
        .filter(|totals| state.include_zero || totals.has_values())
        .collect::<Vec<_>>();
    totals.sort_by(|left, right| {
        right
            .damage_dealt
            .total_cmp(&left.damage_dealt)
            .then_with(|| right.healing_done.total_cmp(&left.healing_done))
            .then_with(|| left.name.cmp(&right.name))
    });
    totals
}

fn participant_totals_entry<'a>(
    totals: &'a mut BTreeMap<String, ParticipantTotals>,
    participant_types: &HashMap<&str, (&str, bool, i32, i32, bool)>,
    id: &str,
    fallback_name: &str,
) -> &'a mut ParticipantTotals {
    totals.entry(id.to_owned()).or_insert_with(|| {
        let (name, player_character, level, current_exp, alive) = participant_types
            .get(id)
            .map(|(name, player, level, exp, alive)| {
                (
                    (*name).to_owned(),
                    Some(*player),
                    Some(*level),
                    Some(*exp),
                    Some(*alive),
                )
            })
            .unwrap_or_else(|| {
                (
                    fallback_name.to_owned(),
                    None,
                    None,
                    None,
                    None,
                )
            });
        ParticipantTotals {
            id: id.to_owned(),
            name,
            player_character,
            level,
            current_exp,
            alive,
            ..Default::default()
        }
    })
}

fn chart_tsv(state: &PostMatchSummaryUiState, rows: &[ChartRow]) -> String {
    let mut output = format!(
        "{}\t{}\t占比\t事件数\n",
        escape_tsv(state.group_by.label()),
        escape_tsv(state.metric.label()),
    );
    let total = rows.iter().map(|row| row.value).sum::<f32>();
    for row in rows {
        let share = if total > f32::EPSILON { row.value / total * 100.0 } else { 0.0 };
        output.push_str(&format!(
            "{}\t{}\t{share:.2}%\t{}\n",
            escape_tsv(&row.label),
            row.value,
            row.event_count,
        ));
    }
    output
}

fn participant_totals_tsv(participants: &[ParticipantTotals]) -> String {
    let mut output =
        "角色\tID\t类型\t当前等级\t当前经验\t状态\t输出伤害\t承受伤害\t输出治疗/护盾\t受到治疗/护盾\t击杀\t助攻\t死亡\t本场经验\t状态次数\t结算事件\n"
            .to_owned();
    for participant in participants {
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            escape_tsv(&participant.name),
            escape_tsv(&participant.id),
            match participant.player_character {
                Some(true) => "玩家角色",
                Some(false) => "单位/NPC",
                None => "历史角色",
            },
            participant
                .level
                .map_or_else(String::new, |value| value.to_string()),
            participant
                .current_exp
                .map_or_else(String::new, |value| value.to_string()),
            participant.alive.map_or("", |alive| if alive {
                "存活"
            } else {
                "倒下"
            }),
            participant.damage_dealt,
            participant.damage_taken,
            participant.healing_done,
            participant.healing_received,
            participant.kills,
            participant.assists,
            participant.deaths,
            participant.experience,
            participant.buffs_applied,
            participant.effect_events,
        ));
    }
    output
}

fn escape_tsv(value: &str) -> String { value.replace(['\t', '\r', '\n'], " ").trim().to_owned() }

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::napcat::UnitRarity;

    fn participant(
        id: &str,
        name: &str,
        player_character: bool,
    ) -> super::super::BattleParticipantSnapshot {
        serde_json::from_value(json!({
            "target_id": id,
            "display_name": name,
            "player_character": player_character,
        }))
        .expect("minimal battle participant should deserialize through defaults")
    }

    fn entry(
        round: u32,
        kind: CombatLogKind,
        source_id: &str,
        source_name: &str,
        target_id: &str,
        target_name: &str,
        action_name: &str,
        base_amount: f32,
        effective_amount: f32,
    ) -> CombatLogEntry {
        CombatLogEntry {
            round,
            kind,
            source_id: source_id.to_owned(),
            source_name: source_name.to_owned(),
            target_id: target_id.to_owned(),
            target_name: target_name.to_owned(),
            action_name: action_name.to_owned(),
            base_amount,
            effective_amount,
            modifiers: Vec::new(),
            benefits: Vec::new(),
        }
    }

    fn encounter_with_events(events: Vec<CombatLogEntry>) -> BattleEncounter {
        BattleEncounter {
            name: "测试战斗".to_owned(),
            participants: vec![
                participant("alice", "Alice", true),
                participant("bob", "Bob", true),
                participant("orc", "Orc", false),
            ],
            combat_log: events,
            ..Default::default()
        }
    }

    #[test]
    fn current_match_chart_skips_prior_history_and_aggregates_by_actor() {
        let mut encounter = encounter_with_events(vec![
            entry(
                1,
                CombatLogKind::Damage,
                "alice",
                "Alice",
                "orc",
                "Orc",
                "旧攻击",
                100.0,
                90.0,
            ),
            entry(
                4,
                CombatLogKind::Damage,
                "alice",
                "Alice",
                "orc",
                "Orc",
                "火球",
                40.0,
                30.0,
            ),
            entry(
                4,
                CombatLogKind::Damage,
                "bob",
                "Bob",
                "orc",
                "Orc",
                "箭",
                12.0,
                12.0,
            ),
        ]);
        encounter.combat_log_start = 1;
        let state = PostMatchSummaryUiState::default();

        let rows = aggregate_chart(&encounter, &state);

        assert_eq!(rows, vec![
            ChartRow {
                label: "Alice".to_owned(),
                value: 30.0,
                event_count: 1,
            },
            ChartRow {
                label: "Bob".to_owned(),
                value: 12.0,
                event_count: 1,
            },
        ]);
    }

    #[test]
    fn chart_supports_metric_group_round_and_text_filters() {
        let encounter = encounter_with_events(vec![
            entry(
                2,
                CombatLogKind::Damage,
                "alice",
                "Alice",
                "orc",
                "Orc",
                "Fire Bolt",
                20.0,
                17.0,
            ),
            entry(
                3,
                CombatLogKind::Damage,
                "alice",
                "Alice",
                "orc",
                "Orc",
                "Fire Bolt",
                30.0,
                24.0,
            ),
            entry(
                3,
                CombatLogKind::Damage,
                "bob",
                "Bob",
                "orc",
                "Orc",
                "Ice Bolt",
                99.0,
                90.0,
            ),
        ]);
        let state = PostMatchSummaryUiState {
            metric: SummaryMetric::RawDamage,
            group_by: SummaryGroupBy::Round,
            sort: SummarySort::LabelAscending,
            source_filter: "ALI".to_owned(),
            action_filter: "fire".to_owned(),
            target_filter: "orc".to_owned(),
            round_from: 2,
            round_to: 3,
            ..Default::default()
        };

        let rows = aggregate_chart(&encounter, &state);

        assert_eq!(rows, vec![
            ChartRow {
                label: "R2".to_owned(),
                value: 20.0,
                event_count: 1,
            },
            ChartRow {
                label: "R3".to_owned(),
                value: 30.0,
                event_count: 1,
            },
        ]);
    }

    #[test]
    fn participant_totals_include_damage_healing_kda_and_experience() {
        let encounter = encounter_with_events(vec![
            entry(
                1,
                CombatLogKind::Damage,
                "alice",
                "Alice",
                "orc",
                "Orc",
                "火球",
                30.0,
                25.0,
            ),
            entry(
                1,
                CombatLogKind::Healing,
                "bob",
                "Bob",
                "alice",
                "Alice",
                "治疗术",
                20.0,
                18.0,
            ),
            entry(
                1,
                CombatLogKind::Elimination,
                "alice",
                "Alice",
                "orc",
                "Orc",
                "击败",
                1.0,
                1.0,
            ),
            entry(
                1,
                CombatLogKind::Assist,
                "bob",
                "Bob",
                "orc",
                "Orc",
                "助攻",
                1.0,
                1.0,
            ),
            entry(
                1,
                CombatLogKind::Experience,
                "alice",
                "Alice",
                "alice",
                "Alice",
                "战斗经验",
                45.0,
                45.0,
            ),
        ]);
        let state = PostMatchSummaryUiState::default();
        let entries = filtered_common_entries(&encounter, &state);

        let totals = aggregate_participants(&encounter, &state, &entries);

        let alice = totals.iter().find(|totals| totals.id == "alice").unwrap();
        assert_eq!(alice.damage_dealt, 25.0);
        assert_eq!(alice.healing_received, 18.0);
        assert_eq!(alice.kills, 1);
        assert_eq!(alice.experience, 45.0);
        let bob = totals.iter().find(|totals| totals.id == "bob").unwrap();
        assert_eq!(bob.healing_done, 18.0);
        assert_eq!(bob.assists, 1);
        let orc = totals.iter().find(|totals| totals.id == "orc").unwrap();
        assert_eq!(orc.damage_taken, 25.0);
        assert_eq!(orc.deaths, 1);
    }

    #[test]
    fn defeat_events_record_killer_damage_and_support_assists() {
        let mut encounter = encounter_with_events(vec![
            entry(
                1,
                CombatLogKind::Healing,
                "bob",
                "Bob",
                "alice",
                "Alice",
                "治疗术",
                10.0,
                10.0,
            ),
            entry(
                1,
                CombatLogKind::Buff,
                "orc",
                "Orc",
                "alice",
                "Alice",
                "敌方减益",
                0.0,
                0.0,
            ),
        ]);
        encounter.combat_log[1].benefits = vec!["有害".to_owned()];
        let outcome = super::super::BattleDefeatOutcome {
            contributors: vec!["alice".to_owned(), "bob".to_owned()],
            contribution_amounts: HashMap::from([
                ("alice".to_owned(), 20.0),
                ("bob".to_owned(), 5.0),
            ]),
            killer_id: Some("alice".to_owned()),
            defeated_id: "orc".to_owned(),
            defeated_player_character: false,
            defeated_level: 1,
            defeated_max_hp: 30.0,
            defeated_base_damage: 5.0,
            defeated_rarity: UnitRarity::Normal,
        };

        super::super::record_battle_defeat_events(&mut encounter, &outcome);

        assert!(
            encounter.combat_log.iter().any(|entry| {
                entry.kind == CombatLogKind::Elimination
                    && entry.source_id == "alice"
                    && entry.target_id == "orc"
            })
        );
        let bob_assists = encounter
            .combat_log
            .iter()
            .filter(|entry| entry.kind == CombatLogKind::Assist && entry.source_id == "bob")
            .count();
        assert_eq!(
            bob_assists, 1,
            "damage and healing support must not double count"
        );
        assert!(!encounter
            .combat_log
            .iter()
            .any(|entry| { entry.kind == CombatLogKind::Assist && entry.source_id == "orc" }));
    }

    #[test]
    fn battle_experience_reward_emits_structured_summary_event() {
        let mut encounter = encounter_with_events(Vec::new());
        let outcome = super::super::BattleDefeatOutcome {
            contributors: vec!["alice".to_owned()],
            contribution_amounts: HashMap::from([("alice".to_owned(), 30.0)]),
            killer_id: Some("alice".to_owned()),
            defeated_id: "orc".to_owned(),
            defeated_player_character: false,
            defeated_level: 2,
            defeated_max_hp: 80.0,
            defeated_base_damage: 4.0,
            defeated_rarity: UnitRarity::Normal,
        };

        super::super::apply_battle_experience_reward(&mut encounter, &outcome);

        let event = encounter
            .combat_log
            .iter()
            .find(|entry| entry.kind == CombatLogKind::Experience)
            .expect("experience reward should be available to post-match summary");
        assert_eq!(event.source_id, "alice");
        assert_eq!(event.target_id, "alice");
        assert!(event.effective_amount > 0.0);
        assert_eq!(
            event.base_amount,
            event.effective_amount
        );
    }

    #[test]
    fn participant_scope_filters_chart_subjects() {
        let encounter = encounter_with_events(vec![
            entry(
                1,
                CombatLogKind::Damage,
                "alice",
                "Alice",
                "orc",
                "Orc",
                "火球",
                20.0,
                20.0,
            ),
            entry(
                1,
                CombatLogKind::Damage,
                "orc",
                "Orc",
                "alice",
                "Alice",
                "爪击",
                10.0,
                10.0,
            ),
        ]);
        let players_only = PostMatchSummaryUiState {
            participant_scope: ParticipantScope::Players,
            ..Default::default()
        };
        let units_only = PostMatchSummaryUiState {
            participant_scope: ParticipantScope::Units,
            ..Default::default()
        };

        assert_eq!(
            aggregate_chart(&encounter, &players_only)[0].label,
            "Alice"
        );
        assert_eq!(
            aggregate_chart(&encounter, &units_only)[0].label,
            "Orc"
        );
    }

    #[test]
    fn participant_group_keeps_same_named_unit_instances_separate() {
        let mut encounter = encounter_with_events(vec![
            entry(
                1,
                CombatLogKind::Damage,
                "wolf-1",
                "Wolf",
                "alice",
                "Alice",
                "爪击",
                5.0,
                5.0,
            ),
            entry(
                1,
                CombatLogKind::Damage,
                "wolf-2",
                "Wolf",
                "alice",
                "Alice",
                "爪击",
                7.0,
                7.0,
            ),
        ]);
        encounter
            .participants
            .push(participant("wolf-1", "Wolf", false));
        encounter
            .participants
            .push(participant("wolf-2", "Wolf", false));

        let rows = aggregate_chart(
            &encounter,
            &PostMatchSummaryUiState::default(),
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value, 7.0);
        assert_eq!(rows[1].value, 5.0);
    }

    #[test]
    fn delayed_damage_and_healing_emit_named_combat_events() {
        let mut target = participant("target", "Target", false);
        target.hp = 50.0;
        target.max_hp = 100.0;
        target.delayed_damage_ticks = vec![super::super::BattleDelayedDamageTick {
            name: "苏萨斯之爪".to_owned(),
            source_id: "alice".to_owned(),
            source_name: "Alice".to_owned(),
            amount: 12.0,
            damage_type: crate::rule_engine::DamageType::Magical,
            turns_remaining: 2,
        }];

        let damage = super::super::advance_participant_delayed_damage_ticks(&mut target, true, 7);

        assert_eq!(damage.combat_log.len(), 1);
        assert_eq!(damage.combat_log[0].round, 7);
        assert_eq!(
            damage.combat_log[0].action_name,
            "苏萨斯之爪"
        );
        assert_eq!(
            damage.combat_log[0].effective_amount,
            12.0
        );

        target.delayed_healing_ticks = vec![super::super::BattleDelayedHealingTick {
            name: "千万回忆".to_owned(),
            source_id: "bob".to_owned(),
            source_name: "Bob".to_owned(),
            amount: 8.0,
            overhealing_shield_cap_rate: 0.0,
            turns_remaining: 1,
        }];
        let healing = super::super::advance_participant_delayed_healing_ticks(&mut target, 8);

        assert_eq!(healing.combat_log.len(), 1);
        assert_eq!(healing.combat_log[0].round, 8);
        assert_eq!(
            healing.combat_log[0].action_name,
            "千万回忆"
        );
        assert_eq!(
            healing.combat_log[0].effective_amount,
            8.0
        );
    }

    #[test]
    fn buff_ticks_keep_the_origin_name_in_post_match_data() {
        let manager: crate::napcat::NapcatMessageManager =
            serde_json::from_value(json!({ "messages": {} }))
                .expect("minimal message manager should deserialize through defaults");
        let mut source = participant("source", "Source", false);
        source.hp = 100.0;
        source.max_hp = 100.0;
        let mut target = participant("target", "Target", false);
        target.hp = 100.0;
        target.max_hp = 100.0;
        let mut encounter = BattleEncounter {
            participants: vec![source, target],
            ..Default::default()
        };

        super::super::apply_battle_buff_ticks(&mut encounter, &manager, &[
            super::super::BattleBuffTick {
                source_id: "source".to_owned(),
                target_id: "target".to_owned(),
                action_name: "燃烧".to_owned(),
                action: crate::rule_engine::BuffTickAction::Damage {
                    amount: 9.0,
                    damage_type: crate::rule_engine::DamageType::Magical,
                },
            },
        ]);

        let event = encounter
            .combat_log
            .iter()
            .find(|entry| entry.kind == CombatLogKind::Damage)
            .expect("buff damage should be available to post-match summary");
        assert_eq!(event.action_name, "燃烧");
        assert_eq!(event.source_name, "Source");
        assert_eq!(event.target_name, "Target");
        assert_eq!(event.effective_amount, 9.0);
    }
}
