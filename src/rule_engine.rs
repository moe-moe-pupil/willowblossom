use std::collections::HashMap;

use bevy::prelude::*;
use bevy_egui::{
    egui,
    EguiContexts,
    EguiPrimaryContextPass,
};

pub struct RuleEnginePlugin;

impl Plugin for RuleEnginePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RuleEngineState>().add_systems(
            EguiPrimaryContextPass,
            rule_engine_panel,
        );
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleAst {
    pub raw: String,
    pub trigger: Trigger,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trigger {
    pub subject: ActorRef,
    pub event: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    DamageTaken,
    DamageDealt,
    SkillCast,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Heal {
        target: ActorRef,
        amount: ValueExpr,
    },
    Damage {
        target: ActorRef,
        amount: ValueExpr,
        damage_type: DamageType,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorRef {
    SelfActor,
    Source,
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueExpr {
    Number(f32),
    EventDamage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageType {
    Physical,
    Magical,
    None,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub owner_id: String,
    pub ast: RuleAst,
}

#[derive(Debug, Clone)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub hp: f32,
    pub max_hp: f32,
    pub damage_dealt_modifier: f32,
    pub damage_taken_modifier: f32,
    pub healing_dealt_modifier: f32,
    pub healing_taken_modifier: f32,
}

#[derive(Debug, Clone)]
pub enum RuleEvent {
    DamageTaken {
        source_id: String,
        target_id: String,
        amount: f32,
        damage_type: DamageType,
    },
    DamageDealt {
        source_id: String,
        target_id: String,
        amount: f32,
        damage_type: DamageType,
    },
    SkillCast {
        source_id: String,
        target_ids: Vec<String>,
    },
}

#[derive(Debug, Default, Clone)]
pub struct RuleEngine {
    pub characters: HashMap<String, Character>,
    pub rules: Vec<Rule>,
    pub log: Vec<String>,
}

#[derive(Resource)]
pub struct RuleEngineState {
    engine: RuleEngine,
    rule_input: String,
    parse_preview: String,
    attack_amount: f32,
}

impl Default for RuleEngineState {
    fn default() -> Self {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));

        let rule_input = "每当自己受到伤害，回复2点生命值".to_owned();
        let parse_preview = match parse_rule(&rule_input) {
            Ok(ast) => {
                engine.add_rule("alice", ast.clone());
                ast.explain()
            },
            Err(err) => err,
        };

        Self {
            engine,
            rule_input,
            parse_preview,
            attack_amount: 3.0,
        }
    }
}

impl Character {
    pub fn new(id: impl Into<String>, name: impl Into<String>, max_hp: f32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            hp: max_hp,
            max_hp,
            damage_dealt_modifier: 1.0,
            damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            healing_taken_modifier: 1.0,
        }
    }
}

impl RuleAst {
    pub fn explain(&self) -> String {
        let mut lines = vec![format!(
            "触发：每当{}{}。",
            self.trigger.subject.explain(),
            self.trigger.event.explain()
        )];
        for action in &self.actions {
            lines.push(format!("动作：{}。", action.explain()));
        }
        lines.join("\n")
    }
}

impl EventKind {
    fn explain(self) -> &'static str {
        match self {
            EventKind::DamageTaken => "受到伤害",
            EventKind::DamageDealt => "造成伤害",
            EventKind::SkillCast => "释放技能",
        }
    }
}

impl ActorRef {
    fn explain(self) -> &'static str {
        match self {
            ActorRef::SelfActor => "自己",
            ActorRef::Source => "伤害来源",
            ActorRef::Target => "目标",
        }
    }
}

impl Action {
    fn explain(&self) -> String {
        match self {
            Action::Heal { target, amount } => {
                format!(
                    "回复{}点生命值给{}",
                    amount.explain(),
                    target.explain()
                )
            },
            Action::Damage {
                target,
                amount,
                damage_type,
            } => {
                format!(
                    "对{}造成{}点{}伤害",
                    target.explain(),
                    amount.explain(),
                    damage_type.explain()
                )
            },
        }
    }
}

impl ValueExpr {
    fn eval(self, event: &RuleEvent) -> f32 {
        match self {
            ValueExpr::Number(value) => value,
            ValueExpr::EventDamage => match event {
                RuleEvent::DamageTaken { amount, .. } | RuleEvent::DamageDealt { amount, .. } => {
                    *amount
                },
                RuleEvent::SkillCast { .. } => 0.0,
            },
        }
    }

    fn explain(self) -> String {
        match self {
            ValueExpr::Number(value) => format_number(value),
            ValueExpr::EventDamage => "本次伤害".to_owned(),
        }
    }
}

impl DamageType {
    fn explain(self) -> &'static str {
        match self {
            DamageType::Physical => "物理",
            DamageType::Magical => "魔法",
            DamageType::None => "无类型",
        }
    }
}

impl RuleEngine {
    pub fn add_character(&mut self, character: Character) {
        self.characters.insert(character.id.clone(), character);
    }

    pub fn add_rule(&mut self, owner_id: impl Into<String>, ast: RuleAst) {
        self.rules.push(Rule {
            owner_id: owner_id.into(),
            ast,
        });
    }

    pub fn replace_rules_for_owner(&mut self, owner_id: &str, ast: RuleAst) {
        self.rules.retain(|rule| rule.owner_id != owner_id);
        self.add_rule(owner_id, ast);
    }

    pub fn attack(
        &mut self,
        source_id: &str,
        target_id: &str,
        amount: f32,
        damage_type: DamageType,
    ) {
        let source_modifier = self
            .characters
            .get(source_id)
            .map(|character| character.damage_dealt_modifier)
            .unwrap_or(1.0);
        let target_modifier = self
            .characters
            .get(target_id)
            .map(|character| character.damage_taken_modifier)
            .unwrap_or(1.0);
        let final_damage = (amount * source_modifier * target_modifier).max(0.0);

        if let Some(target) = self.characters.get_mut(target_id) {
            target.hp = (target.hp - final_damage).max(0.0);
            self.log.push(format!(
                "{}受到{}点伤害，生命值变为 {}/{}",
                target.name,
                format_number(final_damage),
                format_number(target.hp),
                format_number(target.max_hp)
            ));
        }

        self.resolve_event(RuleEvent::DamageTaken {
            source_id: source_id.to_owned(),
            target_id: target_id.to_owned(),
            amount: final_damage,
            damage_type,
        });
        self.resolve_event(RuleEvent::DamageDealt {
            source_id: source_id.to_owned(),
            target_id: target_id.to_owned(),
            amount: final_damage,
            damage_type,
        });
    }

    pub fn heal(&mut self, source_id: &str, target_id: &str, amount: f32) {
        let source_modifier = self
            .characters
            .get(source_id)
            .map(|character| character.healing_dealt_modifier)
            .unwrap_or(1.0);
        let target_modifier = self
            .characters
            .get(target_id)
            .map(|character| character.healing_taken_modifier)
            .unwrap_or(1.0);
        let final_heal = (amount * source_modifier * target_modifier).max(0.0);

        if let Some(target) = self.characters.get_mut(target_id) {
            target.hp = (target.hp + final_heal).min(target.max_hp);
            self.log.push(format!(
                "{}回复{}点生命值，生命值变为 {}/{}",
                target.name,
                format_number(final_heal),
                format_number(target.hp),
                format_number(target.max_hp)
            ));
        }
    }

    pub fn resolve_event(&mut self, event: RuleEvent) {
        let matched_rules = self
            .rules
            .iter()
            .filter(|rule| rule_matches(rule, &event))
            .cloned()
            .collect::<Vec<_>>();

        for rule in matched_rules {
            for action in rule.ast.actions {
                self.apply_action(&rule.owner_id, &event, action);
            }
        }
    }

    fn apply_action(&mut self, owner_id: &str, event: &RuleEvent, action: Action) {
        match action {
            Action::Heal { target, amount } => {
                let Some(target_id) = resolve_actor(target, owner_id, event) else {
                    return;
                };
                let source_id = owner_id.to_owned();
                self.log.push(format!(
                    "规则触发：{} -> {}",
                    rule_event_name(event),
                    action.explain()
                ));
                self.heal(
                    &source_id,
                    &target_id,
                    amount.eval(event),
                );
            },
            Action::Damage {
                target,
                amount,
                damage_type,
            } => {
                let Some(target_id) = resolve_actor(target, owner_id, event) else {
                    return;
                };
                self.log.push(format!(
                    "规则触发：{} -> {}",
                    rule_event_name(event),
                    action.explain()
                ));
                self.attack(
                    owner_id,
                    &target_id,
                    amount.eval(event),
                    damage_type,
                );
            },
        }
    }
}

pub fn parse_rule(input: &str) -> Result<RuleAst, String> {
    let normalized = normalize_rule_text(input);
    if normalized.is_empty() {
        return Err("规则为空".to_owned());
    }

    let trigger = parse_trigger(&normalized)?;
    let actions = parse_actions(&normalized)?;
    if actions.is_empty() {
        return Err("没有找到可执行动作，例如：回复2点生命值".to_owned());
    }

    Ok(RuleAst {
        raw: input.to_owned(),
        trigger,
        actions,
    })
}

fn parse_trigger(text: &str) -> Result<Trigger, String> {
    if text.contains("受到伤害") || text.contains("承受伤害") || text.contains("受伤害")
    {
        return Ok(Trigger {
            subject: parse_trigger_subject(text),
            event: EventKind::DamageTaken,
        });
    }
    if text.contains("造成伤害") {
        return Ok(Trigger {
            subject: parse_trigger_subject(text),
            event: EventKind::DamageDealt,
        });
    }
    if text.contains("释放技能") || text.contains("技能释放") {
        return Ok(Trigger {
            subject: parse_trigger_subject(text),
            event: EventKind::SkillCast,
        });
    }

    Err("没有识别到触发条件；目前支持“受到伤害 / 造成伤害 / 释放技能”".to_owned())
}

fn parse_trigger_subject(text: &str) -> ActorRef {
    let trigger_clause = text.split(['，', ',', '；', ';']).next().unwrap_or(text);
    if trigger_clause.contains("目标") {
        ActorRef::Target
    } else if trigger_clause.contains("来源") || trigger_clause.contains("攻击者") {
        ActorRef::Source
    } else {
        ActorRef::SelfActor
    }
}

fn parse_actions(text: &str) -> Result<Vec<Action>, String> {
    let action_text = action_clause(text);
    let mut actions = Vec::new();

    for clause in action_text.split(['，', ',', '；', ';']) {
        if let Some(amount) = parse_value_before(clause, "点生命值")
            .or_else(|| parse_value_after_action(clause, &["回复", "恢复", "治疗"]))
        {
            if clause.contains("回复") || clause.contains("恢复") || clause.contains("治疗") {
                actions.push(Action::Heal {
                    target: parse_action_target(clause, ActorRef::SelfActor),
                    amount,
                });
            }
        }

        if let Some(amount) = parse_value_before(clause, "点伤害")
            .or_else(|| parse_value_after_action(clause, &["造成"]))
        {
            if clause.contains("伤害") {
                actions.push(Action::Damage {
                    target: parse_action_target(clause, ActorRef::Target),
                    amount,
                    damage_type: parse_damage_type(clause),
                });
            }
        }
    }

    Ok(actions)
}

fn action_clause(text: &str) -> &str {
    for marker in ["则", "时", "，", ","] {
        if let Some((_, tail)) = text.split_once(marker) {
            if contains_action_word(tail) {
                return tail;
            }
        }
    }
    text
}

fn contains_action_word(text: &str) -> bool {
    ["回复", "恢复", "治疗", "造成", "给予"]
        .iter()
        .any(|word| text.contains(word))
}

fn parse_action_target(clause: &str, default_target: ActorRef) -> ActorRef {
    if clause.contains("目标") {
        ActorRef::Target
    } else if clause.contains("来源") || clause.contains("攻击者") {
        ActorRef::Source
    } else if clause.contains("自己") || clause.contains("自身") || clause.contains("你") {
        ActorRef::SelfActor
    } else {
        default_target
    }
}

fn parse_damage_type(clause: &str) -> DamageType {
    if clause.contains("物理") {
        DamageType::Physical
    } else if clause.contains("魔法") || clause.contains("法术") {
        DamageType::Magical
    } else {
        DamageType::None
    }
}

fn parse_value_after_action(clause: &str, actions: &[&str]) -> Option<ValueExpr> {
    for action in actions {
        let Some(index) = clause.find(action) else {
            continue;
        };
        let tail = &clause[index + action.len()..];
        if tail.contains("本次伤害") || tail.contains("此次伤害") {
            return Some(ValueExpr::EventDamage);
        }
        if let Some(value) = parse_leading_number(tail) {
            return Some(ValueExpr::Number(value));
        }
    }
    None
}

fn parse_value_before(clause: &str, marker: &str) -> Option<ValueExpr> {
    let marker_index = clause.find(marker)?;
    let before_marker = &clause[..marker_index];
    if before_marker.contains("本次伤害") || before_marker.contains("此次伤害") {
        return Some(ValueExpr::EventDamage);
    }
    parse_trailing_number(before_marker).map(ValueExpr::Number)
}

fn parse_leading_number(text: &str) -> Option<f32> {
    let digits = text
        .chars()
        .skip_while(|character| !character.is_ascii_digit() && *character != '.')
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    digits.parse().ok()
}

fn parse_trailing_number(text: &str) -> Option<f32> {
    let reversed = text
        .chars()
        .rev()
        .skip_while(|character| !character.is_ascii_digit() && *character != '.')
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    let digits = reversed.chars().rev().collect::<String>();
    digits.parse().ok()
}

fn normalize_rule_text(input: &str) -> String {
    input
        .trim()
        .replace(' ', "")
        .replace('\n', "")
        .replace("每当", "当")
        .replace("无论何时", "当")
}

fn rule_matches(rule: &Rule, event: &RuleEvent) -> bool {
    let expected_actor = match rule.ast.trigger.subject {
        ActorRef::SelfActor => Some(rule.owner_id.as_str()),
        ActorRef::Source => event_source_id(event),
        ActorRef::Target => event_target_id(event),
    };

    if expected_actor != Some(rule.owner_id.as_str()) {
        return false;
    }

    matches!(
        (&rule.ast.trigger.event, event),
        (
            EventKind::DamageTaken,
            RuleEvent::DamageTaken { .. }
        ) | (
            EventKind::DamageDealt,
            RuleEvent::DamageDealt { .. }
        ) | (
            EventKind::SkillCast,
            RuleEvent::SkillCast { .. }
        )
    )
}

fn resolve_actor(actor: ActorRef, owner_id: &str, event: &RuleEvent) -> Option<String> {
    match actor {
        ActorRef::SelfActor => Some(owner_id.to_owned()),
        ActorRef::Source => event_source_id(event).map(ToOwned::to_owned),
        ActorRef::Target => event_target_id(event).map(ToOwned::to_owned),
    }
}

fn event_source_id(event: &RuleEvent) -> Option<&str> {
    match event {
        RuleEvent::DamageTaken { source_id, .. }
        | RuleEvent::DamageDealt { source_id, .. }
        | RuleEvent::SkillCast { source_id, .. } => Some(source_id),
    }
}

fn event_target_id(event: &RuleEvent) -> Option<&str> {
    match event {
        RuleEvent::DamageTaken { target_id, .. } | RuleEvent::DamageDealt { target_id, .. } => {
            Some(target_id)
        },
        RuleEvent::SkillCast { target_ids, .. } => target_ids.first().map(String::as_str),
    }
}

fn rule_event_name(event: &RuleEvent) -> String {
    match event {
        RuleEvent::DamageTaken {
            amount,
            damage_type,
            ..
        } => {
            format!(
                "受到{}点{}伤害",
                format_number(*amount),
                damage_type.explain()
            )
        },
        RuleEvent::DamageDealt {
            amount,
            damage_type,
            ..
        } => {
            format!(
                "造成{}点{}伤害",
                format_number(*amount),
                damage_type.explain()
            )
        },
        RuleEvent::SkillCast { .. } => "释放技能".to_owned(),
    }
}

fn format_number(value: f32) -> String {
    if (value.fract()).abs() < f32::EPSILON {
        format!("{}", value as i32)
    } else {
        format!("{value:.2}")
    }
}

fn rule_engine_panel(mut contexts: EguiContexts, mut state: ResMut<RuleEngineState>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Window::new("Rule Engine")
        .default_pos(egui::pos2(12.0, 430.0))
        .default_width(360.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("中文规则");
            ui.text_edit_multiline(&mut state.rule_input);
            ui.horizontal(|ui| {
                if ui.button("Parse").clicked() {
                    match parse_rule(&state.rule_input) {
                        Ok(ast) => {
                            state.parse_preview = ast.explain();
                            state.engine.replace_rules_for_owner("alice", ast);
                        },
                        Err(err) => state.parse_preview = err,
                    }
                }
                if ui.button("Enemy Attack").clicked() {
                    let attack_amount = state.attack_amount;
                    state.engine.attack(
                        "enemy",
                        "alice",
                        attack_amount,
                        DamageType::Physical,
                    );
                }
                if ui.button("Reset").clicked() {
                    *state = RuleEngineState::default();
                }
            });
            ui.add(egui::Slider::new(&mut state.attack_amount, 0.0..=20.0).text("Damage"));
            ui.separator();
            ui.label(&state.parse_preview);
            ui.separator();
            if let Some(character) = state.engine.characters.get("alice") {
                ui.label(format!(
                    "{} HP: {}/{}",
                    character.name,
                    format_number(character.hp),
                    format_number(character.max_hp)
                ));
            }
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    for line in state.engine.log.iter().rev().take(12) {
                        ui.label(line);
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_damage_taken_heal_rule() {
        let ast = parse_rule("每当自己受到伤害，回复2点生命值").unwrap();

        assert_eq!(
            ast.trigger.event,
            EventKind::DamageTaken
        );
        assert_eq!(ast.trigger.subject, ActorRef::SelfActor);
        assert_eq!(ast.actions, vec![Action::Heal {
            target: ActorRef::SelfActor,
            amount: ValueExpr::Number(2.0),
        }]);
        assert_eq!(
            ast.explain(),
            "触发：每当自己受到伤害。\n动作：回复2点生命值给自己。"
        );
    }

    #[test]
    fn damage_taken_rule_runs_after_attack() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));
        engine.add_rule(
            "alice",
            parse_rule("每当自己受到伤害，回复2点生命值").unwrap(),
        );

        engine.attack(
            "enemy",
            "alice",
            3.0,
            DamageType::Physical,
        );

        let alice = engine.characters.get("alice").unwrap();
        assert_eq!(alice.hp, 9.0);
        assert!(engine
            .log
            .iter()
            .any(|line| line.contains("规则触发：受到3点物理伤害")));
    }

    #[test]
    fn healing_is_capped_at_max_hp() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.heal("alice", "alice", 5.0);

        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            10.0
        );
    }

    #[test]
    fn attack_uses_dealt_and_taken_modifiers() {
        let mut engine = RuleEngine::default();
        let mut alice = Character::new("alice", "自己", 10.0);
        alice.damage_taken_modifier = 0.5;
        let mut enemy = Character::new("enemy", "敌人", 10.0);
        enemy.damage_dealt_modifier = 2.0;
        engine.add_character(alice);
        engine.add_character(enemy);

        engine.attack(
            "enemy",
            "alice",
            4.0,
            DamageType::Physical,
        );

        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            6.0
        );
    }
}
