use bevy_egui::egui::{
    Button,
    ComboBox,
    DragValue,
    Ui,
};

use super::{
    battle_damage_type_label,
    format_number,
    BattleDelayedDamageTick,
    BattleDelayedHealingTick,
    BattleEncounter,
};
use crate::rule_engine::DamageType;

pub(super) const GM_DOT_SOURCE_ID: &str = "system:gm_dot";
pub(super) const GM_HOT_SOURCE_ID: &str = "system:gm_hot";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum GmOverTimeKind {
    #[default]
    Damage,
    Healing,
}

pub(super) struct GmOverTimeDraft {
    pub(super) kind: GmOverTimeKind,
    pub(super) target_id: String,
    pub(super) amount: f32,
    pub(super) rounds: i32,
    pub(super) damage_type: DamageType,
}

impl Default for GmOverTimeDraft {
    fn default() -> Self {
        Self {
            kind: GmOverTimeKind::Damage,
            target_id: String::new(),
            amount: 1.0,
            rounds: 3,
            damage_type: DamageType::Poisoning,
        }
    }
}

pub(super) fn gm_over_time_ui(
    ui: &mut Ui,
    draft: &mut GmOverTimeDraft,
    encounter: &mut BattleEncounter,
) -> bool {
    let mut changed = false;
    ui.collapsing("GM持续效果（DOT / HOT）", |ui| {
        ui.small("从下一轮开始，每轮结算一次固定数值；不消耗角色行动。");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut draft.kind,
                GmOverTimeKind::Damage,
                "持续伤害（DOT）",
            );
            ui.selectable_value(
                &mut draft.kind,
                GmOverTimeKind::Healing,
                "持续治疗（HOT）",
            );
        });
        match draft.kind {
            GmOverTimeKind::Damage => ui.small("持续伤害可被护盾吸收。"),
            GmOverTimeKind::Healing => {
                ui.small("持续治疗最多恢复至生命上限，不会复活已倒下的角色。")
            },
        };
        if encounter.participants.is_empty() {
            ui.label("请先添加参战角色。");
            return;
        }
        if !encounter
            .participants
            .iter()
            .any(|participant| participant.target_id == draft.target_id)
        {
            draft.target_id = encounter
                .participants
                .iter()
                .find(|participant| participant.alive)
                .unwrap_or(&encounter.participants[0])
                .target_id
                .clone();
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("目标");
            let selected_name = encounter
                .participants
                .iter()
                .find(|participant| participant.target_id == draft.target_id)
                .map(|participant| participant.display_name.as_str())
                .unwrap_or("选择角色");
            ComboBox::from_id_salt("gm_over_time_target")
                .selected_text(selected_name)
                .show_ui(ui, |ui| {
                    for participant in &encounter.participants {
                        let label = if participant.alive {
                            participant.display_name.clone()
                        } else {
                            format!("{}（已倒下）", participant.display_name)
                        };
                        ui.selectable_value(
                            &mut draft.target_id,
                            participant.target_id.clone(),
                            label,
                        );
                    }
                });
            if draft.kind == GmOverTimeKind::Damage {
                ui.label("伤害类型");
                ComboBox::from_id_salt("gm_over_time_damage_type")
                    .selected_text(battle_damage_type_label(
                        draft.damage_type,
                    ))
                    .show_ui(ui, |ui| {
                        for damage_type in [
                            DamageType::Poisoning,
                            DamageType::Bleed,
                            DamageType::Cursed,
                            DamageType::Diseased,
                            DamageType::Physical,
                            DamageType::Magical,
                            DamageType::Range,
                            DamageType::None,
                        ] {
                            ui.selectable_value(
                                &mut draft.damage_type,
                                damage_type,
                                battle_damage_type_label(damage_type),
                            );
                        }
                    });
            }
        });
        ui.horizontal_wrapped(|ui| {
            let (amount_label, apply_label) = match draft.kind {
                GmOverTimeKind::Damage => ("每轮伤害", "施加持续伤害"),
                GmOverTimeKind::Healing => ("每轮治疗", "施加持续治疗"),
            };
            ui.label(amount_label);
            ui.add(
                DragValue::new(&mut draft.amount)
                    .speed(1.0)
                    .range(0.0..=9999.0),
            );
            ui.label("持续轮数");
            ui.add(DragValue::new(&mut draft.rounds).range(1..=99));
            if ui
                .add_enabled(
                    can_apply_gm_over_time(encounter, draft),
                    Button::new(apply_label),
                )
                .clicked()
            {
                changed |= match draft.kind {
                    GmOverTimeKind::Damage => apply_gm_dot(encounter, draft),
                    GmOverTimeKind::Healing => apply_gm_hot(encounter, draft),
                };
            }
        });

        let Some(participant) = encounter
            .participants
            .iter()
            .find(|participant| participant.target_id == draft.target_id)
        else {
            return;
        };
        let effects = participant
            .delayed_damage_ticks
            .iter()
            .enumerate()
            .filter(|(_, tick)| tick.source_id == GM_DOT_SOURCE_ID && tick.repeat_each_round)
            .map(|(index, tick)| {
                (
                    GmOverTimeKind::Damage,
                    index,
                    tick.name.as_str(),
                    tick.amount,
                    tick.turns_remaining,
                )
            })
            .chain(
                participant
                    .delayed_healing_ticks
                    .iter()
                    .enumerate()
                    .filter(|(_, tick)| {
                        tick.source_id == GM_HOT_SOURCE_ID && tick.repeat_each_round
                    })
                    .map(|(index, tick)| {
                        (
                            GmOverTimeKind::Healing,
                            index,
                            tick.name.as_str(),
                            tick.amount,
                            tick.turns_remaining,
                        )
                    }),
            );
        let mut remove_effect = None;
        let mut has_effects = false;
        for (kind, index, name, amount, rounds) in effects {
            has_effects = true;
            ui.push_id((kind, index), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "{}：每轮{}点，剩余{}轮",
                        name,
                        format_number(amount),
                        rounds,
                    ));
                    if ui
                        .add_enabled(
                            !encounter.manager_sync_quarantined,
                            Button::new("移除"),
                        )
                        .clicked()
                    {
                        remove_effect = Some((kind, index));
                    }
                });
            });
        }
        if !has_effects {
            ui.small("该角色暂无GM持续效果。");
        }
        if let Some((kind, index)) = remove_effect {
            changed |= match kind {
                GmOverTimeKind::Damage => remove_gm_dot(encounter, &draft.target_id, index),
                GmOverTimeKind::Healing => remove_gm_hot(encounter, &draft.target_id, index),
            };
        }
    });
    changed
}

fn can_apply_gm_over_time(encounter: &BattleEncounter, draft: &GmOverTimeDraft) -> bool {
    !encounter.manager_sync_quarantined
        && draft.amount.is_finite()
        && draft.amount > 0.0
        && draft.amount <= 9999.0
        && (1..=99).contains(&draft.rounds)
        && encounter
            .participants
            .iter()
            .any(|participant| participant.target_id == draft.target_id && participant.alive)
}

pub(super) fn apply_gm_dot(encounter: &mut BattleEncounter, draft: &GmOverTimeDraft) -> bool {
    apply_gm_over_time(encounter, draft, GmOverTimeKind::Damage)
}

pub(super) fn apply_gm_hot(encounter: &mut BattleEncounter, draft: &GmOverTimeDraft) -> bool {
    apply_gm_over_time(
        encounter,
        draft,
        GmOverTimeKind::Healing,
    )
}

fn apply_gm_over_time(
    encounter: &mut BattleEncounter,
    draft: &GmOverTimeDraft,
    kind: GmOverTimeKind,
) -> bool {
    if !can_apply_gm_over_time(encounter, draft) {
        return false;
    }
    let Some(participant) = encounter
        .participants
        .iter_mut()
        .find(|participant| participant.target_id == draft.target_id)
    else {
        return false;
    };
    let name = match kind {
        GmOverTimeKind::Damage => format!(
            "持续伤害·{}",
            battle_damage_type_label(draft.damage_type)
        ),
        GmOverTimeKind::Healing => "持续治疗".to_owned(),
    };
    match kind {
        GmOverTimeKind::Damage => participant
            .delayed_damage_ticks
            .push(BattleDelayedDamageTick {
                name: name.clone(),
                source_id: GM_DOT_SOURCE_ID.to_owned(),
                source_name: "GM".to_owned(),
                amount: draft.amount,
                damage_type: draft.damage_type,
                turns_remaining: draft.rounds,
                repeat_each_round: true,
            }),
        GmOverTimeKind::Healing => {
            participant
                .delayed_healing_ticks
                .push(BattleDelayedHealingTick {
                    name: name.clone(),
                    source_id: GM_HOT_SOURCE_ID.to_owned(),
                    source_name: "GM".to_owned(),
                    amount: draft.amount,
                    overhealing_shield_cap_rate: 0.0,
                    turns_remaining: draft.rounds,
                    repeat_each_round: true,
                })
        },
    }
    encounter.action_log.push(format!(
        "GM为{}施加{}：每轮{}点，持续{}轮（下一轮开始）",
        participant.display_name,
        name,
        format_number(draft.amount),
        draft.rounds,
    ));
    true
}

pub(super) fn remove_gm_dot(
    encounter: &mut BattleEncounter,
    target_id: &str,
    index: usize,
) -> bool {
    remove_gm_over_time(
        encounter,
        target_id,
        index,
        GmOverTimeKind::Damage,
    )
}

pub(super) fn remove_gm_hot(
    encounter: &mut BattleEncounter,
    target_id: &str,
    index: usize,
) -> bool {
    remove_gm_over_time(
        encounter,
        target_id,
        index,
        GmOverTimeKind::Healing,
    )
}

fn remove_gm_over_time(
    encounter: &mut BattleEncounter,
    target_id: &str,
    index: usize,
    kind: GmOverTimeKind,
) -> bool {
    if encounter.manager_sync_quarantined {
        return false;
    }
    let Some(participant) = encounter
        .participants
        .iter_mut()
        .find(|participant| participant.target_id == target_id)
    else {
        return false;
    };
    let (name, rounds) = match kind {
        GmOverTimeKind::Damage => {
            let Some(tick) = participant.delayed_damage_ticks.get(index) else {
                return false;
            };
            if tick.source_id != GM_DOT_SOURCE_ID || !tick.repeat_each_round {
                return false;
            }
            let tick = participant.delayed_damage_ticks.remove(index);
            (tick.name, tick.turns_remaining)
        },
        GmOverTimeKind::Healing => {
            let Some(tick) = participant.delayed_healing_ticks.get(index) else {
                return false;
            };
            if tick.source_id != GM_HOT_SOURCE_ID || !tick.repeat_each_round {
                return false;
            }
            let tick = participant.delayed_healing_ticks.remove(index);
            (tick.name, tick.turns_remaining)
        },
    };
    encounter.action_log.push(format!(
        "GM移除了{}的{}（剩余{}轮）",
        participant.display_name, name, rounds,
    ));
    true
}
