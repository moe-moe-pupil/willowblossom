use std::{
    collections::{
        hash_map::DefaultHasher,
        HashMap,
        HashSet,
    },
    fs,
    hash::{
        Hash,
        Hasher,
    },
    path::{
        Path,
        PathBuf,
    },
    thread,
    time::{
        Duration,
        SystemTime,
        UNIX_EPOCH,
    },
};

use bevy_persistent::prelude::*;
extern crate dirs;

use bevy::prelude::*;
use crossbeam_channel::{
    unbounded,
    Receiver as CBReceiver,
    Sender as CBSender,
};
use futures_util::{
    SinkExt,
    StreamExt,
};
use rand::RngExt;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    json,
    Map,
    Value,
};
use tokio::{
    runtime::Builder,
    sync::mpsc::{
        Receiver,
        Sender,
    },
    time::sleep,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
};

use crate::{
    moonberry_talents::{
        MoonberryTalent,
        NORMAL_TALENT_POOL,
        SUPPORT_TALENT_POOL,
    },
    rule_engine::{
        BuffEffect,
        BuffField,
        BuffKind,
        BuffSpec,
        BuffTickAction,
        BuffValue,
        DamageType,
    },
    scene::{
        SceneCaptureRequest,
        SceneCaptureRequests,
    },
};

#[derive(States, Debug, Default, Clone, Eq, PartialEq, Hash)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connected,
}

#[derive(Resource)]
struct NapcatIOReceiver(CBReceiver<Message>);

#[derive(Resource)]
struct NapcatSendResultReceiver(CBReceiver<NapcatSendResult>);

#[derive(Resource)]
pub struct NapcatIOSender(pub Sender<NapcatOutboundMessage>);

#[derive(Debug)]
pub struct NapcatOutboundMessage {
    pub request_id: u64,
    pub target_id: String,
    pub message: Message,
}

#[derive(Debug, Clone)]
pub struct NapcatSendResult {
    pub request_id: u64,
    pub target_id: String,
    pub error: Option<String>,
}

#[derive(Resource, Default)]
pub struct NapcatSendManager {
    pub results: Vec<NapcatSendResult>,
}

#[derive(Resource)]
struct NapcatAutoForwardRequestIds {
    next_request_id: u64,
}

#[derive(Resource)]
struct NapcatGroupInfoRequests {
    next_request_id: u64,
    pending_group_ids: HashSet<String>,
}

pub struct NapcatPlugin;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NapcatMessage {
    #[serde(flatten)]
    pub data: NapcatMessageData,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TextData {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImageData {
    #[serde(default)]
    #[serde(rename = "subType")]
    pub sub_type: usize,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub file_size: String,
    #[serde(default)]
    pub local_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Source {
    id: u64,
    time: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum NapcatMessageChainType {
    Source(Source),
    Text {
        data: TextData,
    },
    Image {
        data: ImageData,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum NapcatMessageType {
    Private,
    Group,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NapcatMessageChain {
    #[serde(flatten)]
    pub variant: NapcatMessageChainType,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NapcatSender {
    pub user_id: u64,
    pub nickname: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "scope", content = "id", rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Party(String),
    Player(u64),
    Gm,
    System,
}

impl Default for Visibility {
    fn default() -> Self { Self::Public }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageSource {
    Friend { user_id: u64 },
    Group { group_id: u64, user_id: u64 },
    Gui,
    Summary,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct CampaignMessage {
    pub campaign_id: String,
    pub sender_id: u64,
    pub sender_name: String,
    pub source: MessageSource,
    pub character_id: Option<String>,
    pub party_id: Option<String>,
    pub visibility: Visibility,
    pub text: String,
    pub time: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerAccess {
    pub player_id: u64,
    pub character_id: Option<String>,
    pub party_id: Option<String>,
    pub is_gm: bool,
}

impl PlayerAccess {
    pub fn can_read(&self, visibility: &Visibility) -> bool {
        if self.is_gm {
            return true;
        }

        match visibility {
            Visibility::Public => true,
            Visibility::Party(party_id) => self.party_id.as_deref() == Some(party_id.as_str()),
            Visibility::Player(player_id) => self.player_id == *player_id,
            Visibility::Gm | Visibility::System => false,
        }
    }
}

fn deserialize_message_chains<'de, D>(deserializer: D) -> Result<Vec<NapcatMessageChain>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MessageChains {
        Segments(Vec<NapcatMessageChain>),
        Text(String),
    }

    match MessageChains::deserialize(deserializer)? {
        MessageChains::Segments(segments) => Ok(segments),
        MessageChains::Text(text) => Ok(vec![NapcatMessageChain {
            variant: NapcatMessageChainType::Text {
                data: TextData { text },
            },
        }]),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NapcatMessageData {
    pub time: u64,
    pub message_type: NapcatMessageType,
    #[serde(deserialize_with = "deserialize_message_chains")]
    pub message: Vec<NapcatMessageChain>,
    pub self_id: u64,
    pub user_id: u64,
    pub group_id: Option<u64>,
    #[serde(default)]
    pub group_name: Option<String>,
    pub target_id: Option<u64>,
    pub sender: NapcatSender,
    #[serde(default = "default_campaign_id")]
    pub campaign_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party_id: Option<String>,
    #[serde(default)]
    pub visibility: Visibility,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatGroup {
    pub members: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ChatTargetMetadata {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub automatic_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CharacterStatus {
    #[serde(default, rename = "str")]
    pub str_: i32,
    #[serde(default)]
    pub agi: i32,
    #[serde(default)]
    pub dex: i32,
    #[serde(default)]
    pub vit: i32,
    #[serde(default, rename = "int")]
    pub int_: i32,
    #[serde(default)]
    pub wis: i32,
    #[serde(default)]
    pub k: i32,
    #[serde(default)]
    pub cha: i32,
}

impl CharacterStatus {
    pub fn combined(&self, extra: &Self) -> Self {
        Self {
            str_: self.str_ + extra.str_,
            agi: self.agi + extra.agi,
            dex: self.dex + extra.dex,
            vit: self.vit + extra.vit,
            int_: self.int_ + extra.int_,
            wis: self.wis + extra.wis,
            k: self.k + extra.k,
            cha: self.cha + extra.cha,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InventoryQuality {
    Poor,
    #[default]
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentSlot {
    Head,
    Neck,
    Shoulder,
    Back,
    Chest,
    Wrist,
    Hands,
    Waist,
    Legs,
    Feet,
    Finger,
    Trinket,
    MainHand,
    OffHand,
    Ranged,
    #[default]
    None,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InventoryItem {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub quality: InventoryQuality,
    #[serde(default)]
    pub equipment_slot: EquipmentSlot,
    #[serde(default = "default_item_stack")]
    pub stack: u32,
    #[serde(default = "default_item_max_stack")]
    pub max_stack: u32,
    #[serde(default)]
    pub item_level: u32,
    #[serde(default)]
    pub soulbound: bool,
}

impl Default for InventoryItem {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            icon: String::new(),
            quality: InventoryQuality::Common,
            equipment_slot: EquipmentSlot::None,
            stack: default_item_stack(),
            max_stack: default_item_max_stack(),
            item_level: 0,
            soulbound: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CharacterInventory {
    #[serde(default = "default_bag_slots")]
    pub bag_slots: usize,
    #[serde(default)]
    pub gold: u32,
    #[serde(default)]
    pub items: Vec<InventoryItem>,
    #[serde(default)]
    pub equipment: HashMap<EquipmentSlot, InventoryItem>,
}

impl Default for CharacterInventory {
    fn default() -> Self {
        Self {
            bag_slots: default_bag_slots(),
            gold: 0,
            items: Vec::new(),
            equipment: HashMap::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RandomPoolEntry {
    #[serde(default)]
    pub item: InventoryItem,
    #[serde(default = "default_random_pool_weight")]
    pub weight: f32,
    #[serde(default = "default_random_pool_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub result_text: String,
    #[serde(default = "default_random_pool_count")]
    pub min_count: u32,
    #[serde(default = "default_random_pool_count")]
    pub max_count: u32,
}

impl Default for RandomPoolEntry {
    fn default() -> Self {
        Self {
            item: InventoryItem {
                name: "新物品".to_owned(),
                ..Default::default()
            },
            weight: default_random_pool_weight(),
            enabled: true,
            result_text: String::new(),
            min_count: default_random_pool_count(),
            max_count: default_random_pool_count(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct RandomPoolTextResult {
    #[serde(default)]
    pub entry_name: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub count: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RandomPoolCheckedResult {
    #[serde(default = "default_random_pool_checked_result_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub target_id: String,
    #[serde(default)]
    pub text: String,
}

impl Default for RandomPoolCheckedResult {
    fn default() -> Self {
        Self {
            enabled: default_random_pool_checked_result_enabled(),
            target_id: String::new(),
            text: String::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct RandomPool {
    #[serde(default)]
    pub entries: Vec<RandomPoolEntry>,
    #[serde(default)]
    pub last_pick: Option<InventoryItem>,
    #[serde(default)]
    pub last_text_result: Option<RandomPoolTextResult>,
    #[serde(default)]
    pub legacy_pool_id: Option<String>,
    #[serde(default)]
    pub legacy_group: Option<i32>,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub checked_results: Vec<RandomPoolCheckedResult>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnitPoolEntry {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_member_id: Option<String>,
    #[serde(default)]
    pub character: PlayerCharacter,
}

impl Default for UnitPoolEntry {
    fn default() -> Self {
        Self {
            label: "新单位".to_owned(),
            note: String::new(),
            legacy_member_id: None,
            character: PlayerCharacter::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillPoolArg {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkillRuleArgs {
    pub numeric_values: Vec<(String, f32)>,
    pub text_values: Vec<(String, String)>,
}

pub fn skill_rule_args(args: &[SkillPoolArg]) -> SkillRuleArgs {
    SkillRuleArgs {
        numeric_values: skill_numeric_arg_values(args),
        text_values: skill_text_arg_values(args),
    }
}

pub fn skill_numeric_arg_values(args: &[SkillPoolArg]) -> Vec<(String, f32)> {
    args.iter()
        .filter_map(|arg| {
            let name = arg.name.trim();
            if name.is_empty() || !skill_arg_kind_is_numeric(&arg.kind) {
                return None;
            }
            let value = arg.value.trim().parse::<f32>().ok()?;
            Some((name.to_owned(), value))
        })
        .collect()
}

pub fn skill_text_arg_values(args: &[SkillPoolArg]) -> Vec<(String, String)> {
    args.iter()
        .filter_map(|arg| {
            let name = arg.name.trim();
            let value = arg.value.trim();
            if name.is_empty() || value.is_empty() || !skill_arg_kind_is_textual(&arg.kind, value) {
                return None;
            }
            Some((name.to_owned(), value.to_owned()))
        })
        .collect()
}

fn skill_arg_kind_is_numeric(kind: &str) -> bool {
    let kind = kind.trim();
    kind.is_empty() || kind.eq_ignore_ascii_case("number") || kind == "数字"
}

fn skill_arg_kind_is_textual(kind: &str, value: &str) -> bool {
    let kind = kind.trim();
    if kind.is_empty() {
        return value.parse::<f32>().is_err();
    }
    kind.eq_ignore_ascii_case("string")
        || kind.eq_ignore_ascii_case("buff")
        || kind == "字符串"
        || kind == "BUFF"
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct SkillPoolEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub mp_cost: f32,
    #[serde(default)]
    pub cooldown_turns: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_character_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_character_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_skill_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_pool_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default)]
    pub args: Vec<SkillPoolArg>,
    #[serde(default)]
    pub legacy_buff_count: usize,
    #[serde(default)]
    pub legacy_event_buff_count: usize,
    #[serde(default)]
    pub legacy_has_graph: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buff_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_event_buff_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_graph_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buff_machine_json: Option<String>,
}

impl SkillPoolEntry {
    pub fn source_key(&self) -> Option<(String, usize)> {
        Some((
            self.source_character_id.clone()?,
            self.source_skill_index?,
        ))
    }

    pub fn legacy_raw_payload(&self) -> Option<String> { legacy_skill_pool_raw_payload(self) }
}

fn compact_legacy_json(value: &Value) -> Option<String> {
    (!value.is_null())
        .then(|| serde_json::to_string(value).ok())
        .flatten()
        .filter(|text| !text.trim().is_empty() && text != "null")
}

fn moonberry_legacy_json_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(compact_legacy_json)
}

fn legacy_json_value(text: &str) -> Option<Value> { serde_json::from_str(text).ok() }

fn legacy_skill_pool_raw_payload(entry: &SkillPoolEntry) -> Option<String> {
    let mut payload = Map::new();
    if let Some(value) = entry
        .legacy_buff_machine_json
        .as_deref()
        .and_then(legacy_json_value)
    {
        payload.insert("buffMachine".to_owned(), value);
    }
    if let Some(value) = entry
        .legacy_buff_json
        .as_deref()
        .and_then(legacy_json_value)
    {
        payload.insert("buff".to_owned(), value);
    }
    if let Some(value) = entry
        .legacy_event_buff_json
        .as_deref()
        .and_then(legacy_json_value)
    {
        payload.insert("eventBuffs".to_owned(), value);
    }
    if let Some(value) = entry
        .legacy_graph_json
        .as_deref()
        .and_then(legacy_json_value)
    {
        payload.insert("graph".to_owned(), value);
    }
    (!payload.is_empty())
        .then(|| serde_json::to_string(&Value::Object(payload)).ok())
        .flatten()
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CharacterSkillSourceKind {
    Manual,
    Talent,
    SkillPool,
}

impl Default for CharacterSkillSourceKind {
    fn default() -> Self { Self::Manual }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct CharacterSkillMetadata {
    #[serde(default = "default_skill_approved")]
    pub pc_approved: bool,
    #[serde(default = "default_skill_approved")]
    pub st_approved: bool,
    #[serde(default)]
    pub source: CharacterSkillSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_pool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_pool_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_character_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_skill_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange_point: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_left: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_caster: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub talent_trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub talent_effect: Option<String>,
    #[serde(default)]
    pub args: Vec<SkillPoolArg>,
    #[serde(default)]
    pub legacy_has_buff_machine: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buff_machine_json: Option<String>,
}

impl Default for CharacterSkillMetadata {
    fn default() -> Self {
        Self {
            pc_approved: default_skill_approved(),
            st_approved: default_skill_approved(),
            source: CharacterSkillSourceKind::Manual,
            source_pool_id: None,
            source_pool_label: None,
            source_character_id: None,
            source_skill_index: None,
            skill_type: None,
            target_class: None,
            target_count: None,
            range: None,
            exchange_point: None,
            cooldown_left: None,
            legacy_caster: None,
            talent_trigger: None,
            talent_effect: None,
            args: Vec::new(),
            legacy_has_buff_machine: false,
            legacy_buff_machine_json: None,
        }
    }
}

impl CharacterSkillMetadata {
    pub fn player_submitted() -> Self {
        Self {
            pc_approved: true,
            st_approved: false,
            ..Default::default()
        }
    }

    pub fn talent(pool_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            source: CharacterSkillSourceKind::Talent,
            source_pool_id: Some(pool_id.into()),
            source_pool_label: Some(label.into()),
            ..Default::default()
        }
    }

    pub fn moonberry_talent(
        pool_id: impl Into<String>,
        label: impl Into<String>,
        talent: &MoonberryTalent,
    ) -> Self {
        Self {
            talent_trigger: moonberry_talent_trigger(talent).map(str::to_owned),
            talent_effect: moonberry_talent_effect_summary(talent).map(str::to_owned),
            ..Self::talent(pool_id, label)
        }
    }

    pub fn skill_pool(entry: &SkillPoolEntry) -> Self {
        let source_pool_label = entry
            .name
            .trim()
            .is_empty()
            .then(|| entry.category.clone())
            .flatten()
            .or_else(|| Some(entry.name.clone()))
            .filter(|label| !label.trim().is_empty());
        Self {
            source: CharacterSkillSourceKind::SkillPool,
            source_pool_id: entry.legacy_pool_id.clone(),
            source_pool_label,
            source_character_id: entry.source_character_id.clone(),
            source_skill_index: entry.source_skill_index,
            args: entry.args.clone(),
            legacy_has_buff_machine: entry.legacy_buff_machine_json.is_some()
                || entry.legacy_buff_json.is_some()
                || entry.legacy_event_buff_json.is_some()
                || entry.legacy_graph_json.is_some()
                || entry.legacy_buff_count > 0
                || entry.legacy_event_buff_count > 0
                || entry.legacy_has_graph,
            legacy_buff_machine_json: entry
                .legacy_buff_machine_json
                .clone()
                .or_else(|| legacy_skill_pool_raw_payload(entry)),
            ..Default::default()
        }
    }

    pub fn is_approved(&self) -> bool { self.pc_approved && self.st_approved }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerCharacter {
    #[serde(default)]
    pub inited: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub creation_step: CharacterCreationStep,
    #[serde(default = "default_status_points")]
    pub status_points: i32,
    #[serde(default = "default_exchange_points")]
    pub exchange_points: i32,
    #[serde(default = "default_character_hp")]
    pub hp: f32,
    #[serde(default = "default_character_hp")]
    pub max_hp: f32,
    #[serde(default)]
    pub hp_regen: f32,
    #[serde(default)]
    pub mp: f32,
    #[serde(default)]
    pub max_mp: f32,
    #[serde(default)]
    pub mp_regen: f32,
    #[serde(default = "default_character_level")]
    pub level: i32,
    #[serde(default)]
    pub exp: i32,
    #[serde(default = "default_character_speed")]
    pub speed: f32,
    #[serde(default = "default_modifier")]
    pub damage_dealt_modifier: f32,
    #[serde(default = "default_modifier")]
    pub healing_dealt_modifier: f32,
    #[serde(default = "default_modifier")]
    pub damage_taken_modifier: f32,
    #[serde(default = "default_modifier")]
    pub healing_taken_modifier: f32,
    #[serde(default, alias = "tdpt")]
    pub damage_taken_this_turn: f32,
    #[serde(default, alias = "thpt")]
    pub healing_taken_this_turn: f32,
    #[serde(default)]
    pub status: CharacterStatus,
    #[serde(default)]
    pub extra_status: CharacterStatus,
    #[serde(default)]
    pub skill_names: Vec<String>,
    #[serde(default)]
    pub skill_notes: Vec<String>,
    #[serde(default)]
    pub skill_mp_costs: Vec<f32>,
    #[serde(default)]
    pub skill_cooldown_turns: Vec<u32>,
    #[serde(default)]
    pub skill_metadata: Vec<CharacterSkillMetadata>,
    #[serde(default)]
    pub skill_last_cast_turns: HashMap<String, u32>,
    #[serde(default)]
    pub skill_cooldown_ready_turns: HashMap<String, u32>,
    #[serde(default)]
    pub active_buffs: Vec<BuffSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buff_base_stats: Option<CharacterBuffBaseStats>,
    #[serde(default)]
    pub inventory: CharacterInventory,
}

impl Default for PlayerCharacter {
    fn default() -> Self {
        Self {
            inited: false,
            name: String::new(),
            nickname: String::new(),
            image: String::new(),
            creation_step: CharacterCreationStep::Normal,
            status_points: default_status_points(),
            exchange_points: default_exchange_points(),
            hp: default_character_hp(),
            max_hp: default_character_hp(),
            hp_regen: 0.0,
            mp: 0.0,
            max_mp: 0.0,
            mp_regen: 0.0,
            level: default_character_level(),
            exp: 0,
            speed: default_character_speed(),
            damage_dealt_modifier: default_modifier(),
            healing_dealt_modifier: default_modifier(),
            damage_taken_modifier: default_modifier(),
            healing_taken_modifier: default_modifier(),
            damage_taken_this_turn: 0.0,
            healing_taken_this_turn: 0.0,
            status: CharacterStatus::default(),
            extra_status: CharacterStatus::default(),
            skill_names: Vec::new(),
            skill_notes: Vec::new(),
            skill_mp_costs: Vec::new(),
            skill_cooldown_turns: Vec::new(),
            skill_metadata: Vec::new(),
            skill_last_cast_turns: HashMap::new(),
            skill_cooldown_ready_turns: HashMap::new(),
            active_buffs: Vec::new(),
            buff_base_stats: None,
            inventory: CharacterInventory::default(),
        }
    }
}

pub fn record_character_damage_taken(character: &mut PlayerCharacter, amount: f32) -> bool {
    let amount = amount.max(0.0);
    if amount <= f32::EPSILON {
        return false;
    }
    character.damage_taken_this_turn += amount;
    true
}

pub fn record_character_healing_taken(character: &mut PlayerCharacter, amount: f32) -> bool {
    let amount = amount.max(0.0);
    if amount <= f32::EPSILON {
        return false;
    }
    character.healing_taken_this_turn += amount;
    true
}

pub fn reset_character_turn_totals(character: &mut PlayerCharacter) -> bool {
    let changed = character.damage_taken_this_turn.abs() > f32::EPSILON
        || character.healing_taken_this_turn.abs() > f32::EPSILON;
    character.damage_taken_this_turn = 0.0;
    character.healing_taken_this_turn = 0.0;
    changed
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CharacterBuffBaseStats {
    #[serde(default = "default_character_hp")]
    pub hp: f32,
    #[serde(default = "default_character_hp")]
    pub max_hp: f32,
    #[serde(default)]
    pub hp_regen: f32,
    #[serde(default)]
    pub mp: f32,
    #[serde(default)]
    pub max_mp: f32,
    #[serde(default)]
    pub mp_regen: f32,
    #[serde(default = "default_character_speed")]
    pub speed: f32,
    #[serde(default = "default_modifier")]
    pub damage_dealt_modifier: f32,
    #[serde(default = "default_modifier")]
    pub damage_taken_modifier: f32,
    #[serde(default = "default_modifier")]
    pub healing_dealt_modifier: f32,
    #[serde(default = "default_modifier")]
    pub healing_taken_modifier: f32,
    #[serde(default)]
    pub extra_status: CharacterStatus,
}

impl CharacterBuffBaseStats {
    pub fn from_character(character: &PlayerCharacter) -> Self {
        Self {
            hp: character.hp,
            max_hp: character.max_hp,
            hp_regen: character.hp_regen,
            mp: character.mp,
            max_mp: character.max_mp,
            mp_regen: character.mp_regen,
            speed: character.speed,
            damage_dealt_modifier: character.damage_dealt_modifier,
            damage_taken_modifier: character.damage_taken_modifier,
            healing_dealt_modifier: character.healing_dealt_modifier,
            healing_taken_modifier: character.healing_taken_modifier,
            extra_status: character.extra_status.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CharacterCreationStep {
    #[default]
    Normal,
    Str,
    Agi,
    Dex,
    Vit,
    Int,
    Wis,
    K,
    Cha,
    ConfirmStatus,
    Skill,
    ConfirmSkill,
    Image,
    Nickname,
}

impl CharacterCreationStep {
    fn status_key(self) -> Option<StatusKey> {
        match self {
            CharacterCreationStep::Str => Some(StatusKey::Str),
            CharacterCreationStep::Agi => Some(StatusKey::Agi),
            CharacterCreationStep::Dex => Some(StatusKey::Dex),
            CharacterCreationStep::Vit => Some(StatusKey::Vit),
            CharacterCreationStep::Int => Some(StatusKey::Int),
            CharacterCreationStep::Wis => Some(StatusKey::Wis),
            CharacterCreationStep::K => Some(StatusKey::K),
            CharacterCreationStep::Cha => Some(StatusKey::Cha),
            _ => None,
        }
    }

    fn next_status_step(self) -> Self {
        match self {
            CharacterCreationStep::Str => CharacterCreationStep::Agi,
            CharacterCreationStep::Agi => CharacterCreationStep::Dex,
            CharacterCreationStep::Dex => CharacterCreationStep::Vit,
            CharacterCreationStep::Vit => CharacterCreationStep::Int,
            CharacterCreationStep::Int => CharacterCreationStep::Wis,
            CharacterCreationStep::Wis => CharacterCreationStep::K,
            CharacterCreationStep::K => CharacterCreationStep::Cha,
            CharacterCreationStep::Cha => CharacterCreationStep::ConfirmStatus,
            _ => self,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum StatusKey {
    Str,
    Agi,
    Dex,
    Vit,
    Int,
    Wis,
    K,
    Cha,
}

impl StatusKey {
    fn zh(self) -> &'static str {
        match self {
            StatusKey::Str => "力量",
            StatusKey::Agi => "敏捷",
            StatusKey::Dex => "灵巧",
            StatusKey::Vit => "体质",
            StatusKey::Int => "智力",
            StatusKey::Wis => "智慧",
            StatusKey::K => "知识",
            StatusKey::Cha => "魅力",
        }
    }
}

fn default_character_hp() -> f32 { 5.0 }

fn default_character_level() -> i32 { 1 }

fn default_character_speed() -> f32 { 3.0 }

fn default_modifier() -> f32 { 1.0 }

fn default_item_stack() -> u32 { 1 }

fn default_item_max_stack() -> u32 { 1 }

fn default_bag_slots() -> usize { 16 }

fn default_random_pool_weight() -> f32 { 1.0 }

fn default_random_pool_count() -> u32 { 1 }

fn default_random_pool_enabled() -> bool { true }

fn default_random_pool_checked_result_enabled() -> bool { true }

pub fn normalized_random_pool_counts(min_count: u32, max_count: u32) -> (u32, u32) {
    if max_count < min_count {
        (min_count, min_count)
    } else {
        (min_count, max_count)
    }
}

fn default_skill_approved() -> bool { true }

fn default_campaign_id() -> String { "default".to_owned() }

fn default_status_points() -> i32 { 5 }

fn default_exchange_points() -> i32 { 6 }

fn default_allow_join_requests() -> bool { true }

fn default_battle_sort_by_turn() -> bool { true }

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub struct TrpgBasicConfig {
    #[serde(default = "default_base_max_hp")]
    pub base_max_hp: f32,
    #[serde(default = "default_wis_mp_reg")]
    pub wis_mp_reg: f32,
    #[serde(default = "default_wis_max_mp")]
    pub wis_max_mp: f32,
    #[serde(default = "default_int_max_mp")]
    pub int_max_mp: f32,
    #[serde(default = "default_vit_hp_reg")]
    pub vit_hp_reg: f32,
    #[serde(default = "default_vit_max_hp")]
    pub vit_max_hp: f32,
    #[serde(default = "default_lv_max_hp")]
    pub lv_max_hp: f32,
    #[serde(default = "default_str_max_hp")]
    pub str_max_hp: f32,
    #[serde(default = "default_exp_gain_per_level")]
    pub exp_gain_per_level: f32,
    #[serde(default = "default_exp_gain_per_level_pvp")]
    pub exp_gain_per_level_pvp: f32,
    #[serde(default = "default_basic_speed")]
    pub basic_speed: f32,
    #[serde(default = "default_str_damage_bonus")]
    pub str_damage_bonus: f32,
    #[serde(default = "default_int_damage_bonus")]
    pub int_damage_bonus: f32,
    #[serde(default = "default_dex_damage_bonus")]
    pub dex_damage_bonus: f32,
    #[serde(default = "default_dex_range_damage_bonus")]
    pub dex_range_damage_bonus: f32,
    #[serde(default = "default_wis_heal_bonus")]
    pub wis_heal_bonus: f32,
    #[serde(default = "default_int_heal_bonus")]
    pub int_heal_bonus: f32,
    #[serde(default = "default_agi_damage_bonus")]
    pub agi_damage_bonus: f32,
    #[serde(default = "default_str_speed")]
    pub str_speed: f32,
    #[serde(default = "default_agi_speed")]
    pub agi_speed: f32,
    #[serde(default = "default_dex_speed")]
    pub dex_speed: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrpgDamageBonusKind {
    Magical,
    Physical,
    Range,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrpgDamageTakenKind {
    Magical,
    Diseased,
    Poisoning,
    Other,
}

impl Default for TrpgBasicConfig {
    fn default() -> Self {
        Self {
            base_max_hp: default_base_max_hp(),
            wis_mp_reg: default_wis_mp_reg(),
            wis_max_mp: default_wis_max_mp(),
            int_max_mp: default_int_max_mp(),
            vit_hp_reg: default_vit_hp_reg(),
            vit_max_hp: default_vit_max_hp(),
            lv_max_hp: default_lv_max_hp(),
            str_max_hp: default_str_max_hp(),
            exp_gain_per_level: default_exp_gain_per_level(),
            exp_gain_per_level_pvp: default_exp_gain_per_level_pvp(),
            basic_speed: default_basic_speed(),
            str_damage_bonus: default_str_damage_bonus(),
            int_damage_bonus: default_int_damage_bonus(),
            dex_damage_bonus: default_dex_damage_bonus(),
            dex_range_damage_bonus: default_dex_range_damage_bonus(),
            wis_heal_bonus: default_wis_heal_bonus(),
            int_heal_bonus: default_int_heal_bonus(),
            agi_damage_bonus: default_agi_damage_bonus(),
            str_speed: default_str_speed(),
            agi_speed: default_agi_speed(),
            dex_speed: default_dex_speed(),
        }
    }
}

fn default_base_max_hp() -> f32 { 5.0 }
fn default_wis_mp_reg() -> f32 { 1.0 }
fn default_wis_max_mp() -> f32 { 2.5 }
fn default_int_max_mp() -> f32 { 5.0 }
fn default_vit_hp_reg() -> f32 { 1.0 }
fn default_vit_max_hp() -> f32 { 3.0 }
fn default_lv_max_hp() -> f32 { 5.0 }
fn default_str_max_hp() -> f32 { 1.0 }
fn default_exp_gain_per_level() -> f32 { 3.0 }
fn default_exp_gain_per_level_pvp() -> f32 { 0.15 }
fn default_basic_speed() -> f32 { 3.0 }
fn default_str_damage_bonus() -> f32 { 0.025 }
fn default_int_damage_bonus() -> f32 { 0.02 }
fn default_dex_damage_bonus() -> f32 { 0.01 }
fn default_dex_range_damage_bonus() -> f32 { 0.03 }
fn default_wis_heal_bonus() -> f32 { 0.02 }
fn default_int_heal_bonus() -> f32 { 0.01 }
fn default_agi_damage_bonus() -> f32 { 0.02 }
fn default_str_speed() -> f32 { 0.5 }
fn default_agi_speed() -> f32 { 1.0 }
fn default_dex_speed() -> f32 { 0.5 }

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TrpgParty {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub players: Vec<String>,
}

impl PartialEq for TrpgParty {
    fn eq(&self, other: &Self) -> bool { self.name == other.name && self.players == other.players }
}

impl Eq for TrpgParty {}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct TrpgLegacyTeamChatMessage {
    #[serde(default)]
    pub sender_id: String,
    #[serde(default)]
    pub sender_name: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub time: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TrpgLegacyTeam {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub players: Vec<String>,
    #[serde(default = "default_legacy_visible")]
    pub visible: bool,
    #[serde(default)]
    pub allow_pc_nickname_repeat: bool,
    #[serde(default)]
    pub anonymous_speakers: bool,
    #[serde(default)]
    pub buff_count: usize,
    #[serde(default)]
    pub chat_message_count: usize,
    #[serde(default)]
    pub chat_messages: Vec<TrpgLegacyTeamChatMessage>,
    #[serde(default)]
    pub window_x: f32,
    #[serde(default)]
    pub window_y: f32,
    #[serde(default)]
    pub window_width: f32,
    #[serde(default)]
    pub window_height: f32,
}

impl Default for TrpgLegacyTeam {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            players: Vec::new(),
            visible: default_legacy_visible(),
            allow_pc_nickname_repeat: false,
            anonymous_speakers: false,
            buff_count: 0,
            chat_message_count: 0,
            chat_messages: Vec::new(),
            window_x: 0.0,
            window_y: 0.0,
            window_width: 0.0,
            window_height: 0.0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct TrpgLegacyArea {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub width: f32,
    #[serde(default)]
    pub height: f32,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub combat: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TrpgLegacyWorld {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_legacy_visible")]
    pub visible: bool,
    #[serde(default)]
    pub players: Vec<String>,
    #[serde(default)]
    pub npcs: Vec<String>,
    #[serde(default)]
    pub chat_areas: Vec<TrpgLegacyArea>,
    #[serde(default)]
    pub areas: Vec<TrpgLegacyArea>,
}

impl Default for TrpgLegacyWorld {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            visible: default_legacy_visible(),
            players: Vec::new(),
            npcs: Vec::new(),
            chat_areas: Vec::new(),
            areas: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct TrpgLegacySendPane {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default = "default_legacy_closable")]
    pub closable: bool,
}

impl Default for TrpgLegacySendPane {
    fn default() -> Self {
        Self {
            key: String::new(),
            title: String::new(),
            targets: Vec::new(),
            closable: default_legacy_closable(),
        }
    }
}

fn default_legacy_visible() -> bool { true }
fn default_legacy_closable() -> bool { true }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrpgGroup {
    #[serde(default = "default_campaign_id")]
    pub campaign_id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub st_description: String,
    #[serde(default)]
    pub guide: String,
    #[serde(default = "default_allow_join_requests")]
    pub allow_join_requests: bool,
    #[serde(default = "default_status_points")]
    pub initial_status_points: i32,
    #[serde(default = "default_exchange_points")]
    pub initial_exchange_points: i32,
    #[serde(default)]
    pub basic_config: TrpgBasicConfig,
    #[serde(default, alias = "runTimes")]
    pub run_times: u32,
    #[serde(default = "default_battle_sort_by_turn", alias = "orderByTurn")]
    pub battle_sort_by_turn: bool,
    #[serde(default, alias = "negativeEnabled")]
    pub battle_negative_enabled: bool,
    #[serde(default)]
    pub legacy_negative_count: usize,
    #[serde(default)]
    pub legacy_negative_timers: Vec<TrpgLegacyNegativeTimer>,
    #[serde(default)]
    pub gm_users: HashSet<u64>,
    #[serde(default)]
    pub parties: HashMap<String, TrpgParty>,
    #[serde(default)]
    pub player_parties: HashMap<String, String>,
    #[serde(default)]
    pub legacy_teams: Vec<TrpgLegacyTeam>,
    #[serde(default)]
    pub legacy_worlds: Vec<TrpgLegacyWorld>,
    #[serde(default)]
    pub legacy_send_panes: Vec<TrpgLegacySendPane>,
    #[serde(default)]
    pub players: Vec<String>,
    #[serde(default)]
    pub group_chats: Vec<String>,
    #[serde(default)]
    pub world_turn: u32,
    #[serde(default)]
    pub player_turns: HashMap<String, TrpgPlayerTurnState>,
}

impl Default for TrpgGroup {
    fn default() -> Self {
        Self {
            campaign_id: default_campaign_id(),
            description: String::new(),
            st_description: String::new(),
            guide: String::new(),
            allow_join_requests: default_allow_join_requests(),
            initial_status_points: default_status_points(),
            initial_exchange_points: default_exchange_points(),
            basic_config: TrpgBasicConfig::default(),
            run_times: 0,
            battle_sort_by_turn: default_battle_sort_by_turn(),
            battle_negative_enabled: false,
            legacy_negative_count: 0,
            legacy_negative_timers: Vec::new(),
            gm_users: HashSet::default(),
            parties: HashMap::default(),
            player_parties: HashMap::default(),
            legacy_teams: Vec::new(),
            legacy_worlds: Vec::new(),
            legacy_send_panes: Vec::new(),
            players: Vec::new(),
            group_chats: Vec::new(),
            world_turn: 0,
            player_turns: HashMap::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TrpgPlayerTurnState {
    #[serde(default)]
    pub turns_passed: u32,
    #[serde(default)]
    pub acted: bool,
    #[serde(default)]
    pub skipped: bool,
}

pub const LEGACY_NEGATIVE_TIMEOUT_MS: u64 = 2 * 60 * 1000;

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct TrpgLegacyNegativeTimer {
    #[serde(default)]
    pub target_id: String,
    #[serde(default)]
    pub remaining_ms: u64,
    #[serde(default)]
    pub replied: bool,
    #[serde(default)]
    pub generation: u32,
    #[serde(default)]
    pub half_warned: bool,
    #[serde(default)]
    pub negative_layers: u32,
}

impl TrpgLegacyNegativeTimer {
    fn for_target(target_id: &str) -> Self {
        Self {
            target_id: target_id.to_owned(),
            ..Default::default()
        }
    }

    pub fn active(&self) -> bool { self.remaining_ms > 0 && !self.replied }
}

impl TrpgGroup {
    pub fn sync_turn_players(&mut self) -> bool {
        let before_len = self.player_turns.len();
        self.player_turns
            .retain(|target_id, _| self.players.contains(target_id));
        let mut changed = before_len != self.player_turns.len();

        for target_id in &self.players {
            if !self.player_turns.contains_key(target_id) {
                self.player_turns.insert(
                    target_id.clone(),
                    TrpgPlayerTurnState::default(),
                );
                changed = true;
            }
        }
        changed
    }

    pub fn sync_legacy_negative_timers(&mut self) -> bool {
        let before = self.legacy_negative_timers.clone();
        let mut next = Vec::new();
        let mut seen = HashSet::new();

        for mut timer in std::mem::take(&mut self.legacy_negative_timers) {
            timer.target_id = timer.target_id.trim().to_owned();
            if timer.target_id.is_empty() || !seen.insert(timer.target_id.clone()) {
                continue;
            }
            next.push(timer);
        }

        if self.battle_negative_enabled || !next.is_empty() {
            for target_id in &self.players {
                let target_id = target_id.trim();
                if !target_id.is_empty() && seen.insert(target_id.to_owned()) {
                    next.push(TrpgLegacyNegativeTimer::for_target(
                        target_id,
                    ));
                }
            }
        }

        let changed = before != next;
        self.legacy_negative_timers = next;
        changed
    }

    pub fn legacy_negative_timer(&self, target_id: &str) -> Option<&TrpgLegacyNegativeTimer> {
        let target_id = target_id.trim();
        self.legacy_negative_timers
            .iter()
            .find(|timer| timer.target_id == target_id)
    }

    pub fn register_legacy_negative_reply(&mut self, target_id: &str) -> bool {
        let Some(index) = self.ensure_legacy_negative_timer_index(target_id) else {
            return false;
        };
        let timer = &mut self.legacy_negative_timers[index];
        let changed = timer.remaining_ms != 0 || !timer.replied || timer.half_warned;
        if changed {
            timer.remaining_ms = 0;
            timer.replied = true;
            timer.half_warned = false;
            timer.generation = timer.generation.saturating_add(1);
        }
        changed
    }

    pub fn start_legacy_negative_timer(&mut self, target_id: &str, remaining_ms: u64) -> bool {
        if remaining_ms == 0 {
            return self.reset_legacy_negative_timer(target_id);
        }

        let Some(index) = self.ensure_legacy_negative_timer_index(target_id) else {
            return false;
        };
        let timer = &mut self.legacy_negative_timers[index];
        timer.remaining_ms = remaining_ms;
        timer.replied = false;
        timer.half_warned = false;
        timer.generation = timer.generation.saturating_add(1);
        true
    }

    pub fn mark_legacy_negative_half_warned(&mut self, target_id: &str) -> bool {
        let Some(index) = self.ensure_legacy_negative_timer_index(target_id) else {
            return false;
        };
        let timer = &mut self.legacy_negative_timers[index];
        if !timer.active() || timer.half_warned {
            return false;
        }
        timer.half_warned = true;
        true
    }

    pub fn record_legacy_negative_timeout(&mut self, target_id: &str) -> bool {
        let Some(index) = self.ensure_legacy_negative_timer_index(target_id) else {
            return false;
        };
        let timer = &mut self.legacy_negative_timers[index];
        timer.remaining_ms = 0;
        timer.replied = false;
        timer.half_warned = false;
        timer.negative_layers = timer.negative_layers.saturating_add(1);
        timer.generation = timer.generation.saturating_add(1);
        true
    }

    pub fn reset_legacy_negative_timer(&mut self, target_id: &str) -> bool {
        let Some(index) = self.ensure_legacy_negative_timer_index(target_id) else {
            return false;
        };
        let timer = &mut self.legacy_negative_timers[index];
        let changed = timer.remaining_ms != 0 || timer.replied || timer.half_warned;
        if changed {
            timer.remaining_ms = 0;
            timer.replied = false;
            timer.half_warned = false;
            timer.generation = timer.generation.saturating_add(1);
        }
        changed
    }

    pub fn refresh_legacy_negative_timers(&mut self) -> bool {
        if !self.battle_negative_enabled || self.players.is_empty() {
            return false;
        }

        self.sync_turn_players();
        let mut changed = self.sync_legacy_negative_timers();
        let effective_turns = self
            .players
            .iter()
            .map(|target_id| {
                let turn = self.player_turns.get(target_id);
                let finished = turn.is_some_and(|turn| turn.acted || turn.skipped);
                let turns_passed = turn.map(|turn| turn.turns_passed).unwrap_or_default();
                (
                    target_id.clone(),
                    turns_passed.saturating_add(if finished { 1 } else { 0 }),
                )
            })
            .collect::<Vec<_>>();
        let player_count = effective_turns.len();

        for (target_id, turn) in &effective_turns {
            let advanced_count = effective_turns
                .iter()
                .filter(|(_, other_turn)| other_turn > turn)
                .count();
            if advanced_count == 0 || advanced_count * 2 < player_count {
                continue;
            }
            let Some(index) = self.ensure_legacy_negative_timer_index(target_id) else {
                continue;
            };
            let timer = &mut self.legacy_negative_timers[index];
            if timer.active() || timer.replied {
                continue;
            }
            timer.remaining_ms = LEGACY_NEGATIVE_TIMEOUT_MS;
            timer.replied = false;
            timer.half_warned = false;
            timer.generation = timer.generation.saturating_add(1);
            changed = true;
        }

        changed
    }

    fn ensure_legacy_negative_timer_index(&mut self, target_id: &str) -> Option<usize> {
        let target_id = target_id.trim();
        if target_id.is_empty() {
            return None;
        }
        let is_known = self.players.iter().any(|player_id| player_id == target_id)
            || self
                .legacy_negative_timers
                .iter()
                .any(|timer| timer.target_id == target_id);
        if !is_known {
            return None;
        }

        self.sync_legacy_negative_timers();
        self.legacy_negative_timers
            .iter()
            .position(|timer| timer.target_id == target_id)
    }

    fn reset_all_legacy_negative_timers(&mut self) -> bool {
        let mut changed = false;
        for timer in &mut self.legacy_negative_timers {
            if timer.remaining_ms != 0 || timer.replied || timer.half_warned {
                timer.remaining_ms = 0;
                timer.replied = false;
                timer.half_warned = false;
                timer.generation = timer.generation.saturating_add(1);
                changed = true;
            }
        }
        changed
    }

    pub fn sync_parties(&mut self) -> bool {
        let before_parties = self.parties.clone();
        let before_player_parties = self.player_parties.clone();
        let valid_players = self.players.iter().cloned().collect::<HashSet<_>>();

        for party in self.parties.values_mut() {
            party
                .players
                .retain(|target_id| valid_players.contains(target_id));
            party.players.sort();
            party.players.dedup();
        }

        let mut party_ids = self.parties.keys().cloned().collect::<Vec<_>>();
        party_ids.sort();
        let mut inferred_assignments = Vec::new();
        for party_id in party_ids {
            let Some(party) = self.parties.get(&party_id) else {
                continue;
            };
            for target_id in &party.players {
                inferred_assignments.push((target_id.clone(), party_id.clone()));
            }
        }
        for (target_id, party_id) in inferred_assignments {
            self.player_parties.entry(target_id).or_insert(party_id);
        }

        let existing_party_ids = self.parties.keys().cloned().collect::<HashSet<_>>();
        self.player_parties.retain(|target_id, party_id| {
            valid_players.contains(target_id) && existing_party_ids.contains(party_id)
        });

        for party in self.parties.values_mut() {
            party.players.clear();
        }

        let mut assignments = self
            .player_parties
            .iter()
            .map(|(target_id, party_id)| (target_id.clone(), party_id.clone()))
            .collect::<Vec<_>>();
        assignments.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        for (target_id, party_id) in assignments {
            let Some(party) = self.parties.get_mut(&party_id) else {
                continue;
            };
            if !party.players.contains(&target_id) {
                party.players.push(target_id);
            }
        }
        for party in self.parties.values_mut() {
            party.players.sort();
            party.players.dedup();
        }

        self.parties != before_parties || self.player_parties != before_player_parties
    }

    pub fn ensure_party(&mut self, party_id: &str) -> bool {
        let party_id = party_id.trim();
        if party_id.is_empty() {
            return false;
        }
        if self.parties.contains_key(party_id) {
            return false;
        }

        self.parties.insert(party_id.to_owned(), TrpgParty {
            name: party_id.to_owned(),
            players: Vec::new(),
        });
        true
    }

    pub fn remove_party(&mut self, party_id: &str) -> bool {
        let party_id = party_id.trim();
        if party_id.is_empty() || !self.parties.contains_key(party_id) {
            return false;
        }

        let before_parties = self.parties.clone();
        let before_player_parties = self.player_parties.clone();

        self.parties.remove(party_id);
        self.player_parties
            .retain(|_, assigned| assigned != party_id);
        self.sync_parties();

        self.parties != before_parties || self.player_parties != before_player_parties
    }

    pub fn merge_party(&mut self, from_party_id: &str, to_party_id: &str) -> bool {
        let from_party_id = from_party_id.trim();
        let to_party_id = to_party_id.trim();
        if from_party_id.is_empty()
            || to_party_id.is_empty()
            || from_party_id == to_party_id
            || !self.parties.contains_key(from_party_id)
            || !self.parties.contains_key(to_party_id)
        {
            return false;
        }

        let before_parties = self.parties.clone();
        let before_player_parties = self.player_parties.clone();

        let source_players = self
            .parties
            .get(from_party_id)
            .map(|party| party.players.clone())
            .unwrap_or_default();
        for target_id in source_players {
            if self.players.iter().any(|player_id| player_id == &target_id) {
                self.player_parties
                    .insert(target_id, to_party_id.to_owned());
            }
        }
        for assigned in self.player_parties.values_mut() {
            if assigned == from_party_id {
                *assigned = to_party_id.to_owned();
            }
        }
        self.parties.remove(from_party_id);
        self.sync_parties();

        self.parties != before_parties || self.player_parties != before_player_parties
    }

    pub fn set_player_party(&mut self, target_id: &str, party_id: Option<&str>) -> bool {
        if !self.players.iter().any(|player_id| player_id == target_id) {
            return false;
        }

        let before_parties = self.parties.clone();
        let before_player_parties = self.player_parties.clone();

        for party in self.parties.values_mut() {
            party.players.retain(|player_id| player_id != target_id);
        }

        let party_id = party_id.and_then(|party_id| {
            let party_id = party_id.trim();
            (!party_id.is_empty()).then_some(party_id)
        });

        if let Some(party_id) = party_id {
            self.parties
                .entry(party_id.to_owned())
                .or_insert_with(|| TrpgParty {
                    name: party_id.to_owned(),
                    players: Vec::new(),
                });
            self.player_parties.insert(
                target_id.to_owned(),
                party_id.to_owned(),
            );
            if let Some(party) = self.parties.get_mut(party_id) {
                if !party.players.iter().any(|player_id| player_id == target_id) {
                    party.players.push(target_id.to_owned());
                    party.players.sort();
                }
            }
        } else {
            self.player_parties.remove(target_id);
        }

        self.parties != before_parties || self.player_parties != before_player_parties
    }

    pub fn party_id_for_player(&self, target_id: &str) -> Option<&str> {
        self.player_parties.get(target_id).map(String::as_str)
    }

    pub fn legacy_team(&self, team_id: &str) -> Option<&TrpgLegacyTeam> {
        let team_id = team_id.trim();
        self.legacy_teams
            .iter()
            .find(|team| team.id.trim() == team_id)
    }

    pub fn legacy_team_members(&self, team_id: &str) -> Vec<String> {
        self.legacy_team(team_id)
            .map(|team| {
                let targets = vec![team.id.clone()];
                self.legacy_target_members(&targets)
            })
            .unwrap_or_default()
    }

    pub fn append_legacy_team_chat_message(
        &mut self,
        team_id: &str,
        message: TrpgLegacyTeamChatMessage,
    ) -> bool {
        let Some(team) = self
            .legacy_teams
            .iter_mut()
            .find(|team| team.id.trim() == team_id.trim())
        else {
            return false;
        };
        if message.text.trim().is_empty() {
            return false;
        }
        let before_total = team.chat_message_count.max(team.chat_messages.len());
        team.chat_messages.push(message);
        team.chat_message_count = before_total + 1;
        true
    }

    pub fn update_legacy_team_chat_message(
        &mut self,
        team_id: &str,
        message_index: usize,
        text: &str,
    ) -> bool {
        let Some(team) = self
            .legacy_teams
            .iter_mut()
            .find(|team| team.id.trim() == team_id.trim())
        else {
            return false;
        };
        if text.trim().is_empty() {
            return false;
        }
        let Some(message) = team.chat_messages.get_mut(message_index) else {
            return false;
        };
        if message.text == text {
            return false;
        }
        message.text = text.to_owned();
        true
    }

    pub fn remove_legacy_team_chat_message(&mut self, team_id: &str, message_index: usize) -> bool {
        let Some(team) = self
            .legacy_teams
            .iter_mut()
            .find(|team| team.id.trim() == team_id.trim())
        else {
            return false;
        };
        if message_index >= team.chat_messages.len() {
            return false;
        }
        let before_total = team.chat_message_count.max(team.chat_messages.len());
        team.chat_messages.remove(message_index);
        team.chat_message_count = before_total.saturating_sub(1).max(team.chat_messages.len());
        true
    }

    pub fn legacy_chat_area(&self, area_id: &str) -> Option<&TrpgLegacyArea> {
        let area_id = area_id.trim();
        self.legacy_worlds.iter().find_map(|world| {
            world
                .chat_areas
                .iter()
                .chain(world.areas.iter())
                .find(|area| area.id.trim() == area_id)
        })
    }

    pub fn legacy_send_pane(&self, pane_key: &str) -> Option<&TrpgLegacySendPane> {
        let pane_key = pane_key.trim();
        self.legacy_send_panes
            .iter()
            .find(|pane| pane.key.trim() == pane_key)
    }

    pub fn legacy_send_pane_members(&self, pane_key: &str) -> Vec<String> {
        self.legacy_send_pane(pane_key)
            .map(|pane| {
                self.legacy_target_members(&self.legacy_effective_send_targets(&pane.targets))
            })
            .unwrap_or_default()
    }

    pub fn legacy_send_pane_effective_targets(&self, pane_key: &str) -> Vec<String> {
        self.legacy_send_pane(pane_key)
            .map(|pane| self.legacy_effective_send_targets(&pane.targets))
            .unwrap_or_default()
    }

    pub fn legacy_send_pane_disabled_direct_targets(&self, pane_key: &str) -> Vec<String> {
        self.legacy_send_pane(pane_key)
            .map(|pane| self.legacy_disabled_direct_send_targets(&pane.targets))
            .unwrap_or_default()
    }

    pub fn legacy_send_pane_direct_target_is_covered(
        &self,
        pane_key: &str,
        target_id: &str,
    ) -> bool {
        self.legacy_send_pane(pane_key)
            .map(|pane| {
                self.legacy_direct_target_covered_by_group_targets(&pane.targets, target_id)
            })
            .unwrap_or_default()
    }

    pub fn add_legacy_send_pane(&mut self, title: &str) -> Option<String> {
        let title = {
            let title = title.trim();
            if title.is_empty() {
                "多选发送"
            } else {
                title
            }
        };

        for index in 0..99999 {
            let key = index.to_string();
            if self
                .legacy_send_panes
                .iter()
                .any(|pane| pane.key.trim() == key)
            {
                continue;
            }
            self.legacy_send_panes.push(TrpgLegacySendPane {
                key: key.clone(),
                title: title.to_owned(),
                targets: Vec::new(),
                closable: true,
            });
            return Some(key);
        }

        None
    }

    pub fn remove_legacy_send_pane(&mut self, pane_key: &str) -> bool {
        let pane_key = pane_key.trim();
        let before = self.legacy_send_panes.len();
        self.legacy_send_panes
            .retain(|pane| pane.key.trim() != pane_key || !pane.closable);
        self.legacy_send_panes.len() != before
    }

    pub fn clear_legacy_send_pane_targets(&mut self, pane_key: &str) -> bool {
        let Some(pane_index) = self
            .legacy_send_panes
            .iter()
            .position(|pane| pane.key.trim() == pane_key.trim())
        else {
            return false;
        };
        if self.legacy_send_panes[pane_index].targets.is_empty() {
            return false;
        }
        self.legacy_send_panes[pane_index].targets.clear();
        true
    }

    pub fn set_legacy_send_pane_target(
        &mut self,
        pane_key: &str,
        target_id: &str,
        selected: bool,
    ) -> bool {
        let target_id = target_id.trim();
        if target_id.is_empty() {
            return false;
        }
        let Some(pane_index) = self
            .legacy_send_panes
            .iter()
            .position(|pane| pane.key.trim() == pane_key.trim())
        else {
            return false;
        };

        let before = self.legacy_send_panes[pane_index].targets.clone();
        let mut targets = before.clone();
        if selected {
            if target_id != "0" && targets.iter().any(|target| target.trim() == "0") {
                return false;
            }
            if !targets.iter().any(|target| target.trim() == target_id) {
                targets.push(target_id.to_owned());
            }
        } else {
            targets.retain(|target| target.trim() != target_id);
        }

        let targets = self.legacy_normalized_send_targets(&targets);
        if before == targets {
            return false;
        }
        self.legacy_send_panes[pane_index].targets = targets;
        true
    }

    fn legacy_normalized_send_targets(&self, targets: &[String]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut normalized = Vec::new();
        for target in self.legacy_effective_send_targets(targets) {
            if seen.insert(target.clone()) {
                normalized.push(target);
            }
        }
        normalized
    }

    fn legacy_effective_send_targets(&self, targets: &[String]) -> Vec<String> {
        if targets.iter().any(|target| target.trim() == "0") {
            return vec!["0".to_owned()];
        }

        let disabled_direct_targets = self
            .legacy_disabled_direct_send_targets(targets)
            .into_iter()
            .collect::<HashSet<_>>();

        targets
            .iter()
            .map(|target| target.trim())
            .filter(|target| !target.is_empty())
            .filter(|target| !disabled_direct_targets.contains(*target))
            .map(str::to_owned)
            .collect()
    }

    fn legacy_disabled_direct_send_targets(&self, targets: &[String]) -> Vec<String> {
        let mut disabled = targets
            .iter()
            .map(|target| target.trim())
            .filter(|target| {
                moonberry_exact_u64(target).is_some_and(|id| id > 10000)
                    && self.legacy_direct_target_covered_by_group_targets(targets, target)
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        disabled.sort();
        disabled.dedup();
        disabled
    }

    fn legacy_direct_target_covered_by_group_targets(
        &self,
        targets: &[String],
        target_id: &str,
    ) -> bool {
        let target_id = target_id.trim();
        if !moonberry_exact_u64(target_id).is_some_and(|id| id > 10000) {
            return false;
        }

        for target in targets {
            let target = target.trim();
            if target.is_empty() || target == target_id {
                continue;
            }
            if target == "0" {
                return self.players.iter().any(|player_id| player_id == target_id);
            }
            if let Some(numeric_id) = moonberry_exact_u64(target) {
                if numeric_id < 10000 {
                    if self.legacy_team(target).is_some_and(|team| {
                        team.players.iter().any(|player_id| player_id == target_id)
                    }) {
                        return true;
                    }
                }
                continue;
            }
            if self
                .legacy_chat_area(target)
                .is_some_and(|area| area.members.iter().any(|player_id| player_id == target_id))
            {
                return true;
            }
        }

        false
    }

    pub fn promote_legacy_team_to_party(&mut self, team_id: &str) -> bool {
        let Some(team) = self.legacy_team(team_id).cloned() else {
            return false;
        };
        let party_name = legacy_party_name(&team.name, &team.id, "旧频道");
        self.promote_legacy_members_to_party(&party_name, &team.players)
    }

    pub fn promote_legacy_chat_area_to_party(&mut self, area_id: &str) -> bool {
        let Some(area) = self.legacy_chat_area(area_id).cloned() else {
            return false;
        };
        let party_name = legacy_party_name(&area.name, &area.id, "虚拟讨论组");
        self.promote_legacy_members_to_party(&party_name, &area.members)
    }

    fn promote_legacy_members_to_party(&mut self, party_name: &str, members: &[String]) -> bool {
        let party_name = party_name.trim();
        if party_name.is_empty() {
            return false;
        }

        let before_parties = self.parties.clone();
        let before_player_parties = self.player_parties.clone();

        let mut members = members
            .iter()
            .map(|member| member.trim())
            .filter(|member| self.players.iter().any(|player_id| player_id == *member))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        members.sort();
        members.dedup();
        if members.is_empty() {
            return false;
        }

        self.parties
            .entry(party_name.to_owned())
            .or_insert_with(|| TrpgParty {
                name: party_name.to_owned(),
                players: Vec::new(),
            });

        for member_id in members {
            for party in self.parties.values_mut() {
                party.players.retain(|player_id| player_id != &member_id);
            }
            self.player_parties
                .insert(member_id.clone(), party_name.to_owned());
            if let Some(party) = self.parties.get_mut(party_name) {
                if !party
                    .players
                    .iter()
                    .any(|player_id| player_id == &member_id)
                {
                    party.players.push(member_id);
                }
            }
        }
        self.sync_parties();

        self.parties != before_parties || self.player_parties != before_player_parties
    }

    pub fn legacy_target_members(&self, targets: &[String]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut members = Vec::new();

        for target in targets {
            let target = target.trim();
            if target.is_empty() {
                continue;
            }
            if target == "0" {
                for player_id in &self.players {
                    push_legacy_member(&mut members, &mut seen, player_id);
                }
                continue;
            }

            if let Some(numeric_id) = moonberry_exact_u64(target) {
                if numeric_id < 10000 {
                    if let Some(team) = self.legacy_team(target) {
                        for player_id in &team.players {
                            push_legacy_member_if_group_player(
                                self,
                                &mut members,
                                &mut seen,
                                player_id,
                            );
                        }
                    }
                } else if numeric_id > 10000 {
                    push_legacy_member_if_group_player(self, &mut members, &mut seen, target);
                }
                continue;
            }

            if let Some(area) = self.legacy_chat_area(target) {
                for player_id in &area.members {
                    push_legacy_member_if_group_player(self, &mut members, &mut seen, player_id);
                }
            }
        }

        members
    }

    pub fn player_access(&self, player_id: u64) -> PlayerAccess {
        let target_id = player_id.to_string();
        let is_player = self.players.iter().any(|member_id| member_id == &target_id);
        PlayerAccess {
            player_id,
            character_id: is_player.then_some(target_id.clone()),
            party_id: self.party_id_for_player(&target_id).map(str::to_owned),
            is_gm: self.gm_users.contains(&player_id),
        }
    }

    pub fn mark_player_acted(&mut self, target_id: &str) -> bool {
        self.mark_player_turn(target_id, true)
    }

    pub fn mark_player_skipped(&mut self, target_id: &str) -> bool {
        self.mark_player_turn(target_id, false)
    }

    pub fn reset_current_turn(&mut self) -> bool {
        let mut changed = false;
        for turn in self.player_turns.values_mut() {
            if turn.acted || turn.skipped {
                turn.acted = false;
                turn.skipped = false;
                changed = true;
            }
        }
        changed
    }

    pub fn set_player_turns_passed(&mut self, target_id: &str, turns_passed: u32) -> bool {
        if !self.players.iter().any(|player_id| player_id == target_id) {
            return false;
        }

        self.sync_turn_players();
        let Some(turn) = self.player_turns.get_mut(target_id) else {
            return false;
        };
        if turn.turns_passed == turns_passed {
            return false;
        }

        turn.turns_passed = turns_passed;
        self.refresh_legacy_negative_timers();
        true
    }

    pub fn advance_world_turn(&mut self) -> bool {
        self.sync_turn_players();
        self.world_turn += 1;
        for turn in self.player_turns.values_mut() {
            turn.turns_passed += 1;
            turn.acted = false;
            turn.skipped = false;
        }
        self.reset_all_legacy_negative_timers();
        true
    }

    fn mark_player_turn(&mut self, target_id: &str, acted: bool) -> bool {
        if !self.players.iter().any(|player_id| player_id == target_id) {
            return false;
        }

        let negative_reply_changed =
            if acted { self.register_legacy_negative_reply(target_id) } else { false };
        let sync_changed = self.sync_turn_players();
        let Some(turn) = self.player_turns.get_mut(target_id) else {
            return sync_changed || negative_reply_changed;
        };
        let already_set =
            if acted { turn.acted && !turn.skipped } else { !turn.acted && turn.skipped };
        if already_set {
            return sync_changed || negative_reply_changed;
        }

        if acted {
            turn.acted = true;
            turn.skipped = false;
        } else {
            turn.acted = false;
            turn.skipped = true;
        }

        if self.all_players_finished_turn() {
            self.advance_world_turn();
        } else {
            self.refresh_legacy_negative_timers();
        }
        true
    }

    fn all_players_finished_turn(&self) -> bool {
        !self.players.is_empty()
            && self.players.iter().all(|target_id| {
                self.player_turns
                    .get(target_id)
                    .is_some_and(|turn| turn.acted || turn.skipped)
            })
    }
}

fn push_legacy_member(members: &mut Vec<String>, seen: &mut HashSet<String>, target_id: &str) {
    let target_id = target_id.trim();
    if !target_id.is_empty() && seen.insert(target_id.to_owned()) {
        members.push(target_id.to_owned());
    }
}

fn push_legacy_member_if_group_player(
    group: &TrpgGroup,
    members: &mut Vec<String>,
    seen: &mut HashSet<String>,
    target_id: &str,
) {
    if group.players.iter().any(|player_id| player_id == target_id) {
        push_legacy_member(members, seen, target_id);
    }
}

fn legacy_party_name(name: &str, id: &str, fallback: &str) -> String {
    let name = name.trim();
    if !name.is_empty() {
        name.to_owned()
    } else {
        let id = id.trim();
        if id.is_empty() {
            fallback.to_owned()
        } else {
            format!("{fallback}{id}")
        }
    }
}

#[derive(Resource, Serialize, Deserialize)]
pub struct NapcatMessageManager {
    pub messages: HashMap<String, Vec<NapcatMessage>>,
    #[serde(default)]
    pub chat_targets: HashMap<String, ChatTargetMetadata>,
    #[serde(default)]
    pub player_characters: HashMap<String, PlayerCharacter>,
    #[serde(default)]
    pub trpg_groups: HashMap<String, TrpgGroup>,
    #[serde(default)]
    pub current_trpg_group: Option<String>,
    #[serde(default)]
    pub groups: HashMap<String, ChatGroup>,
    #[serde(default)]
    pub read_message_counts: HashMap<String, usize>,
    #[serde(default)]
    pub summarized_message_counts: HashMap<String, usize>,
    #[serde(default)]
    pub open_chat_targets: HashSet<String>,
    #[serde(default)]
    pub pending_chat_targets: HashSet<String>,
    #[serde(default)]
    pub rejected_chat_targets: HashSet<String>,
    #[serde(default)]
    pub random_pools: HashMap<String, RandomPool>,
    #[serde(default)]
    pub skill_pool: Vec<SkillPoolEntry>,
    #[serde(default)]
    pub unit_pool: HashMap<String, UnitPoolEntry>,
}

pub const NAPCAT_MANAGER_EXPORT_VERSION: u32 = 1;

#[derive(Serialize)]
struct NapcatMessageManagerExportRef<'a> {
    version: u32,
    manager: &'a NapcatMessageManager,
}

#[derive(Deserialize)]
struct NapcatMessageManagerExportOwned {
    version: u32,
    manager: NapcatMessageManager,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerCharacterExportEntry {
    pub target_id: String,
    pub character: PlayerCharacter,
}

#[derive(Serialize, Deserialize)]
struct NapcatPlayerCharactersExport {
    version: u32,
    export_type: String,
    players: Vec<PlayerCharacterExportEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatTargetExportKind {
    Private,
    Group,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatTargetExportEntry {
    pub target_id: String,
    pub kind: ChatTargetExportKind,
    pub metadata: ChatTargetMetadata,
    pub read_message_count: usize,
    pub summarized_message_count: usize,
    pub open: bool,
    pub pending: bool,
    pub rejected: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatGroupExportEntry {
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct NapcatChatListExport {
    version: u32,
    export_type: String,
    targets: Vec<ChatTargetExportEntry>,
    groups: Vec<ChatGroupExportEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnitPoolExportEntry {
    pub unit_id: String,
    pub unit: UnitPoolEntry,
}

#[derive(Serialize, Deserialize)]
struct NapcatUnitPoolExport {
    version: u32,
    export_type: String,
    units: Vec<UnitPoolExportEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoonberryLegacyImportSummary {
    pub groups: usize,
    pub players: usize,
    pub chat_targets: usize,
    pub messages: usize,
    pub skill_pools: usize,
    pub unit_templates: usize,
    pub random_pools: usize,
    pub legacy_teams: usize,
    pub legacy_worlds: usize,
    pub legacy_chat_areas: usize,
    pub legacy_send_panes: usize,
    pub legacy_negative_timers: usize,
}

impl MoonberryLegacyImportSummary {
    fn imported_anything(&self) -> bool {
        self.groups > 0
            || self.players > 0
            || self.chat_targets > 0
            || self.messages > 0
            || self.skill_pools > 0
            || self.unit_templates > 0
            || self.random_pools > 0
            || self.legacy_teams > 0
            || self.legacy_worlds > 0
            || self.legacy_chat_areas > 0
            || self.legacy_send_panes > 0
            || self.legacy_negative_timers > 0
    }
}

fn merge_max_usize(map: &mut HashMap<String, usize>, key: String, value: usize) {
    let entry = map.entry(key).or_default();
    *entry = (*entry).max(value);
}

impl NapcatMessageManager {
    pub fn to_export_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&NapcatMessageManagerExportRef {
            version: NAPCAT_MANAGER_EXPORT_VERSION,
            manager: self,
        })
        .map_err(|err| err.to_string())
    }

    pub fn from_export_json(text: &str) -> Result<Self, String> {
        let export: NapcatMessageManagerExportOwned =
            serde_json::from_str(text).map_err(|err| err.to_string())?;
        if export.version != NAPCAT_MANAGER_EXPORT_VERSION {
            return Err(format!(
                "unsupported NapCat manager export version {}; expected {}",
                export.version, NAPCAT_MANAGER_EXPORT_VERSION
            ));
        }
        Ok(export.manager)
    }

    pub fn to_player_characters_export_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&NapcatPlayerCharactersExport {
            version: NAPCAT_MANAGER_EXPORT_VERSION,
            export_type: "player_characters".to_owned(),
            players: self.player_character_export_entries(),
        })
        .map_err(|err| err.to_string())
    }

    pub fn merge_player_characters_export_json(&mut self, text: &str) -> Result<usize, String> {
        let export: NapcatPlayerCharactersExport =
            serde_json::from_str(text).map_err(|err| err.to_string())?;
        if export.version != NAPCAT_MANAGER_EXPORT_VERSION {
            return Err(format!(
                "unsupported NapCat player character export version {}; expected {}",
                export.version, NAPCAT_MANAGER_EXPORT_VERSION
            ));
        }
        if export.export_type != "player_characters" {
            return Err(format!(
                "unsupported NapCat player character export type {}",
                export.export_type
            ));
        }

        let mut imported = HashMap::new();
        for entry in export.players {
            let target_id = entry.target_id.trim();
            if target_id.is_empty() {
                return Err("player character export contains an empty target id".to_owned());
            }
            imported.insert(target_id.to_owned(), entry.character);
        }

        let imported_count = imported.len();
        self.player_characters.extend(imported);
        Ok(imported_count)
    }

    pub fn to_chat_list_export_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&NapcatChatListExport {
            version: NAPCAT_MANAGER_EXPORT_VERSION,
            export_type: "chat_list".to_owned(),
            targets: self.chat_target_export_entries(),
            groups: self.chat_group_export_entries(),
        })
        .map_err(|err| err.to_string())
    }

    pub fn merge_chat_list_export_json(&mut self, text: &str) -> Result<usize, String> {
        let export: NapcatChatListExport =
            serde_json::from_str(text).map_err(|err| err.to_string())?;
        if export.version != NAPCAT_MANAGER_EXPORT_VERSION {
            return Err(format!(
                "unsupported NapCat chat list export version {}; expected {}",
                export.version, NAPCAT_MANAGER_EXPORT_VERSION
            ));
        }
        if export.export_type != "chat_list" {
            return Err(format!(
                "unsupported NapCat chat list export type {}",
                export.export_type
            ));
        }

        let mut imported_targets = 0;
        for entry in export.targets {
            let target_id = entry.target_id.trim();
            if target_id.is_empty() {
                return Err("chat list export contains an empty target id".to_owned());
            }
            let target_id = target_id.to_owned();
            self.chat_targets.insert(target_id.clone(), entry.metadata);
            merge_max_usize(
                &mut self.read_message_counts,
                target_id.clone(),
                entry.read_message_count,
            );
            merge_max_usize(
                &mut self.summarized_message_counts,
                target_id.clone(),
                entry.summarized_message_count,
            );
            self.apply_imported_chat_window_state(
                &target_id,
                entry.open,
                entry.pending,
                entry.rejected,
            );
            imported_targets += 1;
        }

        for group in export.groups {
            let name = group.name.trim();
            if name.is_empty() {
                return Err("chat list export contains an empty chat group name".to_owned());
            }
            let mut members = group
                .members
                .into_iter()
                .map(|member| member.trim().to_owned())
                .filter(|member| !member.is_empty())
                .collect::<Vec<_>>();
            members.sort();
            members.dedup();
            self.groups.insert(name.to_owned(), ChatGroup { members });
        }

        Ok(imported_targets)
    }

    pub fn to_unit_pool_export_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&NapcatUnitPoolExport {
            version: NAPCAT_MANAGER_EXPORT_VERSION,
            export_type: "unit_pool".to_owned(),
            units: self.unit_pool_export_entries(),
        })
        .map_err(|err| err.to_string())
    }

    pub fn merge_unit_pool_export_json(&mut self, text: &str) -> Result<usize, String> {
        let export: NapcatUnitPoolExport =
            serde_json::from_str(text).map_err(|err| err.to_string())?;
        if export.version != NAPCAT_MANAGER_EXPORT_VERSION {
            return Err(format!(
                "unsupported NapCat unit pool export version {}; expected {}",
                export.version, NAPCAT_MANAGER_EXPORT_VERSION
            ));
        }
        if export.export_type != "unit_pool" {
            return Err(format!(
                "unsupported NapCat unit pool export type {}",
                export.export_type
            ));
        }

        let mut imported = HashMap::new();
        for entry in export.units {
            let unit_id = entry.unit_id.trim();
            if unit_id.is_empty() {
                return Err("unit pool export contains an empty unit id".to_owned());
            }
            imported.insert(unit_id.to_owned(), entry.unit);
        }

        let imported_count = imported.len();
        self.unit_pool.extend(imported);
        Ok(imported_count)
    }

    pub fn unit_pool_ids_for_legacy_members(&self, members: &[String]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();

        for member in members {
            let member = member.trim();
            if member.is_empty() {
                continue;
            }

            if self.unit_pool.contains_key(member) && seen.insert(member.to_owned()) {
                resolved.push(member.to_owned());
            }

            let mut alias_matches = self
                .unit_pool
                .iter()
                .filter(|(unit_id, unit)| {
                    unit_id.as_str() != member && unit.legacy_member_id.as_deref() == Some(member)
                })
                .map(|(unit_id, _)| unit_id.clone())
                .collect::<Vec<_>>();
            alias_matches.sort();
            for unit_id in alias_matches {
                if seen.insert(unit_id.clone()) {
                    resolved.push(unit_id);
                }
            }

            let fallback_id = format!("moonberry-unit-{member}");
            if self.unit_pool.contains_key(&fallback_id) && seen.insert(fallback_id.clone()) {
                resolved.push(fallback_id);
            }
        }

        resolved
    }

    pub fn merge_moonberry_legacy_json(
        &mut self,
        text: &str,
    ) -> Result<MoonberryLegacyImportSummary, String> {
        let value: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
        let mut summary = MoonberryLegacyImportSummary::default();
        let mut recognized_shape = false;

        if let Some(skill_pools) = value.get("skillsPool").and_then(Value::as_array) {
            recognized_shape = true;
            self.merge_moonberry_skill_pool(skill_pools, &mut summary)?;
        }

        let root_order_by_turn = value
            .get("config")
            .and_then(|config| moonberry_bool_field(config, "orderByTurn"));
        let root_negative_enabled = value
            .get("config")
            .and_then(|config| moonberry_bool_field(config, "negative"));

        if let Some(groups) = value.get("groups").and_then(Value::as_array) {
            recognized_shape = true;
            let current_group_index = moonberry_usize_field(&value, "currentGroup");
            let mut imported_group_names = Vec::new();
            for (index, group) in groups.iter().enumerate() {
                let group_name = moonberry_group_name(group, index);
                self.merge_moonberry_group(
                    &group_name,
                    group,
                    root_order_by_turn,
                    root_negative_enabled,
                    &mut summary,
                );
                imported_group_names.push(group_name);
            }
            if let Some(index) = current_group_index {
                if let Some(group_name) = imported_group_names.get(index) {
                    self.current_trpg_group = Some(group_name.clone());
                }
            } else if self.current_trpg_group.is_none() {
                if let Some(group_name) = imported_group_names.first() {
                    self.current_trpg_group = Some(group_name.clone());
                }
            }
        }

        if let Some(units) = value.get("unitPool").and_then(Value::as_array) {
            recognized_shape = true;
            self.merge_moonberry_unit_pool(units, &mut summary)?;
        }

        if let Some(pools) = value.get("randomPool").and_then(Value::as_array) {
            recognized_shape = true;
            self.merge_moonberry_random_pool(pools, &mut summary)?;
        }

        let has_bundle_pcs = value.get("Pcs").and_then(Value::as_array).is_some();
        let has_bundle_chatlists = value.get("chatlists").and_then(Value::as_array).is_some();
        let has_bundle_messages = value.get("chatMsgs").and_then(Value::as_array).is_some();
        if has_bundle_pcs || has_bundle_chatlists || has_bundle_messages {
            recognized_shape = true;
            let group_name = "月莓导入".to_owned();
            self.trpg_groups.entry(group_name.clone()).or_default();
            if self.current_trpg_group.is_none() {
                self.current_trpg_group = Some(group_name.clone());
            }
            if let Some(pcs) = value.get("Pcs").and_then(Value::as_array) {
                self.merge_moonberry_pcs(&group_name, pcs, &mut summary);
            }
            if let Some(chatlists) = value.get("chatlists").and_then(Value::as_array) {
                self.merge_moonberry_chatlists(&group_name, chatlists, &mut summary);
            }
            if let Some(messages) = value.get("chatMsgs").and_then(Value::as_array) {
                self.merge_moonberry_chat_messages(&group_name, messages, &mut summary);
            }
        }

        if !recognized_shape {
            return Err("未识别的月莓旧JSON格式".to_owned());
        }
        if !summary.imported_anything() {
            return Err("月莓旧JSON里没有可导入的数据".to_owned());
        }
        Ok(summary)
    }

    fn merge_moonberry_group(
        &mut self,
        group_name: &str,
        group: &Value,
        root_order_by_turn: Option<bool>,
        root_negative_enabled: Option<bool>,
        summary: &mut MoonberryLegacyImportSummary,
    ) {
        let description = moonberry_string_field(group, "description");
        let st_description = moonberry_string_field(group, "stDesc");
        let guide = moonberry_string_field(group, "guide");
        let run_times = moonberry_u32_field(group, "runTimes");
        let battle_sort_by_turn = moonberry_bool_field(group, "orderByTurn").or(root_order_by_turn);
        let battle_negative_enabled =
            moonberry_bool_field(group, "negativeEnabled").or(root_negative_enabled);
        let legacy_negative_timers = group
            .get("negative")
            .and_then(Value::as_array)
            .map(|timers| moonberry_legacy_negative_timers(timers));
        let initial_status_points = group
            .get("basicConfig")
            .and_then(|config| moonberry_i32_field(config, "initStatusPoint"))
            .or_else(|| moonberry_i32_field(group, "initStatusPoint"));
        let initial_exchange_points = group
            .get("basicConfig")
            .and_then(|config| moonberry_i32_field(config, "initExchangePoint"))
            .or_else(|| moonberry_i32_field(group, "initExchangePoint"));
        let basic_config = group.get("basicConfig").map(moonberry_basic_config);
        let legacy_teams = group
            .get("currentTeams")
            .and_then(Value::as_array)
            .map(|teams| moonberry_legacy_teams(teams));
        let legacy_worlds = group
            .get("currentWorlds")
            .and_then(Value::as_array)
            .map(|worlds| moonberry_legacy_worlds(worlds));
        let legacy_send_panes = group
            .get("currentSendPanes")
            .and_then(Value::as_array)
            .map(|panes| moonberry_legacy_send_panes(panes));

        {
            let trpg_group = self.trpg_groups.entry(group_name.to_owned()).or_default();
            trpg_group.campaign_id = group_name.to_owned();
            if let Some(description) = description {
                trpg_group.description = description;
            }
            if let Some(st_description) = st_description {
                trpg_group.st_description = st_description;
            }
            if let Some(guide) = guide {
                trpg_group.guide = guide;
            }
            if let Some(points) = initial_status_points {
                trpg_group.initial_status_points = points;
            }
            if let Some(points) = initial_exchange_points {
                trpg_group.initial_exchange_points = points;
            }
            if let Some(config) = basic_config {
                trpg_group.basic_config = config;
            }
            if let Some(run_times) = run_times {
                trpg_group.run_times = run_times;
            }
            if let Some(sort_by_turn) = battle_sort_by_turn {
                trpg_group.battle_sort_by_turn = sort_by_turn;
            }
            if let Some(negative_enabled) = battle_negative_enabled {
                trpg_group.battle_negative_enabled = negative_enabled;
            }
            if let Some(timers) = legacy_negative_timers {
                summary.legacy_negative_timers += timers.len();
                trpg_group.legacy_negative_count = timers.len();
                trpg_group.legacy_negative_timers = timers;
            }
            if let Some(teams) = legacy_teams {
                summary.legacy_teams += teams.len();
                trpg_group.legacy_teams = teams;
            }
            if let Some(worlds) = legacy_worlds {
                summary.legacy_worlds += worlds.len();
                summary.legacy_chat_areas += worlds
                    .iter()
                    .map(|world| world.chat_areas.len() + world.areas.len())
                    .sum::<usize>();
                trpg_group.legacy_worlds = worlds;
            }
            if let Some(send_panes) = legacy_send_panes {
                summary.legacy_send_panes += send_panes.len();
                trpg_group.legacy_send_panes = send_panes;
            }
            sync_legacy_surface_players(trpg_group);
            trpg_group.sync_turn_players();
            trpg_group.sync_legacy_negative_timers();
            trpg_group.sync_parties();
        }
        summary.groups += 1;

        if let Some(pcs) = group.get("pc").and_then(Value::as_array) {
            self.merge_moonberry_pcs(group_name, pcs, summary);
        }
        if let Some(chatlists) = group.get("currentChatList").and_then(Value::as_array) {
            self.merge_moonberry_chatlists(group_name, chatlists, summary);
        }
        if let Some(messages) = group.get("chatMsg").and_then(Value::as_array) {
            self.merge_moonberry_chat_messages(group_name, messages, summary);
        }
    }

    fn merge_moonberry_pcs(
        &mut self,
        group_name: &str,
        pcs: &[Value],
        summary: &mut MoonberryLegacyImportSummary,
    ) {
        let mut imported_player_ids = Vec::new();
        for pc in pcs {
            let Some(target_id) = moonberry_pc_target_id(pc) else {
                continue;
            };
            let character = moonberry_pc_to_character(pc);
            let display_name = moonberry_character_display_name(&target_id, &character);
            self.player_characters.insert(target_id.clone(), character);
            self.chat_targets
                .entry(target_id.clone())
                .or_default()
                .display_name = display_name;
            self.open_chat_targets.insert(target_id.clone());
            imported_player_ids.push(target_id);
            summary.players += 1;
        }
        if let Some(group) = self.trpg_groups.get_mut(group_name) {
            for target_id in imported_player_ids {
                if !group
                    .players
                    .iter()
                    .any(|player_id| player_id == &target_id)
                {
                    group.players.push(target_id);
                }
            }
            group.sync_turn_players();
            group.sync_legacy_negative_timers();
            group.sync_parties();
        }
    }

    fn merge_moonberry_chatlists(
        &mut self,
        group_name: &str,
        chatlists: &[Value],
        summary: &mut MoonberryLegacyImportSummary,
    ) {
        let mut player_ids = Vec::new();
        for item in chatlists {
            let Some(target_id) = moonberry_target_id_field(item, "Id") else {
                continue;
            };
            if target_id == "0" {
                continue;
            }
            let display_name = moonberry_string_field(item, "nickName")
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| target_id.clone());
            self.chat_targets
                .entry(target_id.clone())
                .or_default()
                .display_name = display_name;
            self.open_chat_targets.insert(target_id.clone());
            if let Some(count) = moonberry_usize_field(item, "notReadCount") {
                merge_max_usize(
                    &mut self.read_message_counts,
                    target_id.clone(),
                    count,
                );
            }
            player_ids.push(target_id);
            summary.chat_targets += 1;
        }
        if let Some(group) = self.trpg_groups.get_mut(group_name) {
            for target_id in player_ids {
                if !group
                    .players
                    .iter()
                    .any(|player_id| player_id == &target_id)
                {
                    group.players.push(target_id);
                }
            }
            group.sync_turn_players();
            group.sync_legacy_negative_timers();
            group.sync_parties();
        }
    }

    fn merge_moonberry_chat_messages(
        &mut self,
        group_name: &str,
        messages: &[Value],
        summary: &mut MoonberryLegacyImportSummary,
    ) {
        for message in messages {
            let Some((target_id, napcat_message)) =
                moonberry_chat_to_napcat_message(group_name, message)
            else {
                continue;
            };
            self.chat_targets
                .entry(target_id.clone())
                .or_default()
                .automatic_name = napcat_message.data.sender.nickname.clone();
            self.open_chat_targets.insert(target_id.clone());
            self.messages
                .entry(target_id)
                .or_default()
                .push(napcat_message);
            summary.messages += 1;
        }
    }

    fn merge_moonberry_unit_pool(
        &mut self,
        units: &[Value],
        summary: &mut MoonberryLegacyImportSummary,
    ) -> Result<(), String> {
        for unit in units {
            let unit_id = moonberry_string_field(unit, "id")
                .filter(|id| !id.trim().is_empty())
                .or_else(|| {
                    unit.get("Pc")
                        .and_then(moonberry_pc_target_id)
                        .map(|id| format!("moonberry-unit-{id}"))
                })
                .ok_or_else(|| "月莓单位池包含缺少id的单位".to_owned())?;
            let pc = unit
                .get("Pc")
                .ok_or_else(|| format!("月莓单位池 {unit_id} 缺少Pc数据"))?;
            let legacy_member_id = moonberry_pc_target_id(pc);
            let character = moonberry_pc_to_character(pc);
            let label = moonberry_character_display_name(&unit_id, &character);
            let mut note_parts = Vec::new();
            if let Some(desc) = moonberry_string_field(unit, "desc") {
                if !desc.trim().is_empty() {
                    note_parts.push(desc);
                }
            }
            if let Some(tags) = moonberry_string_field(unit, "tags") {
                if !tags.trim().is_empty() {
                    note_parts.push(format!("标签：{tags}"));
                }
            }
            self.unit_pool.insert(unit_id, UnitPoolEntry {
                label,
                note: note_parts.join("\n"),
                legacy_member_id,
                character,
            });
            summary.unit_templates += 1;
        }
        Ok(())
    }

    fn merge_moonberry_skill_pool(
        &mut self,
        pools: &[Value],
        summary: &mut MoonberryLegacyImportSummary,
    ) -> Result<(), String> {
        for (index, pool) in pools.iter().enumerate() {
            let legacy_pool_id =
                moonberry_string_field(pool, "id").filter(|id| !id.trim().is_empty());
            let name = moonberry_string_field(pool, "name")
                .or_else(|| legacy_pool_id.clone())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| format!("月莓技能池{}", index + 1));
            let note = moonberry_string_field(pool, "desc").unwrap_or_default();
            let tags = moonberry_string_field(pool, "tags")
                .map(|tags| moonberry_split_tags(&tags))
                .unwrap_or_default();
            let args = pool
                .get("args")
                .and_then(Value::as_array)
                .map(|args| moonberry_skill_pool_args(args))
                .unwrap_or_default();
            let legacy_buff_count = pool
                .get("buff")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            let legacy_event_buff_count = pool
                .get("eventBuffs")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            let legacy_buff_json = moonberry_legacy_json_field(pool, "buff");
            let legacy_event_buff_json = moonberry_legacy_json_field(pool, "eventBuffs");
            let legacy_graph_json = moonberry_legacy_json_field(pool, "graph");
            let legacy_has_graph = legacy_graph_json.is_some();
            let entry = SkillPoolEntry {
                name,
                note,
                mp_cost: 0.0,
                cooldown_turns: 0,
                source_character_id: None,
                source_character_name: None,
                source_skill_index: None,
                legacy_pool_id: legacy_pool_id.clone(),
                tags,
                category: pool.get("type").and_then(moonberry_skill_pool_type_label),
                legacy_group: pool
                    .get("group")
                    .and_then(moonberry_scalar_to_string)
                    .filter(|group| !group.trim().is_empty()),
                created_at: moonberry_string_field(pool, "createdAt")
                    .filter(|created_at| !created_at.trim().is_empty()),
                args,
                legacy_buff_count,
                legacy_event_buff_count,
                legacy_has_graph,
                legacy_buff_json,
                legacy_event_buff_json,
                legacy_graph_json,
                legacy_buff_machine_json: None,
            };
            if let Some(legacy_pool_id) = legacy_pool_id.as_deref() {
                self.skill_pool
                    .retain(|existing| existing.legacy_pool_id.as_deref() != Some(legacy_pool_id));
            }
            self.skill_pool.push(entry);
            summary.skill_pools += 1;
        }
        Ok(())
    }

    fn merge_moonberry_random_pool(
        &mut self,
        pools: &[Value],
        summary: &mut MoonberryLegacyImportSummary,
    ) -> Result<(), String> {
        for pool in pools {
            let legacy_pool_id =
                moonberry_string_field(pool, "id").filter(|id| !id.trim().is_empty());
            let pool_name = moonberry_string_field(pool, "name")
                .or_else(|| legacy_pool_id.clone())
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| "月莓随机池包含缺少名称的池".to_owned())?;
            let tags = moonberry_string_field(pool, "tags").unwrap_or_default();
            let description = moonberry_string_field(pool, "desc").unwrap_or_default();
            let created_at = moonberry_string_field(pool, "createdAt").unwrap_or_default();
            let legacy_group = moonberry_i32_field(pool, "group");
            let items = pool
                .get("IRandomItem")
                .or_else(|| pool.get("IRandomItems"))
                .and_then(Value::as_array)
                .ok_or_else(|| format!("月莓随机池 {pool_name} 缺少随机项"))?;
            let mut entries = Vec::new();
            for item in items {
                let item_name = moonberry_string_field(item, "key")
                    .or_else(|| moonberry_string_field(item, "name"))
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| format!("随机项{}", entries.len() + 1));
                let result_text = moonberry_string_field(item, "RandomItemDesc")
                    .or_else(|| moonberry_string_field(item, "desc"))
                    .unwrap_or_default();
                let min_count = moonberry_u32_field(item, "min").unwrap_or(1);
                let max_count = moonberry_u32_field(item, "max").unwrap_or(min_count);
                let (min_count, max_count) = normalized_random_pool_counts(min_count, max_count);
                let inventory_item = InventoryItem {
                    name: item_name,
                    description: result_text.clone(),
                    stack: min_count.max(1),
                    max_stack: max_count.max(1),
                    ..Default::default()
                };
                entries.push(RandomPoolEntry {
                    item: inventory_item,
                    weight: default_random_pool_weight(),
                    enabled: true,
                    result_text,
                    min_count,
                    max_count,
                });
            }
            let pool = RandomPool {
                entries,
                last_pick: None,
                last_text_result: None,
                legacy_pool_id,
                legacy_group,
                tags,
                description,
                created_at,
                checked_results: Vec::new(),
            };
            self.random_pools.insert(pool_name, pool);
            summary.random_pools += 1;
        }
        Ok(())
    }

    pub fn player_character_export_entries(&self) -> Vec<PlayerCharacterExportEntry> {
        let mut entries = self
            .player_characters
            .iter()
            .map(
                |(target_id, character)| PlayerCharacterExportEntry {
                    target_id: target_id.clone(),
                    character: character.clone(),
                },
            )
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.target_id.cmp(&right.target_id));
        entries
    }

    pub fn unit_pool_export_entries(&self) -> Vec<UnitPoolExportEntry> {
        let mut entries = self
            .unit_pool
            .iter()
            .map(|(unit_id, unit)| UnitPoolExportEntry {
                unit_id: unit_id.clone(),
                unit: unit.clone(),
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.unit_id.cmp(&right.unit_id));
        entries
    }

    pub fn chat_target_export_entries(&self) -> Vec<ChatTargetExportEntry> {
        let mut target_ids = self
            .chat_targets
            .keys()
            .chain(self.messages.keys())
            .chain(self.read_message_counts.keys())
            .chain(self.summarized_message_counts.keys())
            .chain(self.open_chat_targets.iter())
            .chain(self.pending_chat_targets.iter())
            .chain(self.rejected_chat_targets.iter())
            .cloned()
            .collect::<Vec<_>>();
        target_ids.sort();
        target_ids.dedup();

        target_ids
            .into_iter()
            .map(|target_id| ChatTargetExportEntry {
                kind: self.chat_target_export_kind(&target_id),
                metadata: self
                    .chat_targets
                    .get(&target_id)
                    .cloned()
                    .unwrap_or_default(),
                read_message_count: self
                    .read_message_counts
                    .get(&target_id)
                    .copied()
                    .unwrap_or_default(),
                summarized_message_count: self
                    .summarized_message_counts
                    .get(&target_id)
                    .copied()
                    .unwrap_or_default(),
                open: self.open_chat_targets.contains(&target_id),
                pending: self.pending_chat_targets.contains(&target_id),
                rejected: self.rejected_chat_targets.contains(&target_id),
                target_id,
            })
            .collect()
    }

    pub fn chat_group_export_entries(&self) -> Vec<ChatGroupExportEntry> {
        let mut groups = self
            .groups
            .iter()
            .map(|(name, group)| ChatGroupExportEntry {
                name: name.clone(),
                members: group.members.clone(),
            })
            .collect::<Vec<_>>();
        groups.sort_by(|left, right| left.name.cmp(&right.name));
        groups
    }

    fn chat_target_export_kind(&self, target_id: &str) -> ChatTargetExportKind {
        match self
            .messages
            .get(target_id)
            .and_then(|messages| messages.first())
            .map(|message| &message.data.message_type)
        {
            Some(NapcatMessageType::Private) => ChatTargetExportKind::Private,
            Some(NapcatMessageType::Group) => ChatTargetExportKind::Group,
            None => ChatTargetExportKind::Unknown,
        }
    }

    fn apply_imported_chat_window_state(
        &mut self,
        target_id: &str,
        open: bool,
        pending: bool,
        rejected: bool,
    ) {
        if rejected {
            self.rejected_chat_targets.insert(target_id.to_owned());
            self.open_chat_targets.remove(target_id);
            self.pending_chat_targets.remove(target_id);
        } else if open {
            self.open_chat_targets.insert(target_id.to_owned());
            self.pending_chat_targets.remove(target_id);
            self.rejected_chat_targets.remove(target_id);
        } else if pending {
            self.pending_chat_targets.insert(target_id.to_owned());
            self.open_chat_targets.remove(target_id);
            self.rejected_chat_targets.remove(target_id);
        }
    }

    pub fn migrate_chat_window_state(&mut self) -> bool {
        if !self.open_chat_targets.is_empty() || !self.pending_chat_targets.is_empty() {
            return false;
        }

        if !self.chat_targets.is_empty() {
            return false;
        }

        if self.messages.is_empty() {
            return false;
        }

        let before_len = self.open_chat_targets.len();
        self.open_chat_targets.extend(
            self.messages
                .keys()
                .filter(|target_id| !self.rejected_chat_targets.contains(*target_id))
                .cloned(),
        );
        self.open_chat_targets.len() != before_len
    }

    pub fn sync_chat_targets(&mut self) -> bool {
        let mut changed = false;
        for target_id in self.messages.keys() {
            if !self.chat_targets.contains_key(target_id) {
                self.chat_targets.insert(
                    target_id.clone(),
                    ChatTargetMetadata::default(),
                );
                changed = true;
            }
            if is_private_message_target(self.messages.get(target_id))
                && !self.player_characters.contains_key(target_id)
            {
                self.player_characters.insert(
                    target_id.clone(),
                    PlayerCharacter::default(),
                );
                changed = true;
            }
        }
        let character_len = self.player_characters.len();
        self.player_characters
            .retain(|target_id, _| is_private_message_target(self.messages.get(target_id)));
        changed |= character_len != self.player_characters.len();

        for group in self.trpg_groups.values_mut() {
            let player_len = group.players.len();
            group
                .players
                .retain(|target_id| self.messages.contains_key(target_id));
            changed |= player_len != group.players.len();

            let group_chat_len = group.group_chats.len();
            group
                .group_chats
                .retain(|target_id| self.messages.contains_key(target_id));
            changed |= group_chat_len != group.group_chats.len();

            changed |= group.sync_turn_players();
            changed |= group.sync_legacy_negative_timers();
            changed |= group.sync_parties();
        }
        if self
            .current_trpg_group
            .as_ref()
            .is_some_and(|group_name| !self.trpg_groups.contains_key(group_name))
        {
            self.current_trpg_group = None;
            changed = true;
        }
        if self.current_trpg_group.is_none() && self.trpg_groups.len() == 1 {
            if let Some(group_name) = self.trpg_groups.keys().next().cloned() {
                self.current_trpg_group = Some(group_name);
                changed = true;
            }
        }
        changed
    }

    pub fn current_group(&self) -> Option<&TrpgGroup> {
        self.current_trpg_group
            .as_deref()
            .and_then(|group_name| self.trpg_groups.get(group_name))
    }

    pub fn current_group_for_player(&self, target_id: &str) -> Option<&TrpgGroup> {
        self.current_group()
            .filter(|group| group.players.iter().any(|player_id| player_id == target_id))
    }

    pub fn group_for_player_target(&self, target_id: &str) -> Option<&TrpgGroup> {
        if let Some(group) = self.current_group_for_player(target_id) {
            return Some(group);
        }

        let mut groups = self
            .trpg_groups
            .values()
            .filter(|group| group.players.iter().any(|player_id| player_id == target_id));
        let group = groups.next()?;
        groups.next().is_none().then_some(group)
    }

    fn group_for_message_target(
        &self,
        target_id: &str,
        message: &NapcatMessage,
    ) -> Option<&TrpgGroup> {
        let peer_id = if message.data.user_id == message.data.self_id {
            message
                .data
                .target_id
                .or_else(|| target_id.parse::<u64>().ok())
                .unwrap_or(message.data.user_id)
        } else {
            message.data.user_id
        };
        let peer_target_id = peer_id.to_string();
        let matches_target = |group: &&TrpgGroup| match message.data.message_type {
            NapcatMessageType::Private => group
                .players
                .iter()
                .any(|player_id| player_id == &peer_target_id),
            NapcatMessageType::Group => group
                .group_chats
                .iter()
                .any(|group_id| group_id == target_id),
        };

        if let Some(group) = self.current_group().filter(matches_target) {
            return Some(group);
        }

        let mut groups = self.trpg_groups.values().filter(matches_target);
        let group = groups.next()?;
        groups.next().is_none().then_some(group)
    }

    pub fn register_legacy_negative_reply(&mut self, target_id: &str) -> bool {
        let mut changed = false;
        for group in self.trpg_groups.values_mut() {
            if group.players.iter().any(|player_id| player_id == target_id)
                || group
                    .legacy_negative_timers
                    .iter()
                    .any(|timer| timer.target_id == target_id)
            {
                changed |= group.register_legacy_negative_reply(target_id);
            }
        }
        changed
    }

    pub fn character_creation_config_for_target(&self, target_id: &str) -> (i32, i32) {
        self.group_for_player_target(target_id)
            .map(|group| {
                (
                    group.initial_status_points.max(0),
                    group.initial_exchange_points.max(0),
                )
            })
            .unwrap_or_else(|| {
                (
                    default_status_points(),
                    default_exchange_points(),
                )
            })
    }

    pub fn character_stat_config_for_target(&self, target_id: &str) -> TrpgBasicConfig {
        self.group_for_player_target(target_id)
            .map(|group| group.basic_config)
            .unwrap_or_default()
    }

    pub fn current_campaign_id(&self) -> String {
        self.current_group()
            .map(|group| group.campaign_id.trim())
            .filter(|campaign_id| !campaign_id.is_empty())
            .unwrap_or("default")
            .to_owned()
    }

    pub fn player_access_for_user(&self, player_id: u64) -> PlayerAccess {
        self.current_group()
            .map(|group| group.player_access(player_id))
            .unwrap_or(PlayerAccess {
                player_id,
                ..Default::default()
            })
    }

    pub fn gm_access(&self) -> PlayerAccess {
        PlayerAccess {
            is_gm: true,
            ..Default::default()
        }
    }

    pub fn annotate_message_access(&self, target_id: &str, message: &mut NapcatMessage) {
        let campaign_message = self.campaign_message_for_target(target_id, message);
        message.data.campaign_id = campaign_message.campaign_id;
        message.data.character_id = campaign_message.character_id;
        message.data.party_id = campaign_message.party_id;
        message.data.visibility = campaign_message.visibility;
    }

    pub fn annotate_incoming_message_access(&self, target_id: &str, message: &mut NapcatMessage) {
        message.data.campaign_id.clear();
        message.data.character_id = None;
        message.data.party_id = None;
        message.data.visibility = Visibility::Public;
        self.annotate_message_access(target_id, message);
    }

    pub fn visible_campaign_messages_for_summary(
        &self,
        target_id: &str,
        messages: &[NapcatMessage],
    ) -> Vec<CampaignMessage> {
        let campaign_id = self.current_campaign_id();
        let access = if is_group_message_target(messages) {
            self.gm_access()
        } else {
            target_id
                .parse::<u64>()
                .map(|player_id| self.player_access_for_user(player_id))
                .unwrap_or_else(|_| self.gm_access())
        };

        messages
            .iter()
            .filter(|message| message.data.user_id != message.data.self_id)
            .map(|message| self.campaign_message_for_target(target_id, message))
            .filter(|message| message.campaign_id == campaign_id)
            .filter(|message| access.can_read(&message.visibility))
            .collect()
    }

    pub fn visible_messages_for_player(
        &self,
        target_id: &str,
        messages: &[NapcatMessage],
        player_id: u64,
    ) -> Vec<NapcatMessage> {
        let campaign_id = self.current_campaign_id();
        let access = self.player_access_for_user(player_id);
        messages
            .iter()
            .filter(|message| {
                let campaign_message = self.campaign_message_for_target(target_id, message);
                campaign_message.campaign_id == campaign_id
                    && access.can_read(&campaign_message.visibility)
            })
            .cloned()
            .collect()
    }

    pub fn campaign_message_for_target(
        &self,
        target_id: &str,
        message: &NapcatMessage,
    ) -> CampaignMessage {
        let text = message_text(message);
        let message_group = self.group_for_message_target(target_id, message);
        let campaign_id = if message.data.campaign_id.trim().is_empty() {
            message_group
                .map(|group| group.campaign_id.trim())
                .filter(|campaign_id| !campaign_id.is_empty())
                .unwrap_or("default")
                .to_owned()
        } else {
            message.data.campaign_id.clone()
        };

        match message.data.message_type {
            NapcatMessageType::Private => {
                let peer_id = if message.data.user_id == message.data.self_id {
                    message
                        .data
                        .target_id
                        .or_else(|| target_id.parse::<u64>().ok())
                        .unwrap_or(message.data.user_id)
                } else {
                    message.data.user_id
                };
                let access = message_group
                    .map(|group| group.player_access(peer_id))
                    .unwrap_or(PlayerAccess {
                        player_id: peer_id,
                        ..Default::default()
                    });
                CampaignMessage {
                    campaign_id,
                    sender_id: message.data.user_id,
                    sender_name: message.data.sender.nickname.clone(),
                    source: MessageSource::Friend { user_id: peer_id },
                    character_id: access.character_id,
                    party_id: access.party_id,
                    visibility: Visibility::Player(peer_id),
                    text,
                    time: message.data.time,
                }
            },
            NapcatMessageType::Group => {
                let group_id = message
                    .data
                    .group_id
                    .or_else(|| target_id.parse::<u64>().ok())
                    .unwrap_or_default();
                let has_persisted_access = message.data.character_id.is_some()
                    || message.data.party_id.is_some()
                    || !matches!(
                        message.data.visibility,
                        Visibility::Public
                    );
                let (character_id, party_id, visibility) = if has_persisted_access {
                    let party_id = message.data.party_id.clone();
                    let visibility = match (&message.data.visibility, &party_id) {
                        (Visibility::Public, Some(party_id)) => Visibility::Party(party_id.clone()),
                        (visibility, _) => visibility.clone(),
                    };
                    (
                        message.data.character_id.clone(),
                        party_id,
                        visibility,
                    )
                } else {
                    // Legacy messages predate persisted access metadata, so derive from the same
                    // configured target mapping used at ingest. New messages are saved annotated.
                    let access = message_group
                        .map(|group| group.player_access(message.data.user_id))
                        .unwrap_or(PlayerAccess {
                            player_id: message.data.user_id,
                            ..Default::default()
                        });
                    let visibility = access
                        .party_id
                        .as_ref()
                        .map(|party_id| Visibility::Party(party_id.clone()))
                        .unwrap_or(Visibility::Public);
                    (
                        access.character_id,
                        access.party_id,
                        visibility,
                    )
                };
                CampaignMessage {
                    campaign_id,
                    sender_id: message.data.user_id,
                    sender_name: message.data.sender.nickname.clone(),
                    source: MessageSource::Group {
                        group_id,
                        user_id: message.data.user_id,
                    },
                    character_id,
                    party_id,
                    visibility,
                    text,
                    time: message.data.time,
                }
            },
        }
    }

    pub fn sync_skill_pool_from_completed_characters(&mut self) -> bool {
        let mut next_auto_entries = self
            .player_characters
            .iter()
            .filter(|(_, character)| character.inited)
            .flat_map(|(target_id, character)| {
                completed_character_skill_pool_entries(target_id, character)
            })
            .collect::<Vec<_>>();
        next_auto_entries.sort_by(|left, right| {
            left.source_key()
                .cmp(&right.source_key())
                .then_with(|| left.name.cmp(&right.name))
        });

        let mut manual_entries = self
            .skill_pool
            .iter()
            .filter(|entry| entry.source_key().is_none())
            .cloned()
            .collect::<Vec<_>>();
        let manual_len = manual_entries.len();
        manual_entries.dedup_by(|left, right| left == right);
        let manual_changed = manual_entries.len() != manual_len;

        let mut next = manual_entries;
        next.extend(next_auto_entries);
        let changed = manual_changed || self.skill_pool != next;
        if changed {
            self.skill_pool = next;
        }
        changed
    }

    pub fn register_incoming_target(&mut self, target_id: &str, is_new_target: bool) {
        self.chat_targets.entry(target_id.to_owned()).or_default();

        if !is_new_target
            || self.open_chat_targets.contains(target_id)
            || self.rejected_chat_targets.contains(target_id)
        {
            return;
        }

        if !self.join_requests_allowed_for_target(target_id) {
            self.rejected_chat_targets.insert(target_id.to_owned());
            return;
        }

        self.pending_chat_targets.insert(target_id.to_owned());
    }

    fn join_requests_allowed_for_target(&self, target_id: &str) -> bool {
        if !is_private_message_target(self.messages.get(target_id)) {
            return true;
        }

        self.current_group()
            .map(|group| group.allow_join_requests)
            .unwrap_or(true)
    }

    pub fn approve_chat_target(&mut self, target_id: &str) -> bool {
        let mut changed = self.open_chat_targets.insert(target_id.to_owned());
        changed |= self.pending_chat_targets.remove(target_id);
        changed |= self.rejected_chat_targets.remove(target_id);
        if is_private_message_target(self.messages.get(target_id)) {
            if let Some(group_name) = self.current_trpg_group.clone() {
                if let Some(group) = self.trpg_groups.get_mut(&group_name) {
                    if !group.players.iter().any(|player_id| player_id == target_id) {
                        group.players.push(target_id.to_owned());
                        group.players.sort();
                        changed = true;
                    }
                    changed |= group.sync_turn_players();
                    changed |= group.sync_legacy_negative_timers();
                    changed |= group.sync_parties();
                }
            }
        }
        changed
    }

    pub fn reject_chat_target(&mut self, target_id: &str) -> bool {
        let mut changed = self.pending_chat_targets.remove(target_id);
        changed |= self.open_chat_targets.remove(target_id);
        changed |= self.rejected_chat_targets.insert(target_id.to_owned());
        changed
    }

    pub fn set_automatic_target_name(&mut self, target_id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || name == target_id {
            return false;
        }

        let metadata = self.chat_targets.entry(target_id.to_owned()).or_default();
        if metadata.automatic_name.trim() == name {
            return false;
        }

        metadata.automatic_name = name.to_owned();
        true
    }
}

fn moonberry_group_name(group: &Value, index: usize) -> String {
    moonberry_string_field(group, "name")
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("月莓导入{}", index + 1))
}

fn moonberry_basic_config(config: &Value) -> TrpgBasicConfig {
    let mut imported = TrpgBasicConfig {
        base_max_hp: 0.0,
        ..Default::default()
    };
    if let Some(value) = moonberry_f32_field(config, "wisMPReg") {
        imported.wis_mp_reg = value;
    }
    if let Some(value) = moonberry_f32_field(config, "wisMaxMP") {
        imported.wis_max_mp = value;
    }
    if let Some(value) = moonberry_f32_field(config, "intMaxMP") {
        imported.int_max_mp = value;
    }
    if let Some(value) = moonberry_f32_field(config, "vitHPReg") {
        imported.vit_hp_reg = value;
    }
    if let Some(value) = moonberry_f32_field(config, "vitMaxHP") {
        imported.vit_max_hp = value;
    }
    if let Some(value) = moonberry_f32_field(config, "lvMaxHP") {
        imported.lv_max_hp = value;
    }
    if let Some(value) = moonberry_f32_field(config, "strMaxHP") {
        imported.str_max_hp = value;
    }
    if let Some(value) = moonberry_f32_field(config, "expGainPerLv") {
        imported.exp_gain_per_level = value;
    }
    if let Some(value) = moonberry_f32_field(config, "expGainPerLvPvP") {
        imported.exp_gain_per_level_pvp = value;
    }
    if let Some(value) = moonberry_f32_field(config, "basicSpeed") {
        imported.basic_speed = value;
    }
    if let Some(value) = moonberry_f32_field(config, "strDMGBenifit") {
        imported.str_damage_bonus = value;
    }
    if let Some(value) = moonberry_f32_field(config, "intDMGBenifit") {
        imported.int_damage_bonus = value;
    }
    if let Some(value) = moonberry_f32_field(config, "dexDMGBenifit") {
        imported.dex_damage_bonus = value;
    }
    if let Some(value) = moonberry_f32_field(config, "dexRangeDMGBenifit") {
        imported.dex_range_damage_bonus = value;
    }
    if let Some(value) = moonberry_f32_field(config, "wisHealBenifit") {
        imported.wis_heal_bonus = value;
    }
    if let Some(value) = moonberry_f32_field(config, "intHealBenifit") {
        imported.int_heal_bonus = value;
    }
    if let Some(value) = moonberry_f32_field(config, "agiDMGBenifit") {
        imported.agi_damage_bonus = value;
    }
    if let Some(value) = moonberry_f32_field(config, "strSpeed") {
        imported.str_speed = value;
    }
    if let Some(value) = moonberry_f32_field(config, "agiSpeed") {
        imported.agi_speed = value;
    }
    if let Some(value) = moonberry_f32_field(config, "dexSpeed") {
        imported.dex_speed = value;
    }
    imported
}

fn moonberry_legacy_teams(teams: &[Value]) -> Vec<TrpgLegacyTeam> {
    teams
        .iter()
        .enumerate()
        .filter_map(|(index, team)| {
            let id =
                moonberry_target_id_field(team, "Id").unwrap_or_else(|| (index + 1).to_string());
            let name = moonberry_string_field(team, "name")
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| format!("旧频道{id}"));
            let players = team
                .get("pcs")
                .and_then(Value::as_array)
                .map(|pcs| moonberry_legacy_pc_member_ids(pcs))
                .unwrap_or_default();
            let chat_values = team.get("chat").and_then(Value::as_array);
            let chat_message_count = chat_values.map(Vec::len).unwrap_or_default();
            let chat_messages = chat_values
                .map(|chats| moonberry_legacy_team_chat_messages(chats))
                .unwrap_or_default();
            (!id.trim().is_empty()).then_some(TrpgLegacyTeam {
                id,
                name,
                players,
                visible: moonberry_bool_field(team, "visible").unwrap_or(true),
                allow_pc_nickname_repeat: moonberry_bool_field(team, "allowPcNicknameRepeat")
                    .unwrap_or(false),
                anonymous_speakers: moonberry_bool_field(team, "nemo").unwrap_or(false),
                buff_count: team
                    .get("buff")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or_default(),
                chat_message_count,
                chat_messages,
                window_x: team
                    .get("bounds")
                    .and_then(|bounds| moonberry_f32_field(bounds, "x"))
                    .unwrap_or_default(),
                window_y: team
                    .get("bounds")
                    .and_then(|bounds| moonberry_f32_field(bounds, "y"))
                    .unwrap_or_default(),
                window_width: team
                    .get("size")
                    .and_then(|size| moonberry_f32_field(size, "width"))
                    .unwrap_or_default(),
                window_height: team
                    .get("size")
                    .and_then(|size| moonberry_f32_field(size, "height"))
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn moonberry_legacy_team_chat_messages(chats: &[Value]) -> Vec<TrpgLegacyTeamChatMessage> {
    chats
        .iter()
        .filter_map(|chat| {
            let sender = chat.get("sender");
            let sender_id = sender
                .and_then(|sender| moonberry_target_id_field(sender, "id"))
                .unwrap_or_default();
            let sender_name = sender
                .and_then(moonberry_legacy_team_chat_sender_name)
                .or_else(|| (!sender_id.trim().is_empty()).then_some(sender_id.clone()))
                .unwrap_or_default();
            let chain_values = chat.get("messageChain").and_then(Value::as_array)?;
            let text = moonberry_legacy_team_chat_text(chain_values);
            (!text.trim().is_empty()).then_some(TrpgLegacyTeamChatMessage {
                sender_id,
                sender_name,
                text,
                time: moonberry_message_time(chain_values),
            })
        })
        .collect()
}

fn moonberry_legacy_team_chat_sender_name(sender: &Value) -> Option<String> {
    ["memberName", "nickname", "remark"]
        .into_iter()
        .find_map(|key| {
            moonberry_string_field(sender, key)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
}

fn moonberry_legacy_team_chat_text(segments: &[Value]) -> String {
    segments
        .iter()
        .filter_map(moonberry_legacy_team_chat_segment_text)
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn moonberry_legacy_team_chat_segment_text(segment: &Value) -> Option<String> {
    let segment_type = moonberry_string_field(segment, "type").unwrap_or_default();
    match segment_type.as_str() {
        "Source" | "Quote" => None,
        "Plain" => moonberry_string_field(segment, "text"),
        "Image" | "FlashImage" => Some(moonberry_legacy_team_chat_image_text(
            segment,
        )),
        _ => [
            "text", "content", "display", "summary", "name", "title", "value",
        ]
        .into_iter()
        .find_map(|key| {
            moonberry_string_field(segment, key)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            let segment_type = segment_type.trim();
            (!segment_type.is_empty()).then(|| format!("[{segment_type}]"))
        }),
    }
}

fn moonberry_legacy_team_chat_image_text(segment: &Value) -> String {
    ["url", "path", "imageId"]
        .into_iter()
        .find_map(|key| {
            moonberry_string_field(segment, key)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .map(|label| format!("[图片:{label}]"))
        .unwrap_or_else(|| "[图片]".to_owned())
}

fn moonberry_legacy_worlds(worlds: &[Value]) -> Vec<TrpgLegacyWorld> {
    worlds
        .iter()
        .enumerate()
        .filter_map(|(index, modal)| {
            let world = modal.get("world").unwrap_or(modal);
            let id = moonberry_string_field(modal, "Id")
                .or_else(|| moonberry_string_field(world, "Id"))
                .filter(|id| !id.trim().is_empty())
                .unwrap_or_else(|| format!("moonberry-world-{}", index + 1));
            let name = moonberry_string_field(world, "name")
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| format!("旧世界{}", index + 1));
            let players = world
                .get("PcNumbers")
                .and_then(Value::as_array)
                .map(|values| moonberry_legacy_member_ids(values))
                .unwrap_or_default();
            let npcs = world
                .get("NpcNumbers")
                .and_then(Value::as_array)
                .map(|values| moonberry_legacy_member_ids(values))
                .unwrap_or_default();
            let chat_areas = world
                .get("chatAreas")
                .and_then(Value::as_array)
                .map(|areas| moonberry_legacy_areas(areas, "虚拟讨论组"))
                .unwrap_or_default();
            let areas = world
                .get("Areas")
                .and_then(Value::as_array)
                .map(|areas| moonberry_legacy_areas(areas, "区域"))
                .unwrap_or_default();
            (!id.trim().is_empty()).then_some(TrpgLegacyWorld {
                id,
                name,
                visible: moonberry_bool_field(modal, "visible").unwrap_or(true),
                players,
                npcs,
                chat_areas,
                areas,
            })
        })
        .collect()
}

fn moonberry_legacy_areas(areas: &[Value], fallback_prefix: &str) -> Vec<TrpgLegacyArea> {
    areas
        .iter()
        .enumerate()
        .filter_map(|(index, area)| {
            let id = moonberry_string_field(area, "id")
                .filter(|id| !id.trim().is_empty())
                .unwrap_or_else(|| format!("{fallback_prefix}-{}", index + 1));
            let name = moonberry_string_field(area, "name")
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| format!("{fallback_prefix}{}", index + 1));
            let members = area
                .get("member")
                .and_then(Value::as_array)
                .map(|values| moonberry_legacy_member_ids(values))
                .unwrap_or_default();
            (!id.trim().is_empty()).then_some(TrpgLegacyArea {
                id,
                name,
                x: moonberry_f32_field(area, "x").unwrap_or_default(),
                y: moonberry_f32_field(area, "y").unwrap_or_default(),
                width: moonberry_f32_field(area, "width").unwrap_or_default(),
                height: moonberry_f32_field(area, "height").unwrap_or_default(),
                members,
                combat: moonberry_bool_field(area, "combat").unwrap_or(false),
            })
        })
        .collect()
}

fn moonberry_legacy_send_panes(panes: &[Value]) -> Vec<TrpgLegacySendPane> {
    panes
        .iter()
        .enumerate()
        .filter_map(|(index, pane)| {
            let targets = pane
                .get("sendTo")
                .and_then(|send_to| send_to.get("targets"))
                .and_then(Value::as_array)
                .map(|values| moonberry_legacy_member_ids(values))
                .unwrap_or_default();
            if targets.is_empty() {
                return None;
            }
            let key = pane
                .get("key")
                .and_then(moonberry_scalar_to_string)
                .filter(|key| !key.trim().is_empty())
                .unwrap_or_else(|| index.to_string());
            let title = moonberry_string_field(pane, "title")
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| format!("旧发送窗{}", index + 1));
            Some(TrpgLegacySendPane {
                key,
                title,
                targets,
                closable: moonberry_bool_field(pane, "closable").unwrap_or(true),
            })
        })
        .collect()
}

fn moonberry_legacy_negative_timers(timers: &[Value]) -> Vec<TrpgLegacyNegativeTimer> {
    let mut imported = Vec::new();
    let mut seen = HashSet::new();
    for timer in timers {
        let Some(target_id) = moonberry_target_id_field(timer, "Id") else {
            continue;
        };
        if !seen.insert(target_id.clone()) {
            continue;
        }
        imported.push(TrpgLegacyNegativeTimer {
            target_id,
            remaining_ms: moonberry_u64_field(timer, "remain").unwrap_or_default(),
            replied: moonberry_bool_field(timer, "reply").unwrap_or_default(),
            generation: moonberry_u32_field(timer, "idx").unwrap_or_default(),
            half_warned: false,
            negative_layers: 0,
        });
    }
    imported
}

fn moonberry_legacy_pc_member_ids(pcs: &[Value]) -> Vec<String> {
    let mut ids = pcs
        .iter()
        .filter_map(moonberry_pc_target_id)
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

fn moonberry_legacy_member_ids(values: &[Value]) -> Vec<String> {
    let mut ids = values
        .iter()
        .filter_map(moonberry_scalar_to_string)
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty() && id != "-1")
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

fn sync_legacy_surface_players(group: &mut TrpgGroup) {
    let mut imported = Vec::new();
    for team in &group.legacy_teams {
        imported.extend(team.players.iter().cloned());
    }
    for world in &group.legacy_worlds {
        imported.extend(world.players.iter().cloned());
        for area in world.chat_areas.iter().chain(world.areas.iter()) {
            imported.extend(area.members.iter().cloned());
        }
    }
    for pane in &group.legacy_send_panes {
        for target in &pane.targets {
            let target = target.trim();
            if target.is_empty() || target == "0" {
                continue;
            }
            if let Some(numeric_id) = moonberry_exact_u64(target) {
                if numeric_id < 10000 {
                    if let Some(team) = group.legacy_team(target) {
                        imported.extend(team.players.iter().cloned());
                    }
                } else if numeric_id > 10000 {
                    imported.push(target.to_owned());
                }
                continue;
            }
            if let Some(area) = group.legacy_chat_area(target) {
                imported.extend(area.members.iter().cloned());
            }
        }
    }
    for timer in &group.legacy_negative_timers {
        imported.push(timer.target_id.clone());
    }
    for target_id in imported {
        if !group
            .players
            .iter()
            .any(|player_id| player_id == &target_id)
        {
            group.players.push(target_id);
        }
    }
    group.players.sort();
    group.players.dedup();
}

fn moonberry_pc_target_id(pc: &Value) -> Option<String> { moonberry_target_id_field(pc, "Id") }

fn moonberry_target_id_field(value: &Value, key: &str) -> Option<String> {
    let raw = value.get(key)?;
    if let Some(id) = raw.as_i64() {
        return (id >= 0).then(|| id.to_string());
    }
    if let Some(id) = raw.as_u64() {
        return Some(id.to_string());
    }
    raw.as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != "-1")
        .map(str::to_owned)
}

fn moonberry_string_field(value: &Value, key: &str) -> Option<String> {
    let raw = value.get(key)?;
    moonberry_scalar_to_string(raw)
}

fn moonberry_scalar_to_string(raw: &Value) -> Option<String> {
    if let Some(text) = raw.as_str() {
        return Some(text.to_owned());
    }
    if raw.is_number() || raw.is_boolean() {
        return Some(raw.to_string());
    }
    None
}

fn moonberry_split_tags(tags: &str) -> Vec<String> {
    tags.split_whitespace()
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

fn moonberry_skill_pool_type_label(value: &Value) -> Option<String> {
    let numeric = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_f64().map(|value| value.round() as i64))
        .or_else(|| {
            value
                .as_str()
                .and_then(|value| value.trim().parse::<i64>().ok())
        });
    if let Some(index) = numeric {
        return match index {
            0 => Some("支援天赋".to_owned()),
            1 => Some("普通天赋".to_owned()),
            2 => Some("普通".to_owned()),
            3 => Some("BUFF效果".to_owned()),
            _ => Some(index.to_string()),
        };
    }
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn moonberry_skill_pool_args(args: &[Value]) -> Vec<SkillPoolArg> {
    args.iter()
        .filter_map(|arg| {
            let name = moonberry_string_field(arg, "name").unwrap_or_default();
            let kind = arg
                .get("type")
                .and_then(moonberry_scalar_to_string)
                .unwrap_or_default();
            let value = moonberry_string_field(arg, "value").unwrap_or_default();
            (!name.trim().is_empty() || !kind.trim().is_empty() || !value.trim().is_empty())
                .then_some(SkillPoolArg { name, kind, value })
        })
        .collect()
}

fn moonberry_i32_field(value: &Value, key: &str) -> Option<i32> {
    let raw = value.get(key)?;
    raw.as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .or_else(|| raw.as_u64().and_then(|value| i32::try_from(value).ok()))
        .or_else(|| raw.as_f64().map(|value| value.round() as i32))
        .or_else(|| {
            raw.as_str()
                .and_then(|value| value.trim().parse::<i32>().ok())
        })
}

fn moonberry_u32_field(value: &Value, key: &str) -> Option<u32> {
    let raw = value.get(key)?;
    raw.as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| {
            raw.as_i64()
                .filter(|value| *value >= 0)
                .and_then(|value| u32::try_from(value).ok())
        })
        .or_else(|| {
            raw.as_f64()
                .filter(|value| *value >= 0.0)
                .map(|value| value.round() as u32)
        })
        .or_else(|| {
            raw.as_str()
                .and_then(|value| value.trim().parse::<u32>().ok())
        })
}

fn moonberry_u64_field(value: &Value, key: &str) -> Option<u64> {
    let raw = value.get(key)?;
    raw.as_u64()
        .or_else(|| {
            raw.as_i64()
                .filter(|value| *value >= 0)
                .map(|value| value as u64)
        })
        .or_else(|| {
            raw.as_f64()
                .filter(|value| *value >= 0.0)
                .map(|value| value.round() as u64)
        })
        .or_else(|| {
            raw.as_str()
                .and_then(|value| value.trim().parse::<u64>().ok())
        })
}

fn moonberry_exact_u64(value: &str) -> Option<u64> {
    let value = value.trim();
    let parsed = value.parse::<u64>().ok()?;
    (parsed.to_string() == value).then_some(parsed)
}

fn moonberry_usize_field(value: &Value, key: &str) -> Option<usize> {
    let raw = value.get(key)?;
    raw.as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .or_else(|| {
            raw.as_i64()
                .filter(|value| *value >= 0)
                .and_then(|value| usize::try_from(value).ok())
        })
        .or_else(|| {
            raw.as_str()
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
}

fn moonberry_f32_field(value: &Value, key: &str) -> Option<f32> {
    let raw = value.get(key)?;
    raw.as_f64().map(|value| value as f32).or_else(|| {
        raw.as_str()
            .and_then(|value| value.trim().parse::<f32>().ok())
    })
}

fn moonberry_bool_field(value: &Value, key: &str) -> Option<bool> {
    let raw = value.get(key)?;
    raw.as_bool().or_else(|| {
        raw.as_str().and_then(|value| match value.trim() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        })
    })
}

fn moonberry_pc_to_character(pc: &Value) -> PlayerCharacter {
    let mut character = PlayerCharacter::default();
    if let Some(value) = moonberry_bool_field(pc, "inited") {
        character.inited = value;
    }
    if let Some(value) = moonberry_string_field(pc, "name") {
        character.name = value;
    }
    if let Some(value) = moonberry_string_field(pc, "nickname") {
        character.nickname = value;
    }
    if let Some(value) = moonberry_string_field(pc, "img") {
        character.image = value;
    }
    if let Some(value) = moonberry_i32_field(pc, "statusPoint") {
        character.status_points = value;
    }
    if let Some(value) = moonberry_i32_field(pc, "exchangePoint") {
        character.exchange_points = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "hp") {
        character.hp = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "maxHP") {
        character.max_hp = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "hpReg") {
        character.hp_regen = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "mp") {
        character.mp = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "maxMP") {
        character.max_mp = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "mpReg") {
        character.mp_regen = value;
    }
    if let Some(value) = moonberry_i32_field(pc, "lv") {
        character.level = value.max(1);
    }
    if let Some(value) = moonberry_i32_field(pc, "exp") {
        character.exp = value.max(0);
    }
    if let Some(value) = moonberry_f32_field(pc, "speed") {
        character.speed = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "DMGModify") {
        character.damage_dealt_modifier = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "healModify") {
        character.healing_dealt_modifier = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "tDMGModify") {
        character.damage_taken_modifier = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "tHealModify") {
        character.healing_taken_modifier = value;
    }
    if let Some(value) = moonberry_f32_field(pc, "tdpt") {
        character.damage_taken_this_turn = value.max(0.0);
    }
    if let Some(value) = moonberry_f32_field(pc, "thpt") {
        character.healing_taken_this_turn = value.max(0.0);
    }
    if let Some(status) = pc.get("status") {
        character.status = moonberry_status(status);
    }
    if let Some(status) = pc.get("extraStatus") {
        character.extra_status = moonberry_status(status);
    }
    if let Some(skills) = pc.get("skillChain").and_then(Value::as_array) {
        for skill in skills {
            character
                .skill_names
                .push(moonberry_string_field(skill, "name").unwrap_or_default());
            character
                .skill_notes
                .push(moonberry_string_field(skill, "description").unwrap_or_default());
            character.skill_mp_costs.push(
                moonberry_f32_field(skill, "cost")
                    .unwrap_or_default()
                    .max(0.0),
            );
            character
                .skill_cooldown_turns
                .push(moonberry_u32_field(skill, "cooldown").unwrap_or_default());
            let pool_id =
                moonberry_string_field(skill, "poolId").filter(|pool_id| !pool_id.is_empty());
            let skill_args = skill
                .get("args")
                .and_then(Value::as_array)
                .map(|args| moonberry_skill_pool_args(args))
                .unwrap_or_default();
            let legacy_has_buff_machine = skill
                .get("buffMachine")
                .is_some_and(|buff_machine| !buff_machine.is_null());
            let legacy_buff_machine_json = moonberry_legacy_json_field(skill, "buffMachine");
            character.skill_metadata.push(CharacterSkillMetadata {
                pc_approved: moonberry_bool_field(skill, "pcInited").unwrap_or(true),
                st_approved: moonberry_bool_field(skill, "stInited").unwrap_or(true),
                source: if pool_id.is_some() {
                    CharacterSkillSourceKind::SkillPool
                } else {
                    CharacterSkillSourceKind::Manual
                },
                source_pool_id: pool_id.clone(),
                source_pool_label: pool_id,
                source_character_id: None,
                source_skill_index: None,
                skill_type: moonberry_string_field(skill, "type")
                    .filter(|value| !value.trim().is_empty()),
                target_class: moonberry_string_field(skill, "class")
                    .filter(|value| !value.trim().is_empty()),
                target_count: moonberry_u32_field(skill, "target"),
                range: moonberry_i32_field(skill, "range"),
                exchange_point: moonberry_i32_field(skill, "exchangePoint"),
                cooldown_left: moonberry_u32_field(skill, "cooldownLeft"),
                legacy_caster: skill
                    .get("caster")
                    .and_then(moonberry_scalar_to_string)
                    .filter(|value| !value.trim().is_empty()),
                talent_trigger: None,
                talent_effect: None,
                args: skill_args,
                legacy_has_buff_machine,
                legacy_buff_machine_json,
            });
        }
    }
    character
}

fn moonberry_status(value: &Value) -> CharacterStatus {
    CharacterStatus {
        str_: moonberry_i32_field(value, "str").unwrap_or_default(),
        agi: moonberry_i32_field(value, "agi").unwrap_or_default(),
        dex: moonberry_i32_field(value, "dex").unwrap_or_default(),
        vit: moonberry_i32_field(value, "vit").unwrap_or_default(),
        int_: moonberry_i32_field(value, "int").unwrap_or_default(),
        wis: moonberry_i32_field(value, "wis").unwrap_or_default(),
        k: moonberry_i32_field(value, "k").unwrap_or_default(),
        cha: moonberry_i32_field(value, "cha").unwrap_or_default(),
    }
}

fn moonberry_character_display_name(target_id: &str, character: &PlayerCharacter) -> String {
    if !character.nickname.trim().is_empty() {
        return character.nickname.trim().to_owned();
    }
    if !character.name.trim().is_empty() {
        return character.name.trim().to_owned();
    }
    target_id.to_owned()
}

fn moonberry_chat_to_napcat_message(
    group_name: &str,
    chat: &Value,
) -> Option<(String, NapcatMessage)> {
    let chat_type = moonberry_string_field(chat, "type")?;
    let sender = chat.get("sender")?;
    let sender_id = moonberry_u64_field(sender, "id").unwrap_or_default();
    let sender_name = moonberry_string_field(sender, "nickname")
        .or_else(|| moonberry_string_field(sender, "memberName"))
        .or_else(|| moonberry_string_field(sender, "remark"))
        .unwrap_or_else(|| sender_id.to_string());
    let chain_values = chat.get("messageChain").and_then(Value::as_array)?;
    let chains = moonberry_message_chains(chain_values);
    if chains.is_empty() {
        return None;
    }
    let time = moonberry_message_time(chain_values);

    let (target_id, message_type, group_id, group_name_value, visibility) =
        if chat_type == "GroupMessage" {
            let group = sender.get("group");
            let group_id = group
                .and_then(|group| moonberry_u64_field(group, "id"))
                .unwrap_or_default();
            (
                group_id.to_string(),
                NapcatMessageType::Group,
                Some(group_id),
                group.and_then(|group| moonberry_string_field(group, "name")),
                Visibility::Public,
            )
        } else {
            (
                sender_id.to_string(),
                NapcatMessageType::Private,
                None,
                None,
                Visibility::Player(sender_id),
            )
        };

    Some((target_id, NapcatMessage {
        data: NapcatMessageData {
            time,
            message_type,
            message: chains,
            self_id: 0,
            user_id: sender_id,
            group_id,
            group_name: group_name_value,
            target_id: None,
            sender: NapcatSender {
                user_id: sender_id,
                nickname: sender_name,
            },
            campaign_id: group_name.to_owned(),
            character_id: (chat_type != "GroupMessage").then(|| sender_id.to_string()),
            party_id: None,
            visibility,
        },
    }))
}

fn moonberry_message_chains(segments: &[Value]) -> Vec<NapcatMessageChain> {
    segments
        .iter()
        .filter_map(|segment| {
            let segment_type = moonberry_string_field(segment, "type")?;
            match segment_type.as_str() {
                "Source" => Some(NapcatMessageChain {
                    variant: NapcatMessageChainType::Source(Source {
                        id: moonberry_u64_field(segment, "id").unwrap_or_default(),
                        time: moonberry_u64_field(segment, "time").unwrap_or_default(),
                    }),
                }),
                "Plain" => Some(NapcatMessageChain {
                    variant: NapcatMessageChainType::Text {
                        data: TextData {
                            text: moonberry_string_field(segment, "text").unwrap_or_default(),
                        },
                    },
                }),
                "Image" | "FlashImage" => Some(NapcatMessageChain {
                    variant: NapcatMessageChainType::Image {
                        data: ImageData {
                            sub_type: 0,
                            file: moonberry_string_field(segment, "imageId")
                                .or_else(|| moonberry_string_field(segment, "path"))
                                .unwrap_or_default(),
                            url: moonberry_string_field(segment, "url").unwrap_or_default(),
                            file_id: moonberry_string_field(segment, "imageId").unwrap_or_default(),
                            file_size: moonberry_string_field(segment, "size").unwrap_or_default(),
                            local_path: moonberry_string_field(segment, "path").unwrap_or_default(),
                        },
                    },
                }),
                _ => moonberry_string_field(segment, "text")
                    .or_else(|| moonberry_string_field(segment, "content"))
                    .map(|text| NapcatMessageChain {
                        variant: NapcatMessageChainType::Text {
                            data: TextData { text },
                        },
                    }),
            }
        })
        .collect()
}

fn moonberry_message_time(segments: &[Value]) -> u64 {
    segments
        .iter()
        .find_map(|segment| moonberry_u64_field(segment, "time"))
        .unwrap_or_default()
}

fn completed_character_skill_pool_entries(
    target_id: &str,
    character: &PlayerCharacter,
) -> Vec<SkillPoolEntry> {
    let source_character_name = character_display_name(character, target_id);
    let skill_count = character
        .skill_names
        .len()
        .max(character.skill_notes.len())
        .max(character.skill_mp_costs.len())
        .max(character.skill_cooldown_turns.len())
        .max(character.skill_metadata.len());

    (0..skill_count)
        .filter_map(|index| {
            let metadata = character
                .skill_metadata
                .get(index)
                .cloned()
                .unwrap_or_default();
            if !metadata.is_approved() {
                return None;
            }
            let note = character
                .skill_notes
                .get(index)
                .cloned()
                .unwrap_or_default();
            if note.trim().is_empty() {
                return None;
            }
            let name = character
                .skill_names
                .get(index)
                .map(|name| name.trim())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    format!(
                        "{}技能{}",
                        source_character_name,
                        index + 1
                    )
                });
            Some(SkillPoolEntry {
                name,
                note,
                mp_cost: character
                    .skill_mp_costs
                    .get(index)
                    .copied()
                    .unwrap_or_default()
                    .max(0.0),
                cooldown_turns: character
                    .skill_cooldown_turns
                    .get(index)
                    .copied()
                    .unwrap_or_default(),
                source_character_id: Some(target_id.to_owned()),
                source_character_name: Some(source_character_name.clone()),
                source_skill_index: Some(index),
                legacy_pool_id: metadata.source_pool_id.clone(),
                category: metadata.source_pool_label.clone(),
                args: metadata.args.clone(),
                legacy_has_graph: metadata.legacy_has_buff_machine,
                legacy_buff_machine_json: metadata.legacy_buff_machine_json.clone(),
                ..Default::default()
            })
        })
        .collect()
}

fn character_display_name(character: &PlayerCharacter, fallback: &str) -> String {
    if !character.nickname.trim().is_empty() {
        character.nickname.trim().to_owned()
    } else if !character.name.trim().is_empty() {
        character.name.trim().to_owned()
    } else {
        fallback.to_owned()
    }
}

fn is_private_message_target(messages: Option<&Vec<NapcatMessage>>) -> bool {
    matches!(
        messages
            .and_then(|messages| messages.first())
            .map(|message| &message.data.message_type),
        Some(NapcatMessageType::Private)
    )
}

fn is_group_message_target(messages: &[NapcatMessage]) -> bool {
    matches!(
        messages.first().map(|message| &message.data.message_type),
        Some(NapcatMessageType::Group)
    )
}

impl Plugin for NapcatPlugin {
    fn build(&self, app: &mut App) {
        app.insert_state(ConnectionState::Disconnected)
            .add_systems(Startup, setup)
            .add_systems(Update, message_system)
            .add_systems(
                Update,
                request_missing_group_info_system,
            )
            .add_systems(Update, send_result_system);
    }
}

fn setup(mut commands: Commands) {
    let (client_to_game_sender, client_to_game_receiver) = unbounded::<Message>();
    let (game_to_client_sender, game_to_client_receiver) = tokio::sync::mpsc::channel(100);
    let (send_result_sender, send_result_receiver) = unbounded::<NapcatSendResult>();
    let napcat_io = NapcatIOReceiver(client_to_game_receiver.clone());
    let napcat_send_results = NapcatSendResultReceiver(send_result_receiver);
    spawn_napcat_connection(
        client_to_game_sender.clone(),
        game_to_client_receiver,
        send_result_sender,
    );
    commands.insert_resource(napcat_io);
    commands.insert_resource(napcat_send_results);
    commands.insert_resource(NapcatIOSender(game_to_client_sender));
    commands.insert_resource(NapcatSendManager::default());
    commands.insert_resource(NapcatAutoForwardRequestIds {
        next_request_id: 1_000_000,
    });
    commands.insert_resource(NapcatGroupInfoRequests {
        next_request_id: 2_000_000,
        pending_group_ids: HashSet::default(),
    });

    let message_manager = NapcatMessageManager {
        messages: HashMap::default(),
        chat_targets: HashMap::default(),
        player_characters: HashMap::default(),
        trpg_groups: HashMap::default(),
        current_trpg_group: None,
        groups: HashMap::default(),
        read_message_counts: HashMap::default(),
        summarized_message_counts: HashMap::default(),
        open_chat_targets: HashSet::default(),
        pending_chat_targets: HashSet::default(),
        rejected_chat_targets: HashSet::default(),
        random_pools: HashMap::default(),
        skill_pool: Vec::new(),
        unit_pool: HashMap::default(),
    };
    let config_dir = Path::new(".data").join("willowblossom");
    commands.insert_resource(
        Persistent::<NapcatMessageManager>::builder()
            .name("messages")
            .format(StorageFormat::Toml)
            .path(config_dir.join("messages.toml"))
            .default(message_manager)
            .build()
            .expect("failed to init messages"),
    );
}

fn spawn_napcat_connection(
    client_to_game_sender: CBSender<Message>,
    game_to_client_receiver: Receiver<NapcatOutboundMessage>,
    send_result_sender: CBSender<NapcatSendResult>,
) {
    thread::Builder::new()
        .name("napcat-websocket".to_owned())
        .spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create NapCat Tokio runtime");
            runtime.block_on(run_napcat_connection(
                client_to_game_sender,
                game_to_client_receiver,
                send_result_sender,
            ));
        })
        .expect("failed to spawn NapCat websocket thread");
}

async fn run_napcat_connection(
    client_to_game_sender: CBSender<Message>,
    mut game_to_client_receiver: Receiver<NapcatOutboundMessage>,
    send_result_sender: CBSender<NapcatSendResult>,
) {
    const NAPCAT_WS_URL: &str = "ws://localhost:3001";

    loop {
        let (ws_stream, _) = match connect_async(NAPCAT_WS_URL).await {
            Ok(connection) => connection,
            Err(err) => {
                eprintln!("failed to connect NapCat websocket: {err}");
                sleep(Duration::from_secs(2)).await;
                continue;
            },
        };

        eprintln!("connected to NapCat websocket at {NAPCAT_WS_URL}");
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        loop {
            tokio::select! {
                msg = ws_receiver.next() => {
                    match msg {
                        Some(Ok(msg)) => {
                            if msg.is_text() || msg.is_binary() {
                                if client_to_game_sender.send(msg).is_err() {
                                    return;
                                }
                            } else if msg.is_close() {
                                break;
                            }
                        }
                        Some(Err(err)) => {
                            eprintln!("NapCat websocket receive error: {err}");
                            break;
                        },
                        None => break,
                    }
                }
                outbound = game_to_client_receiver.recv() => {
                    let Some(outbound) = outbound else {
                        return;
                    };
                    if let Err(err) = ws_sender.send(outbound.message.clone()).await {
                        let error = format!("NapCat websocket send error: {err}");
                        eprintln!("{error}");
                        let _ = send_result_sender.send(NapcatSendResult {
                            request_id: outbound.request_id,
                            target_id: outbound.target_id,
                            error: Some(error),
                        });
                        break;
                    }
                    let _ = send_result_sender.send(NapcatSendResult {
                        request_id: outbound.request_id,
                        target_id: outbound.target_id,
                        error: None,
                    });
                    eprintln!("sent NapCat websocket message");
                }
            }
        }

        sleep(Duration::from_secs(2)).await;
    }
}

fn send_result_system(
    receiver: Res<NapcatSendResultReceiver>,
    mut send_manager: ResMut<NapcatSendManager>,
) {
    while let Ok(result) = receiver.0.try_recv() {
        send_manager.results.push(result);
    }
}

#[derive(Debug, Deserialize)]
struct NapcatActionResponse {
    #[serde(default)]
    data: Option<NapcatActionResponseData>,
    #[serde(default)]
    echo: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NapcatActionResponseData {
    #[serde(default)]
    group_id: Option<Value>,
    #[serde(default)]
    group_name: Option<String>,
}

fn request_missing_group_info_system(
    sender: Option<Res<NapcatIOSender>>,
    mut group_info_requests: ResMut<NapcatGroupInfoRequests>,
    manager: Res<Persistent<NapcatMessageManager>>,
) {
    let Some(sender) = sender.as_deref() else {
        return;
    };

    for (target_id, messages) in &manager.messages {
        if !matches!(
            messages.first().map(|message| &message.data.message_type),
            Some(NapcatMessageType::Group)
        ) {
            continue;
        }

        let has_name = manager
            .chat_targets
            .get(target_id)
            .map(|metadata| !metadata.automatic_name.trim().is_empty())
            .unwrap_or_default();
        if has_name || group_info_requests.pending_group_ids.contains(target_id) {
            continue;
        }

        let Ok(group_id) = target_id.parse::<u64>() else {
            continue;
        };
        queue_group_info_request(
            sender,
            &mut group_info_requests,
            group_id,
        );
    }
}

fn queue_group_info_request(
    sender: &NapcatIOSender,
    group_info_requests: &mut NapcatGroupInfoRequests,
    group_id: u64,
) {
    let target_id = group_id.to_string();
    let request_id = group_info_requests.next_request_id;
    group_info_requests.next_request_id += 1;
    let message = Message::Text(
        json!({
            "action": "get_group_info",
            "params": {
                "group_id": group_id,
                "no_cache": false
            },
            "echo": format!("group-info:{target_id}")
        })
        .to_string()
        .into(),
    );

    match sender.0.try_send(NapcatOutboundMessage {
        request_id,
        target_id: target_id.clone(),
        message,
    }) {
        Ok(()) => {
            group_info_requests.pending_group_ids.insert(target_id);
        },
        Err(err) => eprintln!("failed to queue NapCat group info request: {err}"),
    }
}

fn apply_group_info_response(
    response: &NapcatActionResponse,
    manager: &mut NapcatMessageManager,
    group_info_requests: &mut NapcatGroupInfoRequests,
) -> bool {
    let Some(target_id) = response_group_target_id(response) else {
        return false;
    };
    group_info_requests.pending_group_ids.remove(&target_id);

    let Some(group_name) = response
        .data
        .as_ref()
        .and_then(|data| data.group_name.as_deref())
    else {
        return false;
    };

    manager.set_automatic_target_name(&target_id, group_name)
}

fn response_group_target_id(response: &NapcatActionResponse) -> Option<String> {
    if let Some(group_id) = response
        .data
        .as_ref()
        .and_then(|data| data.group_id.as_ref())
        .and_then(value_to_target_id)
    {
        return Some(group_id);
    }

    response
        .echo
        .as_deref()
        .and_then(|echo| echo.strip_prefix("group-info:"))
        .map(str::to_owned)
}

fn value_to_target_id(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => number.as_u64().map(|number| number.to_string()),
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_owned())
        },
        _ => None,
    }
}

fn message_system(
    receiver: Res<NapcatIOReceiver>,
    sender: Option<Res<NapcatIOSender>>,
    mut auto_forward_ids: ResMut<NapcatAutoForwardRequestIds>,
    mut group_info_requests: ResMut<NapcatGroupInfoRequests>,
    mut scene_capture_requests: Option<ResMut<SceneCaptureRequests>>,
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
) {
    while let Ok(msg) = receiver.0.try_recv() {
        let json_res = serde_json::from_str::<NapcatMessage>(&msg.to_string());
        if let Ok(mut json) = json_res {
            dbg!(&json);
            cache_message_images(&mut json);
            let target_id = match json.data.message_type {
                NapcatMessageType::Private => {
                    if json.data.user_id == json.data.self_id {
                        json.data.target_id.unwrap_or(json.data.user_id)
                    } else {
                        json.data.user_id
                    }
                },
                NapcatMessageType::Group => json.data.group_id.unwrap_or(json.data.user_id),
            };
            let target_id = target_id.to_string();
            let is_new_target = !manager.messages.contains_key(&target_id);
            let is_incoming_message = json.data.user_id != json.data.self_id;
            let incoming_user_id = json.data.user_id;
            manager.annotate_incoming_message_access(&target_id, &mut json);

            let auto_forward = auto_forward_request(&manager, &json, &target_id);
            let character_creation_response = if is_incoming_message
                && matches!(
                    json.data.message_type,
                    NapcatMessageType::Private
                ) {
                handle_character_creation_message(&mut manager, &json, &target_id)
            } else {
                None
            };
            if is_scene_capture_command(&json) && json.data.user_id != json.data.self_id {
                if let Some(scene_capture_requests) = scene_capture_requests.as_deref_mut() {
                    scene_capture_requests.requests.push(SceneCaptureRequest {
                        user_id: json.data.user_id,
                    });
                }
            }

            manager
                .messages
                .entry(target_id.clone())
                .or_default()
                .push(json);
            manager.chat_targets.entry(target_id.clone()).or_default();
            let group_name = manager
                .messages
                .get(&target_id)
                .and_then(|messages| messages.last())
                .and_then(|message| message.data.group_name.as_deref())
                .map(str::to_owned);
            if let Some(group_name) = group_name {
                manager.set_automatic_target_name(&target_id, &group_name);
            }
            if is_incoming_message {
                manager.register_legacy_negative_reply(&incoming_user_id.to_string());
                manager.register_incoming_target(&target_id, is_new_target);
            }

            if let (Some(sender), Some(response)) = (
                sender.as_deref(),
                character_creation_response.as_deref(),
            ) {
                if queue_private_text_response(
                    sender,
                    &mut auto_forward_ids,
                    incoming_user_id,
                    response.to_owned(),
                ) {
                    append_local_private_text_response(
                        &mut manager,
                        &target_id,
                        incoming_user_id,
                        response,
                    );
                }
            }

            if let Err(err) = manager.persist() {
                eprintln!("failed to persist NapCat messages: {err}");
            }

            if let (Some(sender), Some(auto_forward)) = (sender.as_deref(), auto_forward) {
                for user_id in auto_forward.recipients {
                    queue_private_text_response(
                        sender,
                        &mut auto_forward_ids,
                        user_id,
                        auto_forward.text.clone(),
                    );
                }
            }
        } else {
            let response_res = serde_json::from_str::<NapcatActionResponse>(&msg.to_string());
            if let Ok(response) = response_res {
                if apply_group_info_response(
                    &response,
                    &mut manager,
                    &mut group_info_requests,
                ) {
                    if let Err(err) = manager.persist() {
                        eprintln!("failed to persist NapCat group info: {err}");
                    }
                }
            } else {
                eprintln!(
                    "NapCat websocket response was not a persisted chat message: {}; parse error: {:?}",
                    msg,
                    json_res.err()
                );
            }
        }
    }
}

fn cache_message_images(message: &mut NapcatMessage) {
    for chain in &mut message.data.message {
        let NapcatMessageChainType::Image { data } = &mut chain.variant else {
            continue;
        };
        if !data.local_path.trim().is_empty() || data.url.trim().is_empty() {
            continue;
        }

        match cache_remote_image(data.url.trim()) {
            Ok(path) => data.local_path = path.to_string_lossy().to_string(),
            Err(err) => eprintln!(
                "failed to cache NapCat image {}: {err}",
                data.url
            ),
        }
    }
}

fn cache_remote_image(url: &str) -> Result<PathBuf, String> {
    let response = reqwest::blocking::get(url).map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let bytes = response.bytes().map_err(|err| err.to_string())?;
    let format = image::guess_format(&bytes).map_err(|err| err.to_string())?;
    let extension = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Bmp => "bmp",
        _ => "img",
    };

    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let cache_dir = Path::new(".data").join("willowblossom").join("image_cache");
    fs::create_dir_all(&cache_dir).map_err(|err| err.to_string())?;
    let path = cache_dir.join(format!(
        "{:016x}.{extension}",
        hasher.finish()
    ));
    if !path.exists() {
        fs::write(&path, &bytes).map_err(|err| err.to_string())?;
    }
    Ok(path)
}

fn queue_private_text_response(
    sender: &NapcatIOSender,
    auto_forward_ids: &mut NapcatAutoForwardRequestIds,
    user_id: u64,
    text: String,
) -> bool {
    let request_id = auto_forward_ids.next_request_id;
    auto_forward_ids.next_request_id += 1;
    let message = Message::Text(
        json!({
            "action": "send_private_msg",
            "params": {
                "user_id": user_id,
                "message": [
                    {
                        "type": "text",
                        "data": {
                            "text": text
                        }
                    }
                ]
            }
        })
        .to_string()
        .into(),
    );

    if let Err(err) = sender.0.try_send(NapcatOutboundMessage {
        request_id,
        target_id: user_id.to_string(),
        message,
    }) {
        eprintln!("failed to queue NapCat private text response: {err}");
        false
    } else {
        true
    }
}

fn append_local_private_text_response(
    manager: &mut NapcatMessageManager,
    target_id: &str,
    recipient_id: u64,
    text: &str,
) {
    let Some(self_id) = manager
        .messages
        .get(target_id)
        .and_then(|messages| messages.first())
        .map(|message| message.data.self_id)
    else {
        return;
    };

    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let mut message = NapcatMessage {
        data: NapcatMessageData {
            time,
            message_type: NapcatMessageType::Private,
            message: vec![NapcatMessageChain {
                variant: NapcatMessageChainType::Text {
                    data: TextData {
                        text: text.to_owned(),
                    },
                },
            }],
            self_id,
            user_id: self_id,
            group_id: None,
            group_name: None,
            target_id: Some(recipient_id),
            sender: NapcatSender {
                user_id: self_id,
                nickname: "GM".to_owned(),
            },
            campaign_id: String::new(),
            character_id: None,
            party_id: None,
            visibility: Visibility::Public,
        },
    };
    manager.annotate_message_access(target_id, &mut message);

    manager
        .messages
        .entry(target_id.to_owned())
        .or_default()
        .push(message);
}

fn is_scene_capture_command(message: &NapcatMessage) -> bool {
    is_scene_capture_command_text(&message_text(message))
}

pub(crate) fn is_scene_capture_command_text(text: &str) -> bool {
    matches!(
        text.trim(),
        "#观察" | "#gc" | ".观察" | ".gc" | "。观察" | "。gc"
    )
}

fn handle_character_creation_message(
    manager: &mut NapcatMessageManager,
    message: &NapcatMessage,
    target_id: &str,
) -> Option<String> {
    let text = message_text(message).trim().to_owned();
    let image_reference = message_image_reference(message);
    let current_step = manager
        .player_characters
        .get(target_id)
        .map(|character| character.creation_step)
        .unwrap_or_default();
    let nickname_taken = current_step == CharacterCreationStep::Nickname
        && !text.is_empty()
        && manager.player_characters.iter().any(|(other_id, other)| {
            other_id != target_id && other.inited && other.nickname == text
        });

    if let Some(response) = handle_private_player_command(
        manager,
        target_id,
        &text,
        message.data.time,
    ) {
        return Some(response);
    }

    let creation_config = manager.character_creation_config_for_target(target_id);
    let stat_config = manager.character_stat_config_for_target(target_id);
    let character = manager
        .player_characters
        .entry(target_id.to_owned())
        .or_insert_with(PlayerCharacter::default);

    if is_exchange_command(&text) {
        if character.inited {
            return Some("你已经有完成的角色卡了，如需修改请联系GM在角色编辑器中调整。".to_owned());
        }
        if character.creation_step != CharacterCreationStep::Normal {
            return Some(character_creation_prompt(character));
        }

        *character = PlayerCharacter::default();
        character.name = message.data.sender.nickname.clone();
        character.status_points = creation_config.0;
        character.exchange_points = creation_config.1;
        character.creation_step = CharacterCreationStep::Str;
        return Some(format!(
            "你还没有角色卡呢，接下来会开始建卡。\n你拥有{}点属性点，请将它分配到力量/敏捷/灵巧/体质/智力/智慧/知识/魅力上。\n请直接输入数字来增加当前属性；输入【..】退回上一个属性。\n{}",
            character.status_points,
            character_creation_prompt(character)
        ));
    }

    if character.inited || character.creation_step == CharacterCreationStep::Normal {
        return None;
    }

    if matches!(
        text.as_str(),
        ".." | ".。" | "。." | "。。"
    ) {
        return Some(character_creation_back(character));
    }
    if matches!(text.as_str(), "." | "。") {
        return Some(character_creation_next(character));
    }

    if let Some(status_key) = character.creation_step.status_key() {
        let Ok(points) = text.parse::<i32>() else {
            return Some(format!(
                "请输入0到{}之间的数字来分配{}。",
                character.status_points,
                status_key.zh()
            ));
        };
        if points < 0 || points > character.status_points {
            return Some(format!(
                "输入不合法。你剩余{}点属性点，但试图投入{}点。",
                character.status_points, points
            ));
        }
        set_character_status_value(
            &mut character.status,
            status_key,
            points,
        );
        character.status_points -= points;
        if character.status_points <= 0 {
            character.creation_step = CharacterCreationStep::ConfirmStatus;
            return Some(character_status_confirmation(character));
        }
        character.creation_step = character.creation_step.next_status_step();
        return Some(character_creation_prompt(character));
    }

    match character.creation_step {
        CharacterCreationStep::Skill => {
            character.skill_names.push(String::new());
            character.skill_notes.push(text);
            character.skill_mp_costs.push(0.0);
            character.skill_cooldown_turns.push(0);
            character
                .skill_metadata
                .push(CharacterSkillMetadata::player_submitted());
            Some(format!(
                "技能兑换数据已录入，目前记录{}条，等待GM确认后可使用。继续发送技能，或输入【.】结束技能录入。",
                character.skill_notes.len()
            ))
        },
        CharacterCreationStep::Image => {
            let image = image_reference.unwrap_or(text);
            if image.is_empty() {
                return Some(
                    "请发送人物立绘图片，或发送图片链接；如果暂时没有，输入【.】跳过。".to_owned(),
                );
            }
            character.image = image;
            character.creation_step = CharacterCreationStep::Nickname;
            Some(character_creation_prompt(character))
        },
        CharacterCreationStep::Nickname => {
            if text.is_empty() {
                return Some("角色名不能为空，请重新输入。".to_owned());
            }
            if nickname_taken {
                return Some(format!(
                    "很抱歉，「{text}」昵称已经被人使用了，请更换一个昵称。"
                ));
            }
            character.nickname = text;
            character.inited = true;
            character.creation_step = CharacterCreationStep::Normal;
            update_character_from_status_with_config(character, &stat_config);
            Some(format!(
                "是吗？「{}」真是个好名字呢，我十分期待您以后的表现。\n——兑换结束——",
                character.nickname
            ))
        },
        _ => None,
    }
}

fn is_exchange_command(text: &str) -> bool { matches!(text.trim(), ".兑换" | "。兑换") }

fn handle_private_player_command(
    manager: &mut NapcatMessageManager,
    target_id: &str,
    text: &str,
    message_time: u64,
) -> Option<String> {
    if let Some((status_key, points)) = parse_post_creation_status_spend(text) {
        let stat_config = manager.character_stat_config_for_target(target_id);
        let Some(character) = manager.player_characters.get_mut(target_id) else {
            return Some("你还没有角色卡。输入【.兑换】开始建卡。".to_owned());
        };
        return Some(spend_post_creation_status_points(
            character,
            status_key,
            points,
            &stat_config,
        ));
    }

    let command = private_command_body(text)?;
    match command {
        "抽取天赋" => Some(draw_character_talent(
            manager,
            target_id,
            "天赋",
            NORMAL_TALENT_POOL,
            message_time,
        )),
        "抽取辅助天赋" => Some(draw_character_talent(
            manager,
            target_id,
            "辅助天赋",
            SUPPORT_TALENT_POOL,
            message_time,
        )),
        "状态" => Some(format_private_character_status(
            manager, target_id,
        )),
        "已兑换" => Some(format_private_character_skills(
            manager, target_id,
        )),
        "冷却" => Some(format_private_character_cooldowns(
            manager, target_id,
        )),
        "频道人员" => Some(format_private_channel_members(
            manager, target_id,
        )),
        "指南" | "引导" | "团引导" => Some(format_private_group_guide(
            manager, target_id,
        )),
        _ => None,
    }
}

fn draw_character_talent(
    manager: &mut NapcatMessageManager,
    target_id: &str,
    label: &str,
    pool: &[MoonberryTalent],
    message_time: u64,
) -> String {
    let stat_config = manager.character_stat_config_for_target(target_id);
    let Some(character) = manager.player_characters.get_mut(target_id) else {
        return "你还没有角色卡。输入【.兑换】开始建卡。".to_owned();
    };
    if !character.inited {
        return "角色卡尚未完成。请先完成建卡流程。".to_owned();
    }
    if pool.is_empty() {
        return format!("{label}池为空，请联系GM配置。");
    }
    if character
        .skill_metadata
        .iter()
        .any(|metadata| metadata.source == CharacterSkillSourceKind::Talent)
    {
        return "你已经抽过了！".to_owned();
    }

    let talent = &pool[stable_talent_index(
        target_id,
        label,
        message_time,
        pool.len(),
    )];
    let talent_note = talent_note(talent);
    character.skill_names.push(talent.name.to_owned());
    character.skill_notes.push(talent_note.clone());
    character.skill_mp_costs.push(0.0);
    character.skill_cooldown_turns.push(0);
    character.skill_metadata.push(
        CharacterSkillMetadata::moonberry_talent(
            talent_pool_id(label),
            label.to_owned(),
            talent,
        ),
    );
    let applied_effect = apply_moonberry_immediate_talent_effect(character, talent, &stat_config);
    let mut response = format!("抽取{label}：{talent_note}\n已加入已兑换技能。");
    if let Some(applied_effect) = applied_effect {
        response.push('\n');
        response.push_str(&applied_effect);
    }
    response
}

fn talent_note(talent: &MoonberryTalent) -> String {
    format!(
        "{}：{}",
        talent.name, talent.description
    )
}

fn talent_pool_id(label: &str) -> String {
    match label {
        "天赋" => "normal_talent".to_owned(),
        "辅助天赋" => "support_talent".to_owned(),
        other => other.trim().to_owned(),
    }
}

fn moonberry_talent_trigger(talent: &MoonberryTalent) -> Option<&'static str> {
    let description = talent.description.trim();
    if description.contains("跑团开始") {
        Some("跑团开始")
    } else if description.contains("进入突袭轮") {
        Some("进入突袭轮")
    } else if description.contains("进入战斗轮") {
        Some("进入战斗轮")
    } else if description.contains("脱离战斗轮") {
        Some("脱离战斗轮")
    } else if description.contains("持续治疗") && description.contains("结束") {
        Some("持续治疗结束")
    } else if description.contains("过量治疗") {
        Some("过量治疗")
    } else if description.contains("受到治疗") || description.contains("受到越是高等级的治疗")
    {
        Some("受到治疗")
    } else if description.contains("死亡时") {
        Some("死亡时")
    } else if description.contains("受到致命伤害") || description.contains("足以致死") {
        Some("受到致命伤害")
    } else if description.contains("造成伤害") {
        Some("造成伤害")
    } else if description.contains("承受伤害") || description.contains("受到伤害") {
        Some("受到伤害")
    } else if description.contains("到达") {
        Some("到达位置")
    } else if description.contains("遇到解决不了的问题") {
        Some("遇到难题")
    } else if description.contains("更换道具部件") {
        Some("更换道具")
    } else if description.contains("夜间") {
        Some("夜间")
    } else if description.contains("试图追赶") {
        Some("追赶目标")
    } else if description.contains("每度过一个自然回合") {
        Some("自然回合经过")
    } else if description.contains("移动时") {
        Some("移动时")
    } else if description.starts_with("无论何时") {
        Some("常驻")
    } else {
        None
    }
}

fn moonberry_talent_effect_summary(talent: &MoonberryTalent) -> Option<&'static str> {
    match talent.name {
        "那美克星之慧" => Some("立即获得等级*2的知识额外值"),
        "物理专长" => Some("立即将知识基础值提升到至少2；炮塔效果需GM处理"),
        "苏萨斯之爪" => Some("物理伤害一回合后追加35%实际伤害的魔法伤害"),
        "役于我手" => Some("战斗轮中目标死亡时获得其5%最大生命值，上限为自身20%"),
        "无尽痛楚" => Some("战斗轮中每次实际承伤令下一次命中追加等级*1.5无类型伤害，上限2层"),
        "溃伤" => Some("造成伤害时令目标受到治疗效果-25%，持续1回合"),
        "人类基因工程" => Some("常驻最大生命值+5%；疾病/中毒伤害-15%"),
        "大魔法师" => Some("常驻每点智力+1最大MP并+0.5%魔法伤害"),
        "矢量压缩能量池" => Some("常驻每点知识+2最大MP并+1%治疗效果"),
        "狡黠之思" => Some("常驻每点智慧+2最大MP并+1/回合MP回复"),
        "抗魔体质" => Some("常驻魔法伤害减免10%"),
        "混沌无序" => Some("每次伤害/治疗效果随机-15%~+15%"),
        "数魔转换器" => Some("远程伤害享受正向魔法伤害加成"),
        "瞄准镜Tex-30" => Some("远程技能射程至少为等级*15米"),
        "魔网延伸" => Some("法术技能射程+5%；召唤距离需GM处理"),
        "狂风恶浪" => Some("常驻移动速度+20%；玩家目标存活数<=3时提升至35%"),
        "越战越勇" => Some("战斗轮中每经过任意目标一回合伤害+2%，上限+20%"),
        "斗志昂扬" => Some("战斗轮第1/2/3个自身回合承伤-50%/-10%/-2%"),
        "狂妄" => Some("战斗轮中每个新伤害来源令自身伤害+10%，上限+30%"),
        "疲惫行者" => Some("生命值低下惩罚减轻20%，濒死最多按5%生命的重伤惩罚计算"),
        "无限专注" => Some("战斗轮中连续单体攻击同一目标时伤害逐次+10%，上限+20%"),
        "总冠军" => Some("战斗轮中每名玩家目标淘汰令自身伤害+2%、承伤-1%"),
        "罪上加罪" => Some("每次参与击杀获得2.5%经验加成并回复10%已损生命/魔法"),
        "忏悔" => Some("跑团开始治疗效果+25%；每次击杀/助攻递减10%，下限0%"),
        "禅宗古训" => Some("常驻物理伤害造成15%吸血"),
        "过度免疫" => Some("单次伤害大于20%最大HP时伤害-20%"),
        "生死时速" => Some("治疗濒死目标时治疗效果+50%"),
        "菜鸡猛啄" => Some("单次伤害至少造成等级点无类型伤害"),
        "火源之力" => Some("治疗效果随自身伤势提供0%/10%/20%加成"),
        "互帮互助" => Some("治疗他人/受到治疗时50%治疗量回馈给治疗者"),
        "一心" => Some("战斗轮中连续治疗同一目标时治疗效果每次+5%，上限+25%"),
        "千万回忆" => Some("单体即刻治疗会在之后两回合回响15%/5%治疗量"),
        "液态躯体" => Some("战斗轮中承伤50%延后一回合，且每回合回复上回合承伤5%"),
        "敏锐" => Some("战斗轮中第一次范围/非指向伤害100%闪避"),
        _ => moonberry_talent_effect_category(talent.description),
    }
}

fn moonberry_talent_effect_category(description: &str) -> Option<&'static str> {
    let description = description.trim();
    if description.is_empty() {
        return None;
    }

    if description.contains("召唤物")
        || description.contains("小炮塔")
        || description.contains("小精灵")
    {
        Some("召唤/单位或特殊随从效果，需GM处理")
    } else if description.contains("道具")
        || description.contains("烧鹅")
        || description.contains("折扣")
    {
        Some("道具/物品经济效果，需GM处理")
    } else if description.contains("经验") {
        Some("经验收益修正，需GM处理")
    } else if (description.contains("伤害") || description.contains("减伤"))
        && (description.contains("治疗") || description.contains("回复"))
    {
        Some("伤害与治疗修正，待规则/战斗钩子执行")
    } else if description.contains("治疗") || description.contains("回复") {
        Some("治疗/回复效果，待规则/战斗钩子执行")
    } else if description.contains("伤害")
        || description.contains("减伤")
        || description.contains("减免")
        || description.contains("闪避")
    {
        Some("伤害/减伤效果，待规则/战斗钩子执行")
    } else if description.contains("魔法值")
        || description.contains("蓝量")
        || description.contains("魔法")
    {
        Some("魔法/资源效果，需按场景或规则处理")
    } else if description.contains("移动速度")
        || description.contains("移速")
        || description.contains("速度")
        || description.contains("射程")
    {
        Some("移动/射程效果，需场景或战斗距离处理")
    } else if description.contains("属性")
        || description.contains("生命上限")
        || description.contains("知识")
        || description.contains("智力")
        || description.contains("智慧")
        || description.contains("敏捷")
    {
        Some("属性/派生数值效果，需GM或后续规则处理")
    } else if description.contains("提示")
        || description.contains("地图")
        || description.contains("位置")
        || description.contains("最优解")
    {
        Some("情报/位置提示效果，需GM处理")
    } else if description.contains("选择") || description.contains("指定") {
        Some("跑团选择/指定目标效果，需GM处理")
    } else {
        Some("旧月莓天赋效果已保留，需GM按描述处理")
    }
}

fn apply_moonberry_immediate_talent_effect(
    character: &mut PlayerCharacter,
    talent: &MoonberryTalent,
    stat_config: &TrpgBasicConfig,
) -> Option<String> {
    match talent.name {
        "那美克星之慧" => {
            let amount = character.level.max(1).saturating_mul(2);
            character.extra_status.k = character.extra_status.k.saturating_add(amount);
            update_character_from_status_with_config(character, stat_config);
            Some(format!(
                "已立即应用天赋效果：知识额外值 +{amount}。"
            ))
        },
        "物理专长" => {
            if character.status.k < 2 {
                character.status.k = 2;
                update_character_from_status_with_config(character, stat_config);
                Some("已立即应用天赋效果：知识基础值提升到2。".to_owned())
            } else {
                Some("天赋立即效果已满足：知识基础值已不低于2。".to_owned())
            }
        },
        _ => None,
    }
}

fn stable_talent_index(target_id: &str, label: &str, message_time: u64, len: usize) -> usize {
    let mut hasher = DefaultHasher::new();
    target_id.hash(&mut hasher);
    label.hash(&mut hasher);
    message_time.hash(&mut hasher);
    (hasher.finish() as usize) % len
}

fn private_command_body(text: &str) -> Option<&str> {
    let text = text.trim();
    text.strip_prefix('.')
        .or_else(|| text.strip_prefix('。'))
        .map(str::trim)
        .filter(|command| !command.is_empty())
}

fn parse_post_creation_status_spend(text: &str) -> Option<(StatusKey, i32)> {
    let body = private_command_body(text)?;
    let mut parts = body.split_whitespace();
    let status_key = parse_status_key(parts.next()?)?;
    let points = parts.next()?.parse::<i32>().ok()?;
    parts.next().is_none().then_some((status_key, points))
}

fn parse_status_key(text: &str) -> Option<StatusKey> {
    match text.trim().to_ascii_lowercase().as_str() {
        "力量" | "力" | "str" => Some(StatusKey::Str),
        "敏捷" | "敏" | "agi" => Some(StatusKey::Agi),
        "灵巧" | "巧" | "dex" => Some(StatusKey::Dex),
        "体质" | "体" | "vit" => Some(StatusKey::Vit),
        "智力" | "智" | "int" => Some(StatusKey::Int),
        "智慧" | "慧" | "wis" => Some(StatusKey::Wis),
        "知识" | "知" | "k" => Some(StatusKey::K),
        "魅力" | "魅" | "cha" => Some(StatusKey::Cha),
        _ => None,
    }
}

fn spend_post_creation_status_points(
    character: &mut PlayerCharacter,
    status_key: StatusKey,
    points: i32,
    stat_config: &TrpgBasicConfig,
) -> String {
    if !character.inited {
        return "角色卡尚未完成。请先完成建卡流程。".to_owned();
    }
    if points <= 0 {
        return "请输入正数属性点。".to_owned();
    }
    if character.status_points <= 0 {
        return "你当前没有可用属性点。".to_owned();
    }
    if points > character.status_points {
        return format!(
            "属性点不足。你剩余{}点，但试图投入{}点。",
            character.status_points, points
        );
    }

    let current = get_character_status_value(&character.status, status_key);
    set_character_status_value(
        &mut character.status,
        status_key,
        current + points,
    );
    character.status_points -= points;
    update_character_from_status_with_config(character, stat_config);
    format!(
        "已为{}投入{}点，当前{}为{}。剩余属性点{}。",
        status_key.zh(),
        points,
        status_key.zh(),
        get_character_status_value(&character.status, status_key),
        character.status_points
    )
}

fn format_private_character_status(manager: &NapcatMessageManager, target_id: &str) -> String {
    let Some(character) = manager.player_characters.get(target_id) else {
        return "你还没有角色卡。输入【.兑换】开始建卡。".to_owned();
    };
    if !character.inited {
        return format!(
            "角色卡尚未完成。\n{}",
            character_creation_prompt(character)
        );
    }

    let mut sections = vec![
        format!(
            "角色：{}",
            character_display_name(character, target_id)
        ),
        format!(
            "等级：{}  经验：{} / {}",
            character.level,
            character.exp,
            character_next_level_exp(character.level)
        ),
        format!(
            "HP：{}/{}  MP：{}/{}",
            format_character_number(character.hp),
            format_character_number(character.max_hp),
            format_character_number(character.mp),
            format_character_number(character.max_mp)
        ),
        format!(
            "本轮承伤：{}  本轮受疗：{}",
            format_character_number(character.damage_taken_this_turn),
            format_character_number(character.healing_taken_this_turn)
        ),
        format!(
            "速度：{}",
            format_character_number(character.speed)
        ),
        format!(
            "剩余属性点：{}",
            character.status_points
        ),
        format_character_status_totals(character),
    ];

    if !character.active_buffs.is_empty() {
        let buffs = character
            .active_buffs
            .iter()
            .map(|buff| buff.name.as_str())
            .collect::<Vec<_>>()
            .join("、");
        sections.push(format!("状态效果：{buffs}"));
    }

    sections.join("\n")
}

fn format_private_character_skills(manager: &NapcatMessageManager, target_id: &str) -> String {
    let Some(character) = manager.player_characters.get(target_id) else {
        return "你还没有角色卡。输入【.兑换】开始建卡。".to_owned();
    };
    let skill_count = character_skill_count(character);
    if skill_count == 0 {
        return "还没有已兑换技能。".to_owned();
    }

    let mut lines = vec!["已兑换技能：".to_owned()];
    for index in 0..skill_count {
        let name = character_skill_display_name(character, index);
        let note = character
            .skill_notes
            .get(index)
            .map(|note| note.trim())
            .unwrap_or_default();
        let mp_cost = character
            .skill_mp_costs
            .get(index)
            .copied()
            .unwrap_or_default();
        let cooldown = character
            .skill_cooldown_turns
            .get(index)
            .copied()
            .unwrap_or_default();
        let metadata = character
            .skill_metadata
            .get(index)
            .cloned()
            .unwrap_or_default();
        let mut details = Vec::new();
        if !metadata.pc_approved {
            details.push("PC待确认".to_owned());
        }
        if !metadata.st_approved {
            details.push("GM待确认".to_owned());
        }
        if let Some(source) = character_skill_source_label(&metadata) {
            details.push(source);
        }
        if mp_cost > 0.0 {
            details.push(format!(
                "MP {}",
                format_character_number(mp_cost)
            ));
        }
        if cooldown > 0 {
            details.push(format!("CD {cooldown}轮"));
        }
        let detail = if details.is_empty() {
            String::new()
        } else {
            format!(" ({})", details.join("，"))
        };
        lines.push(format!(
            "{}. {}{}",
            index + 1,
            name,
            detail
        ));
        if !note.is_empty() {
            lines.push(format!("   {note}"));
        }
    }
    lines.join("\n")
}

fn character_skill_source_label(metadata: &CharacterSkillMetadata) -> Option<String> {
    match metadata.source {
        CharacterSkillSourceKind::Manual => None,
        CharacterSkillSourceKind::Talent => {
            let label = metadata
                .source_pool_label
                .as_deref()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or("天赋");
            Some(format!("来源 {label}"))
        },
        CharacterSkillSourceKind::SkillPool => {
            let label = metadata
                .source_pool_label
                .as_deref()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or("技能池");
            Some(format!("来源 {label}"))
        },
    }
}

fn format_private_character_cooldowns(
    manager: &mut NapcatMessageManager,
    target_id: &str,
) -> String {
    let current_turn = current_player_cooldown_turn(manager, target_id);
    let Some(character) = manager.player_characters.get_mut(target_id) else {
        return "你还没有角色卡。输入【.兑换】开始建卡。".to_owned();
    };
    materialize_imported_skill_cooldowns(character, current_turn);
    let skill_count = character_skill_count(character);
    if skill_count == 0 {
        return "还没有已兑换技能。".to_owned();
    }

    let mut lines = Vec::new();
    for index in 0..skill_count {
        let cooldown = character
            .skill_cooldown_turns
            .get(index)
            .copied()
            .unwrap_or_default();
        let has_cast_record = character
            .skill_last_cast_turns
            .contains_key(&index.to_string());
        let cooldown_left = character
            .skill_metadata
            .get(index)
            .and_then(|metadata| metadata.cooldown_left);
        if cooldown == 0 && !has_cast_record && cooldown_left.unwrap_or_default() == 0 {
            continue;
        }

        let name = character_skill_display_name(character, index);
        let remaining = skill_cooldown_remaining(
            character,
            index,
            cooldown,
            cooldown_left,
            current_turn,
        );
        if remaining == 0 {
            lines.push(format!("{name}：可用"));
        } else {
            lines.push(format!("{name}：还剩{remaining}轮"));
        }
    }

    if lines.is_empty() {
        "没有需要冷却的技能。".to_owned()
    } else {
        format!("技能冷却：\n{}", lines.join("\n"))
    }
}

fn format_private_channel_members(manager: &NapcatMessageManager, target_id: &str) -> String {
    let Some(group) = manager.group_for_player_target(target_id) else {
        return if manager.trpg_groups.is_empty() {
            "当前没有TRPG组。".to_owned()
        } else {
            "你还没有加入当前TRPG组。".to_owned()
        };
    };

    let access = target_id
        .parse::<u64>()
        .map(|player_id| group.player_access(player_id))
        .unwrap_or_default();
    let scope_name = access
        .party_id
        .as_deref()
        .map(|party_id| format!("小队「{party_id}」"))
        .unwrap_or_else(|| "公开频道".to_owned());
    let names = group
        .players
        .iter()
        .filter(|member_id| visible_channel_member(group, &access, member_id))
        .map(|member_id| private_target_display_name(manager, member_id))
        .collect::<Vec<_>>();
    if names.is_empty() {
        format!("当前频道：{scope_name}\n成员：无")
    } else {
        format!(
            "当前频道：{scope_name}\n成员：{}",
            names.join("、")
        )
    }
}

fn format_private_group_guide(manager: &NapcatMessageManager, target_id: &str) -> String {
    let Some(group) = manager.group_for_player_target(target_id) else {
        return if manager.trpg_groups.is_empty() {
            "当前没有TRPG组。".to_owned()
        } else {
            "你还没有加入当前TRPG组。".to_owned()
        };
    };

    let guide = group.guide.trim();
    if guide.is_empty() {
        "当前TRPG组还没有引导文本。".to_owned()
    } else {
        format!("团内引导：\n{guide}")
    }
}

fn visible_channel_member(group: &TrpgGroup, access: &PlayerAccess, target_id: &str) -> bool {
    if target_id == access.player_id.to_string() {
        return true;
    }
    match group.party_id_for_player(target_id) {
        Some(party_id) => access.can_read(&Visibility::Party(party_id.to_owned())),
        None => access.can_read(&Visibility::Public),
    }
}

fn private_target_display_name(manager: &NapcatMessageManager, target_id: &str) -> String {
    manager
        .player_characters
        .get(target_id)
        .map(|character| character_display_name(character, target_id))
        .or_else(|| {
            manager.chat_targets.get(target_id).and_then(|metadata| {
                if !metadata.display_name.trim().is_empty() {
                    Some(metadata.display_name.trim().to_owned())
                } else if !metadata.automatic_name.trim().is_empty() {
                    Some(metadata.automatic_name.trim().to_owned())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| target_id.to_owned())
}

fn character_skill_count(character: &PlayerCharacter) -> usize {
    character
        .skill_names
        .len()
        .max(character.skill_notes.len())
        .max(character.skill_mp_costs.len())
        .max(character.skill_cooldown_turns.len())
        .max(character.skill_metadata.len())
}

fn character_skill_display_name(character: &PlayerCharacter, index: usize) -> String {
    character
        .skill_names
        .get(index)
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("技能{}", index + 1))
}

fn current_player_cooldown_turn(manager: &NapcatMessageManager, target_id: &str) -> u32 {
    manager
        .group_for_player_target(target_id)
        .map(|group| {
            group
                .player_turns
                .get(target_id)
                .map(|turn| turn.turns_passed)
                .unwrap_or(group.world_turn)
        })
        .unwrap_or_default()
}

fn skill_cooldown_remaining(
    character: &PlayerCharacter,
    skill_index: usize,
    cooldown_turns: u32,
    cooldown_left: Option<u32>,
    current_turn: u32,
) -> u32 {
    let skill_key = skill_index.to_string();
    if let Some(last_cast_turn) = character.skill_last_cast_turns.get(&skill_key) {
        return cooldown_turns.saturating_sub(current_turn.saturating_sub(*last_cast_turn));
    }
    character
        .skill_cooldown_ready_turns
        .get(&skill_key)
        .map(|ready_turn| ready_turn.saturating_sub(current_turn))
        .unwrap_or_else(|| cooldown_left.unwrap_or_default())
}

pub fn materialize_imported_skill_cooldowns(character: &mut PlayerCharacter, current_turn: u32) {
    for (index, metadata) in character.skill_metadata.iter().enumerate() {
        let skill_key = index.to_string();
        if character.skill_last_cast_turns.contains_key(&skill_key)
            || character
                .skill_cooldown_ready_turns
                .contains_key(&skill_key)
        {
            continue;
        }
        let remaining = metadata.cooldown_left.unwrap_or_default();
        if remaining > 0 {
            character.skill_cooldown_ready_turns.insert(
                skill_key,
                current_turn.saturating_add(remaining),
            );
        }
    }
}

fn format_character_number(value: f32) -> String {
    if (value.fract()).abs() < f32::EPSILON {
        format!("{}", value as i32)
    } else {
        format!("{value:.1}")
    }
}

fn format_character_status_totals(character: &PlayerCharacter) -> String {
    [
        StatusKey::Str,
        StatusKey::Agi,
        StatusKey::Dex,
        StatusKey::Vit,
        StatusKey::Int,
        StatusKey::Wis,
        StatusKey::K,
        StatusKey::Cha,
    ]
    .iter()
    .map(|status_key| {
        let base = get_character_status_value(&character.status, *status_key);
        let extra = get_character_status_value(&character.extra_status, *status_key);
        let total = base + extra;
        if extra == 0 {
            format!("{}:「{}」", status_key.zh(), total)
        } else {
            format!(
                "{}:「{}」({:+})",
                status_key.zh(),
                total,
                extra
            )
        }
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn character_creation_next(character: &mut PlayerCharacter) -> String {
    match character.creation_step {
        CharacterCreationStep::ConfirmStatus => {
            character.creation_step = CharacterCreationStep::Skill;
            "属性数据已录入。\n现在是技能兑换，请按你的技能描述发送文本；输入【.】可以跳过或结束技能录入。".to_owned()
        },
        CharacterCreationStep::Skill => {
            character.creation_step = CharacterCreationStep::ConfirmSkill;
            character_creation_next(character)
        },
        CharacterCreationStep::ConfirmSkill => {
            character.creation_step = CharacterCreationStep::Image;
            "技能数据已录入。现在请发送人物立绘图片链接；如果暂时没有，输入【.】跳过。".to_owned()
        },
        CharacterCreationStep::Image => {
            character.creation_step = CharacterCreationStep::Nickname;
            "图片已跳过。\n最后，请告诉我你的角色名，兑换即将结束。".to_owned()
        },
        CharacterCreationStep::Nickname => "请直接发送角色名完成建卡。".to_owned(),
        _ => character_creation_prompt(character),
    }
}

fn character_creation_back(character: &mut PlayerCharacter) -> String {
    if character.creation_step.status_key().is_some()
        || character.creation_step == CharacterCreationStep::ConfirmStatus
    {
        reset_character_status_phase(character);
        return format!(
            "已退回属性兑换第一步，属性点已全部返还。\n{}",
            character_creation_prompt(character)
        );
    }

    match character.creation_step {
        CharacterCreationStep::Skill | CharacterCreationStep::ConfirmSkill => {
            reset_character_status_phase(character);
            format!(
                "已退回属性兑换第一步，属性点已全部返还。\n{}",
                character_creation_prompt(character)
            )
        },
        CharacterCreationStep::Image => {
            character.creation_step = CharacterCreationStep::Skill;
            "已退回技能兑换。请继续发送技能描述；输入【.】结束技能录入。".to_owned()
        },
        CharacterCreationStep::Nickname => {
            character.creation_step = CharacterCreationStep::Image;
            character.image.clear();
            "已退回图片录入。请发送人物立绘图片链接；如果暂时没有，输入【.】跳过。".to_owned()
        },
        CharacterCreationStep::Normal => "未处于建卡流程。输入【.兑换】开始。".to_owned(),
        CharacterCreationStep::ConfirmStatus
        | CharacterCreationStep::Str
        | CharacterCreationStep::Agi
        | CharacterCreationStep::Dex
        | CharacterCreationStep::Vit
        | CharacterCreationStep::Int
        | CharacterCreationStep::Wis
        | CharacterCreationStep::K
        | CharacterCreationStep::Cha => unreachable!("status steps return before match"),
    }
}

fn character_creation_prompt(character: &PlayerCharacter) -> String {
    if let Some(status_key) = character.creation_step.status_key() {
        return format!(
            "当前{}:「{}」 剩余属性点:「{}」",
            status_key.zh(),
            get_character_status_value(&character.status, status_key),
            character.status_points
        );
    }

    match character.creation_step {
        CharacterCreationStep::ConfirmStatus => character_status_confirmation(character),
        CharacterCreationStep::Skill => {
            format!(
                "现在是技能兑换。你还剩余{}分，请发送技能描述；输入【.】结束技能录入。",
                character.exchange_points
            )
        },
        CharacterCreationStep::ConfirmSkill => "技能数据已录入，输入【.】继续。".to_owned(),
        CharacterCreationStep::Image => "请发送人物立绘图片链接；输入【.】跳过。".to_owned(),
        CharacterCreationStep::Nickname => "最后，请告诉我你的角色名。".to_owned(),
        CharacterCreationStep::Normal => "未处于建卡流程。输入【.兑换】开始。".to_owned(),
        _ => "请继续当前建卡步骤。".to_owned(),
    }
}

fn character_status_confirmation(character: &PlayerCharacter) -> String {
    format!(
        "属性兑换全部完成，输入【.】确认；输入【..】退回上一步。\n{}",
        format_character_status(character)
    )
}

fn format_character_status(character: &PlayerCharacter) -> String {
    [
        StatusKey::Str,
        StatusKey::Agi,
        StatusKey::Dex,
        StatusKey::Vit,
        StatusKey::Int,
        StatusKey::Wis,
        StatusKey::K,
        StatusKey::Cha,
    ]
    .iter()
    .map(|status_key| {
        format!(
            "{}:「{}」",
            status_key.zh(),
            get_character_status_value(&character.status, *status_key)
        )
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn get_character_status_value(status: &CharacterStatus, status_key: StatusKey) -> i32 {
    match status_key {
        StatusKey::Str => status.str_,
        StatusKey::Agi => status.agi,
        StatusKey::Dex => status.dex,
        StatusKey::Vit => status.vit,
        StatusKey::Int => status.int_,
        StatusKey::Wis => status.wis,
        StatusKey::K => status.k,
        StatusKey::Cha => status.cha,
    }
}

fn set_character_status_value(status: &mut CharacterStatus, status_key: StatusKey, value: i32) {
    match status_key {
        StatusKey::Str => status.str_ = value,
        StatusKey::Agi => status.agi = value,
        StatusKey::Dex => status.dex = value,
        StatusKey::Vit => status.vit = value,
        StatusKey::Int => status.int_ = value,
        StatusKey::Wis => status.wis = value,
        StatusKey::K => status.k = value,
        StatusKey::Cha => status.cha = value,
    }
}

fn reset_character_status_phase(character: &mut PlayerCharacter) {
    character.creation_step = CharacterCreationStep::Str;
    character.status_points += total_allocated_status_points(&character.status);
    character.status = CharacterStatus::default();
}

fn total_allocated_status_points(status: &CharacterStatus) -> i32 {
    [
        status.str_,
        status.agi,
        status.dex,
        status.vit,
        status.int_,
        status.wis,
        status.k,
        status.cha,
    ]
    .iter()
    .copied()
    .filter(|value| *value > 0)
    .sum()
}

pub fn update_character_from_status(character: &mut PlayerCharacter) {
    update_character_from_status_with_config(character, &TrpgBasicConfig::default());
}

pub fn update_character_from_status_with_config(
    character: &mut PlayerCharacter,
    config: &TrpgBasicConfig,
) {
    let total = character_total_status(character);

    character.max_hp = (config.base_max_hp
        + character.level as f32 * config.lv_max_hp
        + total.str_ as f32 * config.str_max_hp
        + total.vit as f32 * config.vit_max_hp)
        .max(1.0);
    character.hp = character.max_hp;
    character.hp_regen = total.vit.max(0) as f32 * config.vit_hp_reg;
    character.max_mp = total.int_ as f32 * config.int_max_mp + total.wis as f32 * config.wis_max_mp;
    character.mp = character.max_mp.max(0.0);
    character.mp_regen = total.wis.max(0) as f32 * config.wis_mp_reg;
    character.speed = config.basic_speed
        + total.str_.max(0) as f32 * config.str_speed
        + total.agi.max(0) as f32 * config.agi_speed
        + total.dex.max(0) as f32 * config.dex_speed;
}

pub fn character_next_level_exp(level: i32) -> i32 {
    let level = level.max(1) as i64;
    let required = level * (level - 1) * 25 / 2 + 100;
    required.min(i32::MAX as i64) as i32
}

pub fn grant_character_experience(character: &mut PlayerCharacter, amount: i32) -> i32 {
    if amount <= 0 {
        return 0;
    }
    character.level = character.level.max(1);
    character.exp = character.exp.saturating_add(amount);

    let mut level_ups = 0;
    while character.level < 999 {
        let required = character_next_level_exp(character.level);
        if character.exp < required {
            break;
        }
        character.exp -= required;
        character.level += 1;
        level_ups += 1;
    }
    level_ups
}

pub fn character_total_status(character: &PlayerCharacter) -> CharacterStatus {
    character.status.combined(&character.extra_status)
}

pub fn character_damage_attribute_multiplier(
    character: &PlayerCharacter,
    config: &TrpgBasicConfig,
    kind: TrpgDamageBonusKind,
) -> f32 {
    let total = character_total_status(character);
    let base = status_damage_attribute_multiplier(&total, config, kind)
        + character_moonberry_talent_damage_attribute_bonus(character, &total, kind);
    if kind == TrpgDamageBonusKind::Range {
        base + character_range_magic_converter_damage_bonus(character, &total, config)
    } else {
        base
    }
}

pub fn character_moonberry_talent_damage_attribute_bonus(
    character: &PlayerCharacter,
    total: &CharacterStatus,
    kind: TrpgDamageBonusKind,
) -> f32 {
    match kind {
        TrpgDamageBonusKind::Magical
            if character_has_approved_moonberry_talent(character, "大魔法师") =>
        {
            total.int_ as f32 * 0.005
        },
        _ => 0.0,
    }
}

pub fn character_range_magic_converter_damage_bonus(
    character: &PlayerCharacter,
    total: &CharacterStatus,
    config: &TrpgBasicConfig,
) -> f32 {
    if !character_has_approved_moonberry_talent(character, "数魔转换器") {
        return 0.0;
    }
    (status_damage_attribute_multiplier(
        total,
        config,
        TrpgDamageBonusKind::Magical,
    ) - 1.0
        + character_moonberry_talent_damage_attribute_bonus(
            character,
            total,
            TrpgDamageBonusKind::Magical,
        ))
    .max(0.0)
}

pub fn character_damage_taken_attribute_multiplier(
    character: &PlayerCharacter,
    kind: TrpgDamageTakenKind,
) -> f32 {
    let mut multiplier = 1.0;
    match kind {
        TrpgDamageTakenKind::Magical => {
            if character_has_approved_moonberry_talent(character, "抗魔体质") {
                multiplier *= 0.9;
            }
        },
        TrpgDamageTakenKind::Diseased | TrpgDamageTakenKind::Poisoning => {
            if character_has_approved_moonberry_talent(character, "人类基因工程") {
                multiplier *= 0.85;
            }
        },
        TrpgDamageTakenKind::Other => {},
    }
    multiplier
}

pub fn character_damage_dealt_talent_buffs(
    character: &PlayerCharacter,
    source_id: &str,
) -> Vec<BuffSpec> {
    let mut buffs = Vec::new();
    if character_has_approved_moonberry_talent(character, "溃伤") {
        buffs.push(moonberry_wound_buff(source_id));
    }
    buffs
}

pub fn character_physical_damage_lifesteal(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "禅宗古训") {
        0.15
    } else {
        0.0
    }
}

pub fn character_physical_damage_followup_rate(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "苏萨斯之爪") {
        0.35
    } else {
        0.0
    }
}

pub fn moonberry_physical_damage_followup_buff(source_id: &str, amount: f32) -> BuffSpec {
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

pub fn character_minimum_damage_floor(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "菜鸡猛啄") {
        character.level.max(0) as f32
    } else {
        0.0
    }
}

pub fn character_chaos_output_variance(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "混沌无序") {
        0.15
    } else {
        0.0
    }
}

pub fn moonberry_chaos_output_multiplier(variance: f32) -> f32 {
    let variance = variance.clamp(0.0, 1.0);
    if variance <= f32::EPSILON {
        1.0
    } else {
        rand::rng().random_range((1.0 - variance)..=(1.0 + variance))
    }
}

pub fn character_minimum_range_meters(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "瞄准镜Tex-30") {
        character.level.max(0) as f32 * 15.0
    } else {
        0.0
    }
}

pub fn character_spell_range_multiplier(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "魔网延伸") {
        1.05
    } else {
        1.0
    }
}

pub fn character_gale_force_battle_speeds(character: &PlayerCharacter) -> Option<(f32, f32)> {
    if !character_has_approved_moonberry_talent(character, "狂风恶浪") {
        return None;
    }
    let base_speed = character
        .buff_base_stats
        .as_ref()
        .map(|stats| stats.speed)
        .unwrap_or(character.speed)
        .max(0.0);
    let normal_speed = if character.buff_base_stats.is_some() {
        character.speed.max(0.0)
    } else {
        base_speed * 1.2
    };
    let low_survivor_speed = if character.buff_base_stats.is_some() {
        normal_speed + base_speed * 0.15
    } else {
        base_speed * 1.35
    };
    Some((
        normal_speed,
        low_survivor_speed.max(normal_speed),
    ))
}

pub fn character_valorous_battle_damage_multiplier(
    character: &PlayerCharacter,
    completed_turns: u32,
) -> f32 {
    if character_has_approved_moonberry_talent(character, "越战越勇") {
        1.0 + (completed_turns as f32 * 0.02).min(0.20)
    } else {
        1.0
    }
}

pub fn character_fighting_spirit_damage_taken_multiplier(
    character: &PlayerCharacter,
    completed_own_turns: u32,
) -> f32 {
    if !character_has_approved_moonberry_talent(character, "斗志昂扬") {
        return 1.0;
    }
    match completed_own_turns {
        0 => 0.5,
        1 => 0.9,
        2 => 0.98,
        _ => 1.0,
    }
}

pub fn character_arrogance_damage_bonus_per_source(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "狂妄") {
        0.10
    } else {
        0.0
    }
}

pub fn arrogance_damage_dealt_multiplier(bonus_per_source: f32, source_count: u32) -> f32 {
    1.0 + (bonus_per_source.max(0.0) * source_count as f32).min(0.30)
}

pub fn character_endless_pain_bonus_damage_per_stack(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "无尽痛楚") {
        character.level.max(0) as f32 * 1.5
    } else {
        0.0
    }
}

pub fn endless_pain_bonus_damage(bonus_damage_per_stack: f32, stacks: u32) -> f32 {
    bonus_damage_per_stack.max(0.0) * stacks.min(2) as f32
}

pub fn character_infinite_focus_damage_bonus_per_stack(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "无限专注") {
        0.10
    } else {
        0.0
    }
}

pub fn infinite_focus_damage_dealt_multiplier(bonus_per_stack: f32, stacks: u32) -> f32 {
    1.0 + (bonus_per_stack.max(0.0) * stacks as f32).min(0.20)
}

pub fn character_champion_damage_bonus_per_stack(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "总冠军") {
        0.02
    } else {
        0.0
    }
}

pub fn character_champion_damage_reduction_per_stack(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "总冠军") {
        0.01
    } else {
        0.0
    }
}

pub fn champion_damage_dealt_multiplier(bonus_per_stack: f32, stacks: u32) -> f32 {
    1.0 + bonus_per_stack.max(0.0) * stacks as f32
}

pub fn champion_damage_taken_multiplier(reduction_per_stack: f32, stacks: u32) -> f32 {
    (1.0 - reduction_per_stack.max(0.0) * stacks as f32).max(0.0)
}

pub fn character_dominion_max_hp_gain_rate(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "役于我手") {
        0.05
    } else {
        0.0
    }
}

pub fn character_dominion_max_hp_bonus_cap(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "役于我手") {
        character.max_hp.max(0.0) * 0.20
    } else {
        0.0
    }
}

pub fn character_sin_on_sin_exp_bonus_per_stack(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "罪上加罪") {
        2.5
    } else {
        0.0
    }
}

pub fn character_sin_on_sin_recovery_rate(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "罪上加罪") {
        0.10
    } else {
        0.0
    }
}

pub fn sin_on_sin_exp_bonus_percent(bonus_per_stack: f32, stacks: u32) -> f32 {
    (bonus_per_stack.max(0.0) * stacks as f32).min(10.0)
}

pub fn character_penance_healing_bonus_percent(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "忏悔") {
        25.0
    } else {
        0.0
    }
}

pub fn penance_decayed_healing_dealt_modifier(
    current_modifier: f32,
    initial_bonus_percent: f32,
    kill_assist_count: u32,
) -> f32 {
    let current_modifier = current_modifier.max(0.0);
    if initial_bonus_percent <= f32::EPSILON {
        return current_modifier;
    }
    let initial_multiplier = 1.0 + initial_bonus_percent / 100.0;
    let remaining_bonus_percent =
        (initial_bonus_percent - kill_assist_count as f32 * 10.0).max(0.0);
    current_modifier / initial_multiplier * (1.0 + remaining_bonus_percent / 100.0)
}

pub fn moonberry_skill_type_is_spell(skill_type: Option<&str>) -> bool {
    matches!(skill_type.map(str::trim), Some("法术"))
}

pub fn moonberry_effective_skill_range_radius_with_multiplier(
    skill_range: Option<i32>,
    minimum_range_meters: f32,
    range_multiplier: f32,
) -> Option<f32> {
    let skill_range = skill_range
        .filter(|range| *range > 0)
        .map(|range| range as f32 * range_multiplier.max(0.0));
    if minimum_range_meters > f32::EPSILON {
        Some(skill_range.unwrap_or(0.0).max(minimum_range_meters))
    } else {
        skill_range
    }
}

pub fn character_large_hit_damage_taken_modifier(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "过度免疫") {
        0.8
    } else {
        1.0
    }
}

pub fn large_hit_damage_taken_multiplier(
    max_hp: f32,
    incoming_damage: f32,
    large_hit_modifier: f32,
) -> f32 {
    if max_hp > 0.0 && incoming_damage > max_hp * 0.2 {
        large_hit_modifier.max(0.0)
    } else {
        1.0
    }
}

pub fn character_dying_target_healing_modifier(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "生死时速") {
        1.5
    } else {
        1.0
    }
}

pub fn dying_target_healing_multiplier(
    hp: f32,
    max_hp: f32,
    dying_target_healing_modifier: f32,
) -> f32 {
    if max_hp > 0.0 && hp <= max_hp * 0.2 {
        dying_target_healing_modifier.max(0.0)
    } else {
        1.0
    }
}

pub fn character_wounded_healing_dealt_modifier(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "火源之力") {
        1.2
    } else {
        1.0
    }
}

pub fn wounded_healing_dealt_multiplier(
    hp: f32,
    max_hp: f32,
    wounded_healing_modifier: f32,
) -> f32 {
    if max_hp <= 0.0 || wounded_healing_modifier <= 1.0 {
        return 1.0;
    }
    if hp <= max_hp * 0.2 {
        1.0
    } else if hp <= max_hp * 0.6 {
        1.0 + (wounded_healing_modifier - 1.0) * 0.5
    } else {
        wounded_healing_modifier
    }
}

pub fn character_mutual_aid_healing_rate(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "互帮互助") {
        0.5
    } else {
        0.0
    }
}

pub fn character_one_heart_healing_bonus_per_stack(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "一心") {
        0.05
    } else {
        0.0
    }
}

pub fn one_heart_healing_dealt_multiplier(bonus_per_stack: f32, stacks: u32) -> f32 {
    1.0 + (bonus_per_stack.max(0.0) * stacks as f32).min(0.25)
}

pub fn character_echoing_memory_healing_rates(character: &PlayerCharacter) -> Option<(f32, f32)> {
    if character_has_approved_moonberry_talent(character, "千万回忆") {
        Some((0.15, 0.05))
    } else {
        None
    }
}

pub fn character_liquid_body_damage_delay_rate(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "液态躯体") {
        0.5
    } else {
        0.0
    }
}

pub fn character_liquid_body_self_healing_rate(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "液态躯体") {
        0.05
    } else {
        0.0
    }
}

pub fn character_calm_heart_healing_rate(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "息心") {
        0.5
    } else {
        0.0
    }
}

pub fn character_rest_then_fight_healing_rate(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "以逸待劳") {
        0.05
    } else {
        0.0
    }
}

pub fn character_keen_evasion_available(character: &PlayerCharacter) -> bool {
    character_has_approved_moonberry_talent(character, "敏锐")
}

pub fn character_arcane_shield_amount(character: &PlayerCharacter) -> f32 {
    character.max_mp.max(0.0) * character_arcane_shield_rate(character)
}

pub fn character_arcane_shield_rate(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "奥术护盾") {
        0.10
    } else {
        0.0
    }
}

pub fn character_overhealing_shield_cap_rate(character: &PlayerCharacter) -> f32 {
    if character_has_approved_moonberry_talent(character, "过度治疗") {
        0.30
    } else {
        0.0
    }
}

pub fn character_undying_rage_available(character: &PlayerCharacter) -> bool {
    character_has_approved_moonberry_talent(character, "不死者之怒")
}

pub fn character_hope_avatar_available(character: &PlayerCharacter) -> bool {
    character_has_approved_moonberry_talent(character, "希望化身")
}

pub fn character_inspiration_available(character: &PlayerCharacter) -> bool {
    character_has_approved_moonberry_talent(character, "振奋")
}

pub fn upsert_character_active_buff(character: &mut PlayerCharacter, buff: BuffSpec) -> bool {
    if let Some(existing) = character
        .active_buffs
        .iter_mut()
        .find(|existing| existing.name == buff.name && existing.source_id == buff.source_id)
    {
        if *existing == buff {
            return false;
        }
        *existing = buff;
        true
    } else {
        character.active_buffs.push(buff);
        true
    }
}

pub fn character_has_approved_moonberry_talent(
    character: &PlayerCharacter,
    talent_name: &str,
) -> bool {
    character
        .skill_metadata
        .iter()
        .enumerate()
        .any(|(index, metadata)| {
            metadata.is_approved()
                && metadata.source == CharacterSkillSourceKind::Talent
                && character
                    .skill_names
                    .get(index)
                    .is_some_and(|name| name.trim() == talent_name)
        })
}

fn moonberry_wound_buff(source_id: &str) -> BuffSpec {
    BuffSpec {
        name: "溃伤".to_owned(),
        kind: BuffKind::Bleed,
        priority: 0,
        turns_remaining: 1,
        source_id: format!("{source_id}:talent:溃伤"),
        beneficial: false,
        effects: vec![BuffEffect {
            field: BuffField::HealingTakenModifier,
            value: BuffValue::AddPercent(-25.0),
        }],
        tick_actions: Vec::new(),
    }
}

pub fn status_damage_attribute_multiplier(
    total: &CharacterStatus,
    config: &TrpgBasicConfig,
    kind: TrpgDamageBonusKind,
) -> f32 {
    match kind {
        TrpgDamageBonusKind::Magical => 1.0 + total.int_ as f32 * config.int_damage_bonus,
        TrpgDamageBonusKind::Physical => {
            1.0 + total.str_ as f32 * config.str_damage_bonus
                + (total.agi % 50) as f32 * config.agi_damage_bonus
                + total.dex as f32 * config.dex_damage_bonus
        },
        TrpgDamageBonusKind::Range => 1.0 + total.dex as f32 * config.dex_range_damage_bonus,
        TrpgDamageBonusKind::Other => 1.0,
    }
}

pub fn low_hp_damage_multiplier(hp: f32, max_hp: f32) -> f32 {
    low_hp_damage_multiplier_with_fatigue(hp, max_hp, false)
}

pub fn character_fatigue_walker_available(character: &PlayerCharacter) -> bool {
    character_has_approved_moonberry_talent(character, "疲惫行者")
}

pub fn character_low_hp_damage_multiplier(character: &PlayerCharacter) -> f32 {
    low_hp_damage_multiplier_with_fatigue(
        character.hp,
        character.max_hp,
        character_fatigue_walker_available(character),
    )
}

pub fn low_hp_damage_multiplier_with_fatigue(hp: f32, max_hp: f32, fatigue_walker: bool) -> f32 {
    if max_hp <= 0.0 {
        return 0.0;
    }
    let effective_hp = if fatigue_walker { hp.max(max_hp * 0.05) } else { hp };
    let missing_ratio = ((max_hp - effective_hp) / max_hp).clamp(0.0, 1.0);
    let multiplier = if effective_hp > max_hp * 0.8 {
        1.0
    } else if effective_hp > max_hp * 0.6 {
        1.0 - 0.1 * missing_ratio
    } else if effective_hp > max_hp * 0.4 {
        1.0 - 0.5 * missing_ratio
    } else {
        1.0 - missing_ratio
    };
    if fatigue_walker {
        1.0 - (1.0 - multiplier) * 0.8
    } else {
        multiplier
    }
}

pub fn character_healing_attribute_multiplier(
    character: &PlayerCharacter,
    config: &TrpgBasicConfig,
) -> f32 {
    status_healing_attribute_multiplier(
        &character_total_status(character),
        config,
    )
}

pub fn status_healing_attribute_multiplier(
    total: &CharacterStatus,
    config: &TrpgBasicConfig,
) -> f32 {
    1.0 + total.int_ as f32 * config.int_heal_bonus + total.wis as f32 * config.wis_heal_bonus
}

fn message_text(message: &NapcatMessage) -> String {
    message
        .data
        .message
        .iter()
        .filter_map(|chain| match &chain.variant {
            NapcatMessageChainType::Text { data } => Some(data.text.as_str()),
            NapcatMessageChainType::Source(_) => None,
            NapcatMessageChainType::Image { .. } => None,
            NapcatMessageChainType::Unsupported => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn message_image_reference(message: &NapcatMessage) -> Option<String> {
    message.data.message.iter().find_map(|chain| {
        let NapcatMessageChainType::Image { data } = &chain.variant else {
            return None;
        };
        if !data.local_path.trim().is_empty() {
            Some(data.local_path.trim().to_owned())
        } else if !data.url.trim().is_empty() {
            Some(data.url.trim().to_owned())
        } else if !data.file_id.trim().is_empty() {
            Some(data.file_id.trim().to_owned())
        } else {
            None
        }
    })
}

struct AutoForwardRequest {
    recipients: Vec<u64>,
    text: String,
}

fn auto_forward_request(
    manager: &NapcatMessageManager,
    message: &NapcatMessage,
    target_id: &str,
) -> Option<AutoForwardRequest> {
    if !matches!(
        message.data.message_type,
        NapcatMessageType::Private
    ) || message.data.user_id == message.data.self_id
    {
        return None;
    }

    let text = quoted_auto_forward_text(message)?;
    let sender_access = auto_forward_sender_access(manager, target_id);
    if sender_access.is_none()
        && manager
            .trpg_groups
            .values()
            .any(|group| group.players.iter().any(|player_id| player_id == target_id))
    {
        return None;
    }
    let recipients = manager
        .groups
        .values()
        .find(|group| group.members.iter().any(|member_id| member_id == target_id))?
        .members
        .iter()
        .filter(|member_id| member_id.as_str() != target_id)
        .filter_map(|member_id| {
            let is_private_member = matches!(
                manager
                    .messages
                    .get(member_id)
                    .and_then(|messages| messages.first())
                    .map(|message| &message.data.message_type),
                Some(NapcatMessageType::Private)
            );
            if !is_private_member {
                return None;
            }
            if !auto_forward_recipient_allowed(sender_access.as_ref(), member_id) {
                return None;
            }
            member_id.parse::<u64>().ok()
        })
        .collect::<Vec<_>>();

    if recipients.is_empty() {
        return None;
    }

    Some(AutoForwardRequest {
        recipients,
        text: format!(
            "{}: {}",
            message.data.sender.nickname, text
        ),
    })
}

fn auto_forward_sender_access<'a>(
    manager: &'a NapcatMessageManager,
    target_id: &str,
) -> Option<(&'a TrpgGroup, PlayerAccess)> {
    let group = manager.group_for_player_target(target_id)?;
    let player_id = target_id.parse::<u64>().ok()?;
    Some((group, group.player_access(player_id)))
}

fn auto_forward_recipient_allowed(
    sender_access: Option<&(&TrpgGroup, PlayerAccess)>,
    member_id: &str,
) -> bool {
    let Some((group, access)) = sender_access else {
        return true;
    };
    group.players.iter().any(|player_id| player_id == member_id)
        && visible_channel_member(group, access, member_id)
}

fn quoted_auto_forward_text(message: &NapcatMessage) -> Option<String> {
    let mut text = String::new();
    for chain in &message.data.message {
        if let NapcatMessageChainType::Text { data } = &chain.variant {
            text.push_str(&data.text);
        }
    }

    let text = text.trim();
    let mut indexed_chars = text.char_indices();
    let (_, start_quote) = indexed_chars.next()?;
    let (end_quote_index, end_quote) = indexed_chars.next_back()?;
    if !is_auto_forward_quote(start_quote) || !is_auto_forward_quote(end_quote) {
        return None;
    }

    let inner = text[start_quote.len_utf8()..end_quote_index].trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_owned())
    }
}

fn is_auto_forward_quote(character: char) -> bool {
    matches!(character, '"' | '“' | '”' | '＂')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_manager() -> NapcatMessageManager {
        NapcatMessageManager {
            messages: HashMap::default(),
            chat_targets: HashMap::default(),
            player_characters: HashMap::default(),
            trpg_groups: HashMap::default(),
            current_trpg_group: None,
            groups: HashMap::default(),
            read_message_counts: HashMap::default(),
            summarized_message_counts: HashMap::default(),
            open_chat_targets: HashSet::default(),
            pending_chat_targets: HashSet::default(),
            rejected_chat_targets: HashSet::default(),
            random_pools: HashMap::default(),
            skill_pool: Vec::new(),
            unit_pool: HashMap::default(),
        }
    }

    fn test_message(message_type: NapcatMessageType) -> NapcatMessage {
        test_message_with_text(message_type, "hello")
    }

    fn test_private_message_from(user_id: u64, text: &str) -> NapcatMessage {
        let mut message = test_message_with_text(NapcatMessageType::Private, text);
        message.data.user_id = user_id;
        message.data.sender.user_id = user_id;
        message.data.sender.nickname = format!("user-{user_id}");
        message
    }

    fn test_message_with_text(message_type: NapcatMessageType, text: &str) -> NapcatMessage {
        NapcatMessage {
            data: NapcatMessageData {
                time: 1780132600,
                message_type,
                message: vec![NapcatMessageChain {
                    variant: NapcatMessageChainType::Text {
                        data: TextData {
                            text: text.to_owned(),
                        },
                    },
                }],
                self_id: 1,
                user_id: 2,
                group_id: None,
                group_name: None,
                target_id: None,
                sender: NapcatSender {
                    user_id: 2,
                    nickname: "tester".to_owned(),
                },
                campaign_id: default_campaign_id(),
                character_id: None,
                party_id: None,
                visibility: Visibility::Public,
            },
        }
    }

    fn test_private_image(url: &str) -> NapcatMessage {
        NapcatMessage {
            data: NapcatMessageData {
                time: 1780132600,
                message_type: NapcatMessageType::Private,
                message: vec![NapcatMessageChain {
                    variant: NapcatMessageChainType::Image {
                        data: ImageData {
                            sub_type: 0,
                            file: String::new(),
                            url: url.to_owned(),
                            file_id: String::new(),
                            file_size: String::new(),
                            local_path: String::new(),
                        },
                    },
                }],
                self_id: 1,
                user_id: 2,
                group_id: None,
                group_name: None,
                target_id: None,
                sender: NapcatSender {
                    user_id: 2,
                    nickname: "tester".to_owned(),
                },
                campaign_id: default_campaign_id(),
                character_id: None,
                party_id: None,
                visibility: Visibility::Public,
            },
        }
    }

    #[test]
    fn skill_numeric_arg_values_keeps_only_numeric_values() {
        let args = vec![
            SkillPoolArg {
                name: "伤害值".to_owned(),
                kind: "数字".to_owned(),
                value: "3.5".to_owned(),
            },
            SkillPoolArg {
                name: "英文数字".to_owned(),
                kind: "number".to_owned(),
                value: "2".to_owned(),
            },
            SkillPoolArg {
                name: "默认数字".to_owned(),
                kind: String::new(),
                value: "1".to_owned(),
            },
            SkillPoolArg {
                name: "状态".to_owned(),
                kind: "BUFF".to_owned(),
                value: "守护".to_owned(),
            },
            SkillPoolArg {
                name: "坏值".to_owned(),
                kind: "数字".to_owned(),
                value: "很多".to_owned(),
            },
        ];

        assert_eq!(skill_numeric_arg_values(&args), vec![
            ("伤害值".to_owned(), 3.5),
            ("英文数字".to_owned(), 2.0),
            ("默认数字".to_owned(), 1.0),
        ]);
    }

    #[test]
    fn skill_rule_args_keeps_textual_string_and_buff_values() {
        let args = vec![
            SkillPoolArg {
                name: "伤害值".to_owned(),
                kind: "数字".to_owned(),
                value: "4".to_owned(),
            },
            SkillPoolArg {
                name: "伤害类型".to_owned(),
                kind: "字符串".to_owned(),
                value: "远程".to_owned(),
            },
            SkillPoolArg {
                name: "状态名".to_owned(),
                kind: "BUFF".to_owned(),
                value: "守护".to_owned(),
            },
            SkillPoolArg {
                name: "默认文本".to_owned(),
                kind: String::new(),
                value: "魔法".to_owned(),
            },
            SkillPoolArg {
                name: "空值".to_owned(),
                kind: "字符串".to_owned(),
                value: String::new(),
            },
        ];

        assert_eq!(skill_rule_args(&args), SkillRuleArgs {
            numeric_values: vec![("伤害值".to_owned(), 4.0)],
            text_values: vec![
                ("伤害类型".to_owned(), "远程".to_owned()),
                ("状态名".to_owned(), "守护".to_owned()),
                ("默认文本".to_owned(), "魔法".to_owned()),
            ],
        });
    }

    fn completed_character(nickname: &str) -> PlayerCharacter {
        let mut character = PlayerCharacter {
            inited: true,
            nickname: nickname.to_owned(),
            status: CharacterStatus {
                str_: 1,
                agi: 2,
                dex: 3,
                vit: 4,
                int_: 5,
                wis: 6,
                k: 7,
                cha: 8,
            },
            ..Default::default()
        };
        update_character_from_status(&mut character);
        character
    }

    #[test]
    fn player_access_visibility_matrix_matches_campaign_rules() {
        let red_player = PlayerAccess {
            player_id: 2,
            party_id: Some("red".to_owned()),
            ..Default::default()
        };
        let blue_player = PlayerAccess {
            player_id: 3,
            party_id: Some("blue".to_owned()),
            ..Default::default()
        };
        let gm = PlayerAccess {
            player_id: 9,
            is_gm: true,
            ..Default::default()
        };

        assert!(red_player.can_read(&Visibility::Public));
        assert!(red_player.can_read(&Visibility::Party("red".to_owned())));
        assert!(!blue_player.can_read(&Visibility::Party("red".to_owned())));
        assert!(red_player.can_read(&Visibility::Player(2)));
        assert!(!red_player.can_read(&Visibility::Player(3)));
        assert!(!red_player.can_read(&Visibility::Gm));
        assert!(!red_player.can_read(&Visibility::System));
        assert!(gm.can_read(&Visibility::Public));
        assert!(gm.can_read(&Visibility::Party("red".to_owned())));
        assert!(gm.can_read(&Visibility::Player(2)));
        assert!(gm.can_read(&Visibility::Gm));
        assert!(gm.can_read(&Visibility::System));
    }

    #[test]
    fn trpg_group_player_party_assignment_controls_access() {
        let mut group = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned()],
            ..Default::default()
        };

        assert!(group.ensure_party("red"));
        assert!(group.set_player_party("2", Some("red")));
        assert_eq!(
            group.party_id_for_player("2"),
            Some("red")
        );
        assert_eq!(
            group.player_access(2).party_id.as_deref(),
            Some("red")
        );
        assert!(group
            .player_access(2)
            .can_read(&Visibility::Party("red".to_owned())));
        assert!(!group
            .player_access(3)
            .can_read(&Visibility::Party("red".to_owned())));
    }

    #[test]
    fn trpg_group_merge_party_moves_players_and_access_scope() {
        let mut group = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
            ..Default::default()
        };
        group.ensure_party("red");
        group.ensure_party("blue");
        group.set_player_party("2", Some("red"));
        group.set_player_party("3", Some("blue"));
        group.set_player_party("4", Some("blue"));

        assert!(group.merge_party("blue", "red"));

        assert!(!group.parties.contains_key("blue"));
        assert_eq!(
            group.party_id_for_player("2"),
            Some("red")
        );
        assert_eq!(
            group.party_id_for_player("3"),
            Some("red")
        );
        assert_eq!(
            group.party_id_for_player("4"),
            Some("red")
        );
        assert_eq!(group.parties["red"].players, vec![
            "2".to_owned(),
            "3".to_owned(),
            "4".to_owned()
        ]);
        assert!(group
            .player_access(3)
            .can_read(&Visibility::Party("red".to_owned())));
        assert!(!group
            .player_access(3)
            .can_read(&Visibility::Party("blue".to_owned())));
    }

    #[test]
    fn trpg_group_remove_party_unassigns_players() {
        let mut group = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned()],
            ..Default::default()
        };
        group.ensure_party("red");
        group.set_player_party("2", Some("red"));
        group.set_player_party("3", Some("red"));

        assert!(group.remove_party("red"));

        assert!(!group.parties.contains_key("red"));
        assert_eq!(group.party_id_for_player("2"), None);
        assert_eq!(group.party_id_for_player("3"), None);
        assert!(!group
            .player_access(2)
            .can_read(&Visibility::Party("red".to_owned())));
    }

    #[test]
    fn legacy_team_can_be_promoted_to_live_party_scope() {
        let mut group = TrpgGroup {
            players: vec!["10002".to_owned(), "10003".to_owned(), "10004".to_owned()],
            legacy_teams: vec![TrpgLegacyTeam {
                id: "1".to_owned(),
                name: "红队频道".to_owned(),
                players: vec!["10002".to_owned(), "10003".to_owned(), "99999".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };
        group.ensure_party("蓝队");
        group.set_player_party("10004", Some("蓝队"));

        assert!(group.promote_legacy_team_to_party("1"));

        assert_eq!(
            group.party_id_for_player("10002"),
            Some("红队频道")
        );
        assert_eq!(
            group.party_id_for_player("10003"),
            Some("红队频道")
        );
        assert_eq!(
            group.party_id_for_player("10004"),
            Some("蓝队")
        );
        assert_eq!(group.parties["红队频道"].players, vec![
            "10002".to_owned(),
            "10003".to_owned()
        ]);
        assert!(
            group.player_access(10002).can_read(&Visibility::Party(
                "红队频道".to_owned()
            ))
        );
        assert!(
            !group.player_access(10004).can_read(&Visibility::Party(
                "红队频道".to_owned()
            ))
        );
    }

    #[test]
    fn legacy_team_chat_messages_can_append_gm_local_replies() {
        let mut group = TrpgGroup {
            players: vec!["10002".to_owned(), "10003".to_owned()],
            legacy_teams: vec![TrpgLegacyTeam {
                id: "1".to_owned(),
                name: "红队频道".to_owned(),
                players: vec!["10003".to_owned(), "10002".to_owned(), "missing".to_owned()],
                chat_message_count: 3,
                chat_messages: vec![TrpgLegacyTeamChatMessage {
                    sender_id: "10002".to_owned(),
                    sender_name: "红队".to_owned(),
                    text: "旧消息".to_owned(),
                    time: 1,
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        assert_eq!(group.legacy_team_members("1"), vec![
            "10003".to_owned(),
            "10002".to_owned()
        ]);
        assert!(
            group.append_legacy_team_chat_message("1", TrpgLegacyTeamChatMessage {
                sender_id: "gm".to_owned(),
                sender_name: "GM".to_owned(),
                text: "新的本地回复".to_owned(),
                time: 2,
            },)
        );
        assert_eq!(
            group.legacy_teams[0].chat_messages.len(),
            2
        );
        assert_eq!(
            group.legacy_teams[0].chat_messages[1].text,
            "新的本地回复"
        );
        assert_eq!(
            group.legacy_teams[0].chat_message_count,
            4
        );
        assert!(
            !group.append_legacy_team_chat_message("1", TrpgLegacyTeamChatMessage {
                text: "   ".to_owned(),
                ..Default::default()
            },)
        );
        assert!(
            !group.append_legacy_team_chat_message("missing", TrpgLegacyTeamChatMessage {
                text: "不会写入".to_owned(),
                ..Default::default()
            },)
        );
    }

    #[test]
    fn legacy_team_chat_messages_can_be_edited_and_removed_locally() {
        let mut group = TrpgGroup {
            legacy_teams: vec![TrpgLegacyTeam {
                id: "1".to_owned(),
                name: "红队频道".to_owned(),
                chat_message_count: 3,
                chat_messages: vec![
                    TrpgLegacyTeamChatMessage {
                        sender_id: "10002".to_owned(),
                        sender_name: "红队".to_owned(),
                        text: "旧消息".to_owned(),
                        time: 1,
                    },
                    TrpgLegacyTeamChatMessage {
                        sender_id: "gm".to_owned(),
                        sender_name: "GM".to_owned(),
                        text: "GM备注".to_owned(),
                        time: 2,
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        assert!(group.update_legacy_team_chat_message("1", 0, "修订旧消息"));
        assert_eq!(
            group.legacy_teams[0].chat_messages[0].text,
            "修订旧消息"
        );
        assert_eq!(
            group.legacy_teams[0].chat_message_count,
            3
        );
        assert!(!group.update_legacy_team_chat_message("1", 0, "修订旧消息"));
        assert!(!group.update_legacy_team_chat_message("1", 0, "   "));
        assert!(!group.update_legacy_team_chat_message("1", 9, "无效"));
        assert!(!group.update_legacy_team_chat_message("missing", 0, "无效"));

        assert!(group.remove_legacy_team_chat_message("1", 1));
        assert_eq!(
            group.legacy_teams[0].chat_messages.len(),
            1
        );
        assert_eq!(
            group.legacy_teams[0].chat_message_count,
            2
        );
        assert_eq!(
            group.legacy_teams[0].chat_messages[0].text,
            "修订旧消息"
        );
        assert!(!group.remove_legacy_team_chat_message("1", 9));
        assert!(!group.remove_legacy_team_chat_message("missing", 0));
    }

    #[test]
    fn legacy_chat_area_can_be_promoted_to_live_party_scope() {
        let mut group = TrpgGroup {
            players: vec!["10002".to_owned(), "10003".to_owned(), "10004".to_owned()],
            legacy_worlds: vec![TrpgLegacyWorld {
                id: "world-a".to_owned(),
                name: "旧世界".to_owned(),
                chat_areas: vec![TrpgLegacyArea {
                    id: "area-a".to_owned(),
                    name: "密谈区".to_owned(),
                    members: vec!["10003".to_owned(), "10004".to_owned(), "20001".to_owned()],
                    combat: true,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        assert!(group.promote_legacy_chat_area_to_party("area-a"));

        assert_eq!(group.party_id_for_player("10002"), None);
        assert_eq!(
            group.party_id_for_player("10003"),
            Some("密谈区")
        );
        assert_eq!(
            group.party_id_for_player("10004"),
            Some("密谈区")
        );
        assert_eq!(group.parties["密谈区"].players, vec![
            "10003".to_owned(),
            "10004".to_owned()
        ]);
        assert!(!group.promote_legacy_chat_area_to_party("missing"));
    }

    #[test]
    fn legacy_send_pane_target_editing_post_processes_old_multi_select() {
        let mut group = TrpgGroup {
            players: vec!["10002".to_owned(), "10003".to_owned(), "10004".to_owned()],
            legacy_teams: vec![TrpgLegacyTeam {
                id: "1".to_owned(),
                name: "红队频道".to_owned(),
                players: vec!["10002".to_owned(), "10003".to_owned()],
                ..Default::default()
            }],
            legacy_worlds: vec![TrpgLegacyWorld {
                id: "world-a".to_owned(),
                name: "旧世界".to_owned(),
                chat_areas: vec![TrpgLegacyArea {
                    id: "area-a".to_owned(),
                    name: "密谈区".to_owned(),
                    members: vec!["10003".to_owned(), "10004".to_owned()],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            legacy_send_panes: vec![TrpgLegacySendPane {
                key: "7".to_owned(),
                title: "多选发送".to_owned(),
                targets: vec!["10003".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        assert!(group.set_legacy_send_pane_target("7", "10002", true));
        assert_eq!(
            group.legacy_send_panes[0].targets,
            vec!["10003".to_owned(), "10002".to_owned(),]
        );

        assert!(group.set_legacy_send_pane_target("7", "1", true));
        assert_eq!(
            group.legacy_send_panes[0].targets,
            vec!["1".to_owned()]
        );
        assert!(group.legacy_send_pane_direct_target_is_covered("7", "10003"));
        assert!(!group.set_legacy_send_pane_target("7", "10003", true));

        assert!(group.set_legacy_send_pane_target("7", "area-a", true));
        assert_eq!(
            group.legacy_send_panes[0].targets,
            vec!["1".to_owned(), "area-a".to_owned(),]
        );
        assert!(group.legacy_send_pane_direct_target_is_covered("7", "10004"));

        assert!(group.set_legacy_send_pane_target("7", "0", true));
        assert_eq!(
            group.legacy_send_panes[0].targets,
            vec!["0".to_owned()]
        );
        assert!(group.legacy_send_pane_direct_target_is_covered("7", "10002"));
        assert!(!group.set_legacy_send_pane_target("7", "area-a", true));

        assert!(group.set_legacy_send_pane_target("7", "0", false));
        assert!(group.legacy_send_panes[0].targets.is_empty());
    }

    #[test]
    fn legacy_send_pane_can_be_added_removed_and_cleared() {
        let mut group = TrpgGroup {
            legacy_send_panes: vec![
                TrpgLegacySendPane {
                    key: "0".to_owned(),
                    title: "固定窗".to_owned(),
                    targets: vec!["10002".to_owned()],
                    closable: false,
                },
                TrpgLegacySendPane {
                    key: "1".to_owned(),
                    title: "可关窗".to_owned(),
                    targets: vec!["10003".to_owned()],
                    closable: true,
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            group.add_legacy_send_pane(""),
            Some("2".to_owned())
        );
        assert_eq!(
            group.legacy_send_panes[2].title,
            "多选发送"
        );
        assert!(group.set_legacy_send_pane_target("2", "0", true));
        assert!(group.clear_legacy_send_pane_targets("2"));
        assert!(group.legacy_send_panes[2].targets.is_empty());

        assert!(!group.remove_legacy_send_pane("0"));
        assert!(group.remove_legacy_send_pane("1"));
        assert!(group.legacy_send_pane("1").is_none());
        assert!(group.legacy_send_pane("0").is_some());
    }

    #[test]
    fn trpg_group_defaults_preserve_moonberry_creation_points() {
        let group = TrpgGroup::default();

        assert_eq!(
            group.initial_status_points,
            default_status_points()
        );
        assert_eq!(
            group.initial_exchange_points,
            default_exchange_points()
        );
        assert_eq!(
            group.basic_config,
            TrpgBasicConfig::default()
        );
        assert_eq!(group.run_times, 0);
        assert!(group.battle_sort_by_turn);
        assert!(!group.battle_negative_enabled);
        assert_eq!(group.legacy_negative_count, 0);
        assert!(group.legacy_teams.is_empty());
        assert!(group.legacy_worlds.is_empty());
        assert!(group.legacy_send_panes.is_empty());
        assert!(group.allow_join_requests);

        let legacy_group = serde_json::from_value::<TrpgGroup>(serde_json::json!({
            "players": ["2"]
        }))
        .expect("legacy group without new config fields should load");

        assert_eq!(
            legacy_group.initial_status_points,
            default_status_points()
        );
        assert_eq!(
            legacy_group.initial_exchange_points,
            default_exchange_points()
        );
        assert_eq!(
            legacy_group.basic_config,
            TrpgBasicConfig::default()
        );
        assert_eq!(legacy_group.run_times, 0);
        assert!(legacy_group.battle_sort_by_turn);
        assert!(!legacy_group.battle_negative_enabled);
        assert_eq!(legacy_group.legacy_negative_count, 0);
        assert!(legacy_group.legacy_teams.is_empty());
        assert!(legacy_group.legacy_worlds.is_empty());
        assert!(legacy_group.legacy_send_panes.is_empty());
        assert!(legacy_group.allow_join_requests);
    }

    #[test]
    fn annotates_private_message_with_player_visibility() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["2".to_owned()],
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());
        let mut message = test_message_with_text(NapcatMessageType::Private, "secret");

        manager.annotate_message_access("2", &mut message);

        assert_eq!(
            message.data.visibility,
            Visibility::Player(2)
        );
        assert_eq!(
            message.data.character_id.as_deref(),
            Some("2")
        );
        assert_eq!(message.data.party_id, None);
    }

    #[test]
    fn annotates_group_message_with_sender_party_visibility() {
        let mut manager = empty_manager();
        let mut group = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned()],
            group_chats: vec!["99".to_owned()],
            ..Default::default()
        };
        group.ensure_party("red");
        group.set_player_party("2", Some("red"));
        manager.trpg_groups.insert("table".to_owned(), group);
        manager.current_trpg_group = Some("table".to_owned());
        let mut message = test_message_with_text(NapcatMessageType::Group, "party only");
        message.data.group_id = Some(99);

        manager.annotate_message_access("99", &mut message);

        assert_eq!(
            message.data.visibility,
            Visibility::Party("red".to_owned())
        );
        assert_eq!(
            message.data.character_id.as_deref(),
            Some("2")
        );
        assert_eq!(
            message.data.party_id.as_deref(),
            Some("red")
        );
    }

    #[test]
    fn incoming_message_scope_uses_trusted_noncurrent_target_mapping() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("alpha".to_owned(), TrpgGroup {
            campaign_id: "campaign-a".to_owned(),
            players: vec!["9".to_owned()],
            group_chats: vec!["88".to_owned()],
            ..Default::default()
        });
        let mut beta = TrpgGroup {
            campaign_id: "campaign-b".to_owned(),
            players: vec!["2".to_owned()],
            group_chats: vec!["99".to_owned()],
            ..Default::default()
        };
        beta.ensure_party("red");
        beta.set_player_party("2", Some("red"));
        manager.trpg_groups.insert("beta".to_owned(), beta);
        manager.current_trpg_group = Some("alpha".to_owned());
        let mut message = test_message_with_text(NapcatMessageType::Group, "beta secret");
        message.data.group_id = Some(99);
        message.data.campaign_id = "spoofed-campaign".to_owned();
        message.data.character_id = Some("spoofed-character".to_owned());
        message.data.party_id = Some("blue".to_owned());
        message.data.visibility = Visibility::Player(9);

        manager.annotate_incoming_message_access("99", &mut message);

        assert_eq!(message.data.campaign_id, "campaign-b");
        assert_eq!(
            message.data.character_id.as_deref(),
            Some("2")
        );
        assert_eq!(
            message.data.party_id.as_deref(),
            Some("red")
        );
        assert_eq!(
            message.data.visibility,
            Visibility::Party("red".to_owned())
        );
    }

    #[test]
    fn persisted_group_message_visibility_survives_party_moves() {
        let mut manager = empty_manager();
        let mut group = TrpgGroup {
            players: vec![
                "2".to_owned(),
                "3".to_owned(),
                "4".to_owned(),
                "5".to_owned(),
            ],
            group_chats: vec!["99".to_owned()],
            ..Default::default()
        };
        group.ensure_party("red");
        group.ensure_party("blue");
        group.set_player_party("2", Some("red"));
        group.set_player_party("3", Some("blue"));
        group.set_player_party("4", Some("red"));
        manager.trpg_groups.insert("table".to_owned(), group);
        manager.current_trpg_group = Some("table".to_owned());

        let mut red_message = test_message_with_text(NapcatMessageType::Group, "red history");
        red_message.data.group_id = Some(99);
        manager.annotate_message_access("99", &mut red_message);
        let mut public_message = test_message_with_text(
            NapcatMessageType::Group,
            "public history",
        );
        public_message.data.group_id = Some(99);
        public_message.data.user_id = 5;
        public_message.data.sender.user_id = 5;
        manager.annotate_message_access("99", &mut public_message);
        let red_message: NapcatMessage =
            serde_json::from_str(&serde_json::to_string(&red_message).unwrap()).unwrap();
        let public_message: NapcatMessage =
            serde_json::from_str(&serde_json::to_string(&public_message).unwrap()).unwrap();

        let group = manager.trpg_groups.get_mut("table").unwrap();
        group.set_player_party("2", Some("blue"));
        group.set_player_party("5", Some("blue"));

        let messages = vec![red_message, public_message];
        let red_history = manager.campaign_message_for_target("99", &messages[0]);
        assert_eq!(
            red_history.party_id.as_deref(),
            Some("red")
        );
        assert_eq!(
            red_history.visibility,
            Visibility::Party("red".to_owned())
        );
        let red_view = manager
            .visible_messages_for_player("99", &messages, 4)
            .iter()
            .map(message_text)
            .collect::<Vec<_>>();
        let blue_view = manager
            .visible_messages_for_player("99", &messages, 3)
            .iter()
            .map(message_text)
            .collect::<Vec<_>>();

        assert_eq!(red_view, vec![
            "red history".to_owned(),
            "public history".to_owned()
        ]);
        assert_eq!(blue_view, vec![
            "public history".to_owned()
        ]);
    }

    #[test]
    fn visible_messages_for_player_filters_split_party_group_chat() {
        let mut manager = empty_manager();
        let mut group = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
            group_chats: vec!["99".to_owned()],
            ..Default::default()
        };
        group.ensure_party("red");
        group.ensure_party("blue");
        group.set_player_party("2", Some("red"));
        group.set_player_party("3", Some("blue"));
        manager.trpg_groups.insert("table".to_owned(), group);
        manager.current_trpg_group = Some("table".to_owned());

        let messages = vec![
            test_message_with_text(NapcatMessageType::Group, "red clue"),
            {
                let mut message = test_message_with_text(NapcatMessageType::Group, "blue clue");
                message.data.user_id = 3;
                message.data.sender.user_id = 3;
                message
            },
            {
                let mut message = test_message_with_text(NapcatMessageType::Group, "public clue");
                message.data.user_id = 4;
                message.data.sender.user_id = 4;
                message
            },
        ];

        let red_text = manager
            .visible_messages_for_player("99", &messages, 2)
            .iter()
            .map(message_text)
            .collect::<Vec<_>>();
        let blue_text = manager
            .visible_messages_for_player("99", &messages, 3)
            .iter()
            .map(message_text)
            .collect::<Vec<_>>();

        assert_eq!(red_text, vec![
            "red clue".to_owned(),
            "public clue".to_owned()
        ]);
        assert_eq!(blue_text, vec![
            "blue clue".to_owned(),
            "public clue".to_owned()
        ]);
    }

    #[test]
    fn visible_messages_for_player_keeps_private_local_replies_for_recipient() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![
            test_private_message_from(2, "player asks"),
        ]);
        append_local_private_text_response(&mut manager, "2", 2, "private answer");
        let messages = manager.messages["2"].clone();

        let player_two_text = manager
            .visible_messages_for_player("2", &messages, 2)
            .iter()
            .map(message_text)
            .collect::<Vec<_>>();
        let player_three_text = manager
            .visible_messages_for_player("2", &messages, 3)
            .iter()
            .map(message_text)
            .collect::<Vec<_>>();

        assert_eq!(player_two_text, vec![
            "player asks".to_owned(),
            "private answer".to_owned()
        ]);
        assert!(player_three_text.is_empty());
    }

    #[test]
    fn player_history_and_summary_inputs_exclude_other_campaigns() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            campaign_id: "campaign-a".to_owned(),
            players: vec!["2".to_owned()],
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());
        let mut current = test_private_message_from(2, "current campaign");
        current.data.campaign_id = "campaign-a".to_owned();
        let mut other = test_private_message_from(2, "other campaign secret");
        other.data.campaign_id = "campaign-b".to_owned();
        let messages = vec![current, other];

        let player_text = manager
            .visible_messages_for_player("2", &messages, 2)
            .iter()
            .map(message_text)
            .collect::<Vec<_>>();
        let summary_text = manager
            .visible_campaign_messages_for_summary("2", &messages)
            .into_iter()
            .map(|message| message.text)
            .collect::<Vec<_>>();

        assert_eq!(player_text, vec![
            "current campaign".to_owned()
        ]);
        assert_eq!(summary_text, vec![
            "current campaign".to_owned()
        ]);
    }

    #[test]
    fn napcat_manager_export_json_round_trips_core_campaign_data() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![test_message(
            NapcatMessageType::Private,
        )]);
        manager
            .chat_targets
            .insert("2".to_owned(), ChatTargetMetadata {
                display_name: "玩家".to_owned(),
                automatic_name: "tester".to_owned(),
            });
        manager.player_characters.insert(
            "2".to_owned(),
            completed_character("晨星"),
        );
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            campaign_id: "campaign-a".to_owned(),
            players: vec!["2".to_owned()],
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());

        let json = manager.to_export_json().unwrap();
        let imported = NapcatMessageManager::from_export_json(&json).unwrap();

        assert!(json.contains("\"version\""));
        assert_eq!(imported.messages["2"].len(), 1);
        assert_eq!(
            imported.chat_targets["2"].display_name,
            "玩家"
        );
        assert_eq!(
            imported.player_characters["2"].nickname,
            "晨星"
        );
        assert_eq!(
            imported.trpg_groups["table"].campaign_id,
            "campaign-a"
        );
        assert_eq!(
            imported.current_trpg_group.as_deref(),
            Some("table")
        );
    }

    #[test]
    fn player_character_export_json_contains_only_sorted_pcs() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![test_message(
            NapcatMessageType::Private,
        )]);
        manager.player_characters.insert(
            "9".to_owned(),
            completed_character("后到"),
        );
        manager.player_characters.insert(
            "2".to_owned(),
            completed_character("先到"),
        );

        let json = manager.to_player_characters_export_json().unwrap();
        let export: NapcatPlayerCharactersExport = serde_json::from_str(&json).unwrap();

        assert_eq!(
            export.version,
            NAPCAT_MANAGER_EXPORT_VERSION
        );
        assert_eq!(export.export_type, "player_characters");
        assert_eq!(
            export
                .players
                .iter()
                .map(|entry| entry.target_id.as_str())
                .collect::<Vec<_>>(),
            vec!["2", "9"]
        );
        assert_eq!(
            export.players[0].character.nickname,
            "先到"
        );
        assert!(!json.contains("\"messages\""));
        assert!(!json.contains("\"chat_targets\""));
    }

    #[test]
    fn player_character_export_json_merges_by_target_id_without_chat_data() {
        let mut source = empty_manager();
        source.player_characters.insert(
            "9".to_owned(),
            completed_character("后到"),
        );
        source.player_characters.insert(
            "2".to_owned(),
            completed_character("新角色"),
        );

        let json = source.to_player_characters_export_json().unwrap();
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![test_message(
            NapcatMessageType::Private,
        )]);
        manager
            .chat_targets
            .insert("2".to_owned(), ChatTargetMetadata {
                display_name: "保留聊天名".to_owned(),
                automatic_name: "friend".to_owned(),
            });
        manager.player_characters.insert(
            "2".to_owned(),
            completed_character("旧角色"),
        );

        let imported = manager.merge_player_characters_export_json(&json).unwrap();

        assert_eq!(imported, 2);
        assert_eq!(
            manager.player_characters["2"].nickname,
            "新角色"
        );
        assert_eq!(
            manager.player_characters["9"].nickname,
            "后到"
        );
        assert_eq!(manager.messages["2"].len(), 1);
        assert_eq!(
            manager.chat_targets["2"].display_name,
            "保留聊天名"
        );
    }

    #[test]
    fn player_character_import_rejects_wrong_export_shape() {
        let json = serde_json::json!({
            "version": NAPCAT_MANAGER_EXPORT_VERSION,
            "export_type": "chat_list",
            "players": [],
        })
        .to_string();
        let mut manager = empty_manager();

        let error = manager
            .merge_player_characters_export_json(&json)
            .err()
            .expect("wrong export type should fail");

        assert!(error.contains("unsupported NapCat player character export type"));
        assert!(manager.player_characters.is_empty());
    }

    #[test]
    fn chat_list_export_json_contains_targets_and_groups_without_message_bodies() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![test_message(
            NapcatMessageType::Private,
        )]);
        manager.messages.insert("99".to_owned(), vec![test_message(
            NapcatMessageType::Group,
        )]);
        manager
            .chat_targets
            .insert("2".to_owned(), ChatTargetMetadata {
                display_name: "玩家二".to_owned(),
                automatic_name: "friend".to_owned(),
            });
        manager.read_message_counts.insert("2".to_owned(), 3);
        manager.summarized_message_counts.insert("99".to_owned(), 5);
        manager.open_chat_targets.insert("2".to_owned());
        manager.pending_chat_targets.insert("99".to_owned());
        manager.rejected_chat_targets.insert("404".to_owned());
        manager.groups.insert("讨论组".to_owned(), ChatGroup {
            members: vec!["2".to_owned(), "99".to_owned()],
        });

        let json = manager.to_chat_list_export_json().unwrap();
        let export: NapcatChatListExport = serde_json::from_str(&json).unwrap();

        assert_eq!(
            export.version,
            NAPCAT_MANAGER_EXPORT_VERSION
        );
        assert_eq!(export.export_type, "chat_list");
        assert_eq!(
            export
                .targets
                .iter()
                .map(|entry| entry.target_id.as_str())
                .collect::<Vec<_>>(),
            vec!["2", "404", "99"]
        );
        let private = export
            .targets
            .iter()
            .find(|entry| entry.target_id == "2")
            .unwrap();
        assert_eq!(
            private.kind,
            ChatTargetExportKind::Private
        );
        assert_eq!(private.metadata.display_name, "玩家二");
        assert_eq!(private.read_message_count, 3);
        assert!(private.open);

        let group = export
            .targets
            .iter()
            .find(|entry| entry.target_id == "99")
            .unwrap();
        assert_eq!(group.kind, ChatTargetExportKind::Group);
        assert_eq!(group.summarized_message_count, 5);
        assert!(group.pending);

        let rejected = export
            .targets
            .iter()
            .find(|entry| entry.target_id == "404")
            .unwrap();
        assert_eq!(
            rejected.kind,
            ChatTargetExportKind::Unknown
        );
        assert!(rejected.rejected);

        assert_eq!(export.groups.len(), 1);
        assert_eq!(export.groups[0].name, "讨论组");
        assert!(!json.contains("hello"));
        assert!(!json.contains("\"player_characters\""));
    }

    #[test]
    fn chat_list_export_json_merges_metadata_without_message_bodies() {
        let mut source = empty_manager();
        source.messages.insert("2".to_owned(), vec![test_message(
            NapcatMessageType::Private,
        )]);
        source.messages.insert("99".to_owned(), vec![test_message(
            NapcatMessageType::Group,
        )]);
        source
            .chat_targets
            .insert("2".to_owned(), ChatTargetMetadata {
                display_name: "导入玩家".to_owned(),
                automatic_name: "source".to_owned(),
            });
        source.read_message_counts.insert("2".to_owned(), 3);
        source.summarized_message_counts.insert("99".to_owned(), 5);
        source.open_chat_targets.insert("2".to_owned());
        source.pending_chat_targets.insert("99".to_owned());
        source.rejected_chat_targets.insert("404".to_owned());
        source.groups.insert("讨论组".to_owned(), ChatGroup {
            members: vec!["99".to_owned(), "2".to_owned(), "2".to_owned()],
        });

        let json = source.to_chat_list_export_json().unwrap();
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![test_message(
            NapcatMessageType::Private,
        )]);
        manager.read_message_counts.insert("2".to_owned(), 10);
        manager.summarized_message_counts.insert("99".to_owned(), 2);

        let imported = manager.merge_chat_list_export_json(&json).unwrap();

        assert_eq!(imported, 3);
        assert_eq!(
            manager.chat_targets["2"].display_name,
            "导入玩家"
        );
        assert_eq!(manager.read_message_counts["2"], 10);
        assert_eq!(
            manager.summarized_message_counts["99"],
            5
        );
        assert!(manager.open_chat_targets.contains("2"));
        assert!(!manager.pending_chat_targets.contains("2"));
        assert!(manager.pending_chat_targets.contains("99"));
        assert!(manager.rejected_chat_targets.contains("404"));
        assert_eq!(manager.messages["2"].len(), 1);
        assert!(!manager.messages.contains_key("99"));
        assert_eq!(manager.groups["讨论组"].members, vec![
            "2".to_owned(),
            "99".to_owned(),
        ]);
    }

    #[test]
    fn chat_list_import_rejects_wrong_export_shape() {
        let json = serde_json::json!({
            "version": NAPCAT_MANAGER_EXPORT_VERSION,
            "export_type": "player_characters",
            "targets": [],
            "groups": [],
        })
        .to_string();
        let mut manager = empty_manager();

        let error = manager
            .merge_chat_list_export_json(&json)
            .err()
            .expect("wrong export type should fail");

        assert!(error.contains("unsupported NapCat chat list export type"));
        assert!(manager.chat_targets.is_empty());
    }

    #[test]
    fn chat_list_import_rejected_state_wins_over_open_and_pending() {
        let json = serde_json::json!({
            "version": NAPCAT_MANAGER_EXPORT_VERSION,
            "export_type": "chat_list",
            "targets": [{
                "target_id": "2",
                "kind": "private",
                "metadata": {
                    "display_name": "玩家",
                    "automatic_name": "friend"
                },
                "read_message_count": 0,
                "summarized_message_count": 0,
                "open": true,
                "pending": true,
                "rejected": true
            }],
            "groups": [],
        })
        .to_string();
        let mut manager = empty_manager();

        assert_eq!(
            manager.merge_chat_list_export_json(&json),
            Ok(1)
        );

        assert!(manager.rejected_chat_targets.contains("2"));
        assert!(!manager.open_chat_targets.contains("2"));
        assert!(!manager.pending_chat_targets.contains("2"));
    }

    #[test]
    fn unit_pool_export_json_contains_sorted_reusable_unit_templates() {
        let mut manager = empty_manager();
        manager
            .unit_pool
            .insert("zombie".to_owned(), UnitPoolEntry {
                label: "行尸".to_owned(),
                note: "缓慢近战单位".to_owned(),
                legacy_member_id: None,
                character: completed_character("行尸"),
            });
        manager
            .unit_pool
            .insert("archer".to_owned(), UnitPoolEntry {
                label: "弓手".to_owned(),
                note: "远程单位".to_owned(),
                legacy_member_id: None,
                character: completed_character("弓手"),
            });
        manager.messages.insert("2".to_owned(), vec![test_message(
            NapcatMessageType::Private,
        )]);

        let json = manager.to_unit_pool_export_json().unwrap();
        let export: NapcatUnitPoolExport = serde_json::from_str(&json).unwrap();

        assert_eq!(
            export.version,
            NAPCAT_MANAGER_EXPORT_VERSION
        );
        assert_eq!(export.export_type, "unit_pool");
        assert_eq!(
            export
                .units
                .iter()
                .map(|entry| entry.unit_id.as_str())
                .collect::<Vec<_>>(),
            vec!["archer", "zombie"]
        );
        assert_eq!(export.units[0].unit.label, "弓手");
        assert_eq!(
            export.units[0].unit.character.nickname,
            "弓手"
        );
        assert!(!json.contains("\"messages\""));
        assert!(!json.contains("\"player_characters\""));
    }

    #[test]
    fn unit_pool_legacy_member_ids_resolve_world_npc_template_ids() {
        let mut manager = empty_manager();
        manager
            .unit_pool
            .insert("direct".to_owned(), UnitPoolEntry {
                label: "直接单位".to_owned(),
                note: String::new(),
                legacy_member_id: None,
                character: completed_character("直接单位"),
            });
        manager
            .unit_pool
            .insert("alias-b".to_owned(), UnitPoolEntry {
                label: "别名B".to_owned(),
                note: String::new(),
                legacy_member_id: Some("20001".to_owned()),
                character: completed_character("别名B"),
            });
        manager
            .unit_pool
            .insert("alias-a".to_owned(), UnitPoolEntry {
                label: "别名A".to_owned(),
                note: String::new(),
                legacy_member_id: Some("20001".to_owned()),
                character: completed_character("别名A"),
            });
        manager.unit_pool.insert(
            "moonberry-unit-30001".to_owned(),
            UnitPoolEntry {
                label: "旧兼容单位".to_owned(),
                note: String::new(),
                legacy_member_id: None,
                character: completed_character("旧兼容单位"),
            },
        );

        let resolved = manager.unit_pool_ids_for_legacy_members(&[
            "direct".to_owned(),
            "20001".to_owned(),
            "30001".to_owned(),
            "missing".to_owned(),
            "20001".to_owned(),
        ]);

        assert_eq!(resolved, vec![
            "direct".to_owned(),
            "alias-a".to_owned(),
            "alias-b".to_owned(),
            "moonberry-unit-30001".to_owned(),
        ]);
    }

    #[test]
    fn unit_pool_export_json_merges_by_unit_id_without_chat_data() {
        let mut source = empty_manager();
        source.unit_pool.insert("archer".to_owned(), UnitPoolEntry {
            label: "新弓手".to_owned(),
            note: "导入版本".to_owned(),
            legacy_member_id: None,
            character: completed_character("新弓手"),
        });
        source.unit_pool.insert("zombie".to_owned(), UnitPoolEntry {
            label: "行尸".to_owned(),
            note: "缓慢近战单位".to_owned(),
            legacy_member_id: None,
            character: completed_character("行尸"),
        });
        let json = source.to_unit_pool_export_json().unwrap();

        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![test_message(
            NapcatMessageType::Private,
        )]);
        manager
            .unit_pool
            .insert("archer".to_owned(), UnitPoolEntry {
                label: "旧弓手".to_owned(),
                note: "本地旧版本".to_owned(),
                legacy_member_id: None,
                character: completed_character("旧弓手"),
            });

        let imported = manager.merge_unit_pool_export_json(&json).unwrap();

        assert_eq!(imported, 2);
        assert_eq!(
            manager.unit_pool["archer"].label,
            "新弓手"
        );
        assert_eq!(
            manager.unit_pool["archer"].character.nickname,
            "新弓手"
        );
        assert_eq!(
            manager.unit_pool["zombie"].label,
            "行尸"
        );
        assert_eq!(manager.messages["2"].len(), 1);
    }

    #[test]
    fn unit_pool_import_rejects_wrong_export_shape() {
        let json = serde_json::json!({
            "version": NAPCAT_MANAGER_EXPORT_VERSION,
            "export_type": "chat_list",
            "units": [],
        })
        .to_string();
        let mut manager = empty_manager();

        let error = manager
            .merge_unit_pool_export_json(&json)
            .err()
            .expect("wrong export type should fail");

        assert!(error.contains("unsupported NapCat unit pool export type"));
        assert!(manager.unit_pool.is_empty());
    }

    #[test]
    fn moonberry_legacy_team_chat_messages_preserve_local_timeline_excerpts() {
        let chats = vec![serde_json::json!({
            "type": "FriendMessage",
            "sender": {
                "id": 10001,
                "nickname": "星见QQ",
                "memberName": "星见"
            },
            "messageChain": [{
                "type": "Source",
                "id": 9,
                "time": 789
            }, {
                "type": "Plain",
                "text": "频道内行动"
            }, {
                "type": "Image",
                "url": "https://example.test/scene.png",
                "imageId": "scene-img"
            }, {
                "type": "At",
                "target": 10002
            }]
        })];

        assert_eq!(
            moonberry_legacy_team_chat_messages(&chats),
            vec![TrpgLegacyTeamChatMessage {
                sender_id: "10001".to_owned(),
                sender_name: "星见".to_owned(),
                text: "频道内行动 [图片:https://example.test/scene.png] [At]".to_owned(),
                time: 789,
            }]
        );
    }

    #[test]
    fn moonberry_legacy_root_import_merges_groups_pcs_units_and_messages() {
        let legacy = r#"{
            "discriminator": "Root",
            "currentGroup": 0,
            "config": {
                "orderByTurn": false,
                "negative": true
            },
            "groups": [{
                "name": "旧团",
                "description": "公开说明",
                "stDesc": "GM说明",
                "guide": "旧入团指南",
                "runTimes": 3,
                "negative": [{
                    "Id": 10001,
                    "remain": 1200,
                    "reply": false,
                    "idx": 0
                }],
                "basicConfig": {
                    "initStatusPoint": 7,
                    "initExchangePoint": 8,
                    "wisMPReg": 0.5,
                    "wisMaxMP": 3.0,
                    "intMaxMP": 7.0,
                    "vitHPReg": 1.5,
                    "vitMaxHP": 6.0,
                    "lvMaxHP": 4.0,
                    "strMaxHP": 2.0,
                    "expGainPerLv": 4.0,
                    "expGainPerLvPvP": 0.25,
                    "basicSpeed": 2.0,
                    "strDMGBenifit": 0.03,
                    "intDMGBenifit": 0.04,
                    "dexDMGBenifit": 0.05,
                    "dexRangeDMGBenifit": 0.06,
                    "wisHealBenifit": 0.07,
                    "intHealBenifit": 0.08,
                    "agiDMGBenifit": 0.09,
                    "strSpeed": 0.25,
                    "agiSpeed": 1.5,
                    "dexSpeed": 0.75
                },
                "pc": [{
                    "Id": 10001,
                    "inited": true,
                    "name": "Aster",
                    "nickname": "星见",
                    "img": "https://example.test/aster.png",
                    "statusPoint": 1,
                    "exchangePoint": 2,
                    "hp": 11,
                    "maxHP": 12,
                    "hpReg": 1,
                    "mp": 3,
                    "maxMP": 4,
                    "mpReg": 0.5,
                    "lv": 2,
                    "exp": 9,
                    "speed": 6,
                    "DMGModify": 1.2,
                    "healModify": 1.1,
                    "tDMGModify": 0.9,
                    "tHealModify": 1.3,
                    "tdpt": 4,
                    "thpt": 2,
                    "status": {
                        "str": 1,
                        "agi": 2,
                        "dex": 3,
                        "vit": 4,
                        "int": 5,
                        "wis": 6,
                        "k": 7,
                        "cha": 8
                    },
                    "extraStatus": {
                        "str": 1,
                        "agi": 0,
                        "dex": 0,
                        "vit": 0,
                        "int": 0,
                        "wis": 0,
                        "k": 0,
                        "cha": 0
                    },
                    "skillChain": [{
                        "name": "护盾",
                        "description": "回复3点生命值",
                        "type": "法术",
                        "target": 1,
                        "class": "单目标",
                        "caster": 10001,
                        "cost": 2,
                        "cooldown": 1,
                        "cooldownLeft": 0,
                        "range": 6,
                        "exchangePoint": 2,
                        "pcInited": true,
                        "stInited": false,
                        "poolId": "skill-pool-a",
                        "args": [{
                            "name": "护盾值",
                            "type": "数字",
                            "value": "3"
                        }],
                        "buffMachine": {
                            "技能释放": []
                        }
                    }]
                }],
                "currentChatList": [{
                    "Id": 0,
                    "nickName": "所有消息",
                    "lastWords": "",
                    "notReadCount": 0
                }, {
                    "Id": 10001,
                    "nickName": "星见QQ",
                    "lastWords": "hi",
                    "notReadCount": 2
                }],
                "currentTeams": [{
                    "name": "红队频道",
                    "Id": 1,
                    "visible": true,
                    "bounds": {
                        "x": 12,
                        "y": 34
                    },
                    "size": {
                        "width": 320,
                        "height": 240
                    },
                    "allowPcNicknameRepeat": true,
                    "nemo": true,
                    "buff": [{}],
                    "chat": [{
                        "type": "FriendMessage",
                        "sender": {
                            "id": 10001,
                            "nickname": "星见QQ",
                            "memberName": "星见"
                        },
                        "messageChain": [{
                            "type": "Source",
                            "id": 8,
                            "time": 456
                        }, {
                            "type": "Plain",
                            "text": "旧频道消息"
                        }, {
                            "type": "Image",
                            "url": "https://example.test/a.png",
                            "imageId": "img-a"
                        }]
                    }],
                    "pcs": [{
                        "Id": 10001,
                        "nickname": "星见"
                    }]
                }],
                "currentWorlds": [{
                    "Id": "world-a",
                    "visible": true,
                    "world": {
                        "name": "旧世界",
                        "PcNumbers": [10001],
                        "NpcNumbers": [20001],
                        "chatAreas": [{
                            "id": "area-a",
                            "name": "密谈区",
                            "x": 1,
                            "y": 2,
                            "width": 3,
                            "height": 4,
                            "member": [10001],
                            "combat": true
                        }],
                        "Areas": [{
                            "id": "area-b",
                            "name": "公开区",
                            "x": 5,
                            "y": 6,
                            "width": 7,
                            "height": 8,
                            "member": [10001],
                            "combat": false
                        }]
                    }
                }],
                "currentSendPanes": [{
                    "title": "红队发送窗",
                    "key": 7,
                    "closable": false,
                    "sendTo": {
                        "targets": [1, "area-a", 10001]
                    }
                }],
                "chatMsg": [{
                    "type": "FriendMessage",
                    "sender": {
                        "id": 10001,
                        "nickname": "星见QQ"
                    },
                    "messageChain": [{
                        "type": "Source",
                        "id": 5,
                        "time": 123
                    }, {
                        "type": "Plain",
                        "text": "旧消息"
                    }]
                }]
            }],
            "skillsPool": [{
                "id": "skill-pool-a",
                "name": "护盾池",
                "group": 0,
                "tags": "防御 支援",
                "type": 2,
                "desc": "提供护盾的技能模板",
                "createdAt": "2024-01-02 03:04:05",
                "args": [{
                    "name": "护盾值",
                    "type": "数字",
                    "value": "3"
                }],
                "buff": [{}],
                "eventBuffs": [{}],
                "graph": {
                    "nodes": []
                }
            }],
            "unitPool": [{
                "id": "unit-old",
                "tags": "敌人",
                "desc": "旧单位",
                "Pc": {
                    "Id": 20001,
                    "inited": true,
                    "nickname": "行尸",
                    "hp": 5,
                    "maxHP": 5,
                    "status": {
                        "str": 1,
                        "agi": 1,
                        "dex": 0,
                        "vit": 1,
                        "int": 0,
                        "wis": 0,
                        "k": 0,
                        "cha": 0
                    }
                }
            }],
            "randomPool": [{
                "id": "random-old",
                "name": "遭遇随机",
                "group": 0,
                "tags": "探索",
                "createdAt": "2024-01-02 03:04:05",
                "desc": "旧随机池",
                "IRandomItem": [{
                    "key": "陷阱",
                    "RandomItemDesc": "触发一个陷阱",
                    "min": 1,
                    "max": 3
                }]
            }]
        }"#
        .to_owned();
        let mut manager = empty_manager();

        let summary = manager.merge_moonberry_legacy_json(&legacy).unwrap();

        assert_eq!(summary.groups, 1);
        assert_eq!(summary.players, 1);
        assert_eq!(summary.chat_targets, 1);
        assert_eq!(summary.messages, 1);
        assert_eq!(summary.skill_pools, 1);
        assert_eq!(summary.unit_templates, 1);
        assert_eq!(summary.random_pools, 1);
        assert_eq!(summary.legacy_teams, 1);
        assert_eq!(summary.legacy_worlds, 1);
        assert_eq!(summary.legacy_chat_areas, 2);
        assert_eq!(summary.legacy_send_panes, 1);
        assert_eq!(summary.legacy_negative_timers, 1);
        assert_eq!(
            manager.current_trpg_group.as_deref(),
            Some("旧团")
        );
        let group = &manager.trpg_groups["旧团"];
        assert_eq!(group.description, "公开说明");
        assert_eq!(group.st_description, "GM说明");
        assert_eq!(group.guide, "旧入团指南");
        assert_eq!(group.initial_status_points, 7);
        assert_eq!(group.initial_exchange_points, 8);
        assert_eq!(group.basic_config.base_max_hp, 0.0);
        assert_eq!(group.basic_config.lv_max_hp, 4.0);
        assert_eq!(group.basic_config.str_max_hp, 2.0);
        assert_eq!(group.basic_config.vit_max_hp, 6.0);
        assert_eq!(group.basic_config.int_max_mp, 7.0);
        assert_eq!(group.basic_config.wis_max_mp, 3.0);
        assert_eq!(group.basic_config.basic_speed, 2.0);
        assert_eq!(group.basic_config.agi_speed, 1.5);
        assert_eq!(group.run_times, 3);
        assert!(!group.battle_sort_by_turn);
        assert!(group.battle_negative_enabled);
        assert_eq!(group.legacy_negative_count, 1);
        assert_eq!(group.legacy_negative_timers.len(), 1);
        assert_eq!(
            group.legacy_negative_timers[0],
            TrpgLegacyNegativeTimer {
                target_id: "10001".to_owned(),
                remaining_ms: 1200,
                replied: false,
                generation: 0,
                half_warned: false,
                negative_layers: 0,
            }
        );
        assert_eq!(group.legacy_teams.len(), 1);
        assert_eq!(group.legacy_teams[0].id, "1");
        assert_eq!(group.legacy_teams[0].name, "红队频道");
        assert!(group.legacy_teams[0].allow_pc_nickname_repeat);
        assert!(group.legacy_teams[0].anonymous_speakers);
        assert_eq!(group.legacy_teams[0].buff_count, 1);
        assert_eq!(group.legacy_teams[0].window_x, 12.0);
        assert_eq!(group.legacy_teams[0].window_y, 34.0);
        assert_eq!(
            group.legacy_teams[0].window_width,
            320.0
        );
        assert_eq!(
            group.legacy_teams[0].window_height,
            240.0
        );
        assert_eq!(
            group.legacy_teams[0].chat_message_count,
            1
        );
        assert_eq!(
            group.legacy_teams[0].chat_messages,
            vec![TrpgLegacyTeamChatMessage {
                sender_id: "10001".to_owned(),
                sender_name: "星见".to_owned(),
                text: "旧频道消息 [图片:https://example.test/a.png]".to_owned(),
                time: 456,
            }]
        );
        assert_eq!(group.legacy_teams[0].players, vec![
            "10001".to_owned()
        ]);
        assert_eq!(group.legacy_worlds.len(), 1);
        assert_eq!(group.legacy_worlds[0].id, "world-a");
        assert_eq!(group.legacy_worlds[0].name, "旧世界");
        assert_eq!(group.legacy_worlds[0].players, vec![
            "10001".to_owned()
        ]);
        assert_eq!(group.legacy_worlds[0].npcs, vec![
            "20001".to_owned()
        ]);
        assert_eq!(
            group.legacy_worlds[0].chat_areas.len(),
            1
        );
        assert_eq!(
            group.legacy_worlds[0].chat_areas[0].id,
            "area-a"
        );
        assert!(group.legacy_worlds[0].chat_areas[0].combat);
        assert_eq!(group.legacy_worlds[0].areas.len(), 1);
        assert_eq!(
            group.legacy_worlds[0].areas[0].id,
            "area-b"
        );
        assert_eq!(group.legacy_send_panes.len(), 1);
        assert_eq!(group.legacy_send_panes[0].key, "7");
        assert_eq!(
            group.legacy_send_panes[0].title,
            "红队发送窗"
        );
        assert!(!group.legacy_send_panes[0].closable);
        assert_eq!(
            group.legacy_send_pane_members("7"),
            vec!["10001".to_owned()]
        );
        assert!(group.players.contains(&"10001".to_owned()));
        assert_eq!(
            manager.chat_targets["10001"].display_name,
            "星见QQ"
        );
        assert_eq!(manager.read_message_counts["10001"], 2);
        let character = &manager.player_characters["10001"];
        assert!(character.inited);
        assert_eq!(character.nickname, "星见");
        assert_eq!(
            character.image,
            "https://example.test/aster.png"
        );
        assert_eq!(character.status.agi, 2);
        assert_eq!(character.extra_status.str_, 1);
        assert_eq!(character.damage_taken_this_turn, 4.0);
        assert_eq!(character.healing_taken_this_turn, 2.0);
        assert_eq!(character.skill_names, vec!["护盾"]);
        assert_eq!(character.skill_mp_costs, vec![2.0]);
        assert_eq!(character.skill_cooldown_turns, vec![1]);
        assert!(character.skill_metadata[0].pc_approved);
        assert!(!character.skill_metadata[0].st_approved);
        assert_eq!(
            character.skill_metadata[0].source_pool_id.as_deref(),
            Some("skill-pool-a")
        );
        assert_eq!(
            character.skill_metadata[0].source,
            CharacterSkillSourceKind::SkillPool
        );
        assert_eq!(
            character.skill_metadata[0].skill_type.as_deref(),
            Some("法术")
        );
        assert_eq!(
            character.skill_metadata[0].target_class.as_deref(),
            Some("单目标")
        );
        assert_eq!(
            character.skill_metadata[0].target_count,
            Some(1)
        );
        assert_eq!(
            character.skill_metadata[0].range,
            Some(6)
        );
        assert_eq!(
            character.skill_metadata[0].exchange_point,
            Some(2)
        );
        assert_eq!(
            character.skill_metadata[0].cooldown_left,
            Some(0)
        );
        assert_eq!(
            character.skill_metadata[0].legacy_caster.as_deref(),
            Some("10001")
        );
        assert_eq!(
            character.skill_metadata[0].args.len(),
            1
        );
        assert_eq!(
            character.skill_metadata[0].args[0].name,
            "护盾值"
        );
        assert!(character.skill_metadata[0].legacy_has_buff_machine);
        assert_eq!(
            character.skill_metadata[0]
                .legacy_buff_machine_json
                .as_deref(),
            Some("{\"技能释放\":[]}")
        );
        assert_eq!(manager.messages["10001"].len(), 1);
        assert_eq!(
            manager.messages["10001"][0].data.time,
            123
        );
        assert_eq!(
            manager.messages["10001"][0].data.visibility,
            Visibility::Player(10001)
        );
        assert_eq!(
            manager.unit_pool["unit-old"].label,
            "行尸"
        );
        assert!(manager.unit_pool["unit-old"].note.contains("旧单位"));
        assert_eq!(
            manager.unit_pool["unit-old"].legacy_member_id.as_deref(),
            Some("20001")
        );
        assert_eq!(
            manager.unit_pool["unit-old"].character.max_hp,
            5.0
        );
        let random_pool = &manager.random_pools["遭遇随机"];
        assert_eq!(
            random_pool.legacy_pool_id.as_deref(),
            Some("random-old")
        );
        assert_eq!(random_pool.legacy_group, Some(0));
        assert_eq!(random_pool.tags, "探索");
        assert_eq!(random_pool.description, "旧随机池");
        assert_eq!(
            random_pool.created_at,
            "2024-01-02 03:04:05"
        );
        assert_eq!(random_pool.entries.len(), 1);
        assert_eq!(random_pool.entries[0].item.name, "陷阱");
        assert_eq!(
            random_pool.entries[0].result_text,
            "触发一个陷阱"
        );
        assert_eq!(random_pool.entries[0].min_count, 1);
        assert_eq!(random_pool.entries[0].max_count, 3);
        assert_eq!(manager.skill_pool.len(), 1);
        let skill_pool = &manager.skill_pool[0];
        assert_eq!(
            skill_pool.legacy_pool_id.as_deref(),
            Some("skill-pool-a")
        );
        assert_eq!(skill_pool.name, "护盾池");
        assert_eq!(
            skill_pool.category.as_deref(),
            Some("普通")
        );
        assert_eq!(skill_pool.tags, vec![
            "防御".to_owned(),
            "支援".to_owned(),
        ]);
        assert_eq!(skill_pool.args.len(), 1);
        assert_eq!(skill_pool.args[0].name, "护盾值");
        assert_eq!(skill_pool.args[0].kind, "数字");
        assert_eq!(skill_pool.args[0].value, "3");
        assert_eq!(skill_pool.legacy_buff_count, 1);
        assert_eq!(skill_pool.legacy_event_buff_count, 1);
        assert!(skill_pool.legacy_has_graph);
        assert_eq!(
            skill_pool.legacy_buff_json.as_deref(),
            Some("[{}]")
        );
        assert_eq!(
            skill_pool.legacy_event_buff_json.as_deref(),
            Some("[{}]")
        );
        assert_eq!(
            skill_pool.legacy_graph_json.as_deref(),
            Some("{\"nodes\":[]}")
        );
    }

    #[test]
    fn moonberry_legacy_bundle_import_merges_filtered_export_shape() {
        let legacy = serde_json::json!({
            "Pcs": [{
                "Id": 42,
                "inited": true,
                "nickname": "旅人",
                "hp": 6,
                "maxHP": 9
            }],
            "chatlists": [{
                "Id": 42,
                "nickName": "旅人QQ",
                "lastWords": "hello",
                "notReadCount": 0
            }],
            "chatMsgs": [{
                "type": "FriendMessage",
                "sender": {
                    "id": 42,
                    "nickname": "旅人QQ"
                },
                "messageChain": [{
                    "type": "Plain",
                    "text": "bundle msg"
                }]
            }]
        })
        .to_string();
        let mut manager = empty_manager();

        let summary = manager.merge_moonberry_legacy_json(&legacy).unwrap();

        assert_eq!(summary.players, 1);
        assert_eq!(summary.chat_targets, 1);
        assert_eq!(summary.messages, 1);
        assert_eq!(
            manager.current_trpg_group.as_deref(),
            Some("月莓导入")
        );
        assert!(manager.trpg_groups["月莓导入"]
            .players
            .contains(&"42".to_owned()));
        assert_eq!(
            manager.player_characters["42"].nickname,
            "旅人"
        );
        assert_eq!(
            manager.chat_targets["42"].display_name,
            "旅人QQ"
        );
        assert_eq!(manager.messages["42"].len(), 1);
    }

    #[test]
    fn moonberry_legacy_import_rejects_unrecognized_json() {
        let mut manager = empty_manager();

        let error = manager
            .merge_moonberry_legacy_json("{\"hello\":\"world\"}")
            .err()
            .expect("unknown shape should fail");

        assert!(error.contains("未识别"));
        assert!(manager.player_characters.is_empty());
    }

    #[test]
    fn napcat_manager_import_rejects_unsupported_export_version() {
        let json = serde_json::json!({
            "version": NAPCAT_MANAGER_EXPORT_VERSION + 1,
            "manager": empty_manager(),
        })
        .to_string();

        let error = NapcatMessageManager::from_export_json(&json)
            .err()
            .expect("unsupported version should fail");

        assert!(error.contains("unsupported NapCat manager export version"));
    }

    #[test]
    fn private_status_command_reports_completed_character() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "2".to_owned(),
            completed_character("晨星"),
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".状态"),
            "2",
        )
        .unwrap();

        assert!(response.contains("角色：晨星"));
        assert!(response.contains("HP："));
        assert!(response.contains("力量"));
    }

    #[test]
    fn private_exchanged_skills_command_lists_player_skills() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("2".to_owned(), PlayerCharacter {
                inited: true,
                nickname: "晨星".to_owned(),
                skill_names: vec!["护盾".to_owned()],
                skill_notes: vec!["为自己提供护盾".to_owned()],
                skill_mp_costs: vec![3.0],
                skill_cooldown_turns: vec![2],
                ..Default::default()
            });

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".已兑换"),
            "2",
        )
        .unwrap();

        assert!(response.contains("已兑换技能"));
        assert!(response.contains("护盾"));
        assert!(response.contains("MP 3"));
        assert!(response.contains("CD 2轮"));
    }

    #[test]
    fn private_guide_command_returns_current_group_guide_to_members_only() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["2".to_owned()],
            guide: "开团前请确认角色目标。".to_owned(),
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".指南"),
            "2",
        )
        .unwrap();
        let denied = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".指南"),
            "3",
        )
        .unwrap();

        assert!(response.contains("开团前请确认角色目标"));
        assert_eq!(denied, "你还没有加入当前TRPG组。");
    }

    #[test]
    fn private_group_commands_use_unique_noncurrent_player_group() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("alpha".to_owned(), TrpgGroup {
            players: vec!["9".to_owned()],
            guide: "alpha secret".to_owned(),
            ..Default::default()
        });
        let mut beta = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
            guide: "beta guide".to_owned(),
            ..Default::default()
        };
        beta.ensure_party("red");
        beta.ensure_party("blue");
        beta.set_player_party("2", Some("red"));
        beta.set_player_party("3", Some("red"));
        beta.set_player_party("4", Some("blue"));
        manager.trpg_groups.insert("beta".to_owned(), beta);
        manager.current_trpg_group = Some("alpha".to_owned());
        for (target_id, nickname) in [("2", "晨星"), ("3", "白露"), ("4", "夜航")] {
            manager.player_characters.insert(
                target_id.to_owned(),
                completed_character(nickname),
            );
        }

        let guide = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".指南"),
            "2",
        )
        .unwrap();
        let members = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".频道人员"),
            "2",
        )
        .unwrap();

        assert!(guide.contains("beta guide"));
        assert!(!guide.contains("alpha secret"));
        assert!(members.contains("晨星"));
        assert!(members.contains("白露"));
        assert!(!members.contains("夜航"));
    }

    #[test]
    fn private_talent_draw_commands_add_exchanged_skills() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "2".to_owned(),
            completed_character("晨星"),
        );

        assert_eq!(NORMAL_TALENT_POOL.len(), 45);
        assert_eq!(SUPPORT_TALENT_POOL.len(), 29);
        assert!(NORMAL_TALENT_POOL
            .iter()
            .any(|talent| talent.name == "役于我手"));
        assert!(SUPPORT_TALENT_POOL
            .iter()
            .any(|talent| talent.name == "世界之血"));

        let normal_response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".抽取天赋"),
            "2",
        )
        .unwrap();
        let support_response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(
                NapcatMessageType::Private,
                ".抽取辅助天赋",
            ),
            "2",
        )
        .unwrap();
        let character = manager.player_characters.get("2").unwrap();

        assert!(normal_response.contains("抽取天赋"));
        assert!(normal_response.contains("已加入已兑换技能。"));
        assert!(support_response.contains("你已经抽过了"));
        assert_eq!(character.skill_names.len(), 1);
        assert!(NORMAL_TALENT_POOL
            .iter()
            .any(|talent| talent.name == character.skill_names[0]));
        assert!(NORMAL_TALENT_POOL
            .iter()
            .any(|talent| character.skill_notes[0] == talent_note(talent)));
        assert_eq!(character.skill_notes.len(), 1);
        assert_eq!(character.skill_mp_costs, vec![0.0]);
        assert_eq!(character.skill_cooldown_turns, vec![0]);
        assert_eq!(character.skill_metadata.len(), 1);
        assert_eq!(
            character.skill_metadata[0].source,
            CharacterSkillSourceKind::Talent
        );
        assert_eq!(
            character.skill_metadata[0].source_pool_id.as_deref(),
            Some("normal_talent")
        );
    }

    #[test]
    fn immediate_moonberry_talent_effects_update_character_stats() {
        let namek_talent = NORMAL_TALENT_POOL
            .iter()
            .find(|talent| talent.name == "那美克星之慧")
            .unwrap();
        let physics_talent = NORMAL_TALENT_POOL
            .iter()
            .find(|talent| talent.name == "物理专长")
            .unwrap();
        let mut character = PlayerCharacter {
            inited: true,
            level: 3,
            status: CharacterStatus {
                k: 0,
                ..Default::default()
            },
            ..Default::default()
        };

        let namek_effect = apply_moonberry_immediate_talent_effect(
            &mut character,
            namek_talent,
            &TrpgBasicConfig::default(),
        )
        .unwrap();
        assert_eq!(character.extra_status.k, 6);
        assert!(namek_effect.contains("知识额外值 +6"));

        let physics_effect = apply_moonberry_immediate_talent_effect(
            &mut character,
            physics_talent,
            &TrpgBasicConfig::default(),
        )
        .unwrap();
        assert_eq!(character.status.k, 2);
        assert!(physics_effect.contains("知识基础值提升到2"));
        assert_eq!(
            moonberry_talent_trigger(namek_talent),
            Some("常驻")
        );
        assert_eq!(
            moonberry_talent_effect_summary(namek_talent),
            Some("立即获得等级*2的知识额外值")
        );
    }

    #[test]
    fn moonberry_talent_metadata_covers_all_preserved_talents() {
        for talent in NORMAL_TALENT_POOL.iter().chain(SUPPORT_TALENT_POOL.iter()) {
            assert!(
                moonberry_talent_trigger(talent).is_some(),
                "missing trigger metadata for {}",
                talent.name
            );
            assert!(
                moonberry_talent_effect_summary(talent).is_some(),
                "missing effect metadata for {}",
                talent.name
            );
        }
    }

    #[test]
    fn private_talent_draw_applies_immediate_knowledge_effect_metadata() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "2".to_owned(),
            completed_character("晨星"),
        );
        let talent_index = NORMAL_TALENT_POOL
            .iter()
            .position(|talent| talent.name == "那美克星之慧")
            .unwrap();
        let message_time = (0..20_000)
            .find(|time| {
                stable_talent_index(
                    "2",
                    "天赋",
                    *time,
                    NORMAL_TALENT_POOL.len(),
                ) == talent_index
            })
            .unwrap();
        let mut message = test_message_with_text(NapcatMessageType::Private, ".抽取天赋");
        message.data.time = message_time;

        let response = handle_character_creation_message(&mut manager, &message, "2").unwrap();
        let character = manager.player_characters.get("2").unwrap();

        assert!(response.contains("知识额外值 +2"));
        assert_eq!(character.extra_status.k, 2);
        assert_eq!(character.skill_names, vec![
            "那美克星之慧".to_owned()
        ]);
        assert_eq!(
            character.skill_metadata[0].talent_trigger.as_deref(),
            Some("常驻")
        );
        assert_eq!(
            character.skill_metadata[0].talent_effect.as_deref(),
            Some("立即获得等级*2的知识额外值")
        );
    }

    #[test]
    fn private_support_talent_draw_uses_old_support_pool() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "2".to_owned(),
            completed_character("晨星"),
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(
                NapcatMessageType::Private,
                ".抽取辅助天赋",
            ),
            "2",
        )
        .unwrap();
        let character = manager.player_characters.get("2").unwrap();

        assert!(response.contains("抽取辅助天赋"));
        assert_eq!(character.skill_names.len(), 1);
        assert!(SUPPORT_TALENT_POOL
            .iter()
            .any(|talent| talent.name == character.skill_names[0]));
        assert!(SUPPORT_TALENT_POOL
            .iter()
            .any(|talent| character.skill_notes[0] == talent_note(talent)));
        assert_eq!(
            character.skill_metadata[0].source_pool_id.as_deref(),
            Some("support_talent")
        );
    }

    #[test]
    fn private_cooldown_command_reports_remaining_skill_turns() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("2".to_owned(), PlayerCharacter {
                inited: true,
                nickname: "晨星".to_owned(),
                skill_names: vec!["护盾".to_owned()],
                skill_cooldown_turns: vec![3],
                skill_last_cast_turns: HashMap::from([("0".to_owned(), 4)]),
                ..Default::default()
            });
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["2".to_owned()],
            player_turns: HashMap::from([("2".to_owned(), TrpgPlayerTurnState {
                turns_passed: 5,
                ..Default::default()
            })]),
            ..Default::default()
        });

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".冷却"),
            "2",
        )
        .unwrap();

        assert!(response.contains("护盾：还剩2轮"));
    }

    #[test]
    fn private_cooldown_command_reports_imported_cooldown_left() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("2".to_owned(), PlayerCharacter {
                inited: true,
                nickname: "晨星".to_owned(),
                skill_names: vec!["护盾".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata {
                    cooldown_left: Some(2),
                    ..Default::default()
                }],
                ..Default::default()
            });
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["2".to_owned()],
            ..Default::default()
        });

        let first_response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".冷却"),
            "2",
        )
        .unwrap();
        assert!(first_response.contains("护盾：还剩2轮"));
        assert_eq!(
            manager.player_characters["2"].skill_cooldown_ready_turns["0"],
            2
        );

        manager
            .trpg_groups
            .get_mut("table")
            .unwrap()
            .set_player_turns_passed("2", 1);
        let second_response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".冷却"),
            "2",
        )
        .unwrap();
        assert!(second_response.contains("护盾：还剩1轮"));

        manager
            .trpg_groups
            .get_mut("table")
            .unwrap()
            .set_player_turns_passed("2", 2);
        let ready_response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".冷却"),
            "2",
        )
        .unwrap();
        assert!(ready_response.contains("护盾：可用"));
    }

    #[test]
    fn private_channel_members_command_uses_party_scope() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "2".to_owned(),
            completed_character("晨星"),
        );
        manager.player_characters.insert(
            "3".to_owned(),
            completed_character("白露"),
        );
        manager.player_characters.insert(
            "4".to_owned(),
            completed_character("夜航"),
        );
        let mut group = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
            ..Default::default()
        };
        group.ensure_party("red");
        group.ensure_party("blue");
        group.set_player_party("2", Some("red"));
        group.set_player_party("3", Some("red"));
        group.set_player_party("4", Some("blue"));
        manager.trpg_groups.insert("table".to_owned(), group);
        manager.current_trpg_group = Some("table".to_owned());

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".频道人员"),
            "2",
        )
        .unwrap();

        assert!(response.contains("小队「red」"));
        assert!(response.contains("晨星"));
        assert!(response.contains("白露"));
        assert!(!response.contains("夜航"));
    }

    #[test]
    fn post_creation_status_spend_command_consumes_available_points() {
        let mut manager = empty_manager();
        let mut character = completed_character("晨星");
        character.status_points = 2;
        manager.player_characters.insert("2".to_owned(), character);
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["2".to_owned()],
            basic_config: TrpgBasicConfig {
                base_max_hp: 0.0,
                lv_max_hp: 4.0,
                str_max_hp: 2.0,
                vit_max_hp: 6.0,
                ..Default::default()
            },
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".力量 2"),
            "2",
        )
        .unwrap();
        let character = manager.player_characters.get("2").unwrap();

        assert!(response.contains("已为力量投入2点"));
        assert_eq!(character.status.str_, 3);
        assert_eq!(character.status_points, 0);
        assert_eq!(character.max_hp, 34.0);
    }

    #[test]
    fn completed_character_skills_sync_to_skill_pool() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("player-1".to_owned(), PlayerCharacter {
                inited: true,
                nickname: "小明".to_owned(),
                skill_names: vec!["旋风斩".to_owned()],
                skill_notes: vec!["主动使用对目标造成4点物理伤害".to_owned()],
                skill_mp_costs: vec![2.0],
                skill_cooldown_turns: vec![3],
                skill_metadata: vec![CharacterSkillMetadata {
                    args: vec![SkillPoolArg {
                        name: "伤害".to_owned(),
                        kind: "数字".to_owned(),
                        value: "4".to_owned(),
                    }],
                    legacy_has_buff_machine: true,
                    legacy_buff_machine_json: Some(r#"{"技能释放":[{"id":"n1"}]}"#.to_owned()),
                    ..Default::default()
                }],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("draft".to_owned(), PlayerCharacter {
                inited: false,
                skill_names: vec!["不会同步".to_owned()],
                skill_notes: vec!["每当自己受到伤害时，回复1点生命值".to_owned()],
                ..Default::default()
            });

        assert!(manager.sync_skill_pool_from_completed_characters());
        assert_eq!(manager.skill_pool.len(), 1);
        let entry = &manager.skill_pool[0];
        assert_eq!(entry.name, "旋风斩");
        assert_eq!(
            entry.note,
            "主动使用对目标造成4点物理伤害"
        );
        assert_eq!(entry.mp_cost, 2.0);
        assert_eq!(entry.cooldown_turns, 3);
        assert_eq!(entry.args.len(), 1);
        assert_eq!(entry.args[0].name, "伤害");
        assert!(entry.legacy_has_graph);
        assert_eq!(
            entry.legacy_buff_machine_json.as_deref(),
            Some(r#"{"技能释放":[{"id":"n1"}]}"#)
        );
        assert_eq!(
            entry.source_character_id.as_deref(),
            Some("player-1")
        );
        assert_eq!(
            entry.source_character_name.as_deref(),
            Some("小明")
        );
        assert_eq!(entry.source_skill_index, Some(0));

        manager
            .player_characters
            .get_mut("player-1")
            .unwrap()
            .skill_metadata = vec![CharacterSkillMetadata {
            st_approved: false,
            ..Default::default()
        }];
        assert!(manager.sync_skill_pool_from_completed_characters());
        assert!(manager.skill_pool.is_empty());

        manager
            .player_characters
            .get_mut("player-1")
            .unwrap()
            .skill_metadata = vec![CharacterSkillMetadata::default()];
        assert!(manager.sync_skill_pool_from_completed_characters());
        assert_eq!(manager.skill_pool.len(), 1);

        manager
            .player_characters
            .get_mut("player-1")
            .unwrap()
            .skill_notes
            .clear();
        assert!(manager.sync_skill_pool_from_completed_characters());
        assert!(manager.skill_pool.is_empty());
    }

    #[test]
    fn scene_capture_command_accepts_hash_and_dot_aliases() {
        for command in ["#观察", "#gc", ".观察", ".gc", "。观察", "。gc"] {
            let message = test_message_with_text(NapcatMessageType::Private, command);
            assert!(
                is_scene_capture_command(&message),
                "{command} should trigger capture"
            );
        }
    }

    #[test]
    fn parses_group_message_with_unsupported_segments() {
        let message = serde_json::from_str::<NapcatMessage>(
            r#"{
                "time": 1780132600,
                "message_type": "group",
                "message": [
                    { "type": "at", "data": { "qq": "123" } },
                    { "type": "text", "data": { "text": "hello group" } }
                ],
                "self_id": 3432505351,
                "user_id": 1670426821,
                "group_id": 123456,
                "sender": {
                    "user_id": 1670426821,
                    "nickname": "tester"
                }
            }"#,
        )
        .expect("group message should parse");

        assert_eq!(message.data.message.len(), 2);
        assert!(matches!(
            message.data.message[0].variant,
            NapcatMessageChainType::Unsupported
        ));
        assert!(matches!(
            message.data.message[1].variant,
            NapcatMessageChainType::Text { .. }
        ));
    }

    #[test]
    fn parses_string_message_payload() {
        let message = serde_json::from_str::<NapcatMessage>(
            r#"{
                "time": 1780132600,
                "message_type": "group",
                "message": "plain group text",
                "self_id": 3432505351,
                "user_id": 1670426821,
                "group_id": 123456,
                "sender": {
                    "user_id": 1670426821,
                    "nickname": "tester"
                }
            }"#,
        )
        .expect("string message should parse");

        let NapcatMessageChainType::Text { data } = &message.data.message[0].variant else {
            panic!("string payload should become a text segment");
        };
        assert_eq!(data.text, "plain group text");
    }

    #[test]
    fn detects_auto_forward_text_with_mixed_quote_styles() {
        let message = serde_json::from_str::<NapcatMessage>(
            r#"{
                "time": 1780132600,
                "message_type": "private",
                "message": "“hello players\"",
                "self_id": 3432505351,
                "user_id": 1670426821,
                "sender": {
                    "user_id": 1670426821,
                    "nickname": "tester"
                }
            }"#,
        )
        .expect("private message should parse");

        assert_eq!(
            quoted_auto_forward_text(&message),
            Some("hello players".to_owned())
        );
    }

    #[test]
    fn rejects_auto_forward_text_without_strict_boundary_quotes() {
        let message = serde_json::from_str::<NapcatMessage>(
            r#"{
                "time": 1780132600,
                "message_type": "private",
                "message": "say \"hello players\"",
                "self_id": 3432505351,
                "user_id": 1670426821,
                "sender": {
                    "user_id": 1670426821,
                    "nickname": "tester"
                }
            }"#,
        )
        .expect("private message should parse");

        assert_eq!(quoted_auto_forward_text(&message), None);
    }

    #[test]
    fn rejects_single_quote_character_as_auto_forward_text() {
        let message = serde_json::from_str::<NapcatMessage>(
            r#"{
                "time": 1780132600,
                "message_type": "private",
                "message": "\"",
                "self_id": 3432505351,
                "user_id": 1670426821,
                "sender": {
                    "user_id": 1670426821,
                    "nickname": "tester"
                }
            }"#,
        )
        .expect("private message should parse");

        assert_eq!(quoted_auto_forward_text(&message), None);
    }

    #[test]
    fn auto_forward_without_current_trpg_group_keeps_chat_group_targets() {
        let mut manager = empty_manager();
        for user_id in [2, 3, 4] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message_from(user_id, "hello"),
            ]);
        }
        manager.groups.insert("讨论组".to_owned(), ChatGroup {
            members: vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
        });

        let request = auto_forward_request(
            &manager,
            &test_private_message_from(2, "\"hello party\""),
            "2",
        )
        .expect("quoted private text should auto-forward");

        assert_eq!(request.recipients, vec![3, 4]);
        assert!(request.text.contains("hello party"));
    }

    #[test]
    fn auto_forward_from_party_member_excludes_other_parties() {
        let mut manager = empty_manager();
        for user_id in [2, 3, 4, 5] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message_from(user_id, "hello"),
            ]);
        }
        manager.groups.insert("讨论组".to_owned(), ChatGroup {
            members: vec![
                "2".to_owned(),
                "3".to_owned(),
                "4".to_owned(),
                "5".to_owned(),
            ],
        });
        let mut group = TrpgGroup {
            players: vec![
                "2".to_owned(),
                "3".to_owned(),
                "4".to_owned(),
                "5".to_owned(),
            ],
            ..Default::default()
        };
        group.ensure_party("red");
        group.ensure_party("blue");
        group.set_player_party("2", Some("red"));
        group.set_player_party("3", Some("red"));
        group.set_player_party("4", Some("blue"));
        manager.trpg_groups.insert("table".to_owned(), group);
        manager.current_trpg_group = Some("table".to_owned());

        let request = auto_forward_request(
            &manager,
            &test_private_message_from(2, "\"red-only clue\""),
            "2",
        )
        .expect("same-party recipient should be available");

        assert_eq!(request.recipients, vec![3, 5]);
        assert!(!request.recipients.contains(&4));
    }

    #[test]
    fn auto_forward_from_noncurrent_party_member_excludes_other_parties() {
        let mut manager = empty_manager();
        for user_id in [2, 3, 4] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message_from(user_id, "hello"),
            ]);
        }
        manager.groups.insert("讨论组".to_owned(), ChatGroup {
            members: vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
        });
        manager.trpg_groups.insert("alpha".to_owned(), TrpgGroup {
            players: vec!["9".to_owned()],
            ..Default::default()
        });
        let mut beta = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
            ..Default::default()
        };
        beta.ensure_party("red");
        beta.ensure_party("blue");
        beta.set_player_party("2", Some("red"));
        beta.set_player_party("3", Some("red"));
        beta.set_player_party("4", Some("blue"));
        manager.trpg_groups.insert("beta".to_owned(), beta);
        manager.current_trpg_group = Some("alpha".to_owned());

        let request = auto_forward_request(
            &manager,
            &test_private_message_from(2, "\"red-only clue\""),
            "2",
        )
        .expect("same-party recipient should be available");

        assert_eq!(request.recipients, vec![3]);
    }

    #[test]
    fn auto_forward_refuses_ambiguous_noncurrent_player_group() {
        let mut manager = empty_manager();
        for user_id in [2, 3, 4] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message_from(user_id, "hello"),
            ]);
        }
        manager.groups.insert("讨论组".to_owned(), ChatGroup {
            members: vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
        });
        manager.trpg_groups.insert("alpha".to_owned(), TrpgGroup {
            players: vec!["9".to_owned()],
            ..Default::default()
        });
        for name in ["beta", "gamma"] {
            manager.trpg_groups.insert(name.to_owned(), TrpgGroup {
                players: vec!["2".to_owned(), "3".to_owned()],
                ..Default::default()
            });
        }
        manager.current_trpg_group = Some("alpha".to_owned());

        assert!(auto_forward_request(
            &manager,
            &test_private_message_from(2, "\"ambiguous clue\""),
            "2",
        )
        .is_none());
    }

    #[test]
    fn new_incoming_target_waits_for_chat_window_approval() {
        let mut manager = empty_manager();

        manager.register_incoming_target("12345", true);

        assert!(manager.pending_chat_targets.contains("12345"));
        assert!(!manager.open_chat_targets.contains("12345"));
    }

    #[test]
    fn new_private_target_is_rejected_when_current_group_disallows_join_requests() {
        let mut manager = empty_manager();
        manager
            .messages
            .insert("12345".to_owned(), vec![test_message(
                NapcatMessageType::Private,
            )]);
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            allow_join_requests: false,
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());

        manager.register_incoming_target("12345", true);

        assert!(manager.rejected_chat_targets.contains("12345"));
        assert!(!manager.pending_chat_targets.contains("12345"));
        assert!(!manager.open_chat_targets.contains("12345"));
    }

    #[test]
    fn new_group_target_ignores_player_join_request_gate() {
        let mut manager = empty_manager();
        manager
            .messages
            .insert("98765".to_owned(), vec![test_message(
                NapcatMessageType::Group,
            )]);
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            allow_join_requests: false,
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());

        manager.register_incoming_target("98765", true);

        assert!(manager.pending_chat_targets.contains("98765"));
        assert!(!manager.rejected_chat_targets.contains("98765"));
    }

    #[test]
    fn approved_chat_target_moves_from_pending_to_open() {
        let mut manager = empty_manager();
        manager.register_incoming_target("12345", true);

        assert!(manager.approve_chat_target("12345"));

        assert!(manager.open_chat_targets.contains("12345"));
        assert!(!manager.pending_chat_targets.contains("12345"));
        assert!(!manager.rejected_chat_targets.contains("12345"));
    }

    #[test]
    fn approved_private_chat_target_enters_current_trpg_group() {
        let mut manager = empty_manager();
        manager
            .messages
            .insert("12345".to_owned(), vec![test_message(
                NapcatMessageType::Private,
            )]);
        manager
            .trpg_groups
            .insert("table".to_owned(), TrpgGroup::default());
        manager.current_trpg_group = Some("table".to_owned());
        manager.register_incoming_target("12345", true);

        assert!(manager.approve_chat_target("12345"));

        let group = manager.trpg_groups.get("table").unwrap();
        assert!(group.players.contains(&"12345".to_owned()));
        assert!(group.player_turns.contains_key("12345"));
    }

    #[test]
    fn approved_group_chat_target_does_not_enter_player_list() {
        let mut manager = empty_manager();
        manager
            .messages
            .insert("98765".to_owned(), vec![test_message(
                NapcatMessageType::Group,
            )]);
        manager
            .trpg_groups
            .insert("table".to_owned(), TrpgGroup::default());
        manager.current_trpg_group = Some("table".to_owned());
        manager.register_incoming_target("98765", true);

        assert!(manager.approve_chat_target("98765"));

        let group = manager.trpg_groups.get("table").unwrap();
        assert!(group.players.is_empty());
        assert!(group.group_chats.is_empty());
    }

    #[test]
    fn rejected_chat_target_does_not_reopen_as_pending() {
        let mut manager = empty_manager();
        manager.register_incoming_target("12345", true);

        assert!(manager.reject_chat_target("12345"));
        manager.register_incoming_target("12345", true);

        assert!(manager.rejected_chat_targets.contains("12345"));
        assert!(!manager.pending_chat_targets.contains("12345"));
        assert!(!manager.open_chat_targets.contains("12345"));
    }

    #[test]
    fn rejected_message_target_does_not_migrate_to_open_window() {
        let mut manager = empty_manager();
        manager.messages.insert("12345".to_owned(), Vec::new());
        manager.rejected_chat_targets.insert("12345".to_owned());

        assert!(!manager.migrate_chat_window_state());
        assert!(manager.open_chat_targets.is_empty());
    }

    #[test]
    fn existing_message_targets_migrate_to_open_chat_windows() {
        let mut manager = empty_manager();
        manager.messages.insert("12345".to_owned(), Vec::new());

        assert!(manager.migrate_chat_window_state());
        assert!(manager.open_chat_targets.contains("12345"));
        assert!(manager.pending_chat_targets.is_empty());
    }

    #[test]
    fn synced_message_targets_do_not_reopen_after_all_windows_closed() {
        let mut manager = empty_manager();
        manager.messages.insert("12345".to_owned(), Vec::new());
        manager.chat_targets.insert(
            "12345".to_owned(),
            ChatTargetMetadata::default(),
        );

        assert!(!manager.migrate_chat_window_state());
        assert!(manager.open_chat_targets.is_empty());
        assert!(manager.pending_chat_targets.is_empty());
    }

    #[test]
    fn message_targets_sync_to_editable_chat_metadata() {
        let mut manager = empty_manager();
        manager.messages.insert("12345".to_owned(), Vec::new());

        assert!(manager.sync_chat_targets());
        assert!(manager.chat_targets.contains_key("12345"));
        assert!(!manager.sync_chat_targets());
    }

    #[test]
    fn group_info_response_updates_automatic_chat_name() {
        let mut manager = empty_manager();
        let mut requests = NapcatGroupInfoRequests {
            next_request_id: 2_000_000,
            pending_group_ids: HashSet::from(["976886808".to_owned()]),
        };
        let response = serde_json::from_str::<NapcatActionResponse>(
            r#"{
                "status": "ok",
                "retcode": 0,
                "data": {
                    "group_id": 976886808,
                    "group_name": "柳絮，只是另一个跑团软件"
                },
                "echo": "group-info:976886808"
            }"#,
        )
        .expect("group info response should parse");

        assert!(apply_group_info_response(
            &response,
            &mut manager,
            &mut requests
        ));
        assert_eq!(
            manager.chat_targets["976886808"].automatic_name,
            "柳絮，只是另一个跑团软件"
        );
        assert!(!requests.pending_group_ids.contains("976886808"));
    }

    #[test]
    fn private_message_targets_sync_to_player_characters() {
        let mut manager = empty_manager();
        manager.messages.insert("player-1".to_owned(), vec![
            test_message(NapcatMessageType::Private),
        ]);
        manager.messages.insert("group-1".to_owned(), vec![
            test_message(NapcatMessageType::Group),
        ]);

        assert!(manager.sync_chat_targets());
        assert!(manager.player_characters.contains_key("player-1"));
        assert!(!manager.player_characters.contains_key("group-1"));
        assert!(!manager.sync_chat_targets());
    }

    #[test]
    fn private_exchange_command_runs_character_creation_workflow() {
        let mut manager = empty_manager();
        let target_id = "2";

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            target_id,
        )
        .unwrap();
        assert!(response.contains("开始建卡"));
        assert_eq!(
            manager.player_characters[target_id].creation_step,
            CharacterCreationStep::Str
        );

        for value in ["2", "1", "1", "1"] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        let character = manager.player_characters.get(target_id).unwrap();
        assert_eq!(
            character.creation_step,
            CharacterCreationStep::ConfirmStatus
        );
        assert_eq!(character.status.str_, 2);
        assert_eq!(character.status.vit, 1);

        for value in [".", "."] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        handle_character_creation_message(
            &mut manager,
            &test_private_image("https://example.test/pc.png"),
            target_id,
        );
        assert_eq!(
            manager.player_characters[target_id].creation_step,
            CharacterCreationStep::Nickname
        );
        assert_eq!(
            manager.player_characters[target_id].image,
            "https://example.test/pc.png"
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, "柳生"),
            target_id,
        )
        .unwrap();
        let character = manager.player_characters.get(target_id).unwrap();
        assert!(response.contains("兑换结束"));
        assert!(character.inited);
        assert_eq!(character.nickname, "柳生");
        assert_eq!(
            character.creation_step,
            CharacterCreationStep::Normal
        );
        assert_eq!(character.max_hp, 15.0);
    }

    #[test]
    fn private_exchange_skill_submission_waits_for_gm_approval() {
        let mut manager = empty_manager();
        let target_id = "2";

        handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            target_id,
        );
        for value in ["2", "1", "1", "1"] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, "."),
            target_id,
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(
                NapcatMessageType::Private,
                "主动使用对目标造成2点物理伤害",
            ),
            target_id,
        )
        .unwrap();
        assert!(response.contains("等待GM确认"));
        let character = manager.player_characters.get(target_id).unwrap();
        assert_eq!(character.skill_notes.len(), 1);
        assert!(character.skill_metadata[0].pc_approved);
        assert!(!character.skill_metadata[0].st_approved);
        assert!(format_private_character_skills(&manager, target_id).contains("GM待确认"));

        for value in [".", ".", "柳生"] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        assert!(manager.player_characters[target_id].inited);
        assert!(!manager.sync_skill_pool_from_completed_characters());
        assert!(manager.skill_pool.is_empty());
    }

    #[test]
    fn private_exchange_command_uses_current_group_creation_config() {
        let mut manager = empty_manager();
        let target_id = "2";
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec![target_id.to_owned()],
            initial_status_points: 7,
            initial_exchange_points: 9,
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            target_id,
        )
        .unwrap();
        let character = manager.player_characters.get(target_id).unwrap();

        assert!(response.contains("你拥有7点属性点"));
        assert_eq!(character.status_points, 7);
        assert_eq!(character.exchange_points, 9);

        for value in ["4", "3"] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".."),
            target_id,
        )
        .unwrap();
        let character = manager.player_characters.get(target_id).unwrap();

        assert!(response.contains("属性点已全部返还"));
        assert_eq!(character.status_points, 7);
        assert_eq!(character.status.str_, 0);
        assert_eq!(character.status.agi, 0);
    }

    #[test]
    fn private_exchange_command_uses_unique_noncurrent_group_config() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("alpha".to_owned(), TrpgGroup {
            players: vec!["9".to_owned()],
            initial_status_points: 3,
            initial_exchange_points: 4,
            ..Default::default()
        });
        manager.trpg_groups.insert("beta".to_owned(), TrpgGroup {
            players: vec!["2".to_owned()],
            initial_status_points: 7,
            initial_exchange_points: 9,
            ..Default::default()
        });
        manager.current_trpg_group = Some("alpha".to_owned());

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            "2",
        )
        .unwrap();

        assert!(response.contains("你拥有7点属性点"));
        assert_eq!(
            manager.player_characters["2"].status_points,
            7
        );
        assert_eq!(
            manager.player_characters["2"].exchange_points,
            9
        );
    }

    #[test]
    fn character_stats_are_derived_from_status_and_level() {
        let mut character = PlayerCharacter::default();
        character.level = 2;
        character.status.str_ = 2;
        character.status.vit = 3;
        character.status.int_ = 1;
        character.status.wis = 2;
        character.extra_status.agi = 1;
        character.extra_status.dex = 2;
        character.extra_status.vit = 1;

        update_character_from_status(&mut character);

        assert_eq!(character.max_hp, 29.0);
        assert_eq!(character.hp, 29.0);
        assert_eq!(character.hp_regen, 4.0);
        assert_eq!(character.max_mp, 10.0);
        assert_eq!(character.mp, 10.0);
        assert_eq!(character.mp_regen, 2.0);
        assert_eq!(character.speed, 6.0);
    }

    #[test]
    fn character_stats_can_use_group_basic_config() {
        let mut character = PlayerCharacter {
            level: 2,
            status: CharacterStatus {
                str_: 2,
                agi: 3,
                dex: 4,
                vit: 5,
                int_: 6,
                wis: 7,
                ..Default::default()
            },
            ..Default::default()
        };
        let config = TrpgBasicConfig {
            base_max_hp: 0.0,
            lv_max_hp: 4.0,
            str_max_hp: 2.0,
            vit_max_hp: 6.0,
            vit_hp_reg: 1.5,
            int_max_mp: 7.0,
            wis_max_mp: 3.0,
            wis_mp_reg: 0.5,
            basic_speed: 2.0,
            str_speed: 0.25,
            agi_speed: 1.5,
            dex_speed: 0.75,
            ..Default::default()
        };

        update_character_from_status_with_config(&mut character, &config);

        assert_eq!(character.max_hp, 42.0);
        assert_eq!(character.hp, 42.0);
        assert_eq!(character.hp_regen, 7.5);
        assert_eq!(character.max_mp, 63.0);
        assert_eq!(character.mp, 63.0);
        assert_eq!(character.mp_regen, 3.5);
        assert_eq!(character.speed, 10.0);
    }

    #[test]
    fn moonberry_experience_thresholds_and_grants_level_with_carryover() {
        assert_eq!(character_next_level_exp(1), 100);
        assert_eq!(character_next_level_exp(2), 125);
        assert_eq!(character_next_level_exp(3), 175);
        assert_eq!(character_next_level_exp(0), 100);

        let mut character = PlayerCharacter {
            level: 1,
            exp: 90,
            ..Default::default()
        };

        assert_eq!(
            grant_character_experience(&mut character, 40),
            1
        );
        assert_eq!(character.level, 2);
        assert_eq!(character.exp, 30);

        assert_eq!(
            grant_character_experience(&mut character, 500),
            2
        );
        assert_eq!(character.level, 4);
        assert_eq!(character.exp, 230);

        assert_eq!(
            grant_character_experience(&mut character, 0),
            0
        );
        assert_eq!(character.level, 4);
        assert_eq!(character.exp, 230);
    }

    #[test]
    fn basic_config_applies_moonberry_damage_and_heal_attribute_multipliers() {
        let character = PlayerCharacter {
            status: CharacterStatus {
                str_: 10,
                agi: 51,
                dex: 5,
                int_: 4,
                wis: 6,
                ..Default::default()
            },
            extra_status: CharacterStatus {
                str_: 2,
                agi: 3,
                dex: 1,
                int_: 1,
                wis: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        let config = TrpgBasicConfig {
            str_damage_bonus: 0.1,
            agi_damage_bonus: 0.2,
            dex_damage_bonus: 0.3,
            int_damage_bonus: 0.4,
            dex_range_damage_bonus: 0.5,
            int_heal_bonus: 0.6,
            wis_heal_bonus: 0.7,
            ..Default::default()
        };

        assert!(
            (character_damage_attribute_multiplier(
                &character,
                &config,
                TrpgDamageBonusKind::Magical,
            ) - 3.0)
                .abs()
                < f32::EPSILON
        );
        let mut mage = character.clone();
        mage.skill_names.push("大魔法师".to_owned());
        mage.skill_metadata.push(CharacterSkillMetadata::talent(
            "normal_talent",
            "天赋",
        ));
        assert!(
            (character_damage_attribute_multiplier(
                &mage,
                &config,
                TrpgDamageBonusKind::Magical,
            ) - 3.025)
                .abs()
                < 0.0001
        );
        assert!(
            (character_damage_attribute_multiplier(
                &mage,
                &config,
                TrpgDamageBonusKind::Physical,
            ) - 4.8)
                .abs()
                < f32::EPSILON
        );
        assert!(
            (character_damage_attribute_multiplier(
                &mage,
                &config,
                TrpgDamageBonusKind::Range,
            ) - 4.0)
                .abs()
                < f32::EPSILON
        );
        mage.skill_metadata[0].st_approved = false;
        assert!(
            (character_damage_attribute_multiplier(
                &mage,
                &config,
                TrpgDamageBonusKind::Magical,
            ) - 3.0)
                .abs()
                < f32::EPSILON
        );
        assert!(
            (character_damage_taken_attribute_multiplier(
                &character,
                TrpgDamageTakenKind::Diseased
            ) - 1.0)
                .abs()
                < f32::EPSILON
        );
        let mut human = character.clone();
        human.skill_names.push("人类基因工程".to_owned());
        human.skill_metadata.push(CharacterSkillMetadata::talent(
            "normal_talent",
            "天赋",
        ));
        assert!(
            (character_damage_taken_attribute_multiplier(&human, TrpgDamageTakenKind::Diseased)
                - 0.85)
                .abs()
                < 0.0001
        );
        assert!(
            (character_damage_taken_attribute_multiplier(&human, TrpgDamageTakenKind::Poisoning)
                - 0.85)
                .abs()
                < 0.0001
        );
        assert!(
            (character_damage_taken_attribute_multiplier(&human, TrpgDamageTakenKind::Magical)
                - 1.0)
                .abs()
                < f32::EPSILON
        );
        let mut anti_magic = character.clone();
        anti_magic.skill_names.push("抗魔体质".to_owned());
        anti_magic
            .skill_metadata
            .push(CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            ));
        assert!(
            (character_damage_taken_attribute_multiplier(
                &anti_magic,
                TrpgDamageTakenKind::Magical,
            ) - 0.9)
                .abs()
                < 0.0001
        );
        anti_magic.skill_metadata[0].st_approved = false;
        assert!(
            (character_damage_taken_attribute_multiplier(
                &anti_magic,
                TrpgDamageTakenKind::Magical,
            ) - 1.0)
                .abs()
                < f32::EPSILON
        );
        let mut wounder = character.clone();
        wounder.skill_names.push("溃伤".to_owned());
        wounder.skill_metadata.push(CharacterSkillMetadata::talent(
            "normal_talent",
            "天赋",
        ));
        let wound_buffs = character_damage_dealt_talent_buffs(&wounder, "caster");
        assert_eq!(wound_buffs.len(), 1);
        assert_eq!(wound_buffs[0].name, "溃伤");
        assert_eq!(wound_buffs[0].turns_remaining, 1);
        assert_eq!(wound_buffs[0].effects, vec![
            BuffEffect {
                field: BuffField::HealingTakenModifier,
                value: BuffValue::AddPercent(-25.0),
            }
        ]);
        wounder.skill_metadata[0].st_approved = false;
        assert!(character_damage_dealt_talent_buffs(&wounder, "caster").is_empty());
        let mut lifestealer = character.clone();
        lifestealer.skill_names.push("禅宗古训".to_owned());
        lifestealer
            .skill_metadata
            .push(CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            ));
        assert!((character_physical_damage_lifesteal(&lifestealer) - 0.15).abs() < f32::EPSILON);
        lifestealer.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_physical_damage_lifesteal(&lifestealer),
            0.0
        );
        let mut sousas = character.clone();
        sousas.skill_names.push("苏萨斯之爪".to_owned());
        sousas.skill_metadata.push(CharacterSkillMetadata::talent(
            "normal_talent",
            "天赋",
        ));
        assert!((character_physical_damage_followup_rate(&sousas) - 0.35).abs() < f32::EPSILON);
        let followup = moonberry_physical_damage_followup_buff("caster", 3.5);
        assert_eq!(followup.name, "苏萨斯之爪");
        assert_eq!(followup.turns_remaining, 2);
        assert_eq!(followup.source_id, "caster");
        assert_eq!(followup.tick_actions, vec![
            BuffTickAction::FixedDamage {
                amount: 3.5,
                damage_type: DamageType::Magical,
            }
        ]);
        sousas.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_physical_damage_followup_rate(&sousas),
            0.0
        );
        let mut chicken = character.clone();
        chicken.level = 3;
        chicken.skill_names.push("菜鸡猛啄".to_owned());
        chicken.skill_metadata.push(CharacterSkillMetadata::talent(
            "normal_talent",
            "天赋",
        ));
        assert_eq!(
            character_minimum_damage_floor(&chicken),
            3.0
        );
        chicken.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_minimum_damage_floor(&chicken),
            0.0
        );
        let mut chaos = character.clone();
        chaos.skill_names.push("混沌无序".to_owned());
        chaos.skill_metadata.push(CharacterSkillMetadata::talent(
            "normal_talent",
            "天赋",
        ));
        assert!((character_chaos_output_variance(&chaos) - 0.15).abs() < f32::EPSILON);
        let chaos_roll = moonberry_chaos_output_multiplier(character_chaos_output_variance(&chaos));
        assert!((0.85..=1.15).contains(&chaos_roll));
        chaos.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_chaos_output_variance(&chaos),
            0.0
        );
        let mut scoped = character.clone();
        scoped.level = 2;
        scoped.skill_names.push("瞄准镜Tex-30".to_owned());
        scoped.skill_metadata.push(CharacterSkillMetadata::talent(
            "normal_talent",
            "天赋",
        ));
        assert_eq!(
            character_minimum_range_meters(&scoped),
            30.0
        );
        assert_eq!(
            moonberry_effective_skill_range_radius_with_multiplier(
                Some(3),
                character_minimum_range_meters(&scoped),
                1.0,
            ),
            Some(30.0)
        );
        assert_eq!(
            moonberry_effective_skill_range_radius_with_multiplier(
                Some(45),
                character_minimum_range_meters(&scoped),
                1.0,
            ),
            Some(45.0)
        );
        assert_eq!(
            moonberry_effective_skill_range_radius_with_multiplier(
                None,
                character_minimum_range_meters(&scoped),
                1.0,
            ),
            Some(30.0)
        );
        scoped.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_minimum_range_meters(&scoped),
            0.0
        );
        assert_eq!(
            moonberry_effective_skill_range_radius_with_multiplier(
                None,
                character_minimum_range_meters(&scoped),
                1.0,
            ),
            None
        );
        let mut spell_reacher = character.clone();
        spell_reacher.skill_names.push("魔网延伸".to_owned());
        spell_reacher
            .skill_metadata
            .push(CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            ));
        assert!((character_spell_range_multiplier(&spell_reacher) - 1.05).abs() < 0.0001);
        assert!(moonberry_skill_type_is_spell(Some(
            " 法术 "
        )));
        assert_eq!(
            moonberry_effective_skill_range_radius_with_multiplier(
                Some(20),
                0.0,
                character_spell_range_multiplier(&spell_reacher),
            ),
            Some(21.0)
        );
        assert_eq!(
            moonberry_effective_skill_range_radius_with_multiplier(
                None,
                0.0,
                character_spell_range_multiplier(&spell_reacher),
            ),
            None
        );
        spell_reacher.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_spell_range_multiplier(&spell_reacher),
            1.0
        );
        let mut gale = character.clone();
        gale.speed = 10.0;
        gale.skill_names.push("狂风恶浪".to_owned());
        gale.skill_metadata.push(CharacterSkillMetadata::talent(
            "normal_talent",
            "天赋",
        ));
        assert_eq!(
            character_gale_force_battle_speeds(&gale),
            Some((12.0, 13.5))
        );
        gale.speed = 12.0;
        gale.buff_base_stats = Some(CharacterBuffBaseStats {
            hp: 5.0,
            max_hp: 5.0,
            hp_regen: 0.0,
            mp: 0.0,
            max_mp: 0.0,
            mp_regen: 0.0,
            speed: 10.0,
            damage_dealt_modifier: 1.0,
            damage_taken_modifier: 1.0,
            healing_dealt_modifier: 1.0,
            healing_taken_modifier: 1.0,
            extra_status: CharacterStatus::default(),
        });
        assert_eq!(
            character_gale_force_battle_speeds(&gale),
            Some((12.0, 13.5))
        );
        gale.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_gale_force_battle_speeds(&gale),
            None
        );
        let mut penitent = character.clone();
        penitent.skill_names.push("忏悔".to_owned());
        penitent.skill_metadata.push(CharacterSkillMetadata::talent(
            "support_talent",
            "辅助天赋",
        ));
        assert_eq!(
            character_penance_healing_bonus_percent(&penitent),
            25.0
        );
        assert_eq!(
            penance_decayed_healing_dealt_modifier(1.25, 25.0, 0),
            1.25
        );
        assert!((penance_decayed_healing_dealt_modifier(1.25, 25.0, 1) - 1.15).abs() < 0.0001);
        assert!((penance_decayed_healing_dealt_modifier(1.25, 25.0, 2) - 1.05).abs() < 0.0001);
        assert_eq!(
            penance_decayed_healing_dealt_modifier(1.25, 25.0, 3),
            1.0
        );
        penitent.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_penance_healing_bonus_percent(&penitent),
            0.0
        );
        let mut large_hit_target = character.clone();
        large_hit_target.skill_names.push("过度免疫".to_owned());
        large_hit_target
            .skill_metadata
            .push(CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            ));
        assert!(
            (character_large_hit_damage_taken_modifier(&large_hit_target) - 0.8).abs()
                < f32::EPSILON
        );
        assert_eq!(
            large_hit_damage_taken_multiplier(20.0, 4.0, 0.8),
            1.0
        );
        assert_eq!(
            large_hit_damage_taken_multiplier(20.0, 4.01, 0.8),
            0.8
        );
        large_hit_target.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_large_hit_damage_taken_modifier(&large_hit_target),
            1.0
        );
        let mut dying_target_healer = character.clone();
        dying_target_healer.skill_names.push("生死时速".to_owned());
        dying_target_healer
            .skill_metadata
            .push(CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            ));
        assert!(
            (character_dying_target_healing_modifier(&dying_target_healer) - 1.5).abs()
                < f32::EPSILON
        );
        assert_eq!(
            dying_target_healing_multiplier(
                4.0,
                20.0,
                character_dying_target_healing_modifier(&dying_target_healer),
            ),
            1.5
        );
        assert_eq!(
            dying_target_healing_multiplier(
                5.0,
                20.0,
                character_dying_target_healing_modifier(&dying_target_healer),
            ),
            1.0
        );
        dying_target_healer.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_dying_target_healing_modifier(&dying_target_healer),
            1.0
        );
        let mut wounded_healer = character.clone();
        wounded_healer.hp = 20.0;
        wounded_healer.max_hp = 20.0;
        wounded_healer.skill_names.push("火源之力".to_owned());
        wounded_healer
            .skill_metadata
            .push(CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            ));
        assert!((character_wounded_healing_dealt_modifier(&wounded_healer) - 1.2).abs() < 0.0001);
        assert_eq!(
            wounded_healing_dealt_multiplier(
                wounded_healer.hp,
                wounded_healer.max_hp,
                character_wounded_healing_dealt_modifier(&wounded_healer),
            ),
            1.2
        );
        wounded_healer.hp = 8.0;
        assert!(
            (wounded_healing_dealt_multiplier(
                wounded_healer.hp,
                wounded_healer.max_hp,
                character_wounded_healing_dealt_modifier(&wounded_healer),
            ) - 1.1)
                .abs()
                < 0.0001
        );
        wounded_healer.hp = 4.0;
        assert_eq!(
            wounded_healing_dealt_multiplier(
                wounded_healer.hp,
                wounded_healer.max_hp,
                character_wounded_healing_dealt_modifier(&wounded_healer),
            ),
            1.0
        );
        wounded_healer.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_wounded_healing_dealt_modifier(&wounded_healer),
            1.0
        );
        let mut mutual_aid = character.clone();
        mutual_aid.skill_names.push("互帮互助".to_owned());
        mutual_aid
            .skill_metadata
            .push(CharacterSkillMetadata::talent(
                "support_talent",
                "辅助天赋",
            ));
        assert_eq!(
            character_mutual_aid_healing_rate(&mutual_aid),
            0.5
        );
        mutual_aid.skill_metadata[0].st_approved = false;
        assert_eq!(
            character_mutual_aid_healing_rate(&mutual_aid),
            0.0
        );
        assert!(
            (character_damage_attribute_multiplier(
                &character,
                &config,
                TrpgDamageBonusKind::Physical,
            ) - 4.8)
                .abs()
                < f32::EPSILON
        );
        assert!(
            (character_damage_attribute_multiplier(
                &character,
                &config,
                TrpgDamageBonusKind::Range,
            ) - 4.0)
                .abs()
                < f32::EPSILON
        );
        let mut converter = character.clone();
        converter.skill_names.push("数魔转换器".to_owned());
        converter
            .skill_metadata
            .push(CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            ));
        assert!(
            (character_damage_attribute_multiplier(
                &converter,
                &config,
                TrpgDamageBonusKind::Range,
            ) - 6.0)
                .abs()
                < 0.0001
        );
        converter.skill_names.push("大魔法师".to_owned());
        converter
            .skill_metadata
            .push(CharacterSkillMetadata::talent(
                "normal_talent",
                "天赋",
            ));
        assert!(
            (character_damage_attribute_multiplier(
                &converter,
                &config,
                TrpgDamageBonusKind::Range,
            ) - 6.025)
                .abs()
                < 0.0001
        );
        converter.skill_metadata[0].st_approved = false;
        assert!(
            (character_damage_attribute_multiplier(
                &converter,
                &config,
                TrpgDamageBonusKind::Range,
            ) - 4.0)
                .abs()
                < f32::EPSILON
        );
        assert!((character_healing_attribute_multiplier(&character, &config) - 9.6).abs() < 0.0001);
    }

    #[test]
    fn low_hp_damage_multiplier_matches_moonberry_thresholds() {
        assert!((low_hp_damage_multiplier(9.0, 10.0) - 1.0).abs() < f32::EPSILON);
        assert!((low_hp_damage_multiplier(7.0, 10.0) - 0.97).abs() < 0.0001);
        assert!((low_hp_damage_multiplier(5.0, 10.0) - 0.75).abs() < f32::EPSILON);
        assert!((low_hp_damage_multiplier(2.0, 10.0) - 0.2).abs() < 0.0001);
        assert!((low_hp_damage_multiplier(0.5, 10.0) - 0.05).abs() < 0.0001);
        assert_eq!(low_hp_damage_multiplier(1.0, 0.0), 0.0);

        assert!((low_hp_damage_multiplier_with_fatigue(5.0, 10.0, true) - 0.8).abs() < 0.0001);
        assert!((low_hp_damage_multiplier_with_fatigue(0.1, 10.0, true) - 0.24).abs() < 0.0001);
    }

    #[test]
    fn local_private_text_response_is_appended_as_self_message() {
        let mut manager = empty_manager();
        let target_id = "2";
        manager.messages.insert(target_id.to_owned(), vec![
            test_message_with_text(NapcatMessageType::Private, ".兑换"),
        ]);

        append_local_private_text_response(&mut manager, target_id, 2, "兑换回复");

        let messages = manager.messages.get(target_id).unwrap();
        assert_eq!(messages.len(), 2);
        let response = &messages[1];
        assert!(matches!(
            response.data.message_type,
            NapcatMessageType::Private
        ));
        assert_eq!(
            response.data.user_id,
            response.data.self_id
        );
        assert_eq!(response.data.target_id, Some(2));
        assert_eq!(response.data.sender.nickname, "GM");
        assert_eq!(message_text(response), "兑换回复");
    }

    #[test]
    fn local_private_text_response_uses_noncurrent_target_campaign() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("alpha".to_owned(), TrpgGroup {
            campaign_id: "campaign-a".to_owned(),
            players: vec!["9".to_owned()],
            ..Default::default()
        });
        let mut beta = TrpgGroup {
            campaign_id: "campaign-b".to_owned(),
            players: vec!["2".to_owned()],
            ..Default::default()
        };
        beta.ensure_party("red");
        beta.set_player_party("2", Some("red"));
        manager.trpg_groups.insert("beta".to_owned(), beta);
        manager.current_trpg_group = Some("alpha".to_owned());
        manager.messages.insert("2".to_owned(), vec![
            test_private_message_from(2, "player asks"),
        ]);

        append_local_private_text_response(&mut manager, "2", 2, "private answer");

        let response = manager.messages["2"].last().unwrap();
        assert_eq!(response.data.campaign_id, "campaign-b");
        assert_eq!(
            response.data.character_id.as_deref(),
            Some("2")
        );
        assert_eq!(
            response.data.party_id.as_deref(),
            Some("red")
        );
        assert_eq!(
            response.data.visibility,
            Visibility::Player(2)
        );
    }

    #[test]
    fn character_creation_back_resets_status_phase_and_refunds_points() {
        let mut manager = empty_manager();
        let target_id = "2";

        handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            target_id,
        );
        for value in ["5"] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        assert_eq!(
            manager.player_characters[target_id].creation_step,
            CharacterCreationStep::ConfirmStatus
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".."),
            target_id,
        )
        .unwrap();
        let character = manager.player_characters.get(target_id).unwrap();

        assert!(response.contains("属性点已全部返还"));
        assert_eq!(
            character.creation_step,
            CharacterCreationStep::Str
        );
        assert_eq!(
            character.status_points,
            default_status_points()
        );
        assert_eq!(character.status.str_, 0);
        assert_eq!(character.status.cha, 0);
    }

    #[test]
    fn character_creation_back_from_nickname_returns_to_image_phase() {
        let mut manager = empty_manager();
        let target_id = "2";

        handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            target_id,
        );
        for value in ["2", "1", "1", "1", ".", "."] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        handle_character_creation_message(
            &mut manager,
            &test_private_image("https://example.test/pc.png"),
            target_id,
        );
        assert_eq!(
            manager.player_characters[target_id].creation_step,
            CharacterCreationStep::Nickname
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".."),
            target_id,
        )
        .unwrap();
        let character = manager.player_characters.get(target_id).unwrap();

        assert!(response.contains("图片录入"));
        assert_eq!(
            character.creation_step,
            CharacterCreationStep::Image
        );
        assert!(character.image.is_empty());
    }

    #[test]
    fn character_creation_back_from_image_returns_to_skill_phase() {
        let mut manager = empty_manager();
        let target_id = "2";

        handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".兑换"),
            target_id,
        );
        for value in ["2", "1", "1", "1", ".", "."] {
            handle_character_creation_message(
                &mut manager,
                &test_message_with_text(NapcatMessageType::Private, value),
                target_id,
            );
        }
        assert_eq!(
            manager.player_characters[target_id].creation_step,
            CharacterCreationStep::Image
        );

        let response = handle_character_creation_message(
            &mut manager,
            &test_message_with_text(NapcatMessageType::Private, ".."),
            target_id,
        )
        .unwrap();
        let character = manager.player_characters.get(target_id).unwrap();

        assert!(response.contains("技能兑换"));
        assert_eq!(
            character.creation_step,
            CharacterCreationStep::Skill
        );
    }

    #[test]
    fn chat_target_sync_prunes_missing_trpg_group_members() {
        let mut manager = empty_manager();
        manager.messages.insert("player-1".to_owned(), Vec::new());
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            players: vec!["player-1".to_owned(), "missing-player".to_owned()],
            group_chats: vec!["missing-group".to_owned()],
            ..Default::default()
        });

        assert!(manager.sync_chat_targets());
        let group = manager.trpg_groups.get("table").unwrap();
        assert_eq!(group.players, vec!["player-1"]);
        assert!(group.group_chats.is_empty());
        assert!(group.player_turns.contains_key("player-1"));
        assert!(!group.player_turns.contains_key("missing-player"));
    }

    #[test]
    fn chat_target_sync_selects_only_trpg_group_as_current() {
        let mut manager = empty_manager();
        manager
            .trpg_groups
            .insert("table".to_owned(), TrpgGroup::default());

        assert!(manager.sync_chat_targets());
        assert_eq!(
            manager.current_trpg_group.as_deref(),
            Some("table")
        );
    }

    #[test]
    fn chat_target_sync_clears_deleted_current_group() {
        let mut manager = empty_manager();
        manager.current_trpg_group = Some("deleted".to_owned());
        manager
            .trpg_groups
            .insert("table".to_owned(), TrpgGroup::default());

        assert!(manager.sync_chat_targets());
        assert_eq!(
            manager.current_trpg_group.as_deref(),
            Some("table")
        );
    }

    #[test]
    fn trpg_group_advances_world_turn_after_all_players_finish() {
        let mut group = TrpgGroup {
            players: vec!["a".to_owned(), "b".to_owned()],
            ..Default::default()
        };

        assert!(group.mark_player_acted("a"));
        assert_eq!(group.world_turn, 0);
        assert!(group.player_turns["a"].acted);
        assert!(!group.player_turns["b"].acted);

        assert!(group.mark_player_skipped("b"));
        assert_eq!(group.world_turn, 1);
        assert_eq!(group.player_turns["a"].turns_passed, 1);
        assert_eq!(group.player_turns["b"].turns_passed, 1);
        assert!(!group.player_turns["a"].acted);
        assert!(!group.player_turns["b"].skipped);
    }

    #[test]
    fn trpg_group_turn_action_is_idempotent() {
        let mut group = TrpgGroup {
            players: vec!["a".to_owned(), "b".to_owned()],
            ..Default::default()
        };

        assert!(group.mark_player_acted("a"));
        assert!(!group.mark_player_acted("a"));
        assert_eq!(group.world_turn, 0);
        assert!(group.player_turns["a"].acted);
        assert!(!group.player_turns["a"].skipped);
        assert!(!group.player_turns["b"].acted);
    }

    #[test]
    fn trpg_group_rejects_turn_action_for_non_member() {
        let mut group = TrpgGroup {
            players: vec!["a".to_owned()],
            ..Default::default()
        };

        assert!(!group.mark_player_acted("missing"));
        assert_eq!(group.world_turn, 0);
        assert!(group.player_turns.is_empty());
    }

    #[test]
    fn trpg_group_sets_player_turn_count() {
        let mut group = TrpgGroup {
            players: vec!["a".to_owned()],
            ..Default::default()
        };

        assert!(group.set_player_turns_passed("a", 7));
        assert_eq!(group.player_turns["a"].turns_passed, 7);
        assert!(!group.set_player_turns_passed("missing", 3));
        assert_eq!(group.world_turn, 0);
    }

    #[test]
    fn trpg_group_manual_advance_counts_all_current_players() {
        let mut group = TrpgGroup {
            players: vec!["a".to_owned(), "b".to_owned()],
            ..Default::default()
        };

        assert!(group.advance_world_turn());

        assert_eq!(group.world_turn, 1);
        assert_eq!(group.player_turns["a"].turns_passed, 1);
        assert_eq!(group.player_turns["b"].turns_passed, 1);
    }

    #[test]
    fn trpg_group_negative_timer_starts_when_half_players_are_ahead() {
        let mut group = TrpgGroup {
            battle_negative_enabled: true,
            players: vec!["a".to_owned(), "b".to_owned()],
            ..Default::default()
        };

        assert!(group.mark_player_acted("a"));

        let timer = group.legacy_negative_timer("b").unwrap();
        assert_eq!(
            timer.remaining_ms,
            LEGACY_NEGATIVE_TIMEOUT_MS
        );
        assert!(timer.active());
        assert!(!timer.replied);

        assert!(group.register_legacy_negative_reply("b"));
        let timer = group.legacy_negative_timer("b").unwrap();
        assert_eq!(timer.remaining_ms, 0);
        assert!(!timer.active());
        assert!(timer.replied);
    }

    #[test]
    fn trpg_group_negative_timeout_records_layer_and_uses_turn_skip() {
        let mut group = TrpgGroup {
            battle_negative_enabled: true,
            players: vec!["a".to_owned(), "b".to_owned()],
            ..Default::default()
        };

        assert!(group.mark_player_acted("a"));
        assert!(group.record_legacy_negative_timeout("b"));
        assert_eq!(
            group.legacy_negative_timer("b").unwrap().negative_layers,
            1
        );

        assert!(group.mark_player_skipped("b"));

        assert_eq!(group.world_turn, 1);
        let timer = group.legacy_negative_timer("b").unwrap();
        assert_eq!(timer.negative_layers, 1);
        assert_eq!(timer.remaining_ms, 0);
    }
}
