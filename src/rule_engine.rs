use std::collections::{
    HashMap,
    HashSet,
    VecDeque,
};

use bevy::prelude::*;
use bevy_egui::{
    egui,
    EguiContexts,
    EguiPrimaryContextPass,
};
use serde::{
    Deserialize,
    Serialize,
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
        target: TargetSelector,
        amount: ValueExpr,
    },
    Damage {
        target: TargetSelector,
        amount: ValueExpr,
        damage_type: DamageType,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetSelector {
    pub actor: ActorRef,
    pub area: Option<AreaSelector>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaSelector {
    pub radius_meters: Option<f32>,
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
    Cursed,
    Diseased,
    Bleed,
    Range,
    Poisoning,
    Physical,
    Magical,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealType {
    Instant,
    OverTime,
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

#[derive(Component, Debug, Clone)]
pub struct Combatant {
    pub id: String,
    pub name: String,
    pub hp: f32,
    pub max_hp: f32,
    pub mp: f32,
    pub max_mp: f32,
    pub hp_regen: f32,
    pub mp_regen: f32,
}

#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct StatusBlock {
    pub str_: i32,
    pub agi: i32,
    pub dex: i32,
    pub vit: i32,
    pub int_: i32,
    pub wis: i32,
    pub k: i32,
    pub cha: i32,
}

#[derive(Component, Debug, Clone)]
pub struct CombatModifiers {
    pub damage_dealt: f32,
    pub damage_taken: f32,
    pub healing_dealt: f32,
    pub healing_taken: f32,
}

#[derive(Component, Debug, Clone)]
pub struct BaseCombatant(pub Combatant);

#[derive(Component, Debug, Clone)]
pub struct BaseStatusBlock(pub StatusBlock);

#[derive(Component, Debug, Clone)]
pub struct BaseCombatModifiers(pub CombatModifiers);

#[derive(Component, Debug, Clone)]
pub struct BuffOwner {
    pub target: Entity,
}

#[derive(Component, Debug, Clone)]
pub struct ActiveBuff {
    pub name: String,
    pub kind: BuffKind,
    pub priority: i32,
    pub turns_remaining: i32,
    pub source_id: String,
    pub beneficial: bool,
}

#[derive(Component, Debug, Clone)]
pub struct BuffEffects(pub Vec<BuffEffect>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuffSpec {
    pub name: String,
    #[serde(default)]
    pub kind: BuffKind,
    pub priority: i32,
    pub turns_remaining: i32,
    pub source_id: String,
    pub beneficial: bool,
    pub effects: Vec<BuffEffect>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuffKind {
    #[default]
    #[serde(alias = "normal")]
    None,
    Magic,
    Physical,
    Curse,
    Disease,
    Bleed,
    Range,
    Poison,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuffEffect {
    pub field: BuffField,
    pub value: BuffValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuffField {
    Hp,
    Mp,
    MaxHp,
    MaxMp,
    HpRegen,
    MpRegen,
    Status(StatusKey),
    DamageDealtModifier,
    DamageTakenModifier,
    HealingDealtModifier,
    HealingTakenModifier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusKey {
    Str,
    Agi,
    Dex,
    Vit,
    Int,
    Wis,
    K,
    Cha,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BuffValue {
    Add(f32),
    AddPercent(f32),
    Set(f32),
    SetPercentOfBase(f32),
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

pub struct RuleEngine {
    ecs_world: World,
    entity_by_id: HashMap<String, Entity>,
    pub characters: HashMap<String, Character>,
    pub rules: Vec<Rule>,
    pub log: Vec<String>,
    event_queue: VecDeque<RuleEvent>,
    resolving_events: bool,
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self {
            ecs_world: World::new(),
            entity_by_id: HashMap::new(),
            characters: HashMap::new(),
            rules: Vec::new(),
            log: Vec::new(),
            event_queue: VecDeque::new(),
            resolving_events: false,
        }
    }
}

#[derive(Resource)]
pub struct RuleEngineState {
    engine: RuleEngine,
    rule_input: String,
    parse_preview: String,
    attack_amount: f32,
    panel_open: bool,
}

impl Default for RuleEngineState {
    fn default() -> Self {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));

        let rule_input = "每当自己受到伤害时，回复2点生命值".to_owned();
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
            panel_open: true,
        }
    }
}

impl RuleEngineState {
    pub fn open_panel(&mut self) { self.panel_open = true; }

    pub fn sync_character(
        &mut self,
        owner_id: &str,
        name: &str,
        hp: f32,
        max_hp: f32,
        damage_dealt_modifier: f32,
        damage_taken_modifier: f32,
        healing_dealt_modifier: f32,
        healing_taken_modifier: f32,
        rules: Vec<RuleAst>,
    ) {
        let mut character = Character::new(owner_id, name, max_hp.max(0.0));
        character.hp = hp.clamp(0.0, character.max_hp);
        character.damage_dealt_modifier = damage_dealt_modifier;
        character.damage_taken_modifier = damage_taken_modifier;
        character.healing_dealt_modifier = healing_dealt_modifier;
        character.healing_taken_modifier = healing_taken_modifier;
        self.engine.add_character(character);
        self.engine.replace_rules_for_owner(owner_id, rules);
    }

    pub fn replace_character_buffs(&mut self, target_id: &str, buffs: Vec<BuffSpec>) {
        self.engine.replace_buffs_for_target(target_id, buffs);
    }

    pub fn character(&self, target_id: &str) -> Option<&Character> {
        self.engine.characters.get(target_id)
    }

    pub fn active_buff_names(&mut self, target_id: &str) -> Vec<String> {
        self.engine.active_buff_names(target_id)
    }

    pub fn cast_skill(&mut self, source_id: &str, target_ids: impl IntoIterator<Item = String>) {
        self.engine.cast_skill(source_id, target_ids);
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

impl TargetSelector {
    pub fn single(actor: ActorRef) -> Self { Self { actor, area: None } }

    fn explain(self) -> String {
        match self.area {
            Some(area) => {
                let radius = area
                    .radius_meters
                    .map(|radius| format!("{}米内", format_number(radius)))
                    .unwrap_or_default();
                format!(
                    "周围{}的{}",
                    radius,
                    self.actor.explain()
                )
            },
            None => self.actor.explain().to_owned(),
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
            DamageType::Cursed => "诅咒",
            DamageType::Diseased => "疾病",
            DamageType::Bleed => "流血",
            DamageType::Range => "远程",
            DamageType::Poisoning => "中毒",
            DamageType::Physical => "物理",
            DamageType::Magical => "魔法",
            DamageType::None => "无类型",
        }
    }
}

impl RuleEngine {
    pub fn add_character(&mut self, character: Character) {
        self.upsert_character_entity(&character);
        let character_id = character.id.clone();
        self.characters.insert(character_id.clone(), character);
        self.recompute_character_from_buffs(&character_id);
    }

    fn upsert_character_entity(&mut self, character: &Character) {
        let combatant = Combatant {
            id: character.id.clone(),
            name: character.name.clone(),
            hp: character.hp,
            max_hp: character.max_hp,
            mp: 0.0,
            max_mp: 0.0,
            hp_regen: 0.0,
            mp_regen: 0.0,
        };
        let modifiers = CombatModifiers {
            damage_dealt: character.damage_dealt_modifier,
            damage_taken: character.damage_taken_modifier,
            healing_dealt: character.healing_dealt_modifier,
            healing_taken: character.healing_taken_modifier,
        };

        if let Some(entity) = self.entity_by_id.get(&character.id).copied() {
            let mut entity_mut = self.ecs_world.entity_mut(entity);
            entity_mut.insert((
                combatant.clone(),
                BaseCombatant(combatant),
                StatusBlock::default(),
                BaseStatusBlock(StatusBlock::default()),
                modifiers.clone(),
                BaseCombatModifiers(modifiers),
            ));
        } else {
            let entity = self
                .ecs_world
                .spawn((
                    combatant.clone(),
                    BaseCombatant(combatant),
                    StatusBlock::default(),
                    BaseStatusBlock(StatusBlock::default()),
                    modifiers.clone(),
                    BaseCombatModifiers(modifiers),
                ))
                .id();
            self.entity_by_id.insert(character.id.clone(), entity);
        }
    }

    pub fn add_rule(&mut self, owner_id: impl Into<String>, ast: RuleAst) {
        self.rules.push(Rule {
            owner_id: owner_id.into(),
            ast,
        });
    }

    pub fn replace_rule_for_owner(&mut self, owner_id: &str, ast: RuleAst) {
        self.replace_rules_for_owner(owner_id, vec![ast]);
    }

    pub fn replace_rules_for_owner(&mut self, owner_id: &str, asts: Vec<RuleAst>) {
        self.rules.retain(|rule| rule.owner_id != owner_id);
        for ast in asts {
            self.add_rule(owner_id, ast);
        }
    }

    pub fn clear_rules_for_owner(&mut self, owner_id: &str) {
        self.rules.retain(|rule| rule.owner_id != owner_id);
    }

    pub fn give_buff(&mut self, target_id: &str, spec: BuffSpec) -> bool {
        let Some(target) = self.entity_by_id.get(target_id).copied() else {
            return false;
        };

        self.ecs_world.spawn((
            BuffOwner { target },
            ActiveBuff {
                name: spec.name,
                kind: spec.kind,
                priority: spec.priority,
                turns_remaining: spec.turns_remaining,
                source_id: spec.source_id,
                beneficial: spec.beneficial,
            },
            BuffEffects(spec.effects),
        ));
        self.recompute_character_from_buffs(target_id);
        true
    }

    pub fn replace_buffs_for_target(&mut self, target_id: &str, buffs: Vec<BuffSpec>) {
        let Some(target) = self.entity_by_id.get(target_id).copied() else {
            return;
        };

        let expired = self
            .ecs_world
            .query::<(Entity, &BuffOwner)>()
            .iter(&self.ecs_world)
            .filter_map(|(entity, owner)| (owner.target == target).then_some(entity))
            .collect::<Vec<_>>();
        for entity in expired {
            let _ = self.ecs_world.despawn(entity);
        }
        for buff in buffs {
            self.give_buff(target_id, buff);
        }
        self.recompute_character_from_buffs(target_id);
    }

    pub fn advance_turn(&mut self) {
        let mut expired = Vec::new();
        let mut changed_targets = HashSet::new();
        let mut query = self
            .ecs_world
            .query::<(Entity, &BuffOwner, &mut ActiveBuff)>();
        for (entity, owner, mut buff) in query.iter_mut(&mut self.ecs_world) {
            if buff.turns_remaining == 0 {
                continue;
            }
            if buff.turns_remaining < 0 {
                changed_targets.insert(owner.target);
                expired.push(entity);
                continue;
            }
            buff.turns_remaining -= 1;
            changed_targets.insert(owner.target);
            if buff.turns_remaining == 0 {
                expired.push(entity);
            }
        }
        for entity in expired {
            let _ = self.ecs_world.despawn(entity);
        }
        let changed_ids = self
            .entity_by_id
            .iter()
            .filter_map(|(id, entity)| changed_targets.contains(entity).then(|| id.clone()))
            .collect::<Vec<_>>();
        for id in changed_ids {
            self.recompute_character_from_buffs(&id);
        }
    }

    pub fn active_buff_names(&mut self, target_id: &str) -> Vec<String> {
        let Some(target) = self.entity_by_id.get(target_id).copied() else {
            return Vec::new();
        };
        let mut buffs = self
            .ecs_world
            .query::<(&BuffOwner, &ActiveBuff)>()
            .iter(&self.ecs_world)
            .filter(|(owner, _)| owner.target == target)
            .map(|(_, buff)| (buff.priority, buff.name.clone()))
            .collect::<Vec<_>>();
        buffs.sort_by_key(|(priority, _)| *priority);
        buffs.into_iter().map(|(_, name)| name).collect()
    }

    fn recompute_character_from_buffs(&mut self, character_id: &str) {
        let Some(entity) = self.entity_by_id.get(character_id).copied() else {
            return;
        };
        let Ok((base_combatant, base_status, base_modifiers)) = self
            .ecs_world
            .query::<(
                &BaseCombatant,
                &BaseStatusBlock,
                &BaseCombatModifiers,
            )>()
            .get(&self.ecs_world, entity)
        else {
            return;
        };

        let mut combatant = base_combatant.0.clone();
        let mut status = base_status.0.clone();
        let mut modifiers = base_modifiers.0.clone();
        let mut effects = self
            .ecs_world
            .query::<(&BuffOwner, &ActiveBuff, &BuffEffects)>()
            .iter(&self.ecs_world)
            .filter(|(owner, buff, _)| owner.target == entity && buff.turns_remaining >= 0)
            .map(|(_, buff, effects)| (buff.priority, effects.0.clone()))
            .collect::<Vec<_>>();
        effects.sort_by_key(|(priority, _)| *priority);

        for (_, effects) in effects {
            for effect in effects {
                apply_buff_effect(
                    &mut combatant,
                    &mut status,
                    &mut modifiers,
                    &effect,
                );
            }
        }

        combatant.max_hp = combatant.max_hp.max(0.0);
        combatant.hp = combatant.hp.clamp(0.0, combatant.max_hp);
        combatant.max_mp = combatant.max_mp.max(0.0);
        combatant.mp = combatant.mp.clamp(0.0, combatant.max_mp);

        let mut entity_mut = self.ecs_world.entity_mut(entity);
        entity_mut.insert((
            combatant.clone(),
            status,
            modifiers.clone(),
        ));

        if let Some(character) = self.characters.get_mut(character_id) {
            character.hp = combatant.hp;
            character.max_hp = combatant.max_hp;
            character.damage_dealt_modifier = modifiers.damage_dealt;
            character.damage_taken_modifier = modifiers.damage_taken;
            character.healing_dealt_modifier = modifiers.healing_dealt;
            character.healing_taken_modifier = modifiers.healing_taken;
        }
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

        let mut hp_update = None;
        if let Some(target) = self.characters.get_mut(target_id) {
            target.hp = (target.hp - final_damage).max(0.0);
            hp_update = Some((target.hp, target.max_hp));
            self.log.push(format!(
                "{}受到{}点伤害，生命值变为 {}/{}",
                target.name,
                format_number(final_damage),
                format_number(target.hp),
                format_number(target.max_hp)
            ));
        }
        if let Some((hp, max_hp)) = hp_update {
            self.sync_character_hp_to_ecs(target_id, hp, max_hp);
        }

        self.queue_event(RuleEvent::DamageTaken {
            source_id: source_id.to_owned(),
            target_id: target_id.to_owned(),
            amount: final_damage,
            damage_type,
        });
        self.queue_event(RuleEvent::DamageDealt {
            source_id: source_id.to_owned(),
            target_id: target_id.to_owned(),
            amount: final_damage,
            damage_type,
        });
        self.resolve_queued_events();
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

        let mut hp_update = None;
        if let Some(target) = self.characters.get_mut(target_id) {
            target.hp = (target.hp + final_heal).min(target.max_hp);
            hp_update = Some((target.hp, target.max_hp));
            self.log.push(format!(
                "{}回复{}点生命值，生命值变为 {}/{}",
                target.name,
                format_number(final_heal),
                format_number(target.hp),
                format_number(target.max_hp)
            ));
        }
        if let Some((hp, max_hp)) = hp_update {
            self.sync_character_hp_to_ecs(target_id, hp, max_hp);
        }
    }

    pub fn resolve_event(&mut self, event: RuleEvent) {
        self.queue_event(event);
        self.resolve_queued_events();
    }

    pub fn cast_skill(&mut self, source_id: &str, target_ids: impl IntoIterator<Item = String>) {
        self.resolve_event(RuleEvent::SkillCast {
            source_id: source_id.to_owned(),
            target_ids: target_ids.into_iter().collect(),
        });
    }

    fn queue_event(&mut self, event: RuleEvent) { self.event_queue.push_back(event); }

    fn resolve_queued_events(&mut self) {
        if self.resolving_events {
            return;
        }

        self.resolving_events = true;
        let mut resolved_events = 0;
        while let Some(event) = self.event_queue.pop_front() {
            resolved_events += 1;
            if resolved_events > 128 {
                self.event_queue.clear();
                self.log
                    .push("规则解析停止：触发次数过多，可能存在循环规则。".to_owned());
                break;
            }
            self.resolve_event_now(event);
        }
        self.resolving_events = false;
    }

    fn resolve_event_now(&mut self, event: RuleEvent) {
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
                let target_ids = resolve_targets(target, owner_id, event);
                let source_id = owner_id.to_owned();
                self.log.push(format!(
                    "规则触发：{} -> {}",
                    rule_event_name(event),
                    action.explain()
                ));
                for target_id in target_ids {
                    self.heal(
                        &source_id,
                        &target_id,
                        amount.eval(event),
                    );
                }
            },
            Action::Damage {
                target,
                amount,
                damage_type,
            } => {
                let target_ids = resolve_targets(target, owner_id, event);
                self.log.push(format!(
                    "规则触发：{} -> {}",
                    rule_event_name(event),
                    action.explain()
                ));
                for target_id in target_ids {
                    self.attack(
                        owner_id,
                        &target_id,
                        amount.eval(event),
                        damage_type,
                    );
                }
            },
        }
    }

    fn sync_character_hp_to_ecs(&mut self, character_id: &str, hp: f32, max_hp: f32) {
        let Some(entity) = self.entity_by_id.get(character_id).copied() else {
            return;
        };

        if let Some(mut combatant) = self.ecs_world.get_mut::<Combatant>(entity) {
            combatant.hp = hp;
            combatant.max_hp = max_hp;
        }
        if let Some(mut base) = self.ecs_world.get_mut::<BaseCombatant>(entity) {
            base.0.hp = hp;
            base.0.max_hp = max_hp;
        }
    }
}

fn apply_buff_effect(
    combatant: &mut Combatant,
    status: &mut StatusBlock,
    modifiers: &mut CombatModifiers,
    effect: &BuffEffect,
) {
    match effect.field {
        BuffField::Hp => apply_f32(&mut combatant.hp, effect.value),
        BuffField::Mp => apply_f32(&mut combatant.mp, effect.value),
        BuffField::MaxHp => apply_f32(&mut combatant.max_hp, effect.value),
        BuffField::MaxMp => apply_f32(&mut combatant.max_mp, effect.value),
        BuffField::HpRegen => apply_f32(&mut combatant.hp_regen, effect.value),
        BuffField::MpRegen => apply_f32(&mut combatant.mp_regen, effect.value),
        BuffField::DamageDealtModifier => apply_f32(
            &mut modifiers.damage_dealt,
            effect.value,
        ),
        BuffField::DamageTakenModifier => apply_f32(
            &mut modifiers.damage_taken,
            effect.value,
        ),
        BuffField::HealingDealtModifier => apply_f32(
            &mut modifiers.healing_dealt,
            effect.value,
        ),
        BuffField::HealingTakenModifier => apply_f32(
            &mut modifiers.healing_taken,
            effect.value,
        ),
        BuffField::Status(key) => apply_i32(
            status_value_mut(status, key),
            effect.value,
        ),
    }
}

fn apply_f32(target: &mut f32, value: BuffValue) {
    let base = *target;
    match value {
        BuffValue::Add(delta) => *target += delta,
        BuffValue::AddPercent(percent) => *target *= 1.0 + percent / 100.0,
        BuffValue::Set(new_value) => *target = new_value,
        BuffValue::SetPercentOfBase(percent) => *target = base * percent / 100.0,
    }
}

fn apply_i32(target: &mut i32, value: BuffValue) {
    let mut float_value = *target as f32;
    apply_f32(&mut float_value, value);
    *target = float_value.round() as i32;
}

fn status_value_mut(status: &mut StatusBlock, key: StatusKey) -> &mut i32 {
    match key {
        StatusKey::Str => &mut status.str_,
        StatusKey::Agi => &mut status.agi,
        StatusKey::Dex => &mut status.dex,
        StatusKey::Vit => &mut status.vit,
        StatusKey::Int => &mut status.int_,
        StatusKey::Wis => &mut status.wis,
        StatusKey::K => &mut status.k,
        StatusKey::Cha => &mut status.cha,
    }
}

pub fn parse_rule(input: &str) -> Result<RuleAst, String> {
    let normalized = normalize_rule_text(input);
    if normalized.is_empty() {
        return Err("规则为空".to_owned());
    }
    if is_active_skill_rule(&normalized) {
        let actions = parse_actions(&normalized)?;
        if actions.is_empty() {
            return Err("没有找到可执行动作，例如：造成4点物理伤害".to_owned());
        }

        return Ok(RuleAst {
            raw: input.to_owned(),
            trigger: Trigger {
                subject: ActorRef::SelfActor,
                event: EventKind::SkillCast,
            },
            actions,
        });
    }
    if !normalized.starts_with("每当") {
        return Err("规则必须以“每当”开头".to_owned());
    }
    let Some(trigger_end) = trigger_end_index(&normalized) else {
        return Err("没有识别到触发条件；目前支持“受到伤害 / 造成伤害 / 释放技能”".to_owned());
    };
    if !normalized[trigger_end..].starts_with('时') {
        return Err(
            "触发条件后必须使用“时”连接动作，例如：每当自己受到伤害时，回复2点生命值".to_owned(),
        );
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

fn is_active_skill_rule(text: &str) -> bool {
    ["主动使用", "主动技能", "使用技能", "施放技能", "释放技能"]
        .iter()
        .any(|starter| text.starts_with(starter))
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
                    target: parse_target_selector(clause, ActorRef::SelfActor),
                    amount,
                });
            }
        }

        if let Some(amount) = parse_value_before(clause, "点伤害")
            .or_else(|| parse_value_after_action(clause, &["造成"]))
        {
            if clause.contains("伤害") {
                actions.push(Action::Damage {
                    target: parse_target_selector(clause, ActorRef::Target),
                    amount,
                    damage_type: parse_damage_type(clause),
                });
            }
        }
    }

    Ok(actions)
}

fn action_clause(text: &str) -> &str {
    if let Some(trigger_end) = trigger_end_index(text) {
        let tail = text[trigger_end..].trim_start_matches(['，', ',', '；', ';', '时']);
        if contains_action_word(tail) {
            return tail;
        }
    }

    for marker in ["时", "，", ",", "；", ";"] {
        if let Some((_, tail)) = text.split_once(marker) {
            if contains_action_word(tail) {
                return tail;
            }
        }
    }
    text
}

fn trigger_end_index(text: &str) -> Option<usize> {
    [
        "受到伤害",
        "承受伤害",
        "受伤害",
        "造成伤害",
        "释放技能",
        "技能释放",
    ]
    .iter()
    .filter_map(|event| text.find(event).map(|index| index + event.len()))
    .min()
}

fn contains_action_word(text: &str) -> bool {
    ["回复", "恢复", "治疗", "造成", "给予"]
        .iter()
        .any(|word| text.contains(word))
}

fn parse_target_selector(clause: &str, default_target: ActorRef) -> TargetSelector {
    TargetSelector {
        actor: parse_action_target(clause, default_target),
        area: parse_area_selector(clause),
    }
}

fn parse_area_selector(clause: &str) -> Option<AreaSelector> {
    if !(clause.contains("周围")
        || clause.contains("范围")
        || clause.contains("半径")
        || clause.contains("米内")
        || clause.contains("米范围"))
    {
        return None;
    }

    Some(AreaSelector {
        radius_meters: parse_radius_meters(clause),
    })
}

fn parse_radius_meters(clause: &str) -> Option<f32> {
    let meter_index = clause.find('米')?;
    parse_trailing_number(&clause[..meter_index])
}

fn parse_action_target(clause: &str, default_target: ActorRef) -> ActorRef {
    if clause.contains("目标") {
        ActorRef::Target
    } else if clause.contains("来源") || clause.contains("攻击者") {
        ActorRef::Source
    } else if clause.contains("自己") {
        ActorRef::SelfActor
    } else {
        default_target
    }
}

fn parse_damage_type(clause: &str) -> DamageType {
    if clause.contains("诅咒") {
        DamageType::Cursed
    } else if clause.contains("疾病") {
        DamageType::Diseased
    } else if clause.contains("流血") {
        DamageType::Bleed
    } else if clause.contains("远程") {
        DamageType::Range
    } else if clause.contains("中毒") {
        DamageType::Poisoning
    } else if clause.contains("物理") {
        DamageType::Physical
    } else if clause.contains("魔法") {
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
        .replace("\r\n", "，")
        .replace('\n', "，")
        .replace('\r', "，")
}

fn rule_matches(rule: &Rule, event: &RuleEvent) -> bool {
    let expected_actor = match rule.ast.trigger.subject {
        ActorRef::SelfActor => event_primary_actor_id(event),
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

fn event_primary_actor_id(event: &RuleEvent) -> Option<&str> {
    match event {
        RuleEvent::DamageTaken { target_id, .. } => Some(target_id),
        RuleEvent::DamageDealt { source_id, .. } | RuleEvent::SkillCast { source_id, .. } => {
            Some(source_id)
        },
    }
}

fn resolve_actor(actor: ActorRef, owner_id: &str, event: &RuleEvent) -> Option<String> {
    match actor {
        ActorRef::SelfActor => Some(owner_id.to_owned()),
        ActorRef::Source => event_source_id(event).map(ToOwned::to_owned),
        ActorRef::Target => event_target_id(event).map(ToOwned::to_owned),
    }
}

fn resolve_targets(selector: TargetSelector, owner_id: &str, event: &RuleEvent) -> Vec<String> {
    if selector.area.is_some() {
        if let RuleEvent::SkillCast { target_ids, .. } = event {
            return target_ids
                .iter()
                .filter(|target_id| target_id.as_str() != owner_id)
                .cloned()
                .collect();
        }
    }

    resolve_actor(selector.actor, owner_id, event)
        .into_iter()
        .collect()
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

    if !state.panel_open {
        return;
    }

    let mut panel_open = state.panel_open;
    let mut close_requested = false;

    egui::Window::new("规则引擎")
        .default_pos(egui::pos2(12.0, 430.0))
        .default_width(360.0)
        .resizable(true)
        .open(&mut panel_open)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label("中文规则");
                    let rule_response = ui.add(
                        egui::TextEdit::multiline(&mut state.rule_input)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                    if rule_response.changed() {
                        parse_owner_rule(&mut state, "alice");
                    }
                    ui.horizontal(|ui| {
                        if ui.button("解析").clicked() {
                            parse_owner_rule(&mut state, "alice");
                        }
                        if ui.button("敌人攻击").clicked() {
                            let attack_amount = state.attack_amount;
                            state.engine.attack(
                                "enemy",
                                "alice",
                                attack_amount,
                                DamageType::Physical,
                            );
                        }
                        if ui.button("重置").clicked() {
                            *state = RuleEngineState::default();
                        }
                        if ui.button("清空日志").clicked() {
                            state.engine.log.clear();
                        }
                        if ui.button("关闭").clicked() {
                            close_requested = true;
                        }
                    });
                    ui.add(egui::Slider::new(&mut state.attack_amount, 0.0..=20.0).text("伤害"));
                    ui.separator();
                    ui.label(&state.parse_preview);
                    ui.collapsing("可用词", |ui| {
                        rule_words(ui);
                    });
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        character_hp(ui, &state.engine, "alice");
                        ui.separator();
                        character_hp(ui, &state.engine, "enemy");
                    });
                    ui.separator();
                    ui.label("日志");
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let start = state.engine.log.len().saturating_sub(40);
                            for line in state.engine.log.iter().skip(start) {
                                ui.label(line);
                            }
                        });
                });
        });

    state.panel_open = panel_open && !close_requested;
}

fn parse_owner_rule(state: &mut RuleEngineState, owner_id: &str) {
    match parse_rule(&state.rule_input) {
        Ok(ast) => {
            state.parse_preview = ast.explain();
            state.engine.replace_rule_for_owner(owner_id, ast);
        },
        Err(err) => {
            state.parse_preview = err;
            state.engine.clear_rules_for_owner(owner_id);
        },
    }
}

fn character_hp(ui: &mut egui::Ui, engine: &RuleEngine, id: &str) {
    if let Some(character) = engine.characters.get(id) {
        ui.label(format!(
            "{} HP：{}/{}",
            character.name,
            format_number(character.hp),
            format_number(character.max_hp)
        ));
    }
}

fn rule_words(ui: &mut egui::Ui) {
    egui::Grid::new("rule_engine_words")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label("触发起始词");
            ui.label("每当, 主动使用, 主动技能, 使用技能, 施放技能");
            ui.end_row();

            ui.label("触发主体");
            ui.label("自己, 目标, 来源, 攻击者");
            ui.end_row();

            ui.label("触发事件");
            ui.label("受到伤害, 造成伤害, 释放技能");
            ui.end_row();

            ui.label("动作标记");
            ui.label("时");
            ui.end_row();

            ui.label("治疗动作");
            ui.label("回复, 点生命值");
            ui.end_row();

            ui.label("伤害动作");
            ui.label("造成, 点伤害");
            ui.end_row();

            ui.label("动作目标");
            ui.label("自己, 目标, 来源, 攻击者, 周围N米");
            ui.end_row();

            ui.label("数值");
            ui.label("数字, 本次伤害");
            ui.end_row();

            ui.label("伤害类型");
            ui.label("物理, 魔法, 远程, 诅咒, 疾病, 流血, 中毒");
            ui.end_row();

            ui.label("分隔符");
            ui.label("， , ； ;");
            ui.end_row();
        });

    ui.add_space(6.0);
    ui.label("示例：");
    ui.monospace("每当自己受到伤害时，回复2点生命值");
    ui.monospace("每当自己受到伤害时，对攻击者造成本次伤害点物理伤害");
    ui.monospace("每当自己造成伤害时，回复自己1点生命值");
    ui.monospace("主动使用对周围3米内的目标造成4点物理伤害");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_damage_taken_heal_rule() {
        let ast = parse_rule("每当自己受到伤害时，回复2点生命值").unwrap();

        assert_eq!(
            ast.trigger.event,
            EventKind::DamageTaken
        );
        assert_eq!(ast.trigger.subject, ActorRef::SelfActor);
        assert_eq!(ast.actions, vec![Action::Heal {
            target: TargetSelector::single(ActorRef::SelfActor),
            amount: ValueExpr::Number(2.0),
        }]);
        assert_eq!(
            ast.explain(),
            "触发：每当自己受到伤害。\n动作：回复2点生命值给自己。"
        );
    }

    #[test]
    fn rejects_hidden_starters_and_action_markers() {
        assert!(parse_rule("当自己受到伤害时，回复2点生命值").is_err());
        assert!(parse_rule("无论何时自己受到伤害时，回复2点生命值").is_err());
        assert!(parse_rule("每当自己受到伤害则回复2点生命值").is_err());
        assert!(parse_rule("每当自己受到伤害，回复2点生命值").is_err());
    }

    #[test]
    fn damage_taken_rule_runs_after_attack() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));
        engine.add_rule(
            "alice",
            parse_rule("每当自己受到伤害时，回复2点生命值").unwrap(),
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
    fn damage_taken_rule_applies_damage_before_healing() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));
        engine.add_rule(
            "alice",
            parse_rule("每当自己受到伤害时，回复3点生命值").unwrap(),
        );

        engine.attack(
            "enemy",
            "alice",
            3.0,
            DamageType::Physical,
        );

        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            10.0
        );
        assert!(engine.log[0].contains("自己受到3点伤害"));
        assert!(engine.log[1].contains("规则触发：受到3点物理伤害"));
        assert!(engine.log[2].contains("自己回复3点生命值"));
    }

    #[test]
    fn parses_multiple_actions_split_by_commas_or_lines() {
        let ast = parse_rule("每当自己受到伤害时，回复2点生命值\n回复2点生命值").unwrap();

        assert_eq!(ast.actions, vec![
            Action::Heal {
                target: TargetSelector::single(ActorRef::SelfActor),
                amount: ValueExpr::Number(2.0),
            },
            Action::Heal {
                target: TargetSelector::single(ActorRef::SelfActor),
                amount: ValueExpr::Number(2.0),
            },
        ]);
        assert_eq!(
            ast.explain(),
            "触发：每当自己受到伤害。\n动作：回复2点生命值给自己。\n动作：回复2点生命值给自己。"
        );
    }

    #[test]
    fn parses_active_area_damage_skill() {
        let ast = parse_rule("主动使用对周围3米内的目标造成4点物理伤害").unwrap();

        assert_eq!(ast.trigger.event, EventKind::SkillCast);
        assert_eq!(ast.trigger.subject, ActorRef::SelfActor);
        assert_eq!(ast.actions, vec![Action::Damage {
            target: TargetSelector {
                actor: ActorRef::Target,
                area: Some(AreaSelector {
                    radius_meters: Some(3.0),
                }),
            },
            amount: ValueExpr::Number(4.0),
            damage_type: DamageType::Physical,
        }]);
        assert_eq!(
            ast.explain(),
            "触发：每当自己释放技能。\n动作：对周围3米内的目标造成4点物理伤害。"
        );
    }

    #[test]
    fn active_area_damage_skill_hits_skill_cast_targets() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy_a", "敌人A", 10.0));
        engine.add_character(Character::new("enemy_b", "敌人B", 10.0));
        engine.add_rule(
            "alice",
            parse_rule("主动使用对周围3米内的目标造成4点物理伤害").unwrap(),
        );

        engine.cast_skill("alice", vec![
            "enemy_a".to_owned(),
            "enemy_b".to_owned(),
        ]);

        assert_eq!(
            engine.characters.get("enemy_a").unwrap().hp,
            6.0
        );
        assert_eq!(
            engine.characters.get("enemy_b").unwrap().hp,
            6.0
        );
        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            10.0
        );
    }

    #[test]
    fn multiple_actions_run_in_order() {
        let mut engine = RuleEngine::default();
        let mut alice = Character::new("alice", "自己", 10.0);
        alice.hp = 8.0;
        engine.add_character(alice);
        engine.add_character(Character::new("enemy", "敌人", 10.0));
        engine.add_rule(
            "alice",
            parse_rule("每当自己受到伤害时，回复2点生命值，回复2点生命值").unwrap(),
        );

        engine.attack(
            "enemy",
            "alice",
            3.0,
            DamageType::Physical,
        );

        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            9.0
        );
        assert_eq!(
            engine
                .log
                .iter()
                .filter(|line| line.contains("规则触发"))
                .count(),
            2
        );
    }

    #[test]
    fn damage_taken_rule_does_not_trigger_from_other_target_damage() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));
        engine.add_rule(
            "alice",
            parse_rule("每当自己受到伤害时，回复2点生命值").unwrap(),
        );

        engine.attack(
            "alice",
            "enemy",
            3.0,
            DamageType::Physical,
        );

        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            10.0
        );
        assert_eq!(
            engine.characters.get("enemy").unwrap().hp,
            7.0
        );
        assert_eq!(
            engine
                .log
                .iter()
                .filter(|line| line.contains("规则触发"))
                .count(),
            0
        );
    }

    #[test]
    fn repeated_heals_then_retaliation_damage_run_once() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));
        engine.add_rule(
            "alice",
            parse_rule("每当自己受到伤害时，回复2点生命值，回复2点生命值，回复2点生命值，回复2点生命值，回复2点生命值，对攻击者造成本次伤害点物理伤害").unwrap(),
        );

        engine.attack(
            "enemy",
            "alice",
            3.0,
            DamageType::Physical,
        );

        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            10.0
        );
        assert_eq!(
            engine.characters.get("enemy").unwrap().hp,
            7.0
        );
        assert_eq!(
            engine
                .log
                .iter()
                .filter(|line| line.contains("规则触发"))
                .count(),
            6
        );
        assert!(!engine.log.iter().any(|line| line.contains("触发次数过多")));
    }

    #[test]
    fn invalid_rule_text_can_deactivate_owner_rules() {
        let mut engine = RuleEngine::default();
        engine.add_rule(
            "alice",
            parse_rule("每当自己受到伤害时，回复2点生命值").unwrap(),
        );

        assert_eq!(engine.rules.len(), 1);
        assert!(parse_rule("每当自己未知事件时，回复2点生命值").is_err());
        engine.clear_rules_for_owner("alice");

        assert!(engine.rules.is_empty());
    }

    #[test]
    fn queued_resolution_stops_recursive_damage_rules() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));
        engine.add_rule(
            "alice",
            parse_rule("每当自己造成伤害时，对目标造成1点物理伤害").unwrap(),
        );

        engine.attack(
            "alice",
            "enemy",
            1.0,
            DamageType::Physical,
        );

        assert!(engine.log.iter().any(|line| line.contains("触发次数过多")));
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

    #[test]
    fn ecs_buff_modifier_affects_damage_then_expires() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));

        assert!(engine.give_buff("alice", BuffSpec {
            name: "Guard".to_owned(),
            kind: BuffKind::Magic,
            priority: 0,
            turns_remaining: 1,
            source_id: "alice".to_owned(),
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::DamageTakenModifier,
                value: BuffValue::Set(0.5),
            }],
        }));

        engine.attack(
            "enemy",
            "alice",
            4.0,
            DamageType::Physical,
        );
        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            8.0
        );
        assert_eq!(engine.active_buff_names("alice"), vec![
            "Guard".to_owned()
        ]);

        engine.advance_turn();
        engine.attack(
            "enemy",
            "alice",
            4.0,
            DamageType::Physical,
        );
        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            4.0
        );
        assert!(engine.active_buff_names("alice").is_empty());
    }

    #[test]
    fn ecs_buff_recomputes_from_base_in_priority_order() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));

        engine.give_buff("alice", BuffSpec {
            name: "Brittle".to_owned(),
            kind: BuffKind::Disease,
            priority: 10,
            turns_remaining: 2,
            source_id: "enemy".to_owned(),
            beneficial: false,
            effects: vec![BuffEffect {
                field: BuffField::MaxHp,
                value: BuffValue::Add(-3.0),
            }],
        });
        engine.give_buff("alice", BuffSpec {
            name: "Bless".to_owned(),
            kind: BuffKind::Magic,
            priority: 0,
            turns_remaining: 2,
            source_id: "alice".to_owned(),
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::MaxHp,
                value: BuffValue::Add(5.0),
            }],
        });

        let alice = engine.characters.get("alice").unwrap();
        assert_eq!(alice.max_hp, 12.0);
        assert_eq!(alice.hp, 10.0);
        assert_eq!(engine.active_buff_names("alice"), vec![
            "Bless".to_owned(),
            "Brittle".to_owned(),
        ]);
    }

    #[test]
    fn zero_turn_buff_is_permanent() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));

        assert!(engine.give_buff("alice", BuffSpec {
            name: "Permanent Guard".to_owned(),
            kind: BuffKind::Magic,
            priority: 0,
            turns_remaining: 0,
            source_id: "gm".to_owned(),
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::DamageTakenModifier,
                value: BuffValue::Set(0.5),
            }],
        }));

        engine.advance_turn();
        assert_eq!(engine.active_buff_names("alice"), vec![
            "Permanent Guard".to_owned()
        ]);

        engine.attack(
            "enemy",
            "alice",
            4.0,
            DamageType::Physical,
        );
        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            8.0
        );
    }

    #[test]
    fn replacing_buffs_for_target_does_not_duplicate_persisted_buffs() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        let buffs = vec![BuffSpec {
            name: "Guard".to_owned(),
            kind: BuffKind::Magic,
            priority: 0,
            turns_remaining: 2,
            source_id: "gm".to_owned(),
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::DamageTakenModifier,
                value: BuffValue::Set(0.5),
            }],
        }];

        engine.replace_buffs_for_target("alice", buffs.clone());
        engine.replace_buffs_for_target("alice", buffs);

        assert_eq!(engine.active_buff_names("alice"), vec![
            "Guard".to_owned()
        ]);
        engine.attack(
            "enemy",
            "alice",
            4.0,
            DamageType::Physical,
        );
        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            8.0
        );
    }

    #[test]
    fn replacing_max_hp_percent_buff_recomputes_from_base() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        let buffs = vec![BuffSpec {
            name: "GM查看你".to_owned(),
            kind: BuffKind::None,
            priority: 0,
            turns_remaining: 2,
            source_id: "gm".to_owned(),
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::MaxHp,
                value: BuffValue::AddPercent(100.0),
            }],
        }];

        engine.replace_buffs_for_target("alice", buffs.clone());
        assert_eq!(
            engine.characters.get("alice").unwrap().max_hp,
            20.0
        );

        engine.replace_buffs_for_target("alice", buffs);
        assert_eq!(
            engine.characters.get("alice").unwrap().max_hp,
            20.0
        );
    }
}
