use std::{
    collections::{
        hash_map::Entry,
        HashMap,
    },
    fs,
    path::Path,
};

use serde::{
    Deserialize,
    Serialize,
};

use crate::napcat::{
    CharacterSkillMetadata,
    PlayerCharacter,
};

pub const ALIEN_STAGE_TWO_POINTS: u32 = 6;
pub const ALIEN_STAGE_THREE_POINTS: u32 = 14;
pub const MUTANT_STAGE_TWO_BIOMASS: u32 = 3;
pub const MUTANT_STAGE_THREE_BIOMASS: u32 = 9;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HiddenRoleKind {
    #[default]
    Alien,
    Mutant,
    Android,
}

impl HiddenRoleKind {
    pub const ALL: [Self; 3] = [Self::Alien, Self::Mutant, Self::Android];

    pub fn label(self) -> &'static str {
        match self {
            Self::Alien => "异形",
            Self::Mutant => "变种人",
            Self::Android => "仿生人",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutantEvolutionPath {
    #[default]
    Predator,
    Bulwark,
    Corruptor,
}

impl MutantEvolutionPath {
    pub const ALL: [Self; 3] = [Self::Predator, Self::Bulwark, Self::Corruptor];

    pub fn label(self) -> &'static str {
        match self {
            Self::Predator => "猎杀体",
            Self::Bulwark => "重甲体",
            Self::Corruptor => "腐化体",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HiddenRoleState {
    pub role: HiddenRoleKind,
    #[serde(default = "default_role_stage")]
    pub stage: u8,
    #[serde(default)]
    pub evolution_points: u32,
    #[serde(default)]
    pub transformed: bool,
    #[serde(default)]
    pub protective_suit_worn: bool,
    #[serde(default)]
    pub protective_suit_destroyed: bool,
    #[serde(default)]
    pub cocoon_turns_remaining: u8,
    #[serde(default)]
    pub cocoon_hp: f32,
    #[serde(default)]
    pub mutant_path: Option<MutantEvolutionPath>,
    #[serde(default)]
    pub corpses_consumed: u32,
    #[serde(default)]
    pub android_gear_obtained: bool,
    #[serde(default)]
    pub last_world_turn_awarded: u32,
    #[serde(default)]
    pub last_event: String,
}

fn default_role_stage() -> u8 { 1 }

impl HiddenRoleState {
    pub fn new(role: HiddenRoleKind) -> Self {
        Self {
            role,
            stage: 1,
            evolution_points: 0,
            transformed: false,
            protective_suit_worn: false,
            protective_suit_destroyed: false,
            cocoon_turns_remaining: 0,
            cocoon_hp: 0.0,
            mutant_path: None,
            corpses_consumed: 0,
            android_gear_obtained: false,
            last_world_turn_awarded: 0,
            last_event: format!("已设置隐藏身份：{}", role.label()),
        }
    }

    pub fn normalize(&mut self) {
        self.stage = self.stage.clamp(1, 3);
        if self.role != HiddenRoleKind::Alien {
            self.cocoon_turns_remaining = 0;
            self.cocoon_hp = 0.0;
        }
        if self.role != HiddenRoleKind::Mutant {
            self.mutant_path = None;
            self.corpses_consumed = 0;
        }
        if self.role != HiddenRoleKind::Android {
            self.android_gear_obtained = false;
        }
    }

    pub fn is_cocooning(&self) -> bool {
        self.role == HiddenRoleKind::Alien
            && self.cocoon_turns_remaining > 0
            && self.cocoon_hp > 0.0
    }

    pub fn next_threshold(&self) -> Option<u32> {
        match (self.role, self.stage) {
            (HiddenRoleKind::Alien, 1) => Some(ALIEN_STAGE_TWO_POINTS),
            (HiddenRoleKind::Alien, 2) => Some(ALIEN_STAGE_THREE_POINTS),
            (HiddenRoleKind::Mutant, 1) => Some(MUTANT_STAGE_TWO_BIOMASS),
            (HiddenRoleKind::Mutant, 2) => Some(MUTANT_STAGE_THREE_BIOMASS),
            _ => None,
        }
    }

    pub fn can_begin_cocoon(&self) -> bool {
        self.role == HiddenRoleKind::Alien
            && !self.is_cocooning()
            && self
                .next_threshold()
                .is_some_and(|threshold| self.evolution_points >= threshold)
    }

    pub fn begin_cocoon(&mut self) -> bool {
        if !self.can_begin_cocoon() {
            return false;
        }
        let (turns, hp) = if self.stage == 1 { (2, 24.0) } else { (3, 40.0) };
        self.cocoon_turns_remaining = turns;
        self.cocoon_hp = hp;
        let destroyed_suit_now = self.protective_suit_worn;
        if destroyed_suit_now {
            self.protective_suit_worn = false;
            self.protective_suit_destroyed = true;
        }
        self.transformed = true;
        self.last_event = format!(
            "开始结茧：{hp:.0} HP，需安稳经过{turns}个世界回合{}",
            if destroyed_suit_now { "；防护服已毁" } else { "" }
        );
        true
    }

    pub fn damage_cocoon(&mut self, damage: f32) -> bool {
        if !self.is_cocooning() || damage <= 0.0 {
            return false;
        }
        self.cocoon_hp = (self.cocoon_hp - damage).max(0.0);
        if self.cocoon_hp <= f32::EPSILON {
            self.interrupt_cocoon("茧被破坏");
        } else {
            self.last_event = format!(
                "茧受到{damage:.0}点伤害，剩余{:.0} HP",
                self.cocoon_hp
            );
        }
        true
    }

    pub fn interrupt_cocoon(&mut self, reason: &str) -> bool {
        if self.role != HiddenRoleKind::Alien || self.cocoon_turns_remaining == 0 {
            return false;
        }
        self.cocoon_turns_remaining = 0;
        self.cocoon_hp = 0.0;
        self.evolution_points = self.evolution_points.saturating_sub(2);
        self.last_event = format!("{reason}，进化中断并损失2进化点");
        true
    }

    pub fn remove_protective_suit(&mut self) -> bool {
        if !self.protective_suit_worn {
            return false;
        }
        self.protective_suit_worn = false;
        self.last_event = "已脱下防护服（消耗1回合）".to_owned();
        true
    }

    pub fn transform(&mut self) -> bool {
        let allowed = match self.role {
            HiddenRoleKind::Alien => true,
            HiddenRoleKind::Mutant => self.stage >= 2,
            HiddenRoleKind::Android => false,
        };
        if !allowed || self.transformed || self.is_cocooning() {
            return false;
        }
        if self.protective_suit_worn {
            self.protective_suit_worn = false;
            self.protective_suit_destroyed = true;
            self.last_event = "强行变身，防护服被摧毁".to_owned();
        } else {
            self.last_event = "完成变身".to_owned();
        }
        self.transformed = true;
        true
    }

    pub fn revert_form(&mut self) -> bool {
        if !self.transformed || self.is_cocooning() {
            return false;
        }
        self.transformed = false;
        self.last_event = "恢复伪装形态，战斗加成暂时失效".to_owned();
        true
    }

    pub fn consume_corpse(&mut self, biomass: u32) -> bool {
        if self.role != HiddenRoleKind::Mutant || biomass == 0 || self.protective_suit_worn {
            return false;
        }
        self.corpses_consumed = self.corpses_consumed.saturating_add(1);
        self.evolution_points = self.evolution_points.saturating_add(biomass);
        self.last_event = format!("吞噬尸体获得{biomass}生物质");
        self.apply_automatic_mutant_evolution();
        true
    }

    pub fn force_consume_through_suit(&mut self, biomass: u32) -> bool {
        if self.role != HiddenRoleKind::Mutant || biomass == 0 || !self.protective_suit_worn {
            return false;
        }
        self.protective_suit_worn = false;
        self.protective_suit_destroyed = true;
        self.corpses_consumed = self.corpses_consumed.saturating_add(1);
        self.evolution_points = self.evolution_points.saturating_add(biomass);
        self.last_event = format!("强行进食摧毁防护服，获得{biomass}生物质");
        self.apply_automatic_mutant_evolution();
        true
    }

    pub fn apply_automatic_mutant_evolution(&mut self) -> bool {
        if self.role != HiddenRoleKind::Mutant {
            return false;
        }
        if self.stage == 1 && self.evolution_points >= MUTANT_STAGE_TWO_BIOMASS {
            self.stage = 2;
            self.transformed = false;
            self.last_event = "自动进化至2阶段，并解锁变身".to_owned();
            return true;
        }
        if self.stage == 2
            && self.evolution_points >= MUTANT_STAGE_THREE_BIOMASS
            && self.mutant_path.is_some()
        {
            self.stage = 3;
            self.last_event = format!(
                "自动进化至3阶段·{}",
                self.mutant_path.unwrap_or_default().label()
            );
            return true;
        }
        false
    }

    pub fn advance_world_turn(&mut self, world_turn: u32) -> bool {
        self.normalize();
        if world_turn <= self.last_world_turn_awarded {
            return false;
        }
        let elapsed = world_turn - self.last_world_turn_awarded;
        self.last_world_turn_awarded = world_turn;
        if self.is_cocooning() {
            let elapsed_turns = elapsed.min(u8::MAX as u32) as u8;
            self.cocoon_turns_remaining = self.cocoon_turns_remaining.saturating_sub(elapsed_turns);
            if self.cocoon_turns_remaining == 0 {
                self.cocoon_hp = 0.0;
                self.stage = self.stage.saturating_add(1).min(3);
                self.transformed = true;
                self.last_event = format!("结茧完成，进化至{}阶段", self.stage);
            } else {
                self.last_event = format!(
                    "结茧推进，剩余{}个世界回合",
                    self.cocoon_turns_remaining
                );
            }
            return true;
        }
        if self.role == HiddenRoleKind::Alien && self.stage < 3 {
            self.evolution_points = self.evolution_points.saturating_add(elapsed);
            self.last_event = format!(
                "时间推进获得{elapsed}进化点（当前{}）",
                self.evolution_points
            );
            return true;
        }
        false
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct HiddenRoleBattleState {
    pub cocooning: bool,
    pub free_action_skill: Option<String>,
    pub free_action_available: bool,
    pub second_wind_heal: f32,
    pub second_wind_used: bool,
    pub immune_diseased: bool,
    pub immune_poisoning: bool,
    pub immune_bleed: bool,
}

#[derive(Clone, Copy)]
struct HiddenRoleSkill {
    name: &'static str,
    note: &'static str,
    skill_type: &'static str,
    target_class: &'static str,
    range: i32,
    cooldown: u32,
}

#[derive(Clone)]
struct HiddenRoleCombatProfile {
    stat_bonus: i32,
    max_hp_bonus: f32,
    max_mp_bonus: f32,
    hp_regen_bonus: f32,
    speed_bonus: f32,
    damage_dealt_multiplier: f32,
    damage_taken_multiplier: f32,
    opening_shield: f32,
    free_action_skill: Option<&'static str>,
    second_wind_heal: f32,
    immune_diseased: bool,
    immune_poisoning: bool,
    immune_bleed: bool,
    skills: Vec<HiddenRoleSkill>,
}

impl Default for HiddenRoleCombatProfile {
    fn default() -> Self {
        Self {
            stat_bonus: 0,
            max_hp_bonus: 0.0,
            max_mp_bonus: 0.0,
            hp_regen_bonus: 0.0,
            speed_bonus: 0.0,
            damage_dealt_multiplier: 1.0,
            damage_taken_multiplier: 1.0,
            opening_shield: 0.0,
            free_action_skill: None,
            second_wind_heal: 0.0,
            immune_diseased: false,
            immune_poisoning: false,
            immune_bleed: false,
            skills: Vec::new(),
        }
    }
}

fn skill(
    name: &'static str,
    note: &'static str,
    skill_type: &'static str,
    target_class: &'static str,
    range: i32,
    cooldown: u32,
) -> HiddenRoleSkill {
    HiddenRoleSkill {
        name,
        note,
        skill_type,
        target_class,
        range,
        cooldown,
    }
}

fn combat_profile(state: &HiddenRoleState) -> HiddenRoleCombatProfile {
    if state.is_cocooning() {
        return HiddenRoleCombatProfile {
            damage_taken_multiplier: 1.0,
            ..Default::default()
        };
    }
    match state.role {
        HiddenRoleKind::Alien if state.transformed => match state.stage {
            1 => HiddenRoleCombatProfile {
                stat_bonus: 5,
                max_hp_bonus: 20.0,
                free_action_skill: Some("异形爪击"),
                skills: vec![skill(
                    "异形爪击",
                    "主动使用对近身目标造成6点物理伤害",
                    "动作",
                    "单目标",
                    2,
                    0,
                )],
                ..Default::default()
            },
            2 => HiddenRoleCombatProfile {
                stat_bonus: 8,
                max_hp_bonus: 60.0,
                hp_regen_bonus: 5.0,
                damage_dealt_multiplier: 1.15,
                damage_taken_multiplier: 0.75,
                free_action_skill: Some("异形爪击"),
                skills: vec![
                    skill(
                        "异形爪击",
                        "主动使用对近身目标造成9点物理伤害",
                        "动作",
                        "单目标",
                        2,
                        0,
                    ),
                    skill(
                        "异形捕食飞扑",
                        "主动使用对6米内目标造成10点物理伤害",
                        "动作",
                        "单目标",
                        6,
                        2,
                    ),
                    skill(
                        "酸液喷吐",
                        "主动使用对8米内目标造成7点中毒伤害",
                        "远程",
                        "单目标",
                        8,
                        2,
                    ),
                ],
                ..Default::default()
            },
            _ => HiddenRoleCombatProfile {
                stat_bonus: 12,
                max_hp_bonus: 100.0,
                hp_regen_bonus: 10.0,
                damage_dealt_multiplier: 1.25,
                damage_taken_multiplier: 0.60,
                free_action_skill: Some("异形爪击"),
                second_wind_heal: 20.0,
                skills: vec![
                    skill(
                        "异形爪击",
                        "主动使用对近身目标造成12点物理伤害",
                        "动作",
                        "单目标",
                        2,
                        0,
                    ),
                    skill(
                        "异形尾刺贯穿",
                        "主动使用对3米内目标造成16点物理伤害",
                        "动作",
                        "单目标",
                        3,
                        2,
                    ),
                    skill(
                        "异形震慑尖啸",
                        "主动使用对周围5米内目标造成6点无类型伤害",
                        "异能",
                        "范围",
                        5,
                        3,
                    ),
                ],
                ..Default::default()
            },
        },
        HiddenRoleKind::Mutant if state.transformed && state.stage == 2 => {
            HiddenRoleCombatProfile {
                stat_bonus: 5,
                max_hp_bonus: 45.0,
                max_mp_bonus: 20.0,
                hp_regen_bonus: 5.0,
                damage_dealt_multiplier: 1.20,
                damage_taken_multiplier: 0.75,
                second_wind_heal: 15.0,
                skills: vec![
                    skill(
                        "变种骨刺齐射",
                        "主动使用对10米内目标造成10点远程伤害",
                        "远程",
                        "单目标",
                        10,
                        1,
                    ),
                    skill(
                        "变种捕食冲锋",
                        "主动使用对6米内目标造成12点物理伤害",
                        "动作",
                        "单目标",
                        6,
                        2,
                    ),
                ],
                ..Default::default()
            }
        },
        HiddenRoleKind::Mutant if state.transformed && state.stage >= 3 => {
            match state.mutant_path.unwrap_or_default() {
                MutantEvolutionPath::Predator => HiddenRoleCombatProfile {
                    stat_bonus: 10,
                    max_hp_bonus: 55.0,
                    hp_regen_bonus: 8.0,
                    speed_bonus: 6.0,
                    damage_dealt_multiplier: 1.50,
                    damage_taken_multiplier: 0.65,
                    free_action_skill: Some("猎杀撕裂"),
                    second_wind_heal: 24.0,
                    skills: vec![
                        skill(
                            "猎杀撕裂",
                            "主动使用对近身目标造成10点物理伤害",
                            "动作",
                            "单目标",
                            2,
                            0,
                        ),
                        skill(
                            "猎杀处决",
                            "主动使用对3米内目标造成20点物理伤害",
                            "动作",
                            "单目标",
                            3,
                            2,
                        ),
                    ],
                    ..Default::default()
                },
                MutantEvolutionPath::Bulwark => HiddenRoleCombatProfile {
                    stat_bonus: 10,
                    max_hp_bonus: 100.0,
                    hp_regen_bonus: 15.0,
                    damage_dealt_multiplier: 1.25,
                    damage_taken_multiplier: 0.45,
                    second_wind_heal: 30.0,
                    skills: vec![
                        skill(
                            "重甲震地",
                            "主动使用对周围4米内目标造成12点物理伤害",
                            "动作",
                            "范围",
                            4,
                            2,
                        ),
                        skill(
                            "重甲碾压",
                            "主动使用对3米内目标造成18点物理伤害",
                            "动作",
                            "单目标",
                            3,
                            1,
                        ),
                    ],
                    ..Default::default()
                },
                MutantEvolutionPath::Corruptor => HiddenRoleCombatProfile {
                    stat_bonus: 10,
                    max_hp_bonus: 75.0,
                    max_mp_bonus: 35.0,
                    hp_regen_bonus: 10.0,
                    damage_dealt_multiplier: 1.35,
                    damage_taken_multiplier: 0.55,
                    second_wind_heal: 28.0,
                    skills: vec![
                        skill(
                            "腐化孢子云",
                            "主动使用对周围6米内目标造成10点中毒伤害",
                            "异能",
                            "范围",
                            6,
                            2,
                        ),
                        skill(
                            "生命虹吸",
                            "主动使用对8米内目标造成14点诅咒伤害；对自己回复14点生命值",
                            "异能",
                            "单目标",
                            8,
                            2,
                        ),
                    ],
                    ..Default::default()
                },
            }
        },
        HiddenRoleKind::Android if state.android_gear_obtained => HiddenRoleCombatProfile {
            stat_bonus: 3,
            max_hp_bonus: 40.0,
            hp_regen_bonus: 5.0,
            speed_bonus: 2.0,
            damage_dealt_multiplier: 1.25,
            damage_taken_multiplier: 0.70,
            opening_shield: 30.0,
            second_wind_heal: 18.0,
            immune_diseased: true,
            immune_poisoning: true,
            immune_bleed: true,
            skills: vec![
                skill(
                    "公司等离子步枪",
                    "主动使用对20米内目标造成12点远程伤害",
                    "远程",
                    "单目标",
                    20,
                    0,
                ),
                skill(
                    "电弧净化",
                    "主动使用对周围4米内目标造成8点魔法伤害",
                    "动作",
                    "范围",
                    4,
                    2,
                ),
                skill(
                    "仿生应急维修",
                    "主动使用对自己回复18点生命值",
                    "动作",
                    "无目标",
                    0,
                    3,
                ),
            ],
            ..Default::default()
        },
        _ => HiddenRoleCombatProfile::default(),
    }
}

fn append_hidden_skill(character: &mut PlayerCharacter, role_skill: HiddenRoleSkill) {
    if character
        .skill_names
        .iter()
        .any(|name| name.trim() == role_skill.name)
    {
        return;
    }
    character.skill_names.push(role_skill.name.to_owned());
    character.skill_notes.push(role_skill.note.to_owned());
    character.skill_mp_costs.push(0.0);
    character.skill_cooldown_turns.push(role_skill.cooldown);
    character.skill_metadata.push(CharacterSkillMetadata {
        skill_type: Some(role_skill.skill_type.to_owned()),
        target_class: Some(role_skill.target_class.to_owned()),
        range: Some(role_skill.range),
        ..Default::default()
    });
}

pub fn effective_hidden_role_character(
    target_id: &str,
    character: &PlayerCharacter,
    hidden_roles: &HashMap<String, HiddenRoleState>,
) -> PlayerCharacter {
    let Some(state) = hidden_roles.get(target_id) else {
        return character.clone();
    };
    let profile = combat_profile(state);
    let mut effective = character.clone();
    if state.is_cocooning() {
        effective.max_hp = state.cocoon_hp.max(1.0);
        effective.hp = effective.max_hp;
        effective.hp_regen = 0.0;
        effective.speed = 0.0;
        effective.skill_names.clear();
        effective.skill_notes.clear();
        effective.skill_mp_costs.clear();
        effective.skill_cooldown_turns.clear();
        effective.skill_metadata.clear();
        return effective;
    }
    effective.extra_status.str_ += profile.stat_bonus;
    effective.extra_status.agi += profile.stat_bonus;
    effective.extra_status.dex += profile.stat_bonus;
    effective.extra_status.vit += profile.stat_bonus;
    if state.role == HiddenRoleKind::Mutant && state.transformed && state.stage >= 2 {
        effective.extra_status.int_ += profile.stat_bonus;
        effective.extra_status.wis += profile.stat_bonus;
        effective.extra_status.k += profile.stat_bonus;
        effective.extra_status.cha += profile.stat_bonus;
    } else if state.role == HiddenRoleKind::Android && state.android_gear_obtained {
        effective.extra_status.k += profile.stat_bonus;
    }
    effective.max_hp = (effective.max_hp + profile.max_hp_bonus).max(1.0);
    effective.hp = (effective.hp + profile.max_hp_bonus).min(effective.max_hp);
    effective.max_mp = (effective.max_mp + profile.max_mp_bonus).max(0.0);
    effective.mp = (effective.mp + profile.max_mp_bonus).min(effective.max_mp);
    effective.hp_regen = (effective.hp_regen + profile.hp_regen_bonus).max(0.0);
    effective.speed = (effective.speed + profile.speed_bonus).max(0.0);
    effective.damage_dealt_modifier *= profile.damage_dealt_multiplier;
    effective.damage_taken_modifier *= profile.damage_taken_multiplier;
    for role_skill in profile.skills {
        append_hidden_skill(&mut effective, role_skill);
    }
    effective
}

pub fn hidden_role_battle_state(state: Option<&HiddenRoleState>) -> HiddenRoleBattleState {
    let Some(state) = state else {
        return HiddenRoleBattleState::default();
    };
    let profile = combat_profile(state);
    HiddenRoleBattleState {
        cocooning: state.is_cocooning(),
        free_action_skill: profile.free_action_skill.map(str::to_owned),
        free_action_available: profile.free_action_skill.is_some(),
        second_wind_heal: profile.second_wind_heal,
        second_wind_used: false,
        immune_diseased: profile.immune_diseased,
        immune_poisoning: profile.immune_poisoning,
        immune_bleed: profile.immune_bleed,
    }
}

pub fn hidden_role_opening_shield(state: Option<&HiddenRoleState>) -> f32 {
    let Some(state) = state else {
        return 0.0;
    };
    let suit_shield = if state.protective_suit_worn && !state.protective_suit_destroyed {
        match state.role {
            HiddenRoleKind::Alien => 6.0,
            HiddenRoleKind::Mutant => 3.0,
            HiddenRoleKind::Android => 0.0,
        }
    } else {
        0.0
    };
    combat_profile(state).opening_shield.max(suit_shield)
}

pub fn hidden_role_stage_summary(state: &HiddenRoleState) -> String {
    match (state.role, state.stage) {
        (HiddenRoleKind::Alien, 1) => {
            "1阶段：变身后力量/敏捷/灵巧/体质+5、最大HP+20；异形爪击6伤害且每轮免费1次。基准≈2名人类。".to_owned()
        },
        (HiddenRoleKind::Alien, 2) => {
            "2阶段：四项+8、最大HP+60、每轮回复+5、伤害×1.15、承伤×0.75；免费爪击9、飞扑10、酸液7。基准≈4名人类。".to_owned()
        },
        (HiddenRoleKind::Alien, _) => {
            "3阶段：四项+12、最大HP+100、每轮回复+10、伤害×1.25、承伤×0.60；半血首次回复20；免费爪击12、尾刺16、范围尖啸6并震慑。基准≈6名人类。".to_owned()
        },
        (HiddenRoleKind::Mutant, 1) => {
            "1阶段：无战斗加成；只能吞噬生物尸体。3生物质自动进入2阶段。基准≈1名人类。".to_owned()
        },
        (HiddenRoleKind::Mutant, 2) => {
            "2阶段：变身后全属性+5、最大HP+45、每轮回复+5、伤害×1.20、承伤×0.75；半血首次回复15；骨刺10、冲锋12。基准≈3名人类。".to_owned()
        },
        (HiddenRoleKind::Mutant, _) => format!(
            "3阶段·{}：全属性+10并获得专属机制；猎杀体高爆发免费撕裂，重甲体高生命/55%减伤，腐化体范围中毒/虹吸。各分支基准≈8名人类。",
            state.mutant_path.unwrap_or_default().label()
        ),
        (HiddenRoleKind::Android, _) if state.android_gear_obtained => {
            "已取得公司装备：属性+3、最大HP+40、30护盾、每轮回复+5、伤害×1.25、承伤×0.70；免疫疾病/中毒/流血；步枪12、范围电弧8、维修18。基准≈4名人类。".to_owned()
        },
        (HiddenRoleKind::Android, _) => {
            "未取得公司装备：保持原角色能力；前往女皇号取得装备后达到≈4名人类。".to_owned()
        },
    }
}

pub fn advance_hidden_roles_for_world_turn(
    hidden_roles: &mut HashMap<String, HiddenRoleState>,
    player_ids: &[String],
    world_turn: u32,
) -> bool {
    player_ids.iter().fold(false, |changed, player_id| {
        changed
            | hidden_roles
                .get_mut(player_id)
                .is_some_and(|state| state.advance_world_turn(world_turn))
    })
}

/// Merge a GM-owned campaign seed without overwriting any role that has already
/// accumulated progress in the main persistent message store.
pub fn merge_hidden_role_seed(
    hidden_roles: &mut HashMap<String, HiddenRoleState>,
    path: &Path,
) -> Result<usize, String> {
    if !path.exists() {
        return Ok(0);
    }
    let source = fs::read_to_string(path).map_err(|err| {
        format!(
            "failed to read hidden-role seed {}: {err}",
            path.display()
        )
    })?;
    let seeded = toml::from_str::<HashMap<String, HiddenRoleState>>(&source).map_err(|err| {
        format!(
            "failed to parse hidden-role seed {}: {err}",
            path.display()
        )
    })?;
    let mut inserted = 0;
    for (target_id, mut state) in seeded {
        state.normalize();
        if let Entry::Vacant(entry) = hidden_roles.entry(target_id) {
            entry.insert(state);
            inserted += 1;
        }
    }
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alien_requires_points_and_surviving_cocoon_turns() {
        let mut state = HiddenRoleState::new(HiddenRoleKind::Alien);
        assert!(state.advance_world_turn(6));
        assert_eq!(state.evolution_points, 6);
        assert!(state.begin_cocoon());
        assert_eq!(state.cocoon_hp, 24.0);
        assert!(state.advance_world_turn(7));
        assert_eq!(state.stage, 1);
        assert!(state.advance_world_turn(8));
        assert_eq!(state.stage, 2);
    }

    #[test]
    fn destroying_an_alien_cocoon_interrupts_evolution() {
        let mut state = HiddenRoleState::new(HiddenRoleKind::Alien);
        state.evolution_points = ALIEN_STAGE_TWO_POINTS;
        state.protective_suit_worn = true;
        assert!(state.begin_cocoon());
        assert!(state.protective_suit_destroyed);
        assert!(state.damage_cocoon(24.0));
        assert_eq!(state.cocoon_turns_remaining, 0);
        assert_eq!(
            state.evolution_points,
            ALIEN_STAGE_TWO_POINTS - 2
        );
        assert_eq!(state.stage, 1);
    }

    #[test]
    fn mutant_evolves_automatically_and_waits_for_second_path() {
        let mut state = HiddenRoleState::new(HiddenRoleKind::Mutant);
        assert!(state.consume_corpse(3));
        assert_eq!(state.stage, 2);
        assert!(!state.transformed);
        assert!(state.consume_corpse(6));
        assert_eq!(state.stage, 2);
        state.mutant_path = Some(MutantEvolutionPath::Bulwark);
        assert!(state.apply_automatic_mutant_evolution());
        assert_eq!(state.stage, 3);
    }

    #[test]
    fn role_profiles_add_mechanics_without_mutating_base_character() {
        let base = PlayerCharacter {
            hp: 20.0,
            max_hp: 20.0,
            ..Default::default()
        };
        let mut roles = HashMap::new();
        let mut alien = HiddenRoleState::new(HiddenRoleKind::Alien);
        alien.transformed = true;
        roles.insert("alien".to_owned(), alien);
        let effective = effective_hidden_role_character("alien", &base, &roles);
        assert_eq!(base.max_hp, 20.0);
        assert_eq!(effective.max_hp, 40.0);
        assert_eq!(effective.extra_status.str_, 5);
        assert!(effective.skill_names.iter().any(|name| name == "异形爪击"));
    }

    #[test]
    fn campaign_seed_never_overwrites_existing_progress() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("roles.toml");
        fs::write(
            &path,
            "[player]\nrole = \"alien\"\nstage = 1\nprotective_suit_worn = true\n",
        )
        .unwrap();
        let mut roles = HashMap::new();
        let mut progressed = HiddenRoleState::new(HiddenRoleKind::Alien);
        progressed.stage = 2;
        roles.insert("player".to_owned(), progressed);
        assert_eq!(
            merge_hidden_role_seed(&mut roles, &path).unwrap(),
            0
        );
        assert_eq!(roles["player"].stage, 2);
    }
}
