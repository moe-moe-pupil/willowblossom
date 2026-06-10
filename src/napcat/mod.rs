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
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    json,
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
    rule_engine::BuffSpec,
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

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct RandomPool {
    #[serde(default)]
    pub entries: Vec<RandomPoolEntry>,
    #[serde(default)]
    pub last_pick: Option<InventoryItem>,
    #[serde(default)]
    pub last_text_result: Option<RandomPoolTextResult>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnitPoolEntry {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub character: PlayerCharacter,
}

impl Default for UnitPoolEntry {
    fn default() -> Self {
        Self {
            label: "新单位".to_owned(),
            note: String::new(),
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
}

impl SkillPoolEntry {
    pub fn source_key(&self) -> Option<(String, usize)> {
        Some((
            self.source_character_id.clone()?,
            self.source_skill_index?,
        ))
    }
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
    #[serde(default)]
    pub args: Vec<SkillPoolArg>,
    #[serde(default)]
    pub legacy_has_buff_machine: bool,
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
            args: Vec::new(),
            legacy_has_buff_machine: false,
        }
    }
}

impl CharacterSkillMetadata {
    pub fn talent(pool_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            source: CharacterSkillSourceKind::Talent,
            source_pool_id: Some(pool_id.into()),
            source_pool_label: Some(label.into()),
            ..Default::default()
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
            status: CharacterStatus::default(),
            extra_status: CharacterStatus::default(),
            skill_names: Vec::new(),
            skill_notes: Vec::new(),
            skill_mp_costs: Vec::new(),
            skill_cooldown_turns: Vec::new(),
            skill_metadata: Vec::new(),
            skill_last_cast_turns: HashMap::new(),
            active_buffs: Vec::new(),
            buff_base_stats: None,
            inventory: CharacterInventory::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct CharacterBuffBaseStats {
    #[serde(default = "default_character_hp")]
    pub hp: f32,
    #[serde(default = "default_character_hp")]
    pub max_hp: f32,
    #[serde(default = "default_modifier")]
    pub damage_dealt_modifier: f32,
    #[serde(default = "default_modifier")]
    pub damage_taken_modifier: f32,
    #[serde(default = "default_modifier")]
    pub healing_dealt_modifier: f32,
    #[serde(default = "default_modifier")]
    pub healing_taken_modifier: f32,
}

impl CharacterBuffBaseStats {
    pub fn from_character(character: &PlayerCharacter) -> Self {
        Self {
            hp: character.hp,
            max_hp: character.max_hp,
            damage_dealt_modifier: character.damage_dealt_modifier,
            damage_taken_modifier: character.damage_taken_modifier,
            healing_dealt_modifier: character.healing_dealt_modifier,
            healing_taken_modifier: character.healing_taken_modifier,
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
    #[serde(default)]
    pub gm_users: HashSet<u64>,
    #[serde(default)]
    pub parties: HashMap<String, TrpgParty>,
    #[serde(default)]
    pub player_parties: HashMap<String, String>,
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
            gm_users: HashSet::default(),
            parties: HashMap::default(),
            player_parties: HashMap::default(),
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
        true
    }

    fn mark_player_turn(&mut self, target_id: &str, acted: bool) -> bool {
        if !self.players.iter().any(|player_id| player_id == target_id) {
            return false;
        }

        let sync_changed = self.sync_turn_players();
        let Some(turn) = self.player_turns.get_mut(target_id) else {
            return sync_changed;
        };
        let already_set =
            if acted { turn.acted && !turn.skipped } else { !turn.acted && turn.skipped };
        if already_set {
            return sync_changed;
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

        if let Some(groups) = value.get("groups").and_then(Value::as_array) {
            recognized_shape = true;
            let current_group_index = moonberry_usize_field(&value, "currentGroup");
            let mut imported_group_names = Vec::new();
            for (index, group) in groups.iter().enumerate() {
                let group_name = moonberry_group_name(group, index);
                self.merge_moonberry_group(&group_name, group, &mut summary);
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
        summary: &mut MoonberryLegacyImportSummary,
    ) {
        let description = moonberry_string_field(group, "description");
        let st_description = moonberry_string_field(group, "stDesc");
        let guide = moonberry_string_field(group, "guide");
        let initial_status_points = group
            .get("basicConfig")
            .and_then(|config| moonberry_i32_field(config, "initStatusPoint"))
            .or_else(|| moonberry_i32_field(group, "initStatusPoint"));
        let initial_exchange_points = group
            .get("basicConfig")
            .and_then(|config| moonberry_i32_field(config, "initExchangePoint"))
            .or_else(|| moonberry_i32_field(group, "initExchangePoint"));
        let basic_config = group.get("basicConfig").map(moonberry_basic_config);

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
            trpg_group.sync_turn_players();
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
            let legacy_has_graph = pool.get("graph").is_some_and(|graph| !graph.is_null());
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
            let pool_name = moonberry_string_field(pool, "name")
                .or_else(|| moonberry_string_field(pool, "id"))
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| "月莓随机池包含缺少名称的池".to_owned())?;
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

    pub fn character_creation_config_for_target(&self, target_id: &str) -> (i32, i32) {
        self.current_group_for_player(target_id)
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
        self.current_group_for_player(target_id)
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

    pub fn visible_campaign_messages_for_summary(
        &self,
        target_id: &str,
        messages: &[NapcatMessage],
    ) -> Vec<CampaignMessage> {
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
            .filter(|message| access.can_read(&message.visibility))
            .collect()
    }

    pub fn campaign_message_for_target(
        &self,
        target_id: &str,
        message: &NapcatMessage,
    ) -> CampaignMessage {
        let text = message_text(message);
        let campaign_id = if message.data.campaign_id.trim().is_empty() {
            self.current_campaign_id()
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
                let access = self.player_access_for_user(peer_id);
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
                let access = self.player_access_for_user(message.data.user_id);
                let visibility = access
                    .party_id
                    .as_ref()
                    .map(|party_id| Visibility::Party(party_id.clone()))
                    .unwrap_or(Visibility::Public);
                CampaignMessage {
                    campaign_id,
                    sender_id: message.data.user_id,
                    sender_name: message.data.sender.nickname.clone(),
                    source: MessageSource::Group {
                        group_id,
                        user_id: message.data.user_id,
                    },
                    character_id: access.character_id,
                    party_id: access.party_id,
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
                args: skill_args,
                legacy_has_buff_machine,
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
                legacy_pool_id: metadata.source_pool_id,
                category: metadata.source_pool_label,
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
            // .insert_resource(NapcatSocket { ..default() })
            .add_systems(Startup, setup)
            .add_systems(Update, message_system)
            .add_systems(Update, request_missing_group_info_system)
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
                //Receive messages from the websocket
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
                //Receive messages from the game
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
            manager.annotate_message_access(&target_id, &mut json);

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
    let access = manager.player_access_for_user(recipient_id);
    let message = NapcatMessage {
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
            campaign_id: manager.current_campaign_id(),
            character_id: access.character_id,
            party_id: access.party_id,
            visibility: Visibility::Player(recipient_id),
        },
    };

    manager
        .messages
        .entry(target_id.to_owned())
        .or_default()
        .push(message);
}

fn is_scene_capture_command(message: &NapcatMessage) -> bool {
    let text = message_text(message);
    matches!(
        text.trim(),
        "#观察" | "#gc" | ".观察" | ".gc"
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
                .push(CharacterSkillMetadata::default());
            Some(format!(
                "技能兑换数据已录入，目前记录{}条。继续发送技能，或输入【.】结束技能录入。",
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

const NORMAL_TALENT_POOL: &[&str] = &[
    "坚韧：每次战斗第一次受到伤害时，减少1点受到伤害。",
    "灵感：每场战斗第一次施放技能后，回复1点MP。",
    "疾行：自己的速度提高1。",
    "专注：单目标技能的MP消耗降低1，最低为0。",
    "破势：主动造成物理伤害时，额外造成1点伤害。",
    "回声：治疗技能第一次生效时，额外回复1点HP。",
    "洞察：观察或调查相关判定获得GM提示时，可以额外追问一次细节。",
    "守护：同小队成员受到伤害后，每轮一次可以为其承担1点伤害。",
];

const SUPPORT_TALENT_POOL: &[&str] = &[
    "补给：每次休整后获得一件临时消耗品。",
    "协调：同小队成员第一次行动后，你可以获得1点临时速度，持续1轮。",
    "急救：每场战斗第一次治疗生命低于一半的目标时，额外回复2点HP。",
    "侦查：进入新区域时，可以请求GM公开一个安全路径或危险来源。",
    "掩护：同小队成员被单目标攻击时，每场战斗一次使其受到伤害减少1。",
    "后援：自己跳过行动时，指定同小队一名成员回复1点MP。",
    "整备：战斗开始前选择一名同小队成员，其第一项技能冷却减少1轮。",
    "记录：总结线索时，可以向GM请求一次遗漏线索提示。",
];

fn draw_character_talent(
    manager: &mut NapcatMessageManager,
    target_id: &str,
    label: &str,
    pool: &[&str],
    message_time: u64,
) -> String {
    let Some(character) = manager.player_characters.get_mut(target_id) else {
        return "你还没有角色卡。输入【.兑换】开始建卡。".to_owned();
    };
    if !character.inited {
        return "角色卡尚未完成。请先完成建卡流程。".to_owned();
    }
    if pool.is_empty() {
        return format!("{label}池为空，请联系GM配置。");
    }

    let talent = pool[stable_talent_index(
        target_id,
        label,
        message_time,
        pool.len(),
    )];
    let talent_name = talent
        .split_once('：')
        .map(|(name, _)| name)
        .unwrap_or(label)
        .trim()
        .to_owned();
    character.skill_names.push(talent_name);
    character.skill_notes.push(talent.to_owned());
    character.skill_mp_costs.push(0.0);
    character.skill_cooldown_turns.push(0);
    character
        .skill_metadata
        .push(CharacterSkillMetadata::talent(
            talent_pool_id(label),
            label.to_owned(),
        ));
    format!("抽取{label}：{talent}\n已加入已兑换技能。")
}

fn talent_pool_id(label: &str) -> String {
    match label {
        "天赋" => "normal_talent".to_owned(),
        "辅助天赋" => "support_talent".to_owned(),
        other => other.trim().to_owned(),
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
            "等级：{}  经验：{}",
            character.level, character.exp
        ),
        format!(
            "HP：{}/{}  MP：{}/{}",
            format_character_number(character.hp),
            format_character_number(character.max_hp),
            format_character_number(character.mp),
            format_character_number(character.max_mp)
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

fn format_private_character_cooldowns(manager: &NapcatMessageManager, target_id: &str) -> String {
    let Some(character) = manager.player_characters.get(target_id) else {
        return "你还没有角色卡。输入【.兑换】开始建卡。".to_owned();
    };
    let skill_count = character_skill_count(character);
    if skill_count == 0 {
        return "还没有已兑换技能。".to_owned();
    }

    let current_turn = current_player_cooldown_turn(manager, target_id);
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
        if cooldown == 0 && !has_cast_record {
            continue;
        }

        let name = character_skill_display_name(character, index);
        let remaining = skill_cooldown_remaining(character, index, cooldown, current_turn);
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
    let Some(group) = manager.current_group() else {
        return "当前没有TRPG组。".to_owned();
    };
    if !group.players.iter().any(|player_id| player_id == target_id) {
        return "你还没有加入当前TRPG组。".to_owned();
    }

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
    let Some(group) = manager.current_group() else {
        return "当前没有TRPG组。".to_owned();
    };
    if !group.players.iter().any(|player_id| player_id == target_id) {
        return "你还没有加入当前TRPG组。".to_owned();
    }

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
        .trpg_groups
        .values()
        .filter(|group| group.players.iter().any(|player_id| player_id == target_id))
        .map(|group| {
            group
                .player_turns
                .get(target_id)
                .map(|turn| turn.turns_passed)
                .unwrap_or(group.world_turn)
        })
        .max()
        .unwrap_or_default()
}

fn skill_cooldown_remaining(
    character: &PlayerCharacter,
    skill_index: usize,
    cooldown_turns: u32,
    current_turn: u32,
) -> u32 {
    if cooldown_turns == 0 {
        return 0;
    }
    character
        .skill_last_cast_turns
        .get(&skill_index.to_string())
        .map(|last_cast_turn| {
            cooldown_turns.saturating_sub(current_turn.saturating_sub(*last_cast_turn))
        })
        .unwrap_or(0)
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
    let total_str = character.status.str_ + character.extra_status.str_;
    let total_agi = character.status.agi + character.extra_status.agi;
    let total_dex = character.status.dex + character.extra_status.dex;
    let total_vit = character.status.vit + character.extra_status.vit;
    let total_int = character.status.int_ + character.extra_status.int_;
    let total_wis = character.status.wis + character.extra_status.wis;

    character.max_hp = (config.base_max_hp
        + character.level as f32 * config.lv_max_hp
        + total_str as f32 * config.str_max_hp
        + total_vit as f32 * config.vit_max_hp)
        .max(1.0);
    character.hp = character.max_hp;
    character.hp_regen = total_vit.max(0) as f32 * config.vit_hp_reg;
    character.max_mp = total_int as f32 * config.int_max_mp + total_wis as f32 * config.wis_max_mp;
    character.mp = character.max_mp.max(0.0);
    character.mp_regen = total_wis.max(0) as f32 * config.wis_mp_reg;
    character.speed = config.basic_speed
        + total_str.max(0) as f32 * config.str_speed
        + total_agi.max(0) as f32 * config.agi_speed
        + total_dex.max(0) as f32 * config.dex_speed;
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
    let group = manager
        .current_group()
        .filter(|group| group.players.iter().any(|player_id| player_id == target_id))?;
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
                character: completed_character("行尸"),
            });
        manager
            .unit_pool
            .insert("archer".to_owned(), UnitPoolEntry {
                label: "弓手".to_owned(),
                note: "远程单位".to_owned(),
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
    fn unit_pool_export_json_merges_by_unit_id_without_chat_data() {
        let mut source = empty_manager();
        source.unit_pool.insert("archer".to_owned(), UnitPoolEntry {
            label: "新弓手".to_owned(),
            note: "导入版本".to_owned(),
            character: completed_character("新弓手"),
        });
        source.unit_pool.insert("zombie".to_owned(), UnitPoolEntry {
            label: "行尸".to_owned(),
            note: "缓慢近战单位".to_owned(),
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
    fn moonberry_legacy_root_import_merges_groups_pcs_units_and_messages() {
        let legacy = r#"{
            "discriminator": "Root",
            "currentGroup": 0,
            "groups": [{
                "name": "旧团",
                "description": "公开说明",
                "stDesc": "GM说明",
                "guide": "旧入团指南",
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
            manager.unit_pool["unit-old"].character.max_hp,
            5.0
        );
        let random_pool = &manager.random_pools["遭遇随机"];
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
    fn private_talent_draw_commands_add_exchanged_skills() {
        let mut manager = empty_manager();
        manager.player_characters.insert(
            "2".to_owned(),
            completed_character("晨星"),
        );

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
        assert!(support_response.contains("抽取辅助天赋"));
        assert_eq!(character.skill_names.len(), 2);
        assert_eq!(character.skill_notes.len(), 2);
        assert_eq!(character.skill_mp_costs, vec![0.0, 0.0]);
        assert_eq!(character.skill_cooldown_turns, vec![
            0, 0
        ]);
        assert_eq!(character.skill_metadata.len(), 2);
        assert_eq!(
            character.skill_metadata[0].source,
            CharacterSkillSourceKind::Talent
        );
        assert_eq!(
            character.skill_metadata[0].source_pool_id.as_deref(),
            Some("normal_talent")
        );
        assert_eq!(
            character.skill_metadata[1].source_pool_id.as_deref(),
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
        for command in ["#观察", "#gc", ".观察", ".gc"] {
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
}
