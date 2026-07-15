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
use rand::RngExt;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    Map,
    Value,
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
    GrantBuff {
        target: TargetSelector,
        buff: RuleBuffTemplate,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyMoonberryPoolArg {
    pub name: String,
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyMoonberryPoolEntry {
    pub id: Option<String>,
    pub name: String,
    pub legacy_json: String,
    pub args: Vec<LegacyMoonberryPoolArg>,
}

pub fn apply_skill_type_damage_default(mut ast: RuleAst, skill_type: Option<&str>) -> RuleAst {
    let Some(default_damage_type) = skill_type_default_damage_type(skill_type) else {
        return ast;
    };
    for action in &mut ast.actions {
        if let Action::Damage { damage_type, .. } = action {
            if *damage_type == DamageType::None {
                *damage_type = default_damage_type;
            }
        }
    }
    ast
}

pub fn skill_type_default_damage_type(skill_type: Option<&str>) -> Option<DamageType> {
    match skill_type
        .map(str::trim)
        .filter(|skill_type| !skill_type.is_empty())?
    {
        "动作" => Some(DamageType::Physical),
        "远程" => Some(DamageType::Range),
        "法术" | "道具" | "异能" | "血统" | "职业" | "召唤物" => {
            Some(DamageType::Magical)
        },
        _ => None,
    }
}

pub fn legacy_moonberry_buff_machine_skill_cast_rule(
    legacy_json: &str,
    named_values: &[(String, f32)],
    skill_type: Option<&str>,
) -> Option<RuleAst> {
    legacy_moonberry_buff_machine_skill_cast_rule_with_context(
        legacy_json,
        named_values,
        &[],
        skill_type,
        &[],
    )
}

pub fn legacy_moonberry_buff_machine_skill_cast_rule_with_context(
    legacy_json: &str,
    named_values: &[(String, f32)],
    text_values: &[(String, String)],
    skill_type: Option<&str>,
    pool_entries: &[LegacyMoonberryPoolEntry],
) -> Option<RuleAst> {
    let value = serde_json::from_str::<Value>(legacy_json).ok()?;
    let named_values = normalize_named_values(named_values);
    let text_values = normalize_named_text_values(text_values);
    let mut actions = Vec::new();
    collect_legacy_skill_cast_actions(
        &value,
        &named_values,
        &text_values,
        pool_entries,
        &mut actions,
        0,
    );
    if actions.is_empty() {
        return None;
    }

    Some(apply_skill_type_damage_default(
        RuleAst {
            raw: "旧月莓buff机：技能释放".to_owned(),
            trigger: Trigger {
                subject: ActorRef::SelfActor,
                event: EventKind::SkillCast,
            },
            actions,
        },
        skill_type,
    ))
}

pub fn legacy_moonberry_buff_machine_passive_buffs(
    legacy_json: &str,
    named_values: &[(String, f32)],
    source_id: &str,
) -> Vec<BuffSpec> {
    legacy_moonberry_buff_machine_passive_buffs_with_context(
        legacy_json,
        named_values,
        &[],
        source_id,
        &[],
    )
}

pub fn legacy_moonberry_buff_machine_passive_buffs_with_context(
    legacy_json: &str,
    named_values: &[(String, f32)],
    text_values: &[(String, String)],
    source_id: &str,
    pool_entries: &[LegacyMoonberryPoolEntry],
) -> Vec<BuffSpec> {
    let Ok(value) = serde_json::from_str::<Value>(legacy_json) else {
        return Vec::new();
    };
    let named_values = normalize_named_values(named_values);
    let text_values = normalize_named_text_values(text_values);
    let mut buffs = Vec::new();
    collect_legacy_passive_buffs(
        &value,
        &named_values,
        &text_values,
        source_id,
        pool_entries,
        &mut buffs,
        0,
    );
    buffs
}

fn collect_legacy_skill_cast_actions(
    value: &Value,
    named_values: &[(String, f32)],
    text_values: &[(String, String)],
    pool_entries: &[LegacyMoonberryPoolEntry],
    actions: &mut Vec<Action>,
    depth: usize,
) {
    let Some(object) = value.as_object() else {
        return;
    };
    if depth > 8 {
        return;
    }
    let initial_count = actions.len();

    if let Some(buff_machine) = object.get("buffMachine") {
        collect_legacy_skill_cast_actions(
            buff_machine,
            named_values,
            text_values,
            pool_entries,
            actions,
            depth + 1,
        );
    }
    if let Some(buffs) = object.get("技能释放").and_then(Value::as_array) {
        for buff in buffs {
            append_legacy_buff_actions(
                buff,
                named_values,
                text_values,
                pool_entries,
                actions,
                depth,
            );
        }
    }
    if legacy_event_matches(object.get("event"), "技能释放") {
        if let Some(buffs) = object.get("buffs").and_then(Value::as_array) {
            for buff in buffs {
                append_legacy_buff_actions(
                    buff,
                    named_values,
                    text_values,
                    pool_entries,
                    actions,
                    depth,
                );
            }
        }
    }
    if let Some(event_buffs) = object.get("eventBuffs").and_then(Value::as_array) {
        for event_buff in event_buffs {
            collect_legacy_skill_cast_actions(
                event_buff,
                named_values,
                text_values,
                pool_entries,
                actions,
                depth + 1,
            );
        }
    }
    if actions.len() == initial_count {
        collect_legacy_graph_skill_cast_actions(
            object.get("graph").unwrap_or(value),
            named_values,
            text_values,
            pool_entries,
            actions,
            depth,
        );
    }
}

fn collect_legacy_passive_buffs(
    value: &Value,
    named_values: &[(String, f32)],
    text_values: &[(String, String)],
    source_id: &str,
    pool_entries: &[LegacyMoonberryPoolEntry],
    buffs: &mut Vec<BuffSpec>,
    depth: usize,
) {
    let Some(object) = value.as_object() else {
        return;
    };
    if depth > 8 {
        return;
    }
    let initial_count = buffs.len();

    if let Some(buff_machine) = object.get("buffMachine") {
        collect_legacy_passive_buffs(
            buff_machine,
            named_values,
            text_values,
            source_id,
            pool_entries,
            buffs,
            depth + 1,
        );
    }
    if let Some(entries) = object.get("被动").and_then(Value::as_array) {
        for buff in entries {
            append_legacy_passive_buff(buff, named_values, source_id, buffs);
        }
    }
    if legacy_event_matches(object.get("event"), "被动") {
        if let Some(entries) = object.get("buffs").and_then(Value::as_array) {
            for buff in entries {
                append_legacy_passive_buff(buff, named_values, source_id, buffs);
            }
        }
    }
    if let Some(event_buffs) = object.get("eventBuffs").and_then(Value::as_array) {
        for event_buff in event_buffs {
            collect_legacy_passive_buffs(
                event_buff,
                named_values,
                text_values,
                source_id,
                pool_entries,
                buffs,
                depth + 1,
            );
        }
    }
    if buffs.len() == initial_count {
        collect_legacy_graph_passive_buffs(
            object.get("graph").unwrap_or(value),
            named_values,
            text_values,
            source_id,
            pool_entries,
            buffs,
            depth,
        );
    }
}

fn collect_legacy_graph_skill_cast_actions(
    graph: &Value,
    named_values: &[(String, f32)],
    text_values: &[(String, String)],
    pool_entries: &[LegacyMoonberryPoolEntry],
    actions: &mut Vec<Action>,
    depth: usize,
) {
    for buff in legacy_graph_event_buffs(graph, "技能释放") {
        append_legacy_buff_actions(
            &buff,
            named_values,
            text_values,
            pool_entries,
            actions,
            depth,
        );
    }
}

fn collect_legacy_graph_passive_buffs(
    graph: &Value,
    named_values: &[(String, f32)],
    _text_values: &[(String, String)],
    source_id: &str,
    _pool_entries: &[LegacyMoonberryPoolEntry],
    buffs: &mut Vec<BuffSpec>,
    _depth: usize,
) {
    for buff in legacy_graph_event_buffs(graph, "被动") {
        append_legacy_passive_buff(&buff, named_values, source_id, buffs);
    }
}

#[derive(Debug, Clone)]
struct LegacyGraphNode {
    id: String,
    kind: String,
    component: String,
    name: Option<String>,
}

#[derive(Debug, Clone)]
struct LegacyGraphEdge {
    kind: String,
    source_cell: String,
    source_port: Option<String>,
    target_cell: String,
    target_port: Option<String>,
}

fn legacy_graph_event_buffs(graph: &Value, event_name: &str) -> Vec<Value> {
    let cells = legacy_graph_cells(graph);
    if cells.is_empty() {
        return Vec::new();
    }

    let mut nodes = HashMap::new();
    let mut edges = Vec::new();
    for cell in cells {
        if let Some(edge) = legacy_graph_edge(cell) {
            edges.push(edge);
        } else if let Some(node) = legacy_graph_node(cell) {
            nodes.insert(node.id.clone(), node);
        }
    }

    let mut buffs = Vec::new();
    for event_node in nodes
        .values()
        .filter(|node| legacy_graph_component_matches(&node.component, event_name))
    {
        let mut current_id = event_node.id.clone();
        let mut visited = HashSet::new();
        while visited.insert(current_id.clone()) {
            let Some(edge) = edges
                .iter()
                .find(|edge| edge.kind == "exec" && edge.source_cell == current_id)
            else {
                break;
            };
            let Some(next_node) = nodes.get(&edge.target_cell) else {
                break;
            };
            if let Some(buff) = legacy_graph_node_buff(next_node, &edges, &nodes) {
                buffs.push(buff);
            }
            current_id = next_node.id.clone();
        }
    }
    buffs
}

fn legacy_graph_cells(graph: &Value) -> Vec<&Value> {
    if let Some(cells) = graph.get("cells").and_then(Value::as_array) {
        cells.iter().collect()
    } else if let Some(cells) = graph.as_array() {
        cells.iter().collect()
    } else {
        Vec::new()
    }
}

fn legacy_graph_node(cell: &Value) -> Option<LegacyGraphNode> {
    let id = legacy_graph_string_field(cell, "id")?;
    let component = legacy_graph_string_field(cell, "component")
        .or_else(|| legacy_graph_string_field(cell, "name"))?;
    Some(LegacyGraphNode {
        id,
        kind: legacy_graph_string_field(cell, "type").unwrap_or_default(),
        component,
        name: legacy_graph_string_field(cell, "name"),
    })
}

fn legacy_graph_edge(cell: &Value) -> Option<LegacyGraphEdge> {
    let source = cell.get("source")?;
    let target = cell.get("target")?;
    Some(LegacyGraphEdge {
        kind: legacy_graph_string_field(cell, "type")
            .map(|kind| normalize_rule_text(&kind))
            .unwrap_or_default(),
        source_cell: legacy_graph_endpoint_cell(source)?,
        source_port: legacy_graph_endpoint_port(source),
        target_cell: legacy_graph_endpoint_cell(target)?,
        target_port: legacy_graph_endpoint_port(target),
    })
}

fn legacy_graph_endpoint_cell(endpoint: &Value) -> Option<String> {
    match endpoint {
        Value::String(cell) => Some(cell.clone()),
        Value::Object(object) => object
            .get("cell")
            .or_else(|| object.get("id"))
            .and_then(legacy_string_value),
        _ => None,
    }
}

fn legacy_graph_endpoint_port(endpoint: &Value) -> Option<String> {
    endpoint
        .as_object()
        .and_then(|object| object.get("port"))
        .and_then(legacy_string_value)
}

fn legacy_graph_string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(legacy_string_value)
        .or_else(|| {
            value
                .get("data")
                .and_then(|data| data.get(key))
                .and_then(legacy_string_value)
        })
        .filter(|value| !value.trim().is_empty())
}

fn legacy_graph_component_matches(component: &str, event_name: &str) -> bool {
    normalize_rule_text(component) == normalize_rule_text(event_name)
}

fn legacy_graph_node_buff(
    node: &LegacyGraphNode,
    edges: &[LegacyGraphEdge],
    nodes: &HashMap<String, LegacyGraphNode>,
) -> Option<Value> {
    if node.kind != "function" {
        return None;
    }
    let component = normalize_rule_text(&node.component);
    let args = legacy_graph_node_args(&node.id, edges, nodes);
    let target = legacy_graph_arg(&args, 0)?;
    let value = legacy_graph_arg(&args, 1);

    match component.as_str() {
        "伤害" => Some(legacy_graph_buff(
            &node.component,
            "伤害",
            7,
            target,
            false,
            vec![value?],
            Value::from(0),
        )),
        "治疗" => Some(legacy_graph_buff(
            &node.component,
            "治疗",
            0,
            target,
            true,
            vec![value?],
            Value::from(0),
        )),
        "给予BUFF" => Some(legacy_graph_buff(
            &node.component,
            "给予BUFF",
            0,
            target,
            true,
            vec![value?],
            legacy_graph_arg(&args, 2)
                .map(|value| Value::String(value.to_owned()))
                .unwrap_or_else(|| Value::from(1)),
        )),
        _ => Some(legacy_graph_buff(
            &node.component,
            legacy_graph_basic_effect(&component)?,
            7,
            target,
            true,
            vec![value?],
            Value::from(0),
        )),
    }
}

fn legacy_graph_node_args(
    node_id: &str,
    edges: &[LegacyGraphEdge],
    nodes: &HashMap<String, LegacyGraphNode>,
) -> Vec<Option<String>> {
    let mut args = Vec::new();
    for edge in edges
        .iter()
        .filter(|edge| edge.target_cell == node_id && edge.kind != "exec")
    {
        let Some(target_index) = edge
            .target_port
            .as_deref()
            .and_then(legacy_graph_port_index)
        else {
            continue;
        };
        let value = nodes
            .get(&edge.source_cell)
            .and_then(|node| node.name.clone())
            .or_else(|| {
                edge.source_port
                    .as_deref()
                    .and_then(legacy_graph_port_label)
            });
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            if args.len() <= target_index {
                args.resize(target_index + 1, None);
            }
            args[target_index] = Some(value);
        }
    }
    args
}

fn legacy_graph_port_index(port: &str) -> Option<usize> {
    port.split(':')
        .next_back()
        .and_then(|index| index.parse::<usize>().ok())
        .and_then(|index| index.checked_sub(1))
}

fn legacy_graph_port_label(port: &str) -> Option<String> {
    port.split(':')
        .next()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
}

fn legacy_graph_arg(args: &[Option<String>], index: usize) -> Option<&str> {
    args.get(index)
        .and_then(Option::as_deref)
        .map(str::trim)
        .filter(|arg| !arg.is_empty())
}

fn legacy_graph_basic_effect(component: &str) -> Option<&'static str> {
    match component {
        "设置生命" => Some("hp"),
        "设置魔法" => Some("mp"),
        "设置最大生命值" => Some("maxHP"),
        "设置最大魔法值" => Some("maxMP"),
        "设置生命回复" => Some("hpReg"),
        "设置魔法回复" => Some("mpReg"),
        "设置力量" => Some("str"),
        "设置敏捷" => Some("agi"),
        "设置灵巧" => Some("dex"),
        "设置体质" => Some("vit"),
        "设置智力" => Some("int"),
        "设置睿智" => Some("wis"),
        "设置知识" => Some("k"),
        "设置魅力" => Some("cha"),
        "设置伤害增减" => Some("DMGModify"),
        "设置治疗增减" => Some("healModify"),
        _ => None,
    }
}

fn legacy_graph_buff(
    name: &str,
    effect: &str,
    damage_type: i32,
    target: &str,
    beneficial: bool,
    values: Vec<&str>,
    life: Value,
) -> Value {
    let mut object = Map::new();
    object.insert(
        "name".to_owned(),
        Value::String(name.to_owned()),
    );
    object.insert("prior".to_owned(), Value::from(0));
    object.insert("life".to_owned(), life);
    object.insert(
        "effect".to_owned(),
        Value::Array(vec![Value::String(effect.to_owned())]),
    );
    object.insert(
        "type".to_owned(),
        Value::from(damage_type),
    );
    object.insert(
        "from".to_owned(),
        Value::String(target.to_owned()),
    );
    object.insert(
        "benifit".to_owned(),
        Value::Bool(beneficial),
    );
    object.insert(
        "value".to_owned(),
        Value::Array(
            values
                .into_iter()
                .map(|value| Value::String(value.to_owned()))
                .collect(),
        ),
    );
    Value::Object(object)
}

fn append_legacy_buff_actions(
    buff: &Value,
    named_values: &[(String, f32)],
    text_values: &[(String, String)],
    pool_entries: &[LegacyMoonberryPoolEntry],
    actions: &mut Vec<Action>,
    depth: usize,
) {
    let Some(effects) = buff.get("effect").and_then(Value::as_array) else {
        return;
    };
    let values = buff
        .get("value")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let target = legacy_target_selector(buff.get("from"));
    let mut buff_effects = Vec::new();

    for (index, effect) in effects.iter().enumerate() {
        let Some(effect_name) = legacy_string_value(effect) else {
            continue;
        };
        let value = values.get(index);
        match effect_name.as_str() {
            "伤害" => {
                if let Some(amount) = legacy_numeric_value(value, named_values) {
                    actions.push(Action::Damage {
                        target,
                        amount: ValueExpr::Number(amount),
                        damage_type: legacy_damage_type(buff.get("type")),
                    });
                }
            },
            "治疗" => {
                if let Some(amount) = legacy_numeric_value(value, named_values) {
                    actions.push(Action::Heal {
                        target,
                        amount: ValueExpr::Number(amount),
                    });
                }
            },
            "给予BUFF" => append_legacy_granted_buff_actions(
                buff,
                value,
                values,
                target,
                named_values,
                text_values,
                pool_entries,
                actions,
                depth,
            ),
            _ => {
                if let (Some(field), Some(value)) = (
                    legacy_buff_field(&effect_name),
                    legacy_buff_value(value, named_values),
                ) {
                    buff_effects.push(BuffEffect { field, value });
                }
            },
        }
    }

    if !buff_effects.is_empty() {
        actions.push(Action::GrantBuff {
            target,
            buff: RuleBuffTemplate {
                name: legacy_buff_name(buff),
                kind: legacy_buff_kind(buff.get("type")),
                priority: legacy_i32_value(buff.get("prior")).unwrap_or_default(),
                turns_remaining: legacy_buff_turns_with_named(buff.get("life"), named_values),
                beneficial: legacy_bool_value(buff.get("benifit"))
                    .or_else(|| legacy_bool_value(buff.get("benefit")))
                    .unwrap_or(true),
                effects: buff_effects,
                tick_actions: Vec::new(),
            },
        });
    }
}

fn append_legacy_granted_buff_actions(
    buff: &Value,
    pool_value: Option<&Value>,
    values: &[Value],
    target: TargetSelector,
    named_values: &[(String, f32)],
    text_values: &[(String, String)],
    pool_entries: &[LegacyMoonberryPoolEntry],
    actions: &mut Vec<Action>,
    depth: usize,
) {
    if depth > 8 {
        return;
    }
    let Some(pool_key) = legacy_text_value(pool_value, text_values) else {
        return;
    };
    let Some(pool_entry) = legacy_pool_entry(pool_entries, &pool_key) else {
        return;
    };

    let (nested_numeric_values, nested_text_values) = legacy_pool_named_values(
        pool_entry,
        &values[1..],
        named_values,
        text_values,
    );
    let Ok(pool_value) = serde_json::from_str::<Value>(&pool_entry.legacy_json) else {
        return;
    };

    let mut nested_actions = Vec::new();
    collect_legacy_skill_cast_actions(
        &pool_value,
        &nested_numeric_values,
        &nested_text_values,
        pool_entries,
        &mut nested_actions,
        depth + 1,
    );

    let turns_remaining = legacy_buff_turns_with_named(buff.get("life"), named_values);
    let mut tick_actions = Vec::new();
    for action in nested_actions {
        match action {
            Action::GrantBuff { mut buff, .. } => {
                buff.turns_remaining = turns_remaining;
                actions.push(Action::GrantBuff { target, buff });
            },
            Action::Damage {
                amount: ValueExpr::Number(amount),
                damage_type,
                ..
            } => {
                tick_actions.push(BuffTickAction::Damage {
                    amount: amount.max(0.0),
                    damage_type: if damage_type == DamageType::None {
                        DamageType::Magical
                    } else {
                        damage_type
                    },
                });
            },
            Action::Heal {
                amount: ValueExpr::Number(amount),
                ..
            } => {
                tick_actions.push(BuffTickAction::Heal {
                    amount: amount.max(0.0),
                });
            },
            _ => {},
        }
    }
    if !tick_actions.is_empty() {
        actions.push(Action::GrantBuff {
            target,
            buff: RuleBuffTemplate {
                name: legacy_buff_name(buff),
                kind: legacy_buff_kind(buff.get("type")),
                priority: legacy_i32_value(buff.get("prior")).unwrap_or_default(),
                turns_remaining,
                beneficial: legacy_bool_value(buff.get("benifit"))
                    .or_else(|| legacy_bool_value(buff.get("benefit")))
                    .unwrap_or(false),
                effects: Vec::new(),
                tick_actions,
            },
        });
    }
}

fn append_legacy_passive_buff(
    buff: &Value,
    named_values: &[(String, f32)],
    source_id: &str,
    buffs: &mut Vec<BuffSpec>,
) {
    let Some(effects) = buff.get("effect").and_then(Value::as_array) else {
        return;
    };
    let values = buff
        .get("value")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let buff_effects = effects
        .iter()
        .enumerate()
        .filter_map(|(index, effect)| {
            let effect_name = legacy_string_value(effect)?;
            Some(BuffEffect {
                field: legacy_buff_field(&effect_name)?,
                value: legacy_buff_value(values.get(index), named_values)?,
            })
        })
        .collect::<Vec<_>>();

    if buff_effects.is_empty() {
        return;
    }

    buffs.push(BuffSpec {
        name: legacy_buff_name(buff),
        kind: legacy_buff_kind(buff.get("type")),
        priority: legacy_i32_value(buff.get("prior")).unwrap_or_default(),
        turns_remaining: 0,
        source_id: source_id.to_owned(),
        beneficial: legacy_bool_value(buff.get("benifit"))
            .or_else(|| legacy_bool_value(buff.get("benefit")))
            .unwrap_or(true),
        effects: buff_effects,
        tick_actions: Vec::new(),
    });
}

fn legacy_event_matches(value: Option<&Value>, event_name: &str) -> bool {
    value
        .and_then(legacy_string_value)
        .map(|event| normalize_rule_text(&event).contains(event_name))
        .unwrap_or(false)
}

fn legacy_target_selector(value: Option<&Value>) -> TargetSelector {
    let actor = match value
        .and_then(legacy_string_value)
        .as_deref()
        .map(str::trim)
    {
        Some("-1") | Some("自己") => ActorRef::SelfActor,
        Some("-2") | Some("技能目标") | Some("目标") => ActorRef::Target,
        _ => match value.and_then(Value::as_i64) {
            Some(-1) => ActorRef::SelfActor,
            Some(-2) => ActorRef::Target,
            _ => ActorRef::Target,
        },
    };
    TargetSelector { actor, area: None }
}

fn legacy_damage_type(value: Option<&Value>) -> DamageType {
    if let Some(number) = value.and_then(Value::as_i64) {
        return match number {
            0 => DamageType::Magical,
            1 => DamageType::Physical,
            2 => DamageType::Cursed,
            3 => DamageType::Diseased,
            4 => DamageType::Bleed,
            5 => DamageType::Range,
            6 => DamageType::Poisoning,
            _ => DamageType::None,
        };
    }

    match value
        .and_then(legacy_string_value)
        .map(|value| normalize_rule_text(&value))
        .as_deref()
    {
        Some("Magical") | Some("魔法") | Some("0") => DamageType::Magical,
        Some("Physical") | Some("物理") | Some("1") => DamageType::Physical,
        Some("Cursed") | Some("诅咒") | Some("2") => DamageType::Cursed,
        Some("Diseased") | Some("疾病") | Some("3") => DamageType::Diseased,
        Some("bleed") | Some("Bleed") | Some("流血") | Some("4") => DamageType::Bleed,
        Some("Range") | Some("远程") | Some("5") => DamageType::Range,
        Some("poisoning") | Some("Poisoning") | Some("中毒") | Some("6") => DamageType::Poisoning,
        _ => DamageType::None,
    }
}

fn legacy_buff_kind(value: Option<&Value>) -> BuffKind {
    match legacy_damage_type(value) {
        DamageType::Magical => BuffKind::Magic,
        DamageType::Physical => BuffKind::Physical,
        DamageType::Cursed => BuffKind::Curse,
        DamageType::Diseased => BuffKind::Disease,
        DamageType::Bleed => BuffKind::Bleed,
        DamageType::Range => BuffKind::Range,
        DamageType::Poisoning => BuffKind::Poison,
        DamageType::None => BuffKind::None,
    }
}

fn legacy_buff_field(effect: &str) -> Option<BuffField> {
    match effect {
        "hp" => Some(BuffField::Hp),
        "mp" => Some(BuffField::Mp),
        "maxHP" => Some(BuffField::MaxHp),
        "maxMP" => Some(BuffField::MaxMp),
        "hpReg" => Some(BuffField::HpRegen),
        "mpReg" => Some(BuffField::MpRegen),
        "speed" => Some(BuffField::Speed),
        "str" => Some(BuffField::Status(StatusKey::Str)),
        "agi" => Some(BuffField::Status(StatusKey::Agi)),
        "dex" => Some(BuffField::Status(StatusKey::Dex)),
        "vit" => Some(BuffField::Status(StatusKey::Vit)),
        "int" => Some(BuffField::Status(StatusKey::Int)),
        "wis" => Some(BuffField::Status(StatusKey::Wis)),
        "k" => Some(BuffField::Status(StatusKey::K)),
        "cha" => Some(BuffField::Status(StatusKey::Cha)),
        "DMGModify" => Some(BuffField::DamageDealtModifier),
        "healModify" => Some(BuffField::HealingDealtModifier),
        "tDMGModify" => Some(BuffField::DamageTakenModifier),
        "tHealModify" => Some(BuffField::HealingTakenModifier),
        _ => None,
    }
}

fn legacy_buff_value(value: Option<&Value>, named_values: &[(String, f32)]) -> Option<BuffValue> {
    let raw = legacy_string_value(value?)?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let set = raw.starts_with('=');
    let raw = raw.strip_prefix('=').unwrap_or(raw).trim();
    let percent = raw.ends_with(['%', '％']);
    let raw = raw.trim_end_matches(['%', '％']).trim();
    let value = legacy_numeric_text(raw, named_values)?;
    match (set, percent) {
        (true, true) => Some(BuffValue::SetPercentOfBase(value)),
        (true, false) => Some(BuffValue::Set(value)),
        (false, true) => Some(BuffValue::AddPercent(value)),
        (false, false) => Some(BuffValue::Add(value)),
    }
}

fn legacy_numeric_value(value: Option<&Value>, named_values: &[(String, f32)]) -> Option<f32> {
    match value? {
        Value::Number(number) => number.as_f64().map(|value| value as f32),
        Value::String(text) => legacy_numeric_text(text, named_values),
        _ => None,
    }
}

fn legacy_numeric_text(text: &str, named_values: &[(String, f32)]) -> Option<f32> {
    let text = normalize_rule_text(text);
    if text.is_empty() {
        return None;
    }
    if let Ok(value) = text.parse::<f32>() {
        return Some(value);
    }
    named_values
        .iter()
        .find_map(|(name, value)| (name == &text).then_some(*value))
}

fn legacy_text_value(value: Option<&Value>, text_values: &[(String, String)]) -> Option<String> {
    let raw = legacy_string_value(value?)?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    text_values
        .iter()
        .find_map(|(name, value)| (name == raw).then(|| value.clone()))
        .or_else(|| Some(raw.to_owned()))
}

fn legacy_string_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn legacy_i32_value(value: Option<&Value>) -> Option<i32> {
    value
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .or_else(|| {
            value
                .and_then(Value::as_str)
                .and_then(|value| value.trim().parse::<i32>().ok())
        })
}

fn legacy_bool_value(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(value) => Some(*value),
        Value::String(value) => match value.trim() {
            "true" | "1" | "是" => Some(true),
            "false" | "0" | "否" => Some(false),
            _ => None,
        },
        Value::Number(number) => number.as_i64().map(|value| value != 0),
        _ => None,
    }
}

fn legacy_buff_name(buff: &Value) -> String {
    buff.get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("旧月莓BUFF")
        .to_owned()
}

fn legacy_buff_turns_with_named(value: Option<&Value>, named_values: &[(String, f32)]) -> i32 {
    legacy_i32_value(value)
        .or_else(|| legacy_numeric_value(value, named_values).map(|value| value.round() as i32))
        .filter(|turns| *turns > 0)
        .unwrap_or(1)
}

fn legacy_pool_entry<'a>(
    pool_entries: &'a [LegacyMoonberryPoolEntry],
    key: &str,
) -> Option<&'a LegacyMoonberryPoolEntry> {
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    pool_entries.iter().find(|entry| {
        entry.id.as_deref().is_some_and(|id| id.trim() == key) || entry.name.trim() == key
    })
}

fn legacy_pool_named_values(
    pool_entry: &LegacyMoonberryPoolEntry,
    override_values: &[Value],
    named_values: &[(String, f32)],
    text_values: &[(String, String)],
) -> (
    Vec<(String, f32)>,
    Vec<(String, String)>,
) {
    let mut nested_numeric_values = Vec::new();
    let mut nested_text_values = Vec::new();

    for (index, arg) in pool_entry.args.iter().enumerate() {
        let name = arg.name.trim();
        let normalized_name = normalize_rule_text(name);
        if normalized_name.is_empty() {
            continue;
        }
        let override_value = override_values
            .get(index)
            .and_then(|value| legacy_text_value(Some(value), text_values));
        let value = override_value.unwrap_or_else(|| {
            if named_values
                .iter()
                .any(|(candidate, _)| candidate == &normalized_name)
                || text_values
                    .iter()
                    .any(|(candidate, _)| candidate == &normalized_name)
            {
                name.to_owned()
            } else {
                arg.value.trim().to_owned()
            }
        });
        if legacy_pool_arg_kind_is_numeric(&arg.kind) {
            if let Some(number) = legacy_numeric_text(&value, named_values) {
                nested_numeric_values.push((normalized_name, number));
            }
        } else if legacy_pool_arg_kind_is_textual(&arg.kind, &value) && !value.trim().is_empty() {
            nested_text_values.push((normalized_name, value));
        }
    }

    (
        nested_numeric_values,
        nested_text_values,
    )
}

fn legacy_pool_arg_kind_is_numeric(kind: &str) -> bool {
    let kind = kind.trim();
    kind.is_empty() || kind.eq_ignore_ascii_case("number") || kind == "数字"
}

fn legacy_pool_arg_kind_is_textual(kind: &str, value: &str) -> bool {
    let kind = kind.trim();
    if kind.is_empty() {
        return value.trim().parse::<f32>().is_err();
    }
    kind.eq_ignore_ascii_case("string")
        || kind.eq_ignore_ascii_case("buff")
        || kind == "字符串"
        || kind == "BUFF"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealType {
    Instant,
    OverTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleBuffTemplate {
    pub name: String,
    pub kind: BuffKind,
    pub priority: i32,
    pub turns_remaining: i32,
    pub beneficial: bool,
    pub effects: Vec<BuffEffect>,
    pub tick_actions: Vec<BuffTickAction>,
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
    pub mp: f32,
    pub max_mp: f32,
    pub hp_regen: f32,
    pub mp_regen: f32,
    pub speed: f32,
    pub status: StatusBlock,
    pub damage_dealt_modifier: f32,
    pub physical_damage_dealt_modifier: f32,
    pub magical_damage_dealt_modifier: f32,
    pub range_damage_dealt_modifier: f32,
    pub physical_damage_lifesteal: f32,
    pub physical_damage_followup_rate: f32,
    pub minimum_damage_floor: f32,
    pub chaos_output_variance: f32,
    pub damage_taken_modifier: f32,
    pub large_hit_damage_taken_modifier: f32,
    pub magical_damage_taken_modifier: f32,
    pub diseased_damage_taken_modifier: f32,
    pub poisoning_damage_taken_modifier: f32,
    pub healing_dealt_modifier: f32,
    pub wounded_healing_dealt_modifier: f32,
    pub mutual_aid_healing_rate: f32,
    pub healing_taken_modifier: f32,
    pub dying_healing_taken_modifier: f32,
    pub damage_taken_this_turn: f32,
    pub healing_taken_this_turn: f32,
    pub damage_dealt_buffs: Vec<BuffSpec>,
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
    pub speed: f32,
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
    pub physical_damage_dealt: f32,
    pub magical_damage_dealt: f32,
    pub range_damage_dealt: f32,
    pub damage_taken: f32,
    pub magical_damage_taken: f32,
    pub diseased_damage_taken: f32,
    pub poisoning_damage_taken: f32,
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

#[derive(Component, Debug, Clone)]
pub struct BuffTickActions(pub Vec<BuffTickAction>);

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tick_actions: Vec<BuffTickAction>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BuffTickAction {
    Damage {
        amount: f32,
        damage_type: DamageType,
    },
    FixedDamage {
        amount: f32,
        damage_type: DamageType,
    },
    Heal {
        amount: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuffField {
    Hp,
    Mp,
    MaxHp,
    MaxMp,
    HpRegen,
    MpRegen,
    Speed,
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
        mp: f32,
        max_mp: f32,
        hp_regen: f32,
        mp_regen: f32,
        speed: f32,
        status: StatusBlock,
        damage_dealt_modifier: f32,
        physical_damage_dealt_modifier: f32,
        magical_damage_dealt_modifier: f32,
        range_damage_dealt_modifier: f32,
        physical_damage_lifesteal: f32,
        physical_damage_followup_rate: f32,
        minimum_damage_floor: f32,
        chaos_output_variance: f32,
        damage_taken_modifier: f32,
        large_hit_damage_taken_modifier: f32,
        magical_damage_taken_modifier: f32,
        diseased_damage_taken_modifier: f32,
        poisoning_damage_taken_modifier: f32,
        healing_dealt_modifier: f32,
        wounded_healing_dealt_modifier: f32,
        mutual_aid_healing_rate: f32,
        healing_taken_modifier: f32,
        dying_healing_taken_modifier: f32,
        damage_dealt_buffs: Vec<BuffSpec>,
        rules: Vec<RuleAst>,
    ) {
        let mut character = Character::new(owner_id, name, max_hp.max(0.0));
        character.hp = hp.clamp(0.0, character.max_hp);
        character.max_mp = max_mp.max(0.0);
        character.mp = mp.clamp(0.0, character.max_mp);
        character.hp_regen = hp_regen;
        character.mp_regen = mp_regen;
        character.speed = speed.max(0.0);
        character.status = status;
        character.damage_dealt_modifier = damage_dealt_modifier;
        character.physical_damage_dealt_modifier = physical_damage_dealt_modifier;
        character.magical_damage_dealt_modifier = magical_damage_dealt_modifier;
        character.range_damage_dealt_modifier = range_damage_dealt_modifier;
        character.physical_damage_lifesteal = physical_damage_lifesteal.max(0.0);
        character.physical_damage_followup_rate = physical_damage_followup_rate.max(0.0);
        character.minimum_damage_floor = minimum_damage_floor.max(0.0);
        character.chaos_output_variance = chaos_output_variance.clamp(0.0, 1.0);
        character.damage_taken_modifier = damage_taken_modifier;
        character.large_hit_damage_taken_modifier = large_hit_damage_taken_modifier.max(0.0);
        character.magical_damage_taken_modifier = magical_damage_taken_modifier;
        character.diseased_damage_taken_modifier = diseased_damage_taken_modifier;
        character.poisoning_damage_taken_modifier = poisoning_damage_taken_modifier;
        character.healing_dealt_modifier = healing_dealt_modifier;
        character.wounded_healing_dealt_modifier = wounded_healing_dealt_modifier.max(0.0);
        character.mutual_aid_healing_rate = mutual_aid_healing_rate.max(0.0);
        character.healing_taken_modifier = healing_taken_modifier;
        character.dying_healing_taken_modifier = dying_healing_taken_modifier.max(0.0);
        character.damage_dealt_buffs = damage_dealt_buffs;
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
            mp: 0.0,
            max_mp: 0.0,
            hp_regen: 0.0,
            mp_regen: 0.0,
            speed: 0.0,
            status: StatusBlock::default(),
            damage_dealt_modifier: 1.0,
            physical_damage_dealt_modifier: 1.0,
            magical_damage_dealt_modifier: 1.0,
            range_damage_dealt_modifier: 1.0,
            physical_damage_lifesteal: 0.0,
            physical_damage_followup_rate: 0.0,
            minimum_damage_floor: 0.0,
            chaos_output_variance: 0.0,
            damage_taken_modifier: 1.0,
            large_hit_damage_taken_modifier: 1.0,
            magical_damage_taken_modifier: 1.0,
            diseased_damage_taken_modifier: 1.0,
            poisoning_damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            wounded_healing_dealt_modifier: 1.0,
            mutual_aid_healing_rate: 0.0,
            healing_taken_modifier: 1.0,
            dying_healing_taken_modifier: 1.0,
            damage_taken_this_turn: 0.0,
            healing_taken_this_turn: 0.0,
            damage_dealt_buffs: Vec::new(),
        }
    }

    fn damage_type_modifier(&self, damage_type: DamageType) -> f32 {
        match damage_type {
            DamageType::Physical => self.physical_damage_dealt_modifier,
            DamageType::Magical => self.magical_damage_dealt_modifier,
            DamageType::Range => self.range_damage_dealt_modifier,
            DamageType::Cursed
            | DamageType::Diseased
            | DamageType::Bleed
            | DamageType::Poisoning
            | DamageType::None => 1.0,
        }
    }

    fn damage_taken_type_modifier(&self, damage_type: DamageType) -> f32 {
        match damage_type {
            DamageType::Magical => self.magical_damage_taken_modifier,
            DamageType::Diseased => self.diseased_damage_taken_modifier,
            DamageType::Poisoning => self.poisoning_damage_taken_modifier,
            DamageType::Physical
            | DamageType::Range
            | DamageType::Cursed
            | DamageType::Bleed
            | DamageType::None => 1.0,
        }
    }

    fn large_hit_damage_taken_multiplier(&self, incoming_damage: f32) -> f32 {
        if self.max_hp > 0.0 && incoming_damage > self.max_hp * 0.2 {
            self.large_hit_damage_taken_modifier
        } else {
            1.0
        }
    }

    fn low_hp_damage_multiplier(&self) -> f32 {
        if self.max_hp <= 0.0 {
            return 0.0;
        }
        let missing_ratio = ((self.max_hp - self.hp) / self.max_hp).clamp(0.0, 1.0);
        if self.hp > self.max_hp * 0.8 {
            1.0
        } else if self.hp > self.max_hp * 0.6 {
            1.0 - 0.1 * missing_ratio
        } else if self.hp > self.max_hp * 0.4 {
            1.0 - 0.5 * missing_ratio
        } else {
            1.0 - missing_ratio
        }
    }

    fn dying_healing_taken_multiplier(&self) -> f32 {
        if self.max_hp > 0.0 && self.hp <= self.max_hp * 0.2 {
            self.dying_healing_taken_modifier
        } else {
            1.0
        }
    }

    fn wounded_healing_dealt_multiplier(&self) -> f32 {
        if self.max_hp <= 0.0 || self.wounded_healing_dealt_modifier <= 1.0 {
            return 1.0;
        }
        if self.hp <= self.max_hp * 0.2 {
            1.0
        } else if self.hp <= self.max_hp * 0.6 {
            1.0 + (self.wounded_healing_dealt_modifier - 1.0) * 0.5
        } else {
            self.wounded_healing_dealt_modifier
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
            Action::GrantBuff { target, buff } => {
                let duration = if buff.turns_remaining == 0 {
                    "永久".to_owned()
                } else {
                    format!("{}回合", buff.turns_remaining)
                };
                format!(
                    "给予{}{}{}状态",
                    target.explain(),
                    duration,
                    buff.name
                )
            },
        }
    }
}

impl RuleBuffTemplate {
    pub fn to_buff_spec(&self, source_id: &str) -> BuffSpec {
        BuffSpec {
            name: self.name.clone(),
            kind: self.kind,
            priority: self.priority,
            turns_remaining: self.turns_remaining,
            source_id: source_id.to_owned(),
            beneficial: self.beneficial,
            effects: self.effects.clone(),
            tick_actions: self.tick_actions.clone(),
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
            mp: character.mp,
            max_mp: character.max_mp,
            hp_regen: character.hp_regen,
            mp_regen: character.mp_regen,
            speed: character.speed,
        };
        let status = character.status.clone();
        let modifiers = CombatModifiers {
            damage_dealt: character.damage_dealt_modifier,
            physical_damage_dealt: character.physical_damage_dealt_modifier,
            magical_damage_dealt: character.magical_damage_dealt_modifier,
            range_damage_dealt: character.range_damage_dealt_modifier,
            damage_taken: character.damage_taken_modifier,
            magical_damage_taken: character.magical_damage_taken_modifier,
            diseased_damage_taken: character.diseased_damage_taken_modifier,
            poisoning_damage_taken: character.poisoning_damage_taken_modifier,
            healing_dealt: character.healing_dealt_modifier,
            healing_taken: character.healing_taken_modifier,
        };

        if let Some(entity) = self.entity_by_id.get(&character.id).copied() {
            let mut entity_mut = self.ecs_world.entity_mut(entity);
            entity_mut.insert((
                combatant.clone(),
                BaseCombatant(combatant),
                status.clone(),
                BaseStatusBlock(status),
                modifiers.clone(),
                BaseCombatModifiers(modifiers),
            ));
        } else {
            let entity = self
                .ecs_world
                .spawn((
                    combatant.clone(),
                    BaseCombatant(combatant),
                    status.clone(),
                    BaseStatusBlock(status),
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
            BuffTickActions(spec.tick_actions),
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
        for character in self.characters.values_mut() {
            character.damage_taken_this_turn = 0.0;
            character.healing_taken_this_turn = 0.0;
        }

        let mut expired = Vec::new();
        let mut changed_targets = HashSet::new();
        let mut tick_actions = Vec::new();
        let id_by_entity = self
            .entity_by_id
            .iter()
            .map(|(id, entity)| (*entity, id.clone()))
            .collect::<HashMap<_, _>>();
        let mut query = self.ecs_world.query::<(
            Entity,
            &BuffOwner,
            &mut ActiveBuff,
            &BuffTickActions,
        )>();
        for (entity, owner, mut buff, ticks) in query.iter_mut(&mut self.ecs_world) {
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
            } else if let Some(target_id) = id_by_entity.get(&owner.target) {
                for tick in &ticks.0 {
                    tick_actions.push((
                        buff.source_id.clone(),
                        target_id.clone(),
                        tick.clone(),
                    ));
                }
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
        for (source_id, target_id, tick) in tick_actions {
            self.apply_buff_tick_action(&source_id, &target_id, tick);
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
            status.clone(),
            modifiers.clone(),
        ));

        if let Some(character) = self.characters.get_mut(character_id) {
            character.hp = combatant.hp;
            character.max_hp = combatant.max_hp;
            character.mp = combatant.mp;
            character.max_mp = combatant.max_mp;
            character.hp_regen = combatant.hp_regen;
            character.mp_regen = combatant.mp_regen;
            character.speed = combatant.speed;
            character.status = status;
            character.damage_dealt_modifier = modifiers.damage_dealt;
            character.physical_damage_dealt_modifier = modifiers.physical_damage_dealt;
            character.magical_damage_dealt_modifier = modifiers.magical_damage_dealt;
            character.range_damage_dealt_modifier = modifiers.range_damage_dealt;
            character.damage_taken_modifier = modifiers.damage_taken;
            character.magical_damage_taken_modifier = modifiers.magical_damage_taken;
            character.diseased_damage_taken_modifier = modifiers.diseased_damage_taken;
            character.poisoning_damage_taken_modifier = modifiers.poisoning_damage_taken;
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
            .map(|character| {
                character.damage_dealt_modifier
                    * character.damage_type_modifier(damage_type)
                    * character.low_hp_damage_multiplier()
                    * chaos_output_multiplier(character.chaos_output_variance)
            })
            .unwrap_or(1.0);
        let target_modifier = self
            .characters
            .get(target_id)
            .map(|character| {
                character.damage_taken_modifier * character.damage_taken_type_modifier(damage_type)
            })
            .unwrap_or(1.0);
        let incoming_damage = (amount * source_modifier * target_modifier).max(0.0);
        let target_large_hit_multiplier = self
            .characters
            .get(target_id)
            .map(|character| character.large_hit_damage_taken_multiplier(incoming_damage))
            .unwrap_or(1.0);
        let typed_final_damage = (incoming_damage * target_large_hit_multiplier).max(0.0);
        let source_minimum_damage_floor = self
            .characters
            .get(source_id)
            .map(|character| character.minimum_damage_floor)
            .unwrap_or(0.0)
            .max(0.0);
        let final_damage = if amount > f32::EPSILON && source_minimum_damage_floor > f32::EPSILON {
            typed_final_damage.max(source_minimum_damage_floor)
        } else {
            typed_final_damage
        };
        let damage_dealt_buffs = self
            .characters
            .get(source_id)
            .map(|character| character.damage_dealt_buffs.clone())
            .unwrap_or_default();
        let source_physical_damage_lifesteal = if damage_type == DamageType::Physical {
            self.characters
                .get(source_id)
                .map(|character| character.physical_damage_lifesteal)
                .unwrap_or(0.0)
                .max(0.0)
        } else {
            0.0
        };
        let source_physical_damage_followup_rate = if damage_type == DamageType::Physical {
            self.characters
                .get(source_id)
                .map(|character| character.physical_damage_followup_rate)
                .unwrap_or(0.0)
                .max(0.0)
        } else {
            0.0
        };

        let mut hp_update = None;
        if let Some(target) = self.characters.get_mut(target_id) {
            target.damage_taken_this_turn += final_damage;
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
        if final_damage > f32::EPSILON {
            for buff in damage_dealt_buffs {
                self.give_or_replace_named_buff(target_id, buff);
            }
            if source_physical_damage_followup_rate > f32::EPSILON {
                self.give_buff(
                    target_id,
                    sousas_claw_followup_buff(
                        source_id,
                        final_damage * source_physical_damage_followup_rate,
                    ),
                );
            }
        }
        if typed_final_damage > f32::EPSILON && source_physical_damage_lifesteal > f32::EPSILON {
            let lifesteal_amount = typed_final_damage * source_physical_damage_lifesteal;
            let mut hp_update = None;
            if let Some(source) = self.characters.get_mut(source_id) {
                source.healing_taken_this_turn += lifesteal_amount;
                source.hp = (source.hp + lifesteal_amount).min(source.max_hp);
                hp_update = Some((source.hp, source.max_hp));
                self.log.push(format!(
                    "{}吸血回复{}点生命值，生命值变为 {}/{}",
                    source.name,
                    format_number(lifesteal_amount),
                    format_number(source.hp),
                    format_number(source.max_hp)
                ));
            }
            if let Some((hp, max_hp)) = hp_update {
                self.sync_character_hp_to_ecs(source_id, hp, max_hp);
            }
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

    fn fixed_damage(
        &mut self,
        source_id: &str,
        target_id: &str,
        amount: f32,
        damage_type: DamageType,
    ) {
        let final_damage = amount.max(0.0);
        let mut hp_update = None;
        if let Some(target) = self.characters.get_mut(target_id) {
            target.damage_taken_this_turn += final_damage;
            target.hp = (target.hp - final_damage).max(0.0);
            hp_update = Some((target.hp, target.max_hp));
            self.log.push(format!(
                "{}受到{}点{}伤害，生命值变为 {}/{}",
                target.name,
                format_number(final_damage),
                damage_type.explain(),
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
            .map(|character| {
                character.healing_dealt_modifier
                    * character.wounded_healing_dealt_multiplier()
                    * chaos_output_multiplier(character.chaos_output_variance)
            })
            .unwrap_or(1.0);
        let target_modifier = self
            .characters
            .get(target_id)
            .map(|character| {
                character.healing_taken_modifier * character.dying_healing_taken_multiplier()
            })
            .unwrap_or(1.0);
        let final_heal = (amount * source_modifier * target_modifier).max(0.0);
        let mutual_aid_heal = if source_id != target_id && final_heal > f32::EPSILON {
            let source_rate = self
                .characters
                .get(source_id)
                .map(|character| character.mutual_aid_healing_rate)
                .unwrap_or(0.0)
                .max(0.0);
            let target_rate = self
                .characters
                .get(target_id)
                .map(|character| character.mutual_aid_healing_rate)
                .unwrap_or(0.0)
                .max(0.0);
            final_heal * (source_rate + target_rate)
        } else {
            0.0
        };

        let mut hp_update = None;
        if let Some(target) = self.characters.get_mut(target_id) {
            target.healing_taken_this_turn += final_heal;
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
        if mutual_aid_heal > f32::EPSILON {
            let mut hp_update = None;
            if let Some(source) = self.characters.get_mut(source_id) {
                source.healing_taken_this_turn += mutual_aid_heal;
                source.hp = (source.hp + mutual_aid_heal).min(source.max_hp);
                hp_update = Some((source.hp, source.max_hp));
                self.log.push(format!(
                    "{}触发互帮互助，回复{}点生命值，生命值变为 {}/{}",
                    source.name,
                    format_number(mutual_aid_heal),
                    format_number(source.hp),
                    format_number(source.max_hp)
                ));
            }
            if let Some((hp, max_hp)) = hp_update {
                self.sync_character_hp_to_ecs(source_id, hp, max_hp);
            }
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
        let action_explain = action.explain();
        match action {
            Action::Heal { target, amount } => {
                let target_ids = resolve_targets(target, owner_id, event);
                let source_id = owner_id.to_owned();
                self.log.push(format!(
                    "规则触发：{} -> {}",
                    rule_event_name(event),
                    action_explain
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
                    action_explain
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
            Action::GrantBuff { target, buff } => {
                let target_ids = resolve_targets(target, owner_id, event);
                self.log.push(format!(
                    "规则触发：{} -> {}",
                    rule_event_name(event),
                    action_explain
                ));
                for target_id in target_ids {
                    self.give_buff(&target_id, buff.to_buff_spec(owner_id));
                }
            },
        }
    }

    fn apply_buff_tick_action(&mut self, source_id: &str, target_id: &str, action: BuffTickAction) {
        match action {
            BuffTickAction::Damage {
                amount,
                damage_type,
            } => {
                self.log.push(format!(
                    "BUFF触发：{}对{}造成{}点{}伤害",
                    source_id,
                    target_id,
                    format_number(amount.max(0.0)),
                    damage_type.explain()
                ));
                self.attack(
                    source_id,
                    target_id,
                    amount.max(0.0),
                    damage_type,
                );
            },
            BuffTickAction::FixedDamage {
                amount,
                damage_type,
            } => {
                self.log.push(format!(
                    "BUFF触发：{}对{}造成{}点{}伤害",
                    source_id,
                    target_id,
                    format_number(amount.max(0.0)),
                    damage_type.explain()
                ));
                self.fixed_damage(
                    source_id,
                    target_id,
                    amount.max(0.0),
                    damage_type,
                );
            },
            BuffTickAction::Heal { amount } => {
                self.log.push(format!(
                    "BUFF触发：{}治疗{}{}点生命值",
                    source_id,
                    target_id,
                    format_number(amount.max(0.0))
                ));
                self.heal(source_id, target_id, amount.max(0.0));
            },
        }
    }

    fn give_or_replace_named_buff(&mut self, target_id: &str, spec: BuffSpec) -> bool {
        let Some(target) = self.entity_by_id.get(target_id).copied() else {
            return false;
        };
        let existing = self
            .ecs_world
            .query::<(Entity, &BuffOwner, &ActiveBuff)>()
            .iter(&self.ecs_world)
            .filter_map(|(entity, owner, buff)| {
                (owner.target == target
                    && buff.name == spec.name
                    && buff.source_id == spec.source_id)
                    .then_some(entity)
            })
            .collect::<Vec<_>>();
        for entity in existing {
            let _ = self.ecs_world.despawn(entity);
        }
        self.give_buff(target_id, spec)
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
        BuffField::Speed => apply_f32(&mut combatant.speed, effect.value),
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
    parse_rule_with_named_values(input, &[])
}

pub fn parse_rule_with_named_values(
    input: &str,
    named_values: &[(String, f32)],
) -> Result<RuleAst, String> {
    parse_rule_with_named_args(input, named_values, &[])
}

pub fn parse_rule_with_named_args(
    input: &str,
    named_values: &[(String, f32)],
    text_values: &[(String, String)],
) -> Result<RuleAst, String> {
    let normalized = apply_named_text_values(
        normalize_rule_text(input),
        &normalize_named_text_values(text_values),
    );
    let named_values = normalize_named_values(named_values);
    if normalized.is_empty() {
        return Err("规则为空".to_owned());
    }
    if is_active_skill_rule(&normalized) {
        let actions = parse_actions(&normalized, &named_values)?;
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
    let actions = parse_actions(&normalized, &named_values)?;
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

fn normalize_named_values(named_values: &[(String, f32)]) -> Vec<(String, f32)> {
    let mut named_values = named_values
        .iter()
        .filter_map(|(name, value)| {
            let name = normalize_rule_text(name);
            (!name.is_empty()).then_some((name, *value))
        })
        .collect::<Vec<_>>();
    named_values.sort_by(|left, right| right.0.len().cmp(&left.0.len()));
    named_values
}

fn normalize_named_text_values(text_values: &[(String, String)]) -> Vec<(String, String)> {
    let mut text_values = text_values
        .iter()
        .filter_map(|(name, value)| {
            let name = normalize_rule_text(name);
            let value = normalize_rule_text(value);
            (!name.is_empty() && !value.is_empty()).then_some((name, value))
        })
        .collect::<Vec<_>>();
    text_values.sort_by(|left, right| right.0.len().cmp(&left.0.len()));
    text_values
}

fn apply_named_text_values(mut text: String, text_values: &[(String, String)]) -> String {
    for (name, value) in text_values {
        text = text.replace(name, value);
    }
    text
}

fn parse_actions(text: &str, named_values: &[(String, f32)]) -> Result<Vec<Action>, String> {
    let action_text = action_clause(text);
    let mut actions = Vec::new();

    for clause in action_text.split(['，', ',', '；', ';']) {
        if let Some(action) = parse_grant_buff_action(clause) {
            actions.push(action);
        }

        if let Some(amount) = parse_value_before(clause, "点生命值", named_values).or_else(|| {
            parse_value_after_action(
                clause,
                &["回复", "恢复", "治疗"],
                named_values,
            )
        }) {
            if clause.contains("回复") || clause.contains("恢复") || clause.contains("治疗") {
                actions.push(Action::Heal {
                    target: parse_target_selector(clause, ActorRef::SelfActor),
                    amount,
                });
            }
        }

        if let Some(amount) = parse_value_before(clause, "点伤害", named_values)
            .or_else(|| parse_value_after_action(clause, &["造成"], named_values))
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

fn parse_grant_buff_action(clause: &str) -> Option<Action> {
    if !grant_buff_words().iter().any(|word| clause.contains(word)) {
        return None;
    }
    let name = parse_buff_name(clause)?;
    let default_target = if clause.contains("获得")
        && !clause.contains("目标")
        && !clause.contains("来源")
        && !clause.contains("攻击者")
    {
        ActorRef::SelfActor
    } else {
        ActorRef::Target
    };

    Some(Action::GrantBuff {
        target: parse_target_selector(clause, default_target),
        buff: RuleBuffTemplate {
            name,
            kind: parse_buff_kind(clause),
            priority: 0,
            turns_remaining: parse_buff_turns(clause),
            beneficial: parse_buff_beneficial(clause),
            effects: parse_buff_effects(clause),
            tick_actions: Vec::new(),
        },
    })
}

fn grant_buff_words() -> [&'static str; 5] { ["给予", "施加", "附加", "添加", "获得"] }

fn parse_buff_name(clause: &str) -> Option<String> {
    let tail = if let Some(index) = clause.find("回合") {
        &clause[index + "回合".len()..]
    } else if let Some(index) = clause.find("永久") {
        &clause[index + "永久".len()..]
    } else {
        let (_, tail) = grant_buff_words()
            .iter()
            .filter_map(|word| {
                clause.find(word).map(|index| {
                    (
                        index + word.len(),
                        &clause[index + word.len()..],
                    )
                })
            })
            .min_by_key(|(index, _)| *index)?;
        tail
    };

    let effect_start = buff_effect_start_index(tail).unwrap_or(tail.len());
    let mut name = tail[..effect_start]
        .trim_matches(['的', '：', ':'])
        .trim()
        .to_owned();
    for prefix in ["正面", "负面", "增益", "减益"] {
        if let Some(stripped) = name.strip_prefix(prefix) {
            name = stripped.to_owned();
        }
    }
    for suffix in ["状态效果", "状态", "效果", "buff", "Buff", "BUFF"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.to_owned();
        }
    }
    let name = name.trim().to_owned();
    (!name.is_empty()).then_some(name)
}

fn buff_effect_start_index(text: &str) -> Option<usize> {
    ["使", "并", "且", "，", ",", "；", ";"]
        .iter()
        .filter_map(|marker| text.find(marker))
        .chain(
            buff_effect_field_patterns()
                .into_iter()
                .filter_map(|(_, labels)| labels.iter().filter_map(|label| text.find(label)).min()),
        )
        .min()
}

fn parse_buff_turns(clause: &str) -> i32 {
    if clause.contains("永久") {
        return 0;
    }
    clause
        .find("回合")
        .and_then(|index| parse_trailing_number(&clause[..index]))
        .map(|value| value.round().max(0.0) as i32)
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn parse_buff_kind(clause: &str) -> BuffKind {
    match parse_damage_type(clause) {
        DamageType::Cursed => BuffKind::Curse,
        DamageType::Diseased => BuffKind::Disease,
        DamageType::Bleed => BuffKind::Bleed,
        DamageType::Range => BuffKind::Range,
        DamageType::Poisoning => BuffKind::Poison,
        DamageType::Physical => BuffKind::Physical,
        DamageType::Magical => BuffKind::Magic,
        DamageType::None => BuffKind::None,
    }
}

fn parse_buff_beneficial(clause: &str) -> bool {
    ![
        "负面", "减益", "诅咒", "疾病", "流血", "中毒", "虚弱", "脆弱",
    ]
    .iter()
    .any(|word| clause.contains(word))
}

fn parse_buff_effects(clause: &str) -> Vec<BuffEffect> {
    let mut effects = Vec::new();
    for (field, labels) in buff_effect_field_patterns() {
        let Some((index, label)) = labels
            .iter()
            .filter_map(|label| clause.find(label).map(|index| (index, *label)))
            .min_by_key(|(index, _)| *index)
        else {
            continue;
        };
        if let Some(value) = parse_buff_value_after_field(&clause[index + label.len()..]) {
            effects.push(BuffEffect { field, value });
        }
    }
    effects
}

fn buff_effect_field_patterns() -> Vec<(BuffField, &'static [&'static str])> {
    vec![
        (BuffField::DamageTakenModifier, &[
            "承伤",
            "受到伤害",
            "受伤",
        ]),
        (BuffField::DamageDealtModifier, &[
            "造成伤害",
            "伤害倍率",
        ]),
        (BuffField::HealingTakenModifier, &[
            "受疗",
            "受到治疗",
        ]),
        (BuffField::HealingDealtModifier, &[
            "造成治疗",
            "治疗倍率",
        ]),
        (BuffField::MaxHp, &[
            "最大HP",
            "生命上限",
            "最大生命",
        ]),
        (BuffField::MaxMp, &[
            "最大MP",
            "法力上限",
            "最大法力",
        ]),
        (BuffField::HpRegen, &[
            "HP回复",
            "生命回复",
        ]),
        (BuffField::MpRegen, &[
            "MP回复",
            "法力回复",
            "回蓝",
        ]),
        (BuffField::Speed, &[
            "速度",
            "移速",
            "移动速度",
            "speed",
        ]),
        (BuffField::Status(StatusKey::Str), &[
            "力量", "STR", "str",
        ]),
        (BuffField::Status(StatusKey::Agi), &[
            "敏捷", "AGI", "agi",
        ]),
        (BuffField::Status(StatusKey::Dex), &[
            "灵巧", "DEX", "dex",
        ]),
        (BuffField::Status(StatusKey::Vit), &[
            "体质", "VIT", "vit",
        ]),
        (BuffField::Status(StatusKey::Int), &[
            "智力", "INT", "int",
        ]),
        (BuffField::Status(StatusKey::Wis), &[
            "智慧", "WIS", "wis",
        ]),
        (BuffField::Status(StatusKey::K), &[
            "知识", "K",
        ]),
        (BuffField::Status(StatusKey::Cha), &[
            "魅力", "CHA", "cha",
        ]),
    ]
}

fn parse_buff_value_after_field(text: &str) -> Option<BuffValue> {
    let text = text.trim_start_matches([' ', '　', '：', ':', '为']);
    for marker in ["设为", "变为", "="] {
        if let Some(index) = text.find(marker) {
            let tail = &text[index + marker.len()..];
            return parse_first_signed_number(tail).map(|(value, _)| BuffValue::Set(value));
        }
    }

    let (mut value, percent) = parse_first_signed_number(text)?;
    if text.contains("降低") || text.contains("减少") || text.contains("下降") {
        value = -value.abs();
    }

    if percent {
        Some(BuffValue::AddPercent(value))
    } else {
        Some(BuffValue::Add(value))
    }
}

fn parse_first_signed_number(text: &str) -> Option<(f32, bool)> {
    for (start, character) in text.char_indices() {
        let signed = character == '+' || character == '-';
        if !signed && !character.is_ascii_digit() && character != '.' {
            continue;
        }

        let mut end = start + character.len_utf8();
        let mut saw_digit = character.is_ascii_digit();
        let tail_start = end;
        for (offset, next) in text[tail_start..].char_indices() {
            if next.is_ascii_digit() {
                saw_digit = true;
                end = tail_start + offset + next.len_utf8();
            } else if next == '.' {
                end = tail_start + offset + next.len_utf8();
            } else {
                break;
            }
        }
        if !saw_digit {
            continue;
        }
        let value = text[start..end].parse().ok()?;
        let percent = text[end..].trim_start().starts_with(['%', '％']);
        return Some((value, percent));
    }
    None
}

fn action_clause(text: &str) -> &str {
    if is_active_skill_rule(text) {
        return text;
    }
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
    [
        "回复", "恢复", "治疗", "造成", "给予", "施加", "附加", "添加", "获得",
    ]
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

fn parse_value_after_action(
    clause: &str,
    actions: &[&str],
    named_values: &[(String, f32)],
) -> Option<ValueExpr> {
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
        if let Some(value) = parse_named_value_after_action(tail, named_values) {
            return Some(ValueExpr::Number(value));
        }
    }
    None
}

fn parse_value_before(
    clause: &str,
    marker: &str,
    named_values: &[(String, f32)],
) -> Option<ValueExpr> {
    let marker_index = clause.find(marker)?;
    let before_marker = &clause[..marker_index];
    if before_marker.contains("本次伤害") || before_marker.contains("此次伤害") {
        return Some(ValueExpr::EventDamage);
    }
    parse_trailing_number(before_marker)
        .or_else(|| parse_named_value_before_marker(before_marker, named_values))
        .map(ValueExpr::Number)
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

fn parse_named_value_after_action(text: &str, named_values: &[(String, f32)]) -> Option<f32> {
    named_values
        .iter()
        .filter_map(|(name, value)| text.find(name).map(|index| (index, name.len(), *value)))
        .min_by_key(|(index, len, _)| (*index, std::cmp::Reverse(*len)))
        .map(|(_, _, value)| value)
}

fn parse_named_value_before_marker(text: &str, named_values: &[(String, f32)]) -> Option<f32> {
    named_values
        .iter()
        .filter_map(|(name, value)| text.rfind(name).map(|index| (index, name.len(), *value)))
        .max_by_key(|(index, len, _)| (*index, *len))
        .map(|(_, _, value)| value)
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

fn chaos_output_multiplier(variance: f32) -> f32 {
    let variance = variance.clamp(0.0, 1.0);
    if variance <= f32::EPSILON {
        1.0
    } else {
        rand::rng().random_range((1.0 - variance)..=(1.0 + variance))
    }
}

fn sousas_claw_followup_buff(source_id: &str, amount: f32) -> BuffSpec {
    BuffSpec {
        name: "苏萨斯之爪".to_owned(),
        kind: BuffKind::Magic,
        priority: 0,
        turns_remaining: 2,
        source_id: source_id.to_owned(),
        beneficial: false,
        effects: Vec::new(),
        tick_actions: vec![BuffTickAction::FixedDamage {
            amount: amount.max(0.0),
            damage_type: DamageType::Magical,
        }],
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

            ui.label("状态动作");
            ui.label("给予, 施加, 获得, N回合, 永久, 状态");
            ui.end_row();

            ui.label("状态效果");
            ui.label("承伤设为0.5, 力量+2, 伤害提高50%");
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
    ui.monospace("每当自己受到伤害时，给予自己2回合守护状态");
    ui.monospace("每当自己受到伤害时，给予自己2回合守护状态使承伤设为0.5");
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
    fn parses_active_skill_actions_before_and_after_comma() {
        let ast = parse_rule("主动使用对目标造成3点物理伤害，对目标回复2点生命值").unwrap();

        assert_eq!(ast.actions, vec![
            Action::Damage {
                target: TargetSelector::single(ActorRef::Target),
                amount: ValueExpr::Number(3.0),
                damage_type: DamageType::Physical,
            },
            Action::Heal {
                target: TargetSelector::single(ActorRef::Target),
                amount: ValueExpr::Number(2.0),
            },
        ]);
    }

    #[test]
    fn parses_skill_amount_from_named_numeric_value() {
        let values = vec![("伤害值".to_owned(), 3.0)];
        let ast = parse_rule_with_named_values(
            "主动使用对目标造成伤害值点物理伤害",
            &values,
        )
        .unwrap();

        assert_eq!(ast.actions, vec![Action::Damage {
            target: TargetSelector::single(ActorRef::Target),
            amount: ValueExpr::Number(3.0),
            damage_type: DamageType::Physical,
        }]);
    }

    #[test]
    fn parses_skill_heal_from_named_numeric_value() {
        let values = vec![("治疗量".to_owned(), 5.0)];
        let ast = parse_rule_with_named_values(
            "主动使用对目标回复治疗量点生命值",
            &values,
        )
        .unwrap();

        assert_eq!(ast.actions, vec![Action::Heal {
            target: TargetSelector::single(ActorRef::Target),
            amount: ValueExpr::Number(5.0),
        }]);
    }

    #[test]
    fn parses_skill_damage_type_from_named_text_value() {
        let ast = parse_rule_with_named_args(
            "主动使用对目标造成2点伤害类型伤害",
            &[],
            &[("伤害类型".to_owned(), "远程".to_owned())],
        )
        .unwrap();

        assert_eq!(ast.actions, vec![Action::Damage {
            target: TargetSelector::single(ActorRef::Target),
            amount: ValueExpr::Number(2.0),
            damage_type: DamageType::Range,
        }]);
    }

    #[test]
    fn parses_grant_buff_name_from_named_text_value() {
        let ast = parse_rule_with_named_args(
            "主动使用给予目标2回合状态名状态使承伤设为0.5",
            &[],
            &[("状态名".to_owned(), "守护".to_owned())],
        )
        .unwrap();

        assert_eq!(ast.actions, vec![Action::GrantBuff {
            target: TargetSelector::single(ActorRef::Target),
            buff: RuleBuffTemplate {
                name: "守护".to_owned(),
                kind: BuffKind::None,
                priority: 0,
                turns_remaining: 2,
                beneficial: true,
                effects: vec![BuffEffect {
                    field: BuffField::DamageTakenModifier,
                    value: BuffValue::Set(0.5),
                }],
                tick_actions: Vec::new(),
            },
        }]);
    }

    #[test]
    fn parses_old_skill_target_sentinel_wording() {
        let ast = parse_rule("主动使用对技能目标造成2点物理伤害").unwrap();

        assert_eq!(ast.actions, vec![Action::Damage {
            target: TargetSelector::single(ActorRef::Target),
            amount: ValueExpr::Number(2.0),
            damage_type: DamageType::Physical,
        }]);
    }

    #[test]
    fn skill_type_default_fills_untyped_damage_only() {
        let ast = parse_rule("主动使用对目标造成2点伤害").unwrap();
        let ast = apply_skill_type_damage_default(ast, Some("远程"));

        assert_eq!(ast.actions, vec![Action::Damage {
            target: TargetSelector::single(ActorRef::Target),
            amount: ValueExpr::Number(2.0),
            damage_type: DamageType::Range,
        }]);

        let explicit = parse_rule("主动使用对目标造成2点物理伤害").unwrap();
        let explicit = apply_skill_type_damage_default(explicit, Some("远程"));

        assert_eq!(explicit.actions, vec![Action::Damage {
            target: TargetSelector::single(ActorRef::Target),
            amount: ValueExpr::Number(2.0),
            damage_type: DamageType::Physical,
        }]);
    }

    #[test]
    fn legacy_moonberry_buff_machine_converts_active_damage() {
        let ast = legacy_moonberry_buff_machine_skill_cast_rule(
            r#"{"技能释放":[{"name":"火球","effect":["伤害"],"type":7,"from":"技能目标","value":["伤害值"]}]}"#,
            &[("伤害值".to_owned(), 4.0)],
            Some("远程"),
        )
        .unwrap();

        assert_eq!(ast.trigger.event, EventKind::SkillCast);
        assert_eq!(ast.actions, vec![Action::Damage {
            target: TargetSelector::single(ActorRef::Target),
            amount: ValueExpr::Number(4.0),
            damage_type: DamageType::Range,
        }]);
    }

    #[test]
    fn legacy_moonberry_buff_machine_converts_active_basic_buff() {
        let ast = legacy_moonberry_buff_machine_skill_cast_rule(
            r#"{"buffMachine":{"技能释放":[{"name":"守护","prior":2,"life":3,"effect":["tDMGModify","str"],"type":0,"from":"自己","benifit":true,"value":["=50%","力量"]}]}}"#,
            &[("力量".to_owned(), 2.0)],
            None,
        )
        .unwrap();

        assert_eq!(ast.actions, vec![Action::GrantBuff {
            target: TargetSelector::single(ActorRef::SelfActor),
            buff: RuleBuffTemplate {
                name: "守护".to_owned(),
                kind: BuffKind::Magic,
                priority: 2,
                turns_remaining: 3,
                beneficial: true,
                effects: vec![
                    BuffEffect {
                        field: BuffField::DamageTakenModifier,
                        value: BuffValue::SetPercentOfBase(50.0),
                    },
                    BuffEffect {
                        field: BuffField::Status(StatusKey::Str),
                        value: BuffValue::Add(2.0),
                    },
                ],
                tick_actions: Vec::new(),
            },
        }]);
    }

    #[test]
    fn legacy_moonberry_buff_machine_converts_passive_basic_buffs() {
        let buffs = legacy_moonberry_buff_machine_passive_buffs(
            r#"{"eventBuffs":[{"event":"被动","buffs":[{"name":"强壮","prior":4,"effect":["str","DMGModify","speed"],"type":1,"benifit":true,"value":["力量","25%","20%"]}]}]}"#,
            &[("力量".to_owned(), 3.0)],
            "alice:skill:0",
        );

        assert_eq!(buffs, vec![BuffSpec {
            name: "强壮".to_owned(),
            kind: BuffKind::Physical,
            priority: 4,
            turns_remaining: 0,
            source_id: "alice:skill:0".to_owned(),
            beneficial: true,
            effects: vec![
                BuffEffect {
                    field: BuffField::Status(StatusKey::Str),
                    value: BuffValue::Add(3.0),
                },
                BuffEffect {
                    field: BuffField::DamageDealtModifier,
                    value: BuffValue::AddPercent(25.0),
                },
                BuffEffect {
                    field: BuffField::Speed,
                    value: BuffValue::AddPercent(20.0),
                },
            ],
            tick_actions: Vec::new(),
        }]);
    }

    #[test]
    fn legacy_moonberry_buff_machine_resolves_granted_buff_pool() {
        let pools = vec![LegacyMoonberryPoolEntry {
            id: Some("shield-pool".to_owned()),
            name: "护盾池".to_owned(),
            legacy_json: r#"{"eventBuffs":[{"event":"技能释放","buffs":[{"name":"护盾","prior":2,"life":1,"effect":["tDMGModify"],"type":0,"from":"技能目标","benifit":true,"value":["护盾值"]}]}]}"#.to_owned(),
            args: vec![LegacyMoonberryPoolArg {
                name: "护盾值".to_owned(),
                kind: "数字".to_owned(),
                value: "0.5".to_owned(),
            }],
        }];
        let ast = legacy_moonberry_buff_machine_skill_cast_rule_with_context(
            r#"{"eventBuffs":[{"event":"技能释放","buffs":[{"name":"给予护盾","life":3,"effect":["给予BUFF"],"type":0,"from":"技能目标","benifit":true,"value":["护盾池","减伤"]}]}]}"#,
            &[("减伤".to_owned(), 0.25)],
            &[("护盾池".to_owned(), "shield-pool".to_owned())],
            None,
            &pools,
        )
        .unwrap();

        assert_eq!(ast.actions, vec![Action::GrantBuff {
            target: TargetSelector::single(ActorRef::Target),
            buff: RuleBuffTemplate {
                name: "护盾".to_owned(),
                kind: BuffKind::Magic,
                priority: 2,
                turns_remaining: 3,
                beneficial: true,
                effects: vec![BuffEffect {
                    field: BuffField::DamageTakenModifier,
                    value: BuffValue::Add(0.25),
                }],
                tick_actions: Vec::new(),
            },
        }]);
    }

    #[test]
    fn legacy_moonberry_granted_buff_pool_ticks_damage_on_turn_advance() {
        let pools = vec![LegacyMoonberryPoolEntry {
            id: Some("burn-pool".to_owned()),
            name: "燃烧池".to_owned(),
            legacy_json: r#"{"eventBuffs":[{"event":"技能释放","buffs":[{"name":"燃烧伤害","effect":["伤害"],"from":"技能目标","value":["伤害值"]}]}]}"#.to_owned(),
            args: vec![LegacyMoonberryPoolArg {
                name: "伤害值".to_owned(),
                kind: "数字".to_owned(),
                value: "1".to_owned(),
            }],
        }];
        let ast = legacy_moonberry_buff_machine_skill_cast_rule_with_context(
            r#"{"eventBuffs":[{"event":"技能释放","buffs":[{"name":"燃烧","life":2,"effect":["给予BUFF"],"type":0,"from":"技能目标","benifit":false,"value":["燃烧池","伤害值"]}]}]}"#,
            &[("伤害值".to_owned(), 3.0)],
            &[("燃烧池".to_owned(), "burn-pool".to_owned())],
            None,
            &pools,
        )
        .unwrap();

        assert_eq!(ast.actions, vec![Action::GrantBuff {
            target: TargetSelector::single(ActorRef::Target),
            buff: RuleBuffTemplate {
                name: "燃烧".to_owned(),
                kind: BuffKind::Magic,
                priority: 0,
                turns_remaining: 2,
                beneficial: false,
                effects: Vec::new(),
                tick_actions: vec![BuffTickAction::Damage {
                    amount: 3.0,
                    damage_type: DamageType::Magical,
                }],
            },
        }]);

        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("caster", "施法者", 10.0));
        engine.add_character(Character::new("target", "目标", 10.0));
        engine.add_rule("caster", ast);

        engine.cast_skill("caster", vec!["target".to_owned()]);
        assert_eq!(
            engine.active_buff_names("target"),
            vec!["燃烧".to_owned()]
        );

        engine.advance_turn();
        let target = engine.characters.get("target").unwrap();
        assert_eq!(target.hp, 7.0);
        assert_eq!(target.damage_taken_this_turn, 3.0);
        assert_eq!(
            engine.active_buff_names("target"),
            vec!["燃烧".to_owned()]
        );

        engine.advance_turn();
        let target = engine.characters.get("target").unwrap();
        assert_eq!(target.hp, 7.0);
        assert_eq!(target.damage_taken_this_turn, 0.0);
        assert!(engine.active_buff_names("target").is_empty());
    }

    #[test]
    fn legacy_moonberry_graph_resolves_buff_variable_pool() {
        let pools = vec![LegacyMoonberryPoolEntry {
            id: Some("might-pool".to_owned()),
            name: "力量池".to_owned(),
            legacy_json: r#"{"eventBuffs":[{"event":"技能释放","buffs":[{"name":"力量祝福","prior":1,"life":1,"effect":["str"],"type":1,"from":"技能目标","benifit":true,"value":["力量"]}]}]}"#.to_owned(),
            args: vec![LegacyMoonberryPoolArg {
                name: "力量".to_owned(),
                kind: "数字".to_owned(),
                value: "1".to_owned(),
            }],
        }];
        let ast = legacy_moonberry_buff_machine_skill_cast_rule_with_context(
            r#"{"eventBuffs":[{}],"graph":{"cells":[
                {"id":"event","type":"event","component":"技能释放"},
                {"id":"grant","type":"function","component":"给予BUFF"},
                {"id":"buff-var","type":"var","component":"BUFF变量","name":"BUFF选择"},
                {"id":"duration","type":"var","component":"数字变量","name":"持续轮次"},
                {"id":"exec","type":"exec","source":{"cell":"event","port":":execOut:0"},"target":{"cell":"grant","port":":execIn:0"}},
                {"id":"target","type":"target","source":{"cell":"event","port":"技能目标:targetOut:1"},"target":{"cell":"grant","port":"目标:targetIn:1"}},
                {"id":"buff-edge","type":"buff","source":{"cell":"buff-var","port":"BUFF:buffOut:0"},"target":{"cell":"grant","port":"BUFF:buffIn:2"}},
                {"id":"duration-edge","type":"number","source":{"cell":"duration","port":"持续轮次:numberOut:0"},"target":{"cell":"grant","port":"持续轮次:numberIn:3"}}
            ]}}"#,
            &[("力量".to_owned(), 4.0), ("持续轮次".to_owned(), 2.0)],
            &[("BUFF选择".to_owned(), "might-pool".to_owned())],
            None,
            &pools,
        )
        .unwrap();

        assert_eq!(ast.actions, vec![Action::GrantBuff {
            target: TargetSelector::single(ActorRef::Target),
            buff: RuleBuffTemplate {
                name: "力量祝福".to_owned(),
                kind: BuffKind::Physical,
                priority: 1,
                turns_remaining: 2,
                beneficial: true,
                effects: vec![BuffEffect {
                    field: BuffField::Status(StatusKey::Str),
                    value: BuffValue::Add(4.0),
                }],
                tick_actions: Vec::new(),
            },
        }]);
    }

    #[test]
    fn legacy_moonberry_graph_converts_skill_cast_damage_when_event_buffs_empty() {
        let ast = legacy_moonberry_buff_machine_skill_cast_rule(
            r#"{"eventBuffs":[{}],"graph":{"cells":[
                {"id":"event","type":"event","component":"技能释放"},
                {"id":"damage","type":"function","component":"伤害"},
                {"id":"amount","type":"var","component":"数字变量","name":"伤害值"},
                {"id":"exec","type":"exec","source":{"cell":"event","port":":execOut:0"},"target":{"cell":"damage","port":":execIn:0"}},
                {"id":"target","type":"target","source":{"cell":"event","port":"技能目标:targetOut:1"},"target":{"cell":"damage","port":"目标:targetIn:1"}},
                {"id":"amount-edge","type":"number","source":{"cell":"amount","port":"数字:numberOut:0"},"target":{"cell":"damage","port":"伤害:numberIn:2"}}
            ]}}"#,
            &[("伤害值".to_owned(), 5.0)],
            Some("动作"),
        )
        .unwrap();

        assert_eq!(ast.trigger.event, EventKind::SkillCast);
        assert_eq!(ast.actions, vec![Action::Damage {
            target: TargetSelector::single(ActorRef::Target),
            amount: ValueExpr::Number(5.0),
            damage_type: DamageType::Physical,
        }]);
    }

    #[test]
    fn legacy_moonberry_graph_converts_passive_basic_buff_when_event_buffs_empty() {
        let buffs = legacy_moonberry_buff_machine_passive_buffs(
            r#"{"eventBuffs":[{}],"graph":{"cells":[
                {"id":"event","type":"event","component":"被动"},
                {"id":"strength","type":"function","component":"设置力量"},
                {"id":"value","type":"var","component":"字符串变量","name":"力量"},
                {"id":"exec","type":"exec","source":{"cell":"event","port":":execOut:0"},"target":{"cell":"strength","port":":execIn:0"}},
                {"id":"target","type":"target","source":{"cell":"event","port":"自己:targetOut:1"},"target":{"cell":"strength","port":"目标:targetIn:1"}},
                {"id":"value-edge","type":"string","source":{"cell":"value","port":"字符串:stringOut:0"},"target":{"cell":"strength","port":"力量:stringIn:2"}}
            ]}}"#,
            &[("力量".to_owned(), 3.0)],
            "alice:skill:0",
        );

        assert_eq!(buffs, vec![BuffSpec {
            name: "设置力量".to_owned(),
            kind: BuffKind::None,
            priority: 0,
            turns_remaining: 0,
            source_id: "alice:skill:0".to_owned(),
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::Status(StatusKey::Str),
                value: BuffValue::Add(3.0),
            }],
            tick_actions: Vec::new(),
        }]);
    }

    #[test]
    fn parses_grant_buff_rule() {
        let ast = parse_rule("每当自己受到伤害时，给予自己2回合守护状态").unwrap();

        assert_eq!(
            ast.trigger.event,
            EventKind::DamageTaken
        );
        assert_eq!(ast.actions, vec![Action::GrantBuff {
            target: TargetSelector::single(ActorRef::SelfActor),
            buff: RuleBuffTemplate {
                name: "守护".to_owned(),
                kind: BuffKind::None,
                priority: 0,
                turns_remaining: 2,
                beneficial: true,
                effects: Vec::new(),
                tick_actions: Vec::new(),
            },
        }]);
        assert_eq!(
            ast.explain(),
            "触发：每当自己受到伤害。\n动作：给予自己2回合守护状态。"
        );
    }

    #[test]
    fn parses_grant_buff_rule_with_typed_effects() {
        let ast = parse_rule("每当自己受到伤害时，给予自己2回合守护状态使承伤设为0.5").unwrap();

        assert_eq!(ast.actions, vec![Action::GrantBuff {
            target: TargetSelector::single(ActorRef::SelfActor),
            buff: RuleBuffTemplate {
                name: "守护".to_owned(),
                kind: BuffKind::None,
                priority: 0,
                turns_remaining: 2,
                beneficial: true,
                effects: vec![BuffEffect {
                    field: BuffField::DamageTakenModifier,
                    value: BuffValue::Set(0.5),
                }],
                tick_actions: Vec::new(),
            },
        }]);
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
    fn grant_buff_rule_adds_active_buff_and_expires_by_turn() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));
        engine.add_rule(
            "alice",
            parse_rule("每当自己受到伤害时，给予自己2回合守护状态").unwrap(),
        );

        engine.attack(
            "enemy",
            "alice",
            1.0,
            DamageType::Physical,
        );

        assert_eq!(engine.active_buff_names("alice"), vec![
            "守护".to_owned()
        ]);
        engine.advance_turn();
        assert_eq!(engine.active_buff_names("alice"), vec![
            "守护".to_owned()
        ]);
        engine.advance_turn();
        assert!(engine.active_buff_names("alice").is_empty());
    }

    #[test]
    fn grant_buff_rule_typed_effect_affects_later_damage() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));
        engine.add_rule(
            "alice",
            parse_rule("每当自己受到伤害时，给予自己2回合守护状态使承伤设为0.5").unwrap(),
        );

        engine.attack(
            "enemy",
            "alice",
            1.0,
            DamageType::Physical,
        );
        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            9.0
        );

        engine.clear_rules_for_owner("alice");
        engine.attack(
            "enemy",
            "alice",
            4.0,
            DamageType::Physical,
        );

        assert_eq!(
            engine.characters.get("alice").unwrap().hp,
            7.0
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
    fn healing_dying_target_uses_dying_healing_modifier() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("source", "来源", 10.0));
        let mut target = Character::new("target", "目标", 20.0);
        target.hp = 4.0;
        target.dying_healing_taken_modifier = 1.5;
        engine.add_character(target);

        engine.heal("source", "target", 4.0);
        assert!((engine.characters.get("target").unwrap().hp - 10.0).abs() < 0.0001);
        assert!(
            (engine
                .characters
                .get("target")
                .unwrap()
                .healing_taken_this_turn
                - 6.0)
                .abs()
                < 0.0001
        );

        engine.heal("source", "target", 4.0);
        assert!((engine.characters.get("target").unwrap().hp - 14.0).abs() < 0.0001);
        assert!(
            (engine
                .characters
                .get("target")
                .unwrap()
                .healing_taken_this_turn
                - 10.0)
                .abs()
                < 0.0001
        );
    }

    #[test]
    fn healing_source_uses_wounded_healing_modifier_by_hp_band() {
        let mut engine = RuleEngine::default();
        let mut source = Character::new("source", "来源", 20.0);
        source.hp = 16.0;
        source.wounded_healing_dealt_modifier = 1.2;
        engine.add_character(source);
        let mut target = Character::new("target", "目标", 50.0);
        target.hp = 0.0;
        engine.add_character(target);

        engine.heal("source", "target", 10.0);
        assert!((engine.characters.get("target").unwrap().hp - 12.0).abs() < 0.0001);

        engine.characters.get_mut("source").unwrap().hp = 8.0;
        engine.heal("source", "target", 10.0);
        assert!((engine.characters.get("target").unwrap().hp - 23.0).abs() < 0.0001);

        engine.characters.get_mut("source").unwrap().hp = 4.0;
        engine.heal("source", "target", 10.0);
        let target = engine.characters.get("target").unwrap();
        assert!((target.hp - 33.0).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 33.0).abs() < 0.0001);
    }

    #[test]
    fn healing_applies_mutual_aid_feedback_to_healer() {
        let mut engine = RuleEngine::default();
        let mut source = Character::new("source", "来源", 20.0);
        source.hp = 10.0;
        source.mutual_aid_healing_rate = 0.5;
        engine.add_character(source);
        let mut target = Character::new("target", "目标", 20.0);
        target.hp = 0.0;
        target.mutual_aid_healing_rate = 0.5;
        engine.add_character(target);

        engine.heal("source", "target", 4.0);

        let source = engine.characters.get("source").unwrap();
        assert!((source.hp - 14.0).abs() < 0.0001);
        assert!((source.healing_taken_this_turn - 4.0).abs() < 0.0001);
        let target = engine.characters.get("target").unwrap();
        assert!((target.hp - 4.0).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 4.0).abs() < 0.0001);

        engine.heal("source", "source", 4.0);
        let source = engine.characters.get("source").unwrap();
        assert!((source.hp - 18.0).abs() < 0.0001);
        assert!((source.healing_taken_this_turn - 8.0).abs() < 0.0001);
    }

    #[test]
    fn damage_and_heal_update_turn_totals_until_advance_turn() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("alice", "自己", 10.0));
        engine.add_character(Character::new("enemy", "敌人", 10.0));

        engine.attack(
            "enemy",
            "alice",
            4.0,
            DamageType::Physical,
        );
        engine.heal("enemy", "alice", 2.0);

        let alice = engine.characters.get("alice").unwrap();
        assert_eq!(alice.hp, 8.0);
        assert_eq!(alice.damage_taken_this_turn, 4.0);
        assert_eq!(alice.healing_taken_this_turn, 2.0);

        engine.advance_turn();
        let alice = engine.characters.get("alice").unwrap();
        assert_eq!(alice.damage_taken_this_turn, 0.0);
        assert_eq!(alice.healing_taken_this_turn, 0.0);
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
    fn chaos_output_variance_randomizes_damage_and_healing_in_range() {
        let mut engine = RuleEngine::default();
        let mut source = Character::new("source", "来源", 100.0);
        source.chaos_output_variance = 0.15;
        let mut target = Character::new("target", "目标", 100.0);
        target.hp = 50.0;
        engine.add_character(source);
        engine.add_character(target);

        engine.attack(
            "source",
            "target",
            10.0,
            DamageType::Physical,
        );
        let target = engine.characters.get("target").unwrap();
        let damage = 50.0 - target.hp;
        assert!(
            (8.5..=11.5).contains(&damage),
            "damage roll out of range: {damage}"
        );

        engine.heal("source", "target", 10.0);
        let target = engine.characters.get("target").unwrap();
        assert!(
            (8.5..=11.5).contains(&target.healing_taken_this_turn),
            "healing roll out of range: {}",
            target.healing_taken_this_turn
        );
    }

    #[test]
    fn attack_uses_damage_type_specific_source_modifier() {
        let mut engine = RuleEngine::default();
        let mut alice = Character::new("alice", "自己", 10.0);
        alice.physical_damage_dealt_modifier = 2.0;
        alice.magical_damage_dealt_modifier = 3.0;
        alice.range_damage_dealt_modifier = 4.0;
        engine.add_character(alice);
        engine.add_character(Character::new("enemy", "敌人", 30.0));

        engine.attack(
            "alice",
            "enemy",
            2.0,
            DamageType::Magical,
        );
        engine.attack(
            "alice",
            "enemy",
            2.0,
            DamageType::Physical,
        );
        engine.attack("alice", "enemy", 2.0, DamageType::Range);
        engine.attack(
            "alice",
            "enemy",
            2.0,
            DamageType::Cursed,
        );

        assert_eq!(
            engine.characters.get("enemy").unwrap().hp,
            10.0
        );
    }

    #[test]
    fn attack_uses_damage_type_specific_target_modifier() {
        let mut engine = RuleEngine::default();
        let mut target = Character::new("target", "目标", 30.0);
        target.damage_taken_modifier = 0.5;
        target.magical_damage_taken_modifier = 0.5;
        target.diseased_damage_taken_modifier = 0.25;
        target.poisoning_damage_taken_modifier = 0.1;
        engine.add_character(target);
        engine.add_character(Character::new("source", "来源", 10.0));

        engine.attack(
            "source",
            "target",
            8.0,
            DamageType::Magical,
        );
        engine.attack(
            "source",
            "target",
            8.0,
            DamageType::Diseased,
        );
        engine.attack(
            "source",
            "target",
            8.0,
            DamageType::Poisoning,
        );
        engine.attack(
            "source",
            "target",
            8.0,
            DamageType::Physical,
        );

        assert!((engine.characters.get("target").unwrap().hp - 22.6).abs() < 0.0001);
    }

    #[test]
    fn attack_applies_damage_dealt_healing_taken_debuff() {
        let mut engine = RuleEngine::default();
        let mut source = Character::new("source", "来源", 10.0);
        source.damage_dealt_buffs = vec![BuffSpec {
            name: "溃伤".to_owned(),
            kind: BuffKind::Bleed,
            priority: 0,
            turns_remaining: 1,
            source_id: "source:talent:溃伤".to_owned(),
            beneficial: false,
            effects: vec![BuffEffect {
                field: BuffField::HealingTakenModifier,
                value: BuffValue::AddPercent(-25.0),
            }],
            tick_actions: Vec::new(),
        }];
        engine.add_character(source);
        engine.add_character(Character::new("target", "目标", 20.0));

        engine.attack(
            "source",
            "target",
            10.0,
            DamageType::Physical,
        );
        assert_eq!(
            engine.active_buff_names("target"),
            vec!["溃伤".to_owned()]
        );
        assert!(
            (engine
                .characters
                .get("target")
                .unwrap()
                .healing_taken_modifier
                - 0.75)
                .abs()
                < 0.0001
        );

        engine.heal("source", "target", 4.0);
        assert!((engine.characters.get("target").unwrap().hp - 13.0).abs() < 0.0001);

        engine.advance_turn();
        assert!(engine.active_buff_names("target").is_empty());
        engine.heal("source", "target", 4.0);
        assert!((engine.characters.get("target").unwrap().hp - 17.0).abs() < 0.0001);
    }

    #[test]
    fn physical_damage_lifesteals_to_source() {
        let mut engine = RuleEngine::default();
        let mut source = Character::new("source", "来源", 20.0);
        source.hp = 19.8;
        source.physical_damage_lifesteal = 0.15;
        engine.add_character(source);
        engine.add_character(Character::new("target", "目标", 20.0));

        engine.attack(
            "source",
            "target",
            4.0,
            DamageType::Physical,
        );
        let source = engine.characters.get("source").unwrap();
        assert!((source.hp - 20.0).abs() < 0.0001);
        assert!((source.healing_taken_this_turn - 0.6).abs() < 0.0001);
        assert!((engine.characters.get("target").unwrap().hp - 16.0).abs() < 0.0001);

        engine.attack(
            "source",
            "target",
            4.0,
            DamageType::Magical,
        );
        assert!((engine.characters.get("source").unwrap().hp - 20.0).abs() < 0.0001);
        assert!(
            (engine
                .characters
                .get("source")
                .unwrap()
                .healing_taken_this_turn
                - 0.6)
                .abs()
                < 0.0001
        );
        assert!((engine.characters.get("target").unwrap().hp - 12.0).abs() < 0.0001);
    }

    #[test]
    fn physical_damage_schedules_sousas_claw_followup() {
        let mut engine = RuleEngine::default();
        let mut source = Character::new("source", "来源", 20.0);
        source.physical_damage_followup_rate = 0.35;
        engine.add_character(source);
        engine.add_character(Character::new("target", "目标", 20.0));

        engine.attack(
            "source",
            "target",
            10.0,
            DamageType::Physical,
        );
        assert!((engine.characters.get("target").unwrap().hp - 10.0).abs() < 0.0001);
        assert_eq!(
            engine.active_buff_names("target"),
            vec!["苏萨斯之爪".to_owned()]
        );

        engine.advance_turn();
        let target = engine.characters.get("target").unwrap();
        assert!((target.hp - 6.5).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 3.5).abs() < 0.0001);

        engine.advance_turn();
        assert!(engine.active_buff_names("target").is_empty());
        assert!((engine.characters.get("target").unwrap().hp - 6.5).abs() < 0.0001);
    }

    #[test]
    fn large_hit_damage_reduction_applies_above_max_hp_threshold() {
        let mut engine = RuleEngine::default();
        engine.add_character(Character::new("source", "来源", 10.0));
        let mut target = Character::new("target", "目标", 20.0);
        target.large_hit_damage_taken_modifier = 0.8;
        engine.add_character(target);

        engine.attack(
            "source",
            "target",
            5.0,
            DamageType::Physical,
        );
        assert!((engine.characters.get("target").unwrap().hp - 16.0).abs() < 0.0001);
        assert!(
            (engine
                .characters
                .get("target")
                .unwrap()
                .damage_taken_this_turn
                - 4.0)
                .abs()
                < 0.0001
        );

        engine.attack(
            "source",
            "target",
            4.0,
            DamageType::Physical,
        );
        assert!((engine.characters.get("target").unwrap().hp - 12.0).abs() < 0.0001);
        assert!(
            (engine
                .characters
                .get("target")
                .unwrap()
                .damage_taken_this_turn
                - 8.0)
                .abs()
                < 0.0001
        );
    }

    #[test]
    fn minimum_damage_floor_applies_after_damage_reductions() {
        let mut engine = RuleEngine::default();
        let mut source = Character::new("source", "来源", 10.0);
        source.minimum_damage_floor = 5.0;
        engine.add_character(source);
        let mut target = Character::new("target", "目标", 20.0);
        target.damage_taken_modifier = 0.1;
        engine.add_character(target);

        engine.attack(
            "source",
            "target",
            2.0,
            DamageType::Physical,
        );
        assert!((engine.characters.get("target").unwrap().hp - 15.0).abs() < 0.0001);
        assert!(
            (engine
                .characters
                .get("target")
                .unwrap()
                .damage_taken_this_turn
                - 5.0)
                .abs()
                < 0.0001
        );

        engine.attack(
            "source",
            "target",
            0.0,
            DamageType::Physical,
        );
        assert!((engine.characters.get("target").unwrap().hp - 15.0).abs() < 0.0001);
        assert!(
            (engine
                .characters
                .get("target")
                .unwrap()
                .damage_taken_this_turn
                - 5.0)
                .abs()
                < 0.0001
        );
    }

    #[test]
    fn attack_applies_low_hp_damage_penalty() {
        let mut engine = RuleEngine::default();
        let mut alice = Character::new("alice", "自己", 10.0);
        alice.hp = 5.0;
        engine.add_character(alice);
        engine.add_character(Character::new("enemy", "敌人", 20.0));

        engine.attack(
            "alice",
            "enemy",
            4.0,
            DamageType::Physical,
        );

        assert_eq!(
            engine.characters.get("enemy").unwrap().hp,
            17.0
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
            tick_actions: Vec::new(),
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
            tick_actions: Vec::new(),
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
            tick_actions: Vec::new(),
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
    fn ecs_buff_can_modify_speed() {
        let mut engine = RuleEngine::default();
        let mut alice = Character::new("alice", "自己", 10.0);
        alice.speed = 10.0;
        engine.add_character(alice);

        engine.give_buff("alice", BuffSpec {
            name: "Tailwind".to_owned(),
            kind: BuffKind::Magic,
            priority: 0,
            turns_remaining: 0,
            source_id: "alice".to_owned(),
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::Speed,
                value: BuffValue::AddPercent(20.0),
            }],
            tick_actions: Vec::new(),
        });

        let alice = engine.characters.get("alice").unwrap();
        assert!((alice.speed - 12.0).abs() < 0.0001);
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
            tick_actions: Vec::new(),
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
            tick_actions: Vec::new(),
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
            tick_actions: Vec::new(),
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
